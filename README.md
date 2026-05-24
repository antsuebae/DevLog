# DevLog

A terminal tool that automates the daily workflow of a developer: tracks time per project, captures git commits, generates stand-up messages, and remembers where you left off.

Built in Rust as a learning project.

---

## Features

- **Time tracking** — start and stop sessions tied to the current directory
- **Git integration** — automatically captures commits made during each session
- **Weekly report** — total time per project over the last 7 days
- **Stand-up generator** — formats yesterday/today/blockers ready to paste
- **Context saver** — snapshots your branch, modified files, and a note so you can pick up exactly where you left off
- **TUI dashboard** — interactive terminal UI with bar charts and session history

---

## Requirements

- Rust (install via [rustup](https://rustup.rs))
- Git (must be available in `$PATH`)
- Linux or macOS

---

## Installation

```bash
git clone https://github.com/antsuebae/devlog
cd devlog
cargo install --path .
```

The `devlog` binary will be placed in `~/.cargo/bin/`. Make sure that directory is in your `$PATH`.

---

## Usage

### Start a session

```bash
devlog start
```

Begins tracking time for the current directory. The project name is taken from the folder name.

### Stop a session

```bash
devlog stop
```

Closes the active session, records elapsed time and any commits made since `start`, and auto-saves the current context.

### Check status

```bash
devlog status
```

Shows how long the active session has been running, the current branch, and commits so far.

### Weekly report

```bash
devlog report
```

Displays total time worked per project over the last 7 days, sorted by most time first.

### Stand-up

```bash
devlog standup
```

Generates a ready-to-paste stand-up message with yesterday's sessions, today's activity, and a blockers line.

### Save context

```bash
devlog save
devlog save "finishing the auth refactor"
```

Saves a snapshot of your current branch, modified files, recent shell commands, and an optional note.

### Restore context

```bash
devlog restore
```

Displays the last saved snapshot so you can quickly remember where you left off.

### Dashboard

```bash
devlog dashboard
```

Opens an interactive TUI showing the active session, time per project this week, daily activity bars, and recent session history. Press `q` to exit.

---

## Data storage

All data is stored locally in `~/.local/share/devlog/`:

| File            | Contents                        |
|-----------------|---------------------------------|
| `current.json`  | Active session (if any)         |
| `history.json`  | All completed sessions          |
| `context.json`  | Last saved context snapshot     |

---

## Platform

| OS      | Status | Notes |
|---------|--------|-------|
| Linux   | Full support | Reads bash/zsh history |
| macOS   | Full support | Reads bash/zsh history |
| Windows | Full support | Data stored in `%APPDATA%\devlog`, reads PowerShell history. Requires Windows 10+ for color output |
