use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use git2::Repository;

const REPO_CACHE_TTL: Duration = Duration::from_secs(60 * 60);

struct RepoCacheEntry {
    info: RepoInfo,
    scanned_at: Instant,
}

static REPO_CACHE: std::sync::LazyLock<Mutex<HashMap<PathBuf, RepoCacheEntry>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone)]
pub enum RepoStatus {
    Clean,
    Dirty {
        modified: usize,
        added: usize,
        deleted: usize,
    },
}

#[derive(Debug, Clone)]
pub struct GitHubItem {
    pub number: u64,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct GitHubData {
    pub open_issues: usize,
    pub open_prs: usize,
    pub recent_issues: Vec<GitHubItem>,
    pub recent_prs: Vec<GitHubItem>,
}

#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub hash: String,
    pub message: String,
    pub author: String,
    pub date: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct RepoInfo {
    pub name: String,
    pub path: PathBuf,
    pub status: RepoStatus,
    pub current_branch: String,
    pub branches: Vec<String>,
    pub remote_url: Option<String>,
    pub github_repo: Option<(String, String)>,
    pub github_data: Option<GitHubData>,
    pub github_error: Option<String>,
    pub recent_commits: Vec<CommitInfo>,
    pub changed_files: Vec<String>,
}

/// Recursively scan a directory for git repositories.
/// Stops recursing into directories that are themselves git repos.
pub fn scan_directory(path: &Path) -> Vec<RepoInfo> {
    let mut repos = Vec::new();

    // Check if the starting directory itself is a repo
    if is_git_repo(path) {
        if let Some(info) = analyze_repo(path) {
            repos.push(info);
        }
        return repos;
    }

    scan_recursive(path, &mut repos);
    repos.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    repos
}

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "build",
    "dist",
    "out",
    "vendor",
    "venv",
    ".venv",
    "__pycache__",
    "Pods",
    "DerivedData",
    ".gradle",
    ".cargo",
    ".rustup",
    "Library",
    "Applications",
    "Music",
    "Movies",
    "Pictures",
    "Photos",
    ".Trash",
];

fn is_git_repo(path: &Path) -> bool {
    let git_path = path.join(".git");
    git_path.is_dir() || git_path.is_file()
}

fn scan_recursive(path: &Path, repos: &mut Vec<RepoInfo>) {
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let entry_path = entry.path();
        if !entry_path.is_dir() {
            continue;
        }

        let dir_name = entry_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        // Skip hidden directories and common non-project directories
        if dir_name.starts_with('.') || SKIP_DIRS.contains(&dir_name) {
            continue;
        }

        if is_git_repo(&entry_path) {
            if let Some(info) = analyze_repo(&entry_path) {
                repos.push(info);
            }
        }
        // Always recurse - there may be nested repos inside
        scan_recursive(&entry_path, repos);
    }
}

/// Analyze a single git repository and extract information.
/// Results are cached for 1 hour per repo path.
fn analyze_repo(path: &Path) -> Option<RepoInfo> {
    // Check cache
    if let Ok(cache) = REPO_CACHE.lock() {
        if let Some(entry) = cache.get(path) {
            if entry.scanned_at.elapsed() < REPO_CACHE_TTL {
                return Some(entry.info.clone());
            }
        }
    }

    let info = analyze_repo_uncached(path)?;

    // Store in cache
    if let Ok(mut cache) = REPO_CACHE.lock() {
        cache.insert(
            path.to_path_buf(),
            RepoCacheEntry {
                info: info.clone(),
                scanned_at: Instant::now(),
            },
        );
    }

    Some(info)
}

fn analyze_repo_uncached(path: &Path) -> Option<RepoInfo> {
    let repo = Repository::open(path).ok()?;

    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let current_branch = get_current_branch(&repo);
    let branches = list_branches(&repo);
    let (status, changed_files) = get_repo_status(&repo);
    let remote_url = get_remote_url(&repo);
    let github_repo = remote_url.as_deref().and_then(parse_github_url);
    let recent_commits = get_recent_commits(&repo, 20);

    Some(RepoInfo {
        name,
        path: path.to_path_buf(),
        status,
        current_branch,
        branches,
        remote_url,
        github_repo,
        github_data: None,
        github_error: None,
        recent_commits,
        changed_files,
    })
}

fn get_current_branch(repo: &Repository) -> String {
    if repo.head_detached().unwrap_or(false) {
        if let Ok(head) = repo.head() {
            if let Some(oid) = head.target() {
                return format!("detached@{}", &oid.to_string()[..7]);
            }
        }
        return "detached".to_string();
    }

    repo.head()
        .ok()
        .and_then(|h| h.shorthand().map(String::from))
        .unwrap_or_else(|| "HEAD".to_string())
}

fn list_branches(repo: &Repository) -> Vec<String> {
    let mut branch_names = Vec::new();
    if let Ok(branches) = repo.branches(Some(git2::BranchType::Local)) {
        for branch in branches.flatten() {
            if let Some(name) = branch.0.name().ok().flatten() {
                branch_names.push(name.to_string());
            }
        }
    }
    branch_names
}

fn get_repo_status(repo: &Repository) -> (RepoStatus, Vec<String>) {
    let statuses = match repo.statuses(None) {
        Ok(s) => s,
        Err(_) => return (RepoStatus::Clean, Vec::new()),
    };

    let mut modified = 0;
    let mut added = 0;
    let mut deleted = 0;
    let mut changed_files = Vec::new();

    for entry in statuses.iter() {
        let s = entry.status();
        let file_path = entry.path().unwrap_or("?").to_string();

        if s.intersects(
            git2::Status::WT_MODIFIED
                | git2::Status::INDEX_MODIFIED
                | git2::Status::WT_RENAMED
                | git2::Status::INDEX_RENAMED,
        ) {
            modified += 1;
            changed_files.push(format!("M {file_path}"));
        } else if s.intersects(git2::Status::WT_NEW | git2::Status::INDEX_NEW) {
            added += 1;
            changed_files.push(format!("A {file_path}"));
        } else if s.intersects(git2::Status::WT_DELETED | git2::Status::INDEX_DELETED) {
            deleted += 1;
            changed_files.push(format!("D {file_path}"));
        }
    }

    let status = if modified == 0 && added == 0 && deleted == 0 {
        RepoStatus::Clean
    } else {
        RepoStatus::Dirty {
            modified,
            added,
            deleted,
        }
    };

    (status, changed_files)
}

fn get_remote_url(repo: &Repository) -> Option<String> {
    repo.find_remote("origin")
        .ok()
        .and_then(|r| r.url().map(String::from))
}

fn get_recent_commits(repo: &Repository, count: usize) -> Vec<CommitInfo> {
    let head = match repo.head() {
        Ok(h) => h,
        Err(_) => return Vec::new(),
    };
    let oid = match head.target() {
        Some(o) => o,
        None => return Vec::new(),
    };
    let mut revwalk = match repo.revwalk() {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    if revwalk.push(oid).is_err() {
        return Vec::new();
    }

    let mut commits = Vec::new();
    for oid in revwalk.flatten().take(count) {
        if let Ok(commit) = repo.find_commit(oid) {
            commits.push(CommitInfo {
                hash: oid.to_string()[..7].to_string(),
                message: commit.summary().unwrap_or("").to_string(),
                author: commit.author().name().unwrap_or("unknown").to_string(),
                date: format_timestamp(commit.time().seconds()),
            });
        }
    }
    commits
}

fn format_timestamp(secs: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let diff = now - secs;
    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else if diff < 2592000 {
        format!("{}d ago", diff / 86400)
    } else {
        format!("{}mo ago", diff / 2592000)
    }
}

/// Invalidate all repo scan caches.
pub fn invalidate_all_repo_caches() {
    if let Ok(mut cache) = REPO_CACHE.lock() {
        cache.clear();
    }
}

/// Parse a GitHub URL (HTTPS or SSH) into (owner, repo).
pub fn parse_github_url(url: &str) -> Option<(String, String)> {
    // SSH: git@github.com:owner/repo.git
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        let rest = rest.strip_suffix(".git").unwrap_or(rest);
        let parts: Vec<&str> = rest.splitn(2, '/').collect();
        if parts.len() == 2 {
            return Some((parts[0].to_string(), parts[1].to_string()));
        }
    }

    // HTTPS: https://github.com/owner/repo.git
    if url.contains("github.com") {
        let url = url.strip_suffix(".git").unwrap_or(url);
        let parts: Vec<&str> = url.rsplitn(3, '/').collect();
        if parts.len() >= 2 {
            return Some((parts[1].to_string(), parts[0].to_string()));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_ssh_url() {
        let result = parse_github_url("git@github.com:user/repo.git");
        assert_eq!(result, Some(("user".to_string(), "repo".to_string())));
    }

    #[test]
    fn test_parse_github_https_url() {
        let result = parse_github_url("https://github.com/user/repo.git");
        assert_eq!(result, Some(("user".to_string(), "repo".to_string())));
    }

    #[test]
    fn test_parse_github_https_no_git_suffix() {
        let result = parse_github_url("https://github.com/user/repo");
        assert_eq!(result, Some(("user".to_string(), "repo".to_string())));
    }

    #[test]
    fn test_parse_non_github_url() {
        let result = parse_github_url("https://gitlab.com/user/repo.git");
        assert_eq!(result, None);
    }

    #[test]
    fn test_scan_finds_nested_repos() {
        let tmp = std::env::temp_dir().join("project-dash-test-nested");
        let _ = std::fs::remove_dir_all(&tmp);

        // Create nested structure: tmp/group/repo-a and tmp/group/repo-b
        let repo_a = tmp.join("group").join("repo-a");
        let repo_b = tmp.join("group").join("repo-b");
        std::fs::create_dir_all(&repo_a).unwrap();
        std::fs::create_dir_all(&repo_b).unwrap();

        git2::Repository::init(&repo_a).unwrap();
        git2::Repository::init(&repo_b).unwrap();

        let repos = scan_directory(&tmp);
        let mut names: Vec<&str> = repos.iter().map(|r| r.name.as_str()).collect();
        names.sort();

        assert_eq!(names, vec!["repo-a", "repo-b"]);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
