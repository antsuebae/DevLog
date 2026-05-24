# DevLog

A terminal tool that automates the daily workflow of a developer: tracks time per project, captures git commits, generates stand-up messages, and remembers where you left off.

Built in Rust.

---

## Features

- **Time tracking** — start and stop sessions tied to the current directory
- **Git integration** — automatically captures commits made during each session
- **Weekly report** — total time per project over the last 7 days
- **Stand-up generator** — formats yesterday/today/blockers ready to paste
- **Context saver** — snapshots your branch, modified files, and a note so you can pick up exactly where you left off
- **TUI dashboard** — interactive terminal UI with bar charts and session history
- **Claude Code skill** — maintains project context between AI sessions via `.devlog.json`

---

## Requirements

- Rust (install via [rustup](https://rustup.rs))
- Git (must be available in `$PATH`)
- Linux or macOS

---

## Installation

```bash
git clone https://github.com/antsuebae/DevLog
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

### Session history

```bash
devlog log
```

Shows all recorded sessions grouped by date, with ID, time, project, duration and commit count.

### Edit a session

```bash
devlog edit            # shows last session
devlog edit 3          # shows session #3
devlog edit 3 --end 18:30       # set end time (today, local)
devlog edit 3 --duration 90     # set duration in minutes
```

Useful when you forgot to stop a session and need to correct the recorded time.

### Rename a project

```bash
devlog rename old-name new-name
```

Renames a project across all recorded sessions. Useful when the project name changed or was recorded from the folder name instead of the repository name.

---

### Shell prompt integration (optional)

`devlog shell-init` prints a shell snippet that adds a `● devlog` indicator to your prompt when a session is active.

```bash
# bash (~/.bashrc)
eval "$(devlog shell-init)"

# zsh (~/.zshrc)
eval "$(devlog shell-init)"
```

> **Note:** This may not work with prompt frameworks like Starship or oh-my-posh, which manage the terminal title and prompt independently. In those cases, check session status with `devlog status`.

---

## Claude Code integration

DevLog includes a [Claude Code](https://claude.ai/code) skill that maintains project context between AI sessions. Instead of re-explaining the project at the start of every conversation, the skill reads and writes a `.devlog.json` file in the project root.

### Setup

Copy the skill to your Claude Code skills directory:

```bash
cp -r skills/devlog ~/.claude/skills/devlog
```

### Usage

At the start of any session, invoke the skill:

```
/devlog
```

Claude will read `.devlog.json` and present a summary of the last session: what was done, pending tasks, decisions, and blockers. At the end of the session it writes the updated context back to the file.

On first run in a project with no `.devlog.json`, the skill bootstraps the file automatically by reading the README, recent commits, and git status.

The `.devlog.json` file is meant to be committed — it works as shared context for any AI session on the project.

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
