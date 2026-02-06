use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table, Wrap},
    Frame,
};

use crate::app::{ActivePane, App, AppState, DetailTab};
use crate::repo::RepoStatus;

fn block(title: &str, focused: bool) -> Block<'_> {
    let style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(style)
        .title(format!(" {title} "))
        .title_style(if focused {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        })
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    let [title_area, main_area, status_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(8),
        Constraint::Length(1),
    ])
    .areas(area);

    // Title bar
    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            " Project Dashboard ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            match app.state {
                AppState::Scanning => " (scanning...)",
                AppState::Ready => "",
            },
            Style::default().fg(Color::Yellow),
        ),
    ]));

    frame.render_widget(title, title_area);

    // Measure left panel width
    let mut max_name: u16 = 4;
    let mut max_status: u16 = 6;
    for repo in &app.repos {
        max_name = max_name.max(repo.name.len() as u16);
        max_status = max_status.max(status_width(repo));
    }
    let list_width = 2 + 2 + max_name + PAD + max_status + PAD;

    // Main area: repo list (left) + right side (info panel + detail tabs)
    let [list_area, right_area] = Layout::horizontal([
        Constraint::Length(list_width),
        Constraint::Fill(1),
    ])
    .areas(main_area);

    app.list_area = list_area;
    app.click_zones.clear();
    draw_repo_list(frame, app, list_area);

    // Right side: info panel (fixed height) + tabbed detail pane (fill)
    let info_height = if app.selected_repo().is_some() { 5 } else { 0 };
    let [info_area, detail_area] = Layout::vertical([
        Constraint::Length(info_height),
        Constraint::Fill(1),
    ])
    .areas(right_area);

    if app.selected_repo().is_some() {
        draw_info_panel(frame, app, info_area);
    }
    draw_detail_pane(frame, app, detail_area);

    // Status bar
    let key = Style::default().fg(Color::DarkGray);
    let desc = Style::default().fg(Color::Rgb(100, 100, 100));

    let keybinds = match app.active_pane {
        ActivePane::RepoList => vec![
            Span::styled(" [↑/k] ", key),
            Span::styled("Up  ", desc),
            Span::styled("[↓/j] ", key),
            Span::styled("Down  ", desc),
            Span::styled("[Tab/Enter] ", key),
            Span::styled("Detail  ", desc),
            Span::styled("[r] ", key),
            Span::styled("Refresh  ", desc),
            Span::styled("[R] ", key),
            Span::styled("Hard Refresh  ", desc),
            Span::styled("[q] ", key),
            Span::styled("Quit", desc),
        ],
        ActivePane::Detail => vec![
            Span::styled(" [↑/k] ", key),
            Span::styled("Scroll Up  ", desc),
            Span::styled("[↓/j] ", key),
            Span::styled("Scroll Down  ", desc),
            Span::styled("[[] ", key),
            Span::styled("Prev Tab  ", desc),
            Span::styled("[]] ", key),
            Span::styled("Next Tab  ", desc),
            Span::styled("[r] ", key),
            Span::styled("Retry  ", desc),
            Span::styled("[Tab/Esc] ", key),
            Span::styled("Back  ", desc),
            Span::styled("[q] ", key),
            Span::styled("Quit", desc),
        ],
    };

    let status = Paragraph::new(Line::from(keybinds));
    frame.render_widget(status, status_area);
}

fn status_width(repo: &crate::repo::RepoInfo) -> u16 {
    match &repo.status {
        RepoStatus::Clean => 1,
        RepoStatus::Dirty { modified, added, deleted } => {
            let w = format!("+{added} ~{modified} -{deleted}");
            w.len() as u16
        }
    }
}

const PAD: u16 = 2;

fn draw_repo_list(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.active_pane == ActivePane::RepoList;

    let mut max_status: u16 = 6;
    for repo in &app.repos {
        max_status = max_status.max(status_width(repo));
    }

    let header_style = Style::default().add_modifier(Modifier::BOLD);
    let header = Row::new(vec![
        Cell::from("Name").style(header_style),
        Cell::from("Status").style(header_style),
    ])
    .style(Style::default().fg(Color::White));

    let rows: Vec<Row> = app
        .repos
        .iter()
        .map(|repo| {
            let status_cell = match &repo.status {
                RepoStatus::Clean => Cell::from("✓").style(Style::default().fg(Color::Green)),
                RepoStatus::Dirty {
                    modified,
                    added,
                    deleted,
                } => Cell::from(Line::from(vec![
                    Span::styled(format!("+{added}"), Style::default().fg(Color::Green)),
                    Span::raw(" "),
                    Span::styled(format!("~{modified}"), Style::default().fg(Color::Yellow)),
                    Span::raw(" "),
                    Span::styled(format!("-{deleted}"), Style::default().fg(Color::Red)),
                ])),
            };

            Row::new(vec![
                Cell::from(repo.name.clone()),
                status_cell,
            ])
        })
        .collect();

    let widths = [
        Constraint::Fill(1),
        Constraint::Length(max_status + PAD),
    ];

    let repo_count = format!(" {} repos ", app.repos.len());
    let table = Table::new(rows, widths)
        .header(header)
        .block(
            block("Repositories", focused)
                .title_bottom(Line::from(repo_count).style(Style::default().fg(Color::DarkGray)))
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(table, area, &mut app.table_state);
}

fn draw_info_panel(frame: &mut Frame, app: &mut App, area: Rect) {
    let repo = match app.selected_repo() {
        Some(r) => r,
        None => return,
    };

    // Clone what we need to avoid borrow conflicts
    let repo_name = repo.name.clone();
    let branch = repo.current_branch.clone();
    let status = repo.status.clone();
    let path_str = repo.path.display().to_string();
    let branches = repo.branches.join(", ");
    let github_repo = repo.github_repo.clone();

    let label = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let value = Style::default().fg(Color::White);
    let dim = Style::default().fg(Color::DarkGray);
    let link_style = Style::default()
        .fg(Color::Blue)
        .add_modifier(Modifier::UNDERLINED);

    let mut lines: Vec<Line> = Vec::new();

    // Row 1: name + branch + status
    let mut row1 = vec![
        Span::styled(" ", label),
        Span::styled(repo_name.clone(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled("  ", dim),
        Span::styled(branch, value),
        Span::styled("  ", dim),
    ];
    match &status {
        RepoStatus::Clean => {
            row1.push(Span::styled("✓", Style::default().fg(Color::Green)));
        }
        RepoStatus::Dirty { modified, added, deleted } => {
            row1.push(Span::styled(format!("+{added}"), Style::default().fg(Color::Green)));
            row1.push(Span::raw(" "));
            row1.push(Span::styled(format!("~{modified}"), Style::default().fg(Color::Yellow)));
            row1.push(Span::raw(" "));
            row1.push(Span::styled(format!("-{deleted}"), Style::default().fg(Color::Red)));
        }
    }
    lines.push(Line::from(row1));

    // Row 2: path
    lines.push(Line::from(vec![
        Span::styled(" ", dim),
        Span::styled(path_str, dim),
    ]));

    // Row 3: branches + remote/github
    let mut row3: Vec<Span> = vec![
        Span::styled(" branches: ", dim),
        Span::styled(branches.clone(), dim),
    ];
    if let Some((owner, name)) = &github_repo {
        row3.push(Span::styled("  ", dim));
        row3.push(Span::styled(
            format!("↗ {owner}/{name}"),
            link_style,
        ));
    }
    lines.push(Line::from(row3));

    // Register click zone for the github link
    if let Some((owner, name)) = &github_repo {
        let github_text = format!("↗ {owner}/{name}");
        let branches_text = format!(" branches: {}  ", branches);
        let link_x = area.x + 1 + branches_text.len() as u16;
        let link_row = area.y + 3;
        app.click_zones.push((
            Rect::new(link_x, link_row, github_text.len() as u16, 1),
            format!("https://github.com/{owner}/{name}"),
        ));
    }

    let info = Paragraph::new(lines)
        .block(block(&repo_name, false));

    frame.render_widget(info, area);
}

fn draw_detail_pane(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.active_pane == ActivePane::Detail;

    let repo = match app.selected_repo() {
        Some(r) => r.clone(),
        None => {
            let empty = Paragraph::new(" Select a repository to view details")
                .style(Style::default().fg(Color::DarkGray))
                .block(block("Detail", focused));
            frame.render_widget(empty, area);
            return;
        }
    };

    let outer_block = block("", focused);
    let inner = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    if inner.height < 2 {
        return;
    }

    let [tab_area, content_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .areas(inner);

    app.tab_bar_area = tab_area;
    app.detail_content_area = content_area;

    draw_tab_bar(frame, app.detail_tab, tab_area);

    let detail_tab = app.detail_tab;
    let detail_scroll = app.detail_scroll;

    // Build lines + collect click zones for the content
    let (lines, zones) = match detail_tab {
        DetailTab::Changes => (tab_changes_lines(&repo), Vec::new()),
        DetailTab::Commits => tab_commits_content(&repo, content_area, detail_scroll),
        DetailTab::Issues => tab_issues_content(&repo, content_area, detail_scroll),
        DetailTab::Prs => tab_prs_content(&repo, content_area, detail_scroll),
    };

    app.click_zones.extend(zones);

    let content = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((detail_scroll, 0));

    frame.render_widget(content, content_area);
}

fn draw_tab_bar(frame: &mut Frame, active: DetailTab, area: Rect) {
    let tabs = [
        ("Changes", DetailTab::Changes),
        ("Commits", DetailTab::Commits),
        ("Issues", DetailTab::Issues),
        ("PRs", DetailTab::Prs),
    ];

    let active_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let inactive_style = Style::default().fg(Color::DarkGray);
    let sep_style = Style::default().fg(Color::DarkGray);

    let mut spans = Vec::new();
    spans.push(Span::raw(" "));
    for (i, (name, tab)) in tabs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" │ ", sep_style));
        }
        if *tab == active {
            spans.push(Span::styled(*name, active_style));
        } else {
            spans.push(Span::styled(*name, inactive_style));
        }
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn tab_changes_lines(repo: &crate::repo::RepoInfo) -> Vec<Line<'static>> {
    let dim = Style::default().fg(Color::DarkGray);

    let mut lines = Vec::new();
    lines.push(Line::from(""));

    if repo.changed_files.is_empty() {
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled("No changes", dim),
        ]));
        return lines;
    }

    for f in &repo.changed_files {
        let (prefix, rest) = f.split_at(1);
        let color = match prefix {
            "M" => Color::Yellow,
            "A" => Color::Green,
            "D" => Color::Red,
            _ => Color::White,
        };
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(prefix.to_string(), Style::default().fg(color)),
            Span::styled(rest.to_string(), dim),
        ]));
    }

    lines
}

fn tab_commits_content(
    repo: &crate::repo::RepoInfo,
    _area: Rect,
    _scroll: u16,
) -> (Vec<Line<'static>>, Vec<(Rect, String)>) {
    let dim = Style::default().fg(Color::DarkGray);
    let value = Style::default().fg(Color::White);

    let mut lines = Vec::new();
    lines.push(Line::from(""));

    if repo.recent_commits.is_empty() {
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled("No commits", dim),
        ]));
        return (lines, Vec::new());
    }

    for commit in &repo.recent_commits {
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(commit.hash.clone(), Style::default().fg(Color::Yellow)),
            Span::raw("  "),
            Span::styled(commit.message.clone(), value),
        ]));
        lines.push(Line::from(vec![
            Span::raw("          "),
            Span::styled(commit.author.clone(), dim),
            Span::raw("  "),
            Span::styled(commit.date.clone(), dim),
        ]));
        lines.push(Line::from(""));
    }

    (lines, Vec::new())
}

fn tab_issues_content(
    repo: &crate::repo::RepoInfo,
    area: Rect,
    scroll: u16,
) -> (Vec<Line<'static>>, Vec<(Rect, String)>) {
    let label = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let value = Style::default().fg(Color::White);
    let dim = Style::default().fg(Color::DarkGray);
    let clickable = Style::default().fg(Color::Green);

    let mut lines = Vec::new();
    let mut zones = Vec::new();

    lines.push(Line::from(""));

    let (owner, name) = match &repo.github_repo {
        Some(pair) => pair,
        None => {
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled("No GitHub remote", dim),
            ]));
            return (lines, zones);
        }
    };

    if let Some(data) = &repo.github_data {
        lines.push(Line::from(vec![
            Span::styled(format!(" Open Issues ({})", data.open_issues), label),
        ]));
        lines.push(Line::from(""));

        if data.recent_issues.is_empty() {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("None", dim),
            ]));
        } else {
            for issue in &data.recent_issues {
                let line_idx = lines.len();
                lines.push(Line::from(vec![
                    Span::styled(format!("  #{}", issue.number), clickable),
                    Span::raw(" "),
                    Span::styled(issue.title.clone(), value),
                ]));
                let visual_row = line_idx as i32 - scroll as i32;
                if visual_row >= 0 && (visual_row as u16) < area.height {
                    zones.push((
                        Rect::new(area.x, area.y + visual_row as u16, area.width, 1),
                        format!("https://github.com/{owner}/{name}/issues/{}", issue.number),
                    ));
                }
            }
        }

        lines.push(Line::from(""));
        let new_issue_idx = lines.len();
        lines.push(Line::from(vec![
            Span::styled("  + New Issue", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        ]));
        let visual_row = new_issue_idx as i32 - scroll as i32;
        if visual_row >= 0 && (visual_row as u16) < area.height {
            zones.push((
                Rect::new(area.x, area.y + visual_row as u16, area.width, 1),
                format!("https://github.com/{owner}/{name}/issues/new"),
            ));
        }
    } else if let Some(err) = &repo.github_error {
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(err.clone(), Style::default().fg(Color::Red)),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled("Loading...", dim),
        ]));
    }

    (lines, zones)
}

fn tab_prs_content(
    repo: &crate::repo::RepoInfo,
    area: Rect,
    scroll: u16,
) -> (Vec<Line<'static>>, Vec<(Rect, String)>) {
    let label = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let value = Style::default().fg(Color::White);
    let dim = Style::default().fg(Color::DarkGray);
    let clickable = Style::default().fg(Color::Magenta);

    let mut lines = Vec::new();
    let mut zones = Vec::new();

    lines.push(Line::from(""));

    let (owner, name) = match &repo.github_repo {
        Some(pair) => pair,
        None => {
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled("No GitHub remote", dim),
            ]));
            return (lines, zones);
        }
    };

    if let Some(data) = &repo.github_data {
        lines.push(Line::from(vec![
            Span::styled(format!(" Open PRs ({})", data.open_prs), label),
        ]));
        lines.push(Line::from(""));

        if data.recent_prs.is_empty() {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("None", dim),
            ]));
        } else {
            for pr in &data.recent_prs {
                let line_idx = lines.len();
                lines.push(Line::from(vec![
                    Span::styled(format!("  #{}", pr.number), clickable),
                    Span::raw(" "),
                    Span::styled(pr.title.clone(), value),
                ]));
                let visual_row = line_idx as i32 - scroll as i32;
                if visual_row >= 0 && (visual_row as u16) < area.height {
                    zones.push((
                        Rect::new(area.x, area.y + visual_row as u16, area.width, 1),
                        format!("https://github.com/{owner}/{name}/pull/{}", pr.number),
                    ));
                }
            }
        }
    } else if let Some(err) = &repo.github_error {
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(err.clone(), Style::default().fg(Color::Red)),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled("Loading...", dim),
        ]));
    }

    (lines, zones)
}
