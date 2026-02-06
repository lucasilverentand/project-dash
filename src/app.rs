use std::path::PathBuf;

use ratatui::widgets::TableState;
use tokio::sync::mpsc;

use std::collections::HashSet;

use crate::github;
use crate::repo::{GitHubData, RepoInfo};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ActivePane {
    RepoList,
    Detail,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DetailTab {
    Changes,
    Commits,
    Issues,
    Prs,
}

impl DetailTab {
    pub fn next(self) -> Self {
        match self {
            Self::Changes => Self::Commits,
            Self::Commits => Self::Issues,
            Self::Issues => Self::Prs,
            Self::Prs => Self::Changes,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Changes => Self::Prs,
            Self::Commits => Self::Changes,
            Self::Issues => Self::Commits,
            Self::Prs => Self::Issues,
        }
    }

}

#[derive(Debug)]
pub enum Message {
    Quit,
    MoveUp,
    MoveDown,
    Refresh,
    ForceRefresh,
    RetryGitHub,
    ForceRetryGitHub,
    Tick,
    SwitchPane,
    FocusList,
    NextTab,
    PrevTab,
    Click { column: u16, row: u16 },
    ReposScanned(Vec<RepoInfo>),
    GitHubDataReceived { path: PathBuf, data: GitHubData },
    GitHubError { path: PathBuf, error: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    Scanning,
    Ready,
}

pub struct App {
    pub repos: Vec<RepoInfo>,
    pub table_state: TableState,
    pub state: AppState,
    pub scan_path: PathBuf,
    pub github_token: Option<String>,
    pub should_quit: bool,
    pub tx: mpsc::UnboundedSender<Message>,
    pub active_pane: ActivePane,
    pub detail_tab: DetailTab,
    pub detail_scroll: u16,
    pub list_area: ratatui::layout::Rect,
    pub tab_bar_area: ratatui::layout::Rect,
    pub detail_content_area: ratatui::layout::Rect,
    /// Clickable regions: (rect, url)
    pub click_zones: Vec<(ratatui::layout::Rect, String)>,
    github_fetching: HashSet<PathBuf>,
}

impl App {
    pub fn new(
        scan_path: PathBuf,
        github_token: Option<String>,
        tx: mpsc::UnboundedSender<Message>,
    ) -> Self {
        Self {
            repos: Vec::new(),
            table_state: TableState::default(),
            state: AppState::Scanning,
            scan_path,
            github_token,
            should_quit: false,
            tx,
            active_pane: ActivePane::RepoList,
            detail_tab: DetailTab::Changes,
            detail_scroll: 0,
            list_area: ratatui::layout::Rect::default(),
            tab_bar_area: ratatui::layout::Rect::default(),
            detail_content_area: ratatui::layout::Rect::default(),
            click_zones: Vec::new(),
            github_fetching: HashSet::new(),
        }
    }

    pub fn selected_repo(&self) -> Option<&RepoInfo> {
        self.table_state
            .selected()
            .and_then(|i| self.repos.get(i))
    }

    pub fn update(&mut self, msg: Message) {
        match msg {
            Message::Quit => {
                self.should_quit = true;
            }
            Message::MoveUp => match self.active_pane {
                ActivePane::RepoList => {
                    if self.repos.is_empty() {
                        return;
                    }
                    let i = match self.table_state.selected() {
                        Some(i) => {
                            if i == 0 {
                                self.repos.len() - 1
                            } else {
                                i - 1
                            }
                        }
                        None => 0,
                    };
                    self.table_state.select(Some(i));
                    self.detail_scroll = 0;
                    self.detail_tab = DetailTab::Changes;
                    self.maybe_fetch_selected_github();
                }
                ActivePane::Detail => {
                    self.detail_scroll = self.detail_scroll.saturating_sub(1);
                }
            },
            Message::MoveDown => match self.active_pane {
                ActivePane::RepoList => {
                    if self.repos.is_empty() {
                        return;
                    }
                    let i = match self.table_state.selected() {
                        Some(i) => {
                            if i >= self.repos.len() - 1 {
                                0
                            } else {
                                i + 1
                            }
                        }
                        None => 0,
                    };
                    self.table_state.select(Some(i));
                    self.detail_scroll = 0;
                    self.detail_tab = DetailTab::Changes;
                    self.maybe_fetch_selected_github();
                }
                ActivePane::Detail => {
                    self.detail_scroll = self.detail_scroll.saturating_add(1);
                }
            },
            Message::SwitchPane => {
                self.active_pane = match self.active_pane {
                    ActivePane::RepoList => ActivePane::Detail,
                    ActivePane::Detail => ActivePane::RepoList,
                };
                self.detail_scroll = 0;
                self.maybe_fetch_selected_github();
            }
            Message::FocusList => {
                self.active_pane = ActivePane::RepoList;
                self.detail_scroll = 0;
            }
            Message::NextTab => {
                self.detail_tab = self.detail_tab.next();
                self.detail_scroll = 0;
            }
            Message::PrevTab => {
                self.detail_tab = self.detail_tab.prev();
                self.detail_scroll = 0;
            }
            Message::Click { column, row } => {
                // Check repo list click
                let area = self.list_area;
                if column >= area.x
                    && column < area.x + area.width
                    && row >= area.y
                    && row < area.y + area.height
                {
                    let data_start = area.y + 2; // border + header
                    if row >= data_start {
                        let idx = (row - data_start) as usize;
                        if idx < self.repos.len() {
                            self.table_state.select(Some(idx));
                            self.detail_scroll = 0;
                            self.detail_tab = DetailTab::Changes;
                            self.active_pane = ActivePane::RepoList;
                            self.maybe_fetch_selected_github();
                        }
                    }
                    return;
                }

                // Check tab bar click
                let tb = self.tab_bar_area;
                if row == tb.y && column >= tb.x && column < tb.x + tb.width {
                    let rel = (column - tb.x) as usize;
                    // Tab layout: " Changes │ Commits │ Issues │ PRs "
                    // positions:   1-7       11-17      21-26    30-32
                    let tab = if rel < 9 {
                        Some(DetailTab::Changes)
                    } else if rel < 19 {
                        Some(DetailTab::Commits)
                    } else if rel < 27 {
                        Some(DetailTab::Issues)
                    } else {
                        Some(DetailTab::Prs)
                    };
                    if let Some(t) = tab {
                        self.detail_tab = t;
                        self.detail_scroll = 0;
                    }
                    return;
                }

                // Check click zones (clickable items in detail content)
                for (rect, url) in &self.click_zones {
                    if column >= rect.x
                        && column < rect.x + rect.width
                        && row >= rect.y
                        && row < rect.y + rect.height
                    {
                        let _ = open::that(url);
                        return;
                    }
                }
            }
            Message::RetryGitHub => {
                if let Some(idx) = self.table_state.selected() {
                    if let Some(repo) = self.repos.get_mut(idx) {
                        repo.github_error = None;
                        repo.github_data = None;
                        self.github_fetching.remove(&repo.path);
                    }
                }
                self.maybe_fetch_selected_github();
            }
            Message::ForceRetryGitHub => {
                if let Some(idx) = self.table_state.selected() {
                    if let Some(repo) = self.repos.get_mut(idx) {
                        if let Some((owner, name)) = &repo.github_repo {
                            github::invalidate_cached(owner, name);
                        }
                        repo.github_error = None;
                        repo.github_data = None;
                        self.github_fetching.remove(&repo.path);
                    }
                }
                self.maybe_fetch_selected_github();
            }
            Message::Refresh => match self.active_pane {
                ActivePane::Detail => {
                    self.update(Message::RetryGitHub);
                    return;
                }
                ActivePane::RepoList => {
                    self.state = AppState::Scanning;
                    self.repos.clear();
                    self.table_state.select(None);
                    self.detail_scroll = 0;
                    let path = self.scan_path.clone();
                    let tx = self.tx.clone();
                    tokio::spawn(async move {
                        let repos = tokio::task::spawn_blocking(move || {
                            crate::repo::scan_directory(&path)
                        })
                        .await
                        .unwrap_or_default();
                        let _ = tx.send(Message::ReposScanned(repos));
                    });
                }
            },
            Message::ForceRefresh => match self.active_pane {
                ActivePane::Detail => {
                    self.update(Message::ForceRetryGitHub);
                    return;
                }
                ActivePane::RepoList => {
                    crate::repo::invalidate_all_repo_caches();
                    self.state = AppState::Scanning;
                    self.repos.clear();
                    self.table_state.select(None);
                    self.detail_scroll = 0;
                    let path = self.scan_path.clone();
                    let tx = self.tx.clone();
                    tokio::spawn(async move {
                        let repos = tokio::task::spawn_blocking(move || {
                            crate::repo::scan_directory(&path)
                        })
                        .await
                        .unwrap_or_default();
                        let _ = tx.send(Message::ReposScanned(repos));
                    });
                }
            },
            Message::Tick => {}
            Message::ReposScanned(repos) => {
                self.repos = repos;
                self.state = AppState::Ready;
                self.github_fetching.clear();
                if !self.repos.is_empty() {
                    self.table_state.select(Some(0));
                }
            }
            Message::GitHubDataReceived { path, data } => {
                if let Some(repo) = self.repos.iter_mut().find(|r| r.path == path) {
                    repo.github_data = Some(data);
                    repo.github_error = None;
                }
            }
            Message::GitHubError { path, error } => {
                if let Some(repo) = self.repos.iter_mut().find(|r| r.path == path) {
                    repo.github_error = Some(error);
                }
            }
        }
    }

    fn maybe_fetch_selected_github(&mut self) {
        let repo = match self.selected_repo() {
            Some(r) => r,
            None => return,
        };

        // Skip if already fetched, errored, or in-flight
        if repo.github_data.is_some()
            || repo.github_error.is_some()
            || self.github_fetching.contains(&repo.path)
        {
            return;
        }

        // Extract what we need before mutating self
        let path = repo.path.clone();
        let (owner, name) = match &repo.github_repo {
            Some(pair) => pair.clone(),
            None => return,
        };

        self.github_fetching.insert(path.clone());
        github::spawn_github_fetch(
            path,
            owner,
            name,
            self.github_token.clone(),
            self.tx.clone(),
        );
    }
}
