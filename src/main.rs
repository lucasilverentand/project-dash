mod app;
mod github;
mod repo;
mod ui;

use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use tokio::sync::mpsc;

use app::{App, Message};

#[derive(Parser)]
#[command(name = "project-dash", about = "Git repository dashboard")]
struct Cli {
    /// Path to scan for git repositories
    #[arg(default_value = ".")]
    path: PathBuf,

    /// GitHub personal access token (or set GITHUB_TOKEN env var)
    #[arg(long = "github-token", env = "GITHUB_TOKEN")]
    github_token: Option<String>,
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let cli = Cli::parse();
    let scan_path = cli.path.canonicalize().unwrap_or(cli.path);

    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();

    let mut app = App::new(scan_path, cli.github_token, tx.clone());

    // Initial scan in a blocking task
    let scan_path = app.scan_path.clone();
    let scan_tx = tx.clone();
    tokio::spawn(async move {
        let repos =
            tokio::task::spawn_blocking(move || repo::scan_directory(&scan_path))
                .await
                .unwrap_or_default();
        let _ = scan_tx.send(Message::ReposScanned(repos));
    });

    // Set up panic hook to restore terminal
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture);
        let _ = ratatui::restore();
        original_hook(panic_info);
    }));

    let mut terminal = ratatui::init();
    crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture)?;

    // Spawn keyboard event reader
    let key_tx = tx.clone();
    tokio::spawn(async move {
        loop {
            // Poll for events with a timeout to allow the task to be cooperative
            let has_event = tokio::task::spawn_blocking(|| {
                event::poll(Duration::from_millis(100)).unwrap_or(false)
            })
            .await
            .unwrap_or(false);

            if has_event {
                let ev = tokio::task::spawn_blocking(event::read)
                    .await
                    .unwrap_or(Ok(Event::FocusLost));

                let msg = match ev {
                    Ok(Event::Key(key)) => {
                        if key.kind != KeyEventKind::Press {
                            continue;
                        }
                        match key.code {
                            KeyCode::Char('q') => Some(Message::Quit),
                            KeyCode::Up | KeyCode::Char('k') => Some(Message::MoveUp),
                            KeyCode::Down | KeyCode::Char('j') => Some(Message::MoveDown),
                            KeyCode::Char('r') => Some(Message::Refresh),
                            KeyCode::Char('R') => Some(Message::ForceRefresh),
                            KeyCode::Tab | KeyCode::Enter => Some(Message::SwitchPane),
                            KeyCode::Esc => Some(Message::FocusList),
                            KeyCode::Char(']') => Some(Message::NextTab),
                            KeyCode::Char('[') => Some(Message::PrevTab),
                            _ => None,
                        }
                    }
                    Ok(Event::Mouse(mouse)) => match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            Some(Message::Click { column: mouse.column, row: mouse.row })
                        }
                        _ => None,
                    },
                    _ => None,
                };

                if let Some(msg) = msg {
                    if key_tx.send(msg).is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Main event loop
    loop {
        terminal.draw(|frame| ui::draw(frame, &mut app))?;

        // Wait for messages with a tick timeout for periodic redraws
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(msg) => app.update(msg),
                    None => break,
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(250)) => {
                app.update(Message::Tick);
            }
        }

        if app.should_quit {
            break;
        }
    }

    crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture)?;
    ratatui::restore();
    Ok(())
}
