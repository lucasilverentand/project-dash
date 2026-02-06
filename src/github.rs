use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use octocrab::Octocrab;
use tokio::sync::mpsc;

use crate::app::Message;
use crate::repo::{GitHubData, GitHubItem};

const CACHE_TTL: Duration = Duration::from_secs(60 * 60);
const RECENT_ITEMS: u8 = 5;

struct CacheEntry {
    data: GitHubData,
    fetched_at: Instant,
}

static CACHE: std::sync::LazyLock<Mutex<HashMap<String, CacheEntry>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

fn cache_key(owner: &str, repo: &str) -> String {
    format!("{owner}/{repo}")
}

fn get_cached(owner: &str, repo: &str) -> Option<GitHubData> {
    let cache = CACHE.lock().ok()?;
    let entry = cache.get(&cache_key(owner, repo))?;
    if entry.fetched_at.elapsed() < CACHE_TTL {
        Some(entry.data.clone())
    } else {
        None
    }
}

fn set_cached(owner: &str, repo: &str, data: &GitHubData) {
    if let Ok(mut cache) = CACHE.lock() {
        cache.insert(
            cache_key(owner, repo),
            CacheEntry {
                data: data.clone(),
                fetched_at: Instant::now(),
            },
        );
    }
}

pub fn invalidate_cached(owner: &str, repo: &str) {
    if let Ok(mut cache) = CACHE.lock() {
        cache.remove(&cache_key(owner, repo));
    }
}

pub struct GitHubClient {
    client: Octocrab,
}

impl GitHubClient {
    pub fn new(token: Option<String>) -> color_eyre::Result<Self> {
        let mut builder = Octocrab::builder();
        if let Some(token) = token {
            builder = builder.personal_token(token);
        }
        let client = builder.build()?;
        Ok(Self { client })
    }

    pub async fn fetch_repo_data(
        &self,
        owner: &str,
        repo: &str,
    ) -> color_eyre::Result<GitHubData> {
        if let Some(cached) = get_cached(owner, repo) {
            return Ok(cached);
        }

        let issues_page = self
            .client
            .issues(owner, repo)
            .list()
            .state(octocrab::params::State::Open)
            .per_page(RECENT_ITEMS)
            .send()
            .await?;

        let prs_page = self
            .client
            .pulls(owner, repo)
            .list()
            .state(octocrab::params::State::Open)
            .per_page(RECENT_ITEMS)
            .send()
            .await?;

        let total_issues =
            issues_page.total_count.unwrap_or(issues_page.items.len() as u64);
        let total_prs =
            prs_page.total_count.unwrap_or(prs_page.items.len() as u64);

        // GitHub issues endpoint includes PRs, so subtract for "pure" issues
        let open_issues = (total_issues as usize).saturating_sub(total_prs as usize);
        let open_prs = total_prs as usize;

        // Filter out PRs from the issues list (they have a pull_request field)
        let recent_issues: Vec<GitHubItem> = issues_page
            .items
            .iter()
            .filter(|i| i.pull_request.is_none())
            .take(RECENT_ITEMS as usize)
            .map(|i| GitHubItem {
                number: i.number,
                title: i.title.clone(),
            })
            .collect();

        let recent_prs: Vec<GitHubItem> = prs_page
            .items
            .iter()
            .take(RECENT_ITEMS as usize)
            .map(|pr| GitHubItem {
                number: pr.number,
                title: pr.title.as_deref().unwrap_or("(no title)").to_string(),
            })
            .collect();

        let data = GitHubData {
            open_issues,
            open_prs,
            recent_issues,
            recent_prs,
        };

        set_cached(owner, repo, &data);
        Ok(data)
    }
}

/// Spawn a single background task to fetch GitHub data for one repo.
/// Result is sent back via the provided channel.
pub fn spawn_github_fetch(
    path: PathBuf,
    owner: String,
    repo: String,
    token: Option<String>,
    tx: mpsc::UnboundedSender<Message>,
) {
    tokio::spawn(async move {
        let client = match GitHubClient::new(token) {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(Message::GitHubError {
                    path,
                    error: e.to_string(),
                });
                return;
            }
        };

        match client.fetch_repo_data(&owner, &repo).await {
            Ok(data) => {
                let _ = tx.send(Message::GitHubDataReceived { path, data });
            }
            Err(e) => {
                let _ = tx.send(Message::GitHubError {
                    path,
                    error: e.to_string(),
                });
            }
        }
    });
}
