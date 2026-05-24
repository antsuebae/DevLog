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
    /// Start a new tracking session in the current project
    Start,
    /// Stop the active session and record commits
    Stop,
    /// Show active session time and commits
    Status,
    /// Show a weekly report of time per project
    Report,
    /// Generate your daily stand-up message
    Standup,
    /// Save current work context (branch, files, optional note)
    Save {
        #[arg(value_name = "NOTE")]
        note: Option<String>,
    },
    /// Restore the last saved context
    Restore,
    /// Open the interactive TUI dashboard
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
    #[cfg(windows)]
    {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(appdata).join("devlog")
    }
    #[cfg(not(windows))]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".local/share/devlog")
    }
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

// ── Color Helpers ─────────────────────────────────────────────────────────────

fn cyan(s: &str) -> String   { format!("\x1b[36m{}\x1b[0m", s) }
fn bold(s: &str) -> String   { format!("\x1b[1m{}\x1b[0m", s) }
fn green(s: &str) -> String  { format!("\x1b[32m{}\x1b[0m", s) }
fn gray(s: &str) -> String   { format!("\x1b[90m{}\x1b[0m", s) }
fn yellow(s: &str) -> String { format!("\x1b[33m{}\x1b[0m", s) }

fn fmt_duration(mins: i64) -> String {
    if mins < 60 {
        format!("{}min", mins)
    } else {
        format!("{}h {}min", mins / 60, mins % 60)
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
    #[cfg(windows)]
    let source = {
        let appdata = std::env::var("APPDATA").unwrap_or_default();
        let ps_history = PathBuf::from(appdata)
            .join("Microsoft/Windows/PowerShell/PSReadLine/ConsoleHost_history.txt");
        if ps_history.exists() {
            fs::read_to_string(ps_history).unwrap_or_default()
        } else {
            String::new()
        }
    };

    #[cfg(not(windows))]
    let source = {
        let home = std::env::var("HOME").unwrap_or_default();
        let bash_history = PathBuf::from(&home).join(".bash_history");
        let zsh_history = PathBuf::from(&home).join(".zsh_history");
        if bash_history.exists() {
            fs::read_to_string(bash_history).unwrap_or_default()
        } else if zsh_history.exists() {
            fs::read_to_string(zsh_history).unwrap_or_default()
        } else {
            String::new()
        }
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
            println!("{} {} {}", green("●"), bold("Session started"), cyan(&project));
            println!("  {} {}", gray("branch:"), current_branch());
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

            let context = Context {
                saved_at: Utc::now().to_rfc3339(),
                project: Some(session.project.clone()),
                branch: current_branch(),
                modified_files: get_modified_files(),
                note: None,
                recent_commands: get_recent_commands(10),
            };
            fs::write(context_path(), serde_json::to_string_pretty(&context).unwrap()).unwrap();

            println!("{} {} {}", yellow("■"), bold("Session closed"), cyan(&session.project));
            println!("  {} {}   {} {}",
                gray("time:"), fmt_duration(elapsed.num_minutes()),
                gray("branch:"), session.commits.first().map(|c| c.branch.as_str()).unwrap_or(&current_branch()),
            );

            if session.commits.is_empty() {
                println!("  {}", gray("no commits this session"));
            } else {
                println!("  {} {}", gray("commits:"), session.commits.len());
                for c in &session.commits {
                    println!("    {} {} {}", gray(&c.hash), c.message, gray(&format!("({})", c.branch)));
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

            println!("{} {} {}", green("●"), bold("Active:"), cyan(&session.project));
            println!("  {} {}h {:02}min {:02}sec   {} {}   {} {}",
                gray("time:"),
                elapsed.num_hours(), elapsed.num_minutes() % 60, elapsed.num_seconds() % 60,
                gray("branch:"), current_branch(),
                gray("commits:"), commits_so_far.len(),
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

            println!("{}", bold("── Weekly Report ─────────────────────"));
            for (project, mins) in &entries {
                println!("  {:<20} {}", cyan(project), fmt_duration(*mins));
            }
            println!("─────────────────────────────────────");
            println!("  {:<20} {}", bold("Total"), bold(&fmt_duration(total)));
        }

        Commands::Standup => {
            let history = load_history();
            let today = Local::now().date_naive();
            let yesterday = today - Duration::days(1);

            let closed_on = |day| -> Vec<&Session> {
                history.iter()
                    .filter(|s| {
                        let start: DateTime<Utc> = s.start_time.parse().unwrap();
                        start.with_timezone(&Local).date_naive() == day && s.end_time.is_some()
                    })
                    .collect()
            };

            let yesterday_sessions = closed_on(yesterday);
            let today_sessions = closed_on(today);

            let active: Option<Session> = {
                let path = current_path();
                if path.exists() {
                    fs::read_to_string(&path).ok().and_then(|c| serde_json::from_str(&c).ok())
                } else {
                    None
                }
            };

            println!("{}", bold("── Stand-up ──────────────────────────"));

            print!("{} ", cyan("Yesterday:"));
            if yesterday_sessions.is_empty() {
                println!("{}", gray("no sessions recorded"));
            } else {
                println!();
                for s in &yesterday_sessions {
                    let mins = session_minutes(s);
                    println!("  {} {} — {}", gray("▸"), bold(&s.project), fmt_duration(mins));
                    for c in &s.commits {
                        println!("      {} {}", gray("·"), c.message);
                    }
                }
            }

            print!("{} ", cyan("Today:    "));
            if today_sessions.is_empty() && active.is_none() {
                println!("{}", gray("nothing started yet"));
            } else {
                println!();
                for s in &today_sessions {
                    let mins = session_minutes(s);
                    println!("  {} {} — {}", gray("▸"), bold(&s.project), fmt_duration(mins));
                }
                if let Some(ref a) = active {
                    let start: DateTime<Utc> = a.start_time.parse().unwrap();
                    let elapsed = Utc::now() - start;
                    println!("  {} {} {} {}", green("●"), bold(&a.project), gray("(active)"), fmt_duration(elapsed.num_minutes()));
                }
            }

            println!("{} {}", cyan("Blockers: "), gray("none"));
            println!("{}", bold("──────────────────────────────────────"));
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

            println!("{} {}", green("✔"), bold("Context saved"));
            println!("  {} {}   {} {}", gray("branch:"), cyan(&branch), gray("files:"), modified.len());
            if let Some(n) = &note {
                println!("  {} {}", gray("note:"), n);
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

            println!("{}", bold("── Saved Context ─────────────────────"));
            println!("  {} {}", gray("saved:"), cyan(&saved.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string()));
            if let Some(p) = &ctx.project {
                println!("  {} {}", gray("project:"), bold(p));
            }
            println!("  {} {}", gray("branch:"), cyan(&ctx.branch));
            if !ctx.modified_files.is_empty() {
                println!("  {} {}", gray("modified:"), ctx.modified_files.len());
                for file in &ctx.modified_files {
                    println!("    {} {}", gray("▸"), file);
                }
            }
            if let Some(note) = &ctx.note {
                println!("  {} {}", gray("note:"), yellow(note));
            }
            if !ctx.recent_commands.is_empty() {
                println!("  {}", gray("last commands:"));
                for cmd in ctx.recent_commands.iter().rev().take(5) {
                    println!("    {} {}", gray("$"), cmd);
                }
            }
            println!("{}", bold("──────────────────────────────────────"));
        }

        Commands::Dashboard => {
            if let Err(e) = run_dashboard() {
                eprintln!("Dashboard error: {}", e);
            }
        }
    }
}
