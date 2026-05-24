use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Datelike, Duration, Local, Utc};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
};

// ── CLI ──────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(about = "Track your development time")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Start,
    Stop,
    Status,
    Report,
    Standup,
    Save {
        #[arg(value_name = "NOTE")]
        note: Option<String>,
    },
    Restore,
    Dashboard,
}

// ── Data Structures ──────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Commit {
    hash: String,
    message: String,
    branch: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Session {
    project: String,
    start_time: String,
    end_time: Option<String>,
    #[serde(default)]
    commits: Vec<Commit>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Context {
    saved_at: String,
    project: Option<String>,
    branch: String,
    modified_files: Vec<String>,
    note: Option<String>,
    recent_commands: Vec<String>,
}

// ── Storage ──────────────────────────────────────────────────────────────────

fn data_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap();
    PathBuf::from(home).join(".local/share/devlog")
}

fn current_path() -> PathBuf { data_dir().join("current.json") }
fn history_path() -> PathBuf { data_dir().join("history.json") }
fn context_path() -> PathBuf { data_dir().join("context.json") }

fn load_history() -> Vec<Session> {
    let path = history_path();
    if !path.exists() { return vec![]; }
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap_or_default()
}

fn save_history(history: &[Session]) {
    fs::write(history_path(), serde_json::to_string_pretty(history).unwrap()).unwrap();
}

fn session_minutes(session: &Session) -> i64 {
    if let Some(end) = &session.end_time {
        let start: DateTime<Utc> = session.start_time.parse().unwrap_or_else(|_| Utc::now());
        let end: DateTime<Utc> = end.parse().unwrap_or_else(|_| Utc::now());
        (end - start).num_minutes()
    } else {
        0
    }
}

// ── Git Helpers ───────────────────────────────────────────────────────────────

fn current_branch() -> String {
    Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

fn get_commits_since(since: &str) -> Vec<Commit> {
    let branch = current_branch();
    let output = Command::new("git")
        .args(["log", "--oneline", &format!("--since={}", since)])
        .output();

    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| {
                let mut parts = l.splitn(2, ' ');
                Commit {
                    hash: parts.next().unwrap_or("").to_string(),
                    message: parts.next().unwrap_or("").to_string(),
                    branch: branch.clone(),
                }
            })
            .collect(),
        _ => vec![],
    }
}

fn get_modified_files() -> Vec<String> {
    Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .map(|l| l.trim().to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn get_recent_commands(n: usize) -> Vec<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let bash_history = PathBuf::from(&home).join(".bash_history");
    let zsh_history = PathBuf::from(&home).join(".zsh_history");

    let source = if bash_history.exists() {
        fs::read_to_string(bash_history).unwrap_or_default()
    } else if zsh_history.exists() {
        fs::read_to_string(zsh_history).unwrap_or_default()
    } else {
        String::new()
    };

    let cmds: Vec<String> = source
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            if l.starts_with(": ") {
                l.splitn(2, ';').nth(1).unwrap_or(l).to_string()
            } else {
                l.to_string()
            }
        })
        .collect();

    let start = cmds.len().saturating_sub(n);
    cmds[start..].to_vec()
}

// ── TUI Dashboard ─────────────────────────────────────────────────────────────

fn ascii_bar(value: u64, max: u64, width: usize) -> String {
    if max == 0 { return " ".repeat(width); }
    let filled = ((value as f64 / max as f64) * width as f64).round() as usize;
    format!("{}{}", "█".repeat(filled.min(width)), " ".repeat(width.saturating_sub(filled)))
}

fn run_dashboard() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = dashboard_loop(&mut terminal);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn dashboard_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    loop {
        terminal.draw(draw_ui)?;

        if event::poll(std::time::Duration::from_secs(5))? {
            if let Event::Key(key) = event::read()? {
                match (key.code, key.modifiers) {
                    (KeyCode::Char('q'), _)
                    | (KeyCode::Char('Q'), _)
                    | (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

fn draw_ui(f: &mut Frame) {
    let area = f.area();

    let history = load_history();
    let active: Option<Session> = {
        let path = current_path();
        if path.exists() {
            fs::read_to_string(&path).ok().and_then(|c| serde_json::from_str(&c).ok())
        } else {
            None
        }
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Length(10),
            Constraint::Min(6),
            Constraint::Length(1),
        ])
        .split(area);

    // ── Active Session ────────────────────────────────────────────────────
    let active_lines = if let Some(ref s) = active {
        let start: DateTime<Utc> = s.start_time.parse().unwrap_or_else(|_| Utc::now());
        let elapsed = Utc::now() - start;
        let commits = get_commits_since(&s.start_time);
        vec![
            Line::from(vec![
                Span::styled("  Project : ", Style::default().fg(Color::Cyan)),
                Span::styled(s.project.clone(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("  Time    : ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("{}h {:02}min {:02}sec", elapsed.num_hours(), elapsed.num_minutes() % 60, elapsed.num_seconds() % 60),
                    Style::default().fg(Color::White),
                ),
            ]),
            Line::from(vec![
                Span::styled("  Commits : ", Style::default().fg(Color::Cyan)),
                Span::styled(commits.len().to_string(), Style::default().fg(Color::White)),
            ]),
        ]
    } else {
        vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No active session — run 'devlog start' to begin",
                Style::default().fg(Color::DarkGray),
            )),
        ]
    };

    f.render_widget(
        Paragraph::new(active_lines).block(
            Block::default()
                .title(Span::styled(" ● Active Session ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Blue)),
        ),
        chunks[0],
    );

    // ── Charts ────────────────────────────────────────────────────────────
    let chart_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    // Weekly time per project
    let week_ago = Utc::now() - Duration::days(7);
    let mut by_project: HashMap<String, u64> = HashMap::new();
    for s in &history {
        let start: DateTime<Utc> = s.start_time.parse().unwrap_or_else(|_| Utc::now());
        if start > week_ago && s.end_time.is_some() {
            *by_project.entry(s.project.clone()).or_insert(0) += session_minutes(s) as u64;
        }
    }
    let mut weekly: Vec<(String, u64)> = by_project.into_iter().collect();
    weekly.sort_by(|a, b| b.1.cmp(&a.1));
    let max_w = weekly.iter().map(|(_, m)| *m).max().unwrap_or(1);

    let weekly_lines: Vec<Line> = weekly.iter().take(7).map(|(name, mins)| {
        let bar = ascii_bar(*mins, max_w, 14);
        let name_trimmed = if name.len() > 10 { &name[..10] } else { name.as_str() };
        Line::from(Span::styled(
            format!("  {:<10} {} {:2}h{:02}m", name_trimmed, bar, mins / 60, mins % 60),
            Style::default().fg(Color::Cyan),
        ))
    }).collect();

    f.render_widget(
        Paragraph::new(weekly_lines).block(
            Block::default()
                .title(Span::styled(" ▇ Time This Week ", Style::default().fg(Color::Cyan)))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Blue)),
        ),
        chart_chunks[0],
    );

    // Daily activity last 7 days
    let today = Local::now().date_naive();
    let day_names = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    let daily: Vec<(String, u64)> = (0..7i64).map(|i| {
        let day = today - Duration::days(6 - i);
        let mins: i64 = history.iter()
            .filter(|s| {
                let start: DateTime<Utc> = s.start_time.parse().unwrap_or_else(|_| Utc::now());
                start.with_timezone(&Local).date_naive() == day && s.end_time.is_some()
            })
            .map(|s| session_minutes(s))
            .sum();
        let weekday = day.weekday().num_days_from_monday() as usize;
        (day_names[weekday % 7].to_string(), mins as u64)
    }).collect();

    let max_d = daily.iter().map(|(_, m)| *m).max().unwrap_or(1);
    let daily_lines: Vec<Line> = daily.iter().map(|(name, mins)| {
        let label = if *mins == 0 {
            format!("  {} ─", name)
        } else {
            let bar = ascii_bar(*mins, max_d, 14);
            format!("  {} {} {:2}h{:02}m", name, bar, mins / 60, mins % 60)
        };
        Line::from(Span::styled(label, Style::default().fg(Color::Blue)))
    }).collect();

    f.render_widget(
        Paragraph::new(daily_lines).block(
            Block::default()
                .title(Span::styled(" ▇ Daily Activity ", Style::default().fg(Color::Cyan)))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Blue)),
        ),
        chart_chunks[1],
    );

    // ── Recent Sessions ───────────────────────────────────────────────────
    let header = Row::new(["Date", "Project", "Duration", "Commits"])
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = history.iter().rev().take(8).map(|s| {
        let start: DateTime<Utc> = s.start_time.parse().unwrap_or_else(|_| Utc::now());
        let date = start.with_timezone(&Local).format("%m-%d %H:%M").to_string();
        let mins = session_minutes(s);
        Row::new([
            Cell::from(date).style(Style::default().fg(Color::DarkGray)),
            Cell::from(s.project.clone()).style(Style::default().fg(Color::White)),
            Cell::from(format!("{}h {:02}min", mins / 60, mins % 60)).style(Style::default().fg(Color::Cyan)),
            Cell::from(s.commits.len().to_string()).style(Style::default().fg(Color::White)),
        ])
    }).collect();

    f.render_widget(
        Table::new(
            rows,
            [Constraint::Length(12), Constraint::Min(14), Constraint::Length(10), Constraint::Length(8)],
        )
        .header(header)
        .block(
            Block::default()
                .title(Span::styled(" ≡ Recent Sessions ", Style::default().fg(Color::Cyan)))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Blue)),
        ),
        chunks[2],
    );

    // ── Status Bar ────────────────────────────────────────────────────────
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  [q] quit", Style::default().fg(Color::Cyan)),
            Span::raw("  │  Refreshing every 5s"),
        ])),
        chunks[3],
    );
}

// ── Main ──────────────────────────────────────────────────────────────────────

fn main() {
    fs::create_dir_all(data_dir()).unwrap();
    let cli = Cli::parse();

    match cli.command {
        Commands::Start => {
            let path = current_path();
            if path.exists() {
                let session: Session =
                    serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
                if session.end_time.is_none() {
                    println!("Session already active in '{}'. Use 'stop' first.", session.project);
                    return;
                }
            }

            let project = std::env::current_dir()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string();

            let session = Session {
                project: project.clone(),
                start_time: Utc::now().to_rfc3339(),
                end_time: None,
                commits: vec![],
            };

            fs::write(&path, serde_json::to_string_pretty(&session).unwrap()).unwrap();
            println!("Session started in '{}'.", project);
        }

        Commands::Stop => {
            let path = current_path();
            if !path.exists() {
                println!("No active session.");
                return;
            }

            let mut session: Session =
                serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();

            if session.end_time.is_some() {
                println!("Session already closed. Use 'start' to begin a new one.");
                return;
            }

            let start: DateTime<Utc> = session.start_time.parse().unwrap();
            let elapsed = Utc::now() - start;

            session.commits = get_commits_since(&session.start_time);
            session.end_time = Some(Utc::now().to_rfc3339());

            let mut history = load_history();
            history.push(session.clone());
            save_history(&history);
            fs::remove_file(&path).unwrap();

            println!(
                "Session closed in '{}' — {} min {} sec worked.",
                session.project,
                elapsed.num_minutes(),
                elapsed.num_seconds() % 60
            );

            if session.commits.is_empty() {
                println!("No commits during this session.");
            } else {
                println!("{} commit(s) this session:", session.commits.len());
                for c in &session.commits {
                    println!("  [{}] {} ({})", c.hash, c.message, c.branch);
                }
            }
        }

        Commands::Status => {
            let path = current_path();
            if !path.exists() {
                println!("No active session.");
                return;
            }

            let session: Session =
                serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();

            let start: DateTime<Utc> = session.start_time.parse().unwrap();
            let elapsed = Utc::now() - start;
            let commits_so_far = get_commits_since(&session.start_time);

            println!(
                "Active session in '{}' — {} min {} sec | {} commit(s) so far",
                session.project,
                elapsed.num_minutes(),
                elapsed.num_seconds() % 60,
                commits_so_far.len()
            );
        }

        Commands::Report => {
            let history = load_history();
            let week_ago = Utc::now() - Duration::days(7);

            let mut by_project: HashMap<String, i64> = HashMap::new();
            for s in &history {
                let start: DateTime<Utc> = s.start_time.parse().unwrap();
                if start > week_ago && s.end_time.is_some() {
                    *by_project.entry(s.project.clone()).or_insert(0) += session_minutes(s);
                }
            }

            if by_project.is_empty() {
                println!("No sessions recorded in the last 7 days.");
                return;
            }

            let mut entries: Vec<(String, i64)> = by_project.into_iter().collect();
            entries.sort_by(|a, b| b.1.cmp(&a.1));
            let total: i64 = entries.iter().map(|(_, m)| m).sum();

            println!("── Weekly Report ─────────────────────");
            for (project, mins) in &entries {
                println!("  {:<20} {}h {}min", project, mins / 60, mins % 60);
            }
            println!("─────────────────────────────────────");
            println!("  Total: {}h {}min", total / 60, total % 60);
        }

        Commands::Standup => {
            let history = load_history();
            let today = Local::now().date_naive();
            let yesterday = today - Duration::days(1);

            let sessions_on = |day| -> Vec<&Session> {
                history.iter()
                    .filter(|s| {
                        let start: DateTime<Utc> = s.start_time.parse().unwrap();
                        start.with_timezone(&Local).date_naive() == day
                    })
                    .collect()
            };

            let yesterday_sessions = sessions_on(yesterday);
            let today_sessions = sessions_on(today);

            println!("── Stand-up ──────────────────────────");
            print!("Yesterday: ");
            if yesterday_sessions.is_empty() {
                println!("No sessions recorded.");
            } else {
                println!();
                for s in &yesterday_sessions {
                    let mins = session_minutes(s);
                    println!("  - {} ({}h {}min, {} commit(s))", s.project, mins / 60, mins % 60, s.commits.len());
                    for c in &s.commits {
                        println!("      · {}", c.message);
                    }
                }
            }

            print!("Today:     ");
            if today_sessions.is_empty() {
                println!("Nothing started yet.");
            } else {
                println!();
                for s in &today_sessions {
                    println!("  - {} (in progress)", s.project);
                }
            }
            println!("Blockers:  None");
            println!("──────────────────────────────────────");
        }

        Commands::Save { note } => {
            let branch = current_branch();
            let modified = get_modified_files();
            let commands = get_recent_commands(10);

            let active_project: Option<String> = {
                let path = current_path();
                if path.exists() {
                    fs::read_to_string(&path)
                        .ok()
                        .and_then(|c| serde_json::from_str::<Session>(&c).ok())
                        .map(|s| s.project)
                } else {
                    None
                }
            };

            let context = Context {
                saved_at: Utc::now().to_rfc3339(),
                project: active_project,
                branch: branch.clone(),
                modified_files: modified.clone(),
                note: note.clone(),
                recent_commands: commands,
            };

            fs::write(context_path(), serde_json::to_string_pretty(&context).unwrap()).unwrap();

            println!("Context saved.");
            println!("  Branch : {}", branch);
            println!("  Files  : {} modified", modified.len());
            if let Some(n) = &note {
                println!("  Note   : {}", n);
            }
        }

        Commands::Restore => {
            let path = context_path();
            if !path.exists() {
                println!("No saved context found. Use 'devlog save' first.");
                return;
            }

            let ctx: Context =
                serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();

            let saved: DateTime<Utc> = ctx.saved_at.parse().unwrap();

            println!("── Saved Context ─────────────────────");
            println!("  Saved    : {}", saved.with_timezone(&Local).format("%Y-%m-%d %H:%M"));
            if let Some(p) = &ctx.project {
                println!("  Project  : {}", p);
            }
            println!("  Branch   : {}", ctx.branch);
            if !ctx.modified_files.is_empty() {
                println!("  Modified ({}):", ctx.modified_files.len());
                for file in &ctx.modified_files {
                    println!("    {}", file);
                }
            }
            if let Some(note) = &ctx.note {
                println!("  Note     : {}", note);
            }
            if !ctx.recent_commands.is_empty() {
                println!("  Last commands:");
                for cmd in ctx.recent_commands.iter().rev().take(5) {
                    println!("    $ {}", cmd);
                }
            }
            println!("──────────────────────────────────────");
        }

        Commands::Dashboard => {
            if let Err(e) = run_dashboard() {
                eprintln!("Dashboard error: {}", e);
            }
        }
    }
}
