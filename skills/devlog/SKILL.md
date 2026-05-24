---
name: devlog
description: Manages persistent project context between AI sessions using .devlog.json. Reads the file at session start to load project state, tracks progress during the session, and writes updated context when the session ends. Use at the start of any coding session on a project that uses DevLog.
---

# DevLog Context Manager

This skill maintains project state between AI sessions via `.devlog.json` in the project root. It eliminates the need to re-explain context at the start of every conversation.

## At session start

1. Look for `.devlog.json` in the current directory or git root.
2. If the file exists:
   - Read it and present a structured summary: last session summary, pending tasks (numbered), blockers, and key decisions.
   - Check the current git branch (`git branch --show-current`). If it differs from the saved branch, warn: *"El contexto fue guardado en `<branch>`, estás en `<current>`."*
   - Ask the user if they want to continue with the pending tasks or take a different direction.
3. If the file does not exist, bootstrap it from the project:
   - Inform the user this is the first tracked session and that you will derive the initial context from the project.
   - Run the following to gather information:
     - `ls` — detect tech stack from config files (Cargo.toml → Rust, package.json → Node, pyproject.toml → Python, etc.)
     - Read `README.md` if it exists — extract project purpose and main features
     - `git log --oneline -20` — understand what has been built recently
     - `git status` — detect work in progress
     - `git remote get-url origin` — extract the repo name from the remote URL (e.g. `github.com/user/my-repo` → `my-repo`); fall back to folder name if no remote exists
     - `pwd` — get the absolute path of the project root
   - Generate `.devlog.json` with:
     - `project` — repo name from remote URL (not the folder name)
     - `summary` derived from README and recent commits — what the project is and where it stands
     - `decisions` — leave empty, these cannot be inferred without session history
     - `pending` — derive from git status and any obvious next steps visible in the README or recent commits
     - `notes` — include the absolute path to the project root on the first line, then any other key facts not obvious from reading the code
   - Write the file and tell the user what was inferred so they can correct anything before the session begins.

## During the session

Track internally — no need to mention it:
- Which `pending` tasks get completed
- Decisions made, including the reasoning behind them
- Approaches tried and discarded, and why
- New tasks or blockers that emerge
- Notes that would be useful to a future session

Do NOT track:
- Code that is already written — it is in the repo
- Commit history — it is in git
- File paths or lists — git status covers this
- Implementation details Claude can derive by reading the code

## Detecting session end

Save the updated context when:
1. The user uses closing language: "gracias", "hasta luego", "terminamos", "lo dejamos aquí", "eso es todo", "hasta mañana", "done", "thanks", "bye", "that's it".
2. After completing a significant block of work, ask once: *"¿Guardo el contexto en DevLog?"* — do not ask repeatedly within the same session.

If the user declines or says not yet, respect it and do not ask again for that session.

## Writing the updated context

Update `.devlog.json` with the following fields:

- `updated_at` — current timestamp in ISO 8601
- `branch` — current git branch from `git branch --show-current`
- `summary` — 2–3 sentences describing what was accomplished this session. Focus on outcomes, not process.
- `decisions` — add new decisions made this session. Keep existing ones that are still relevant. Remove ones that are no longer applicable. Always include `why`.
- `discarded` — add approaches tried and abandoned this session, with the reason.
- `pending` — remove completed tasks. Add new ones discovered. Be specific: *"implement login endpoint"* not *"continue with auth"*.
- `blockers` — update with current open blockers. Remove resolved ones.
- `notes` — anything relevant that does not fit elsewhere.

After saving, remind the user: *"Contexto guardado en `.devlog.json` — recuerda hacer commit si quieres compartirlo con el equipo."*

## File schema

```json
{
  "project": "<repo name from git remote, or folder name if no remote>",
  "branch": "<current git branch>",
  "updated_at": "<ISO 8601 timestamp>",
  "summary": "<2-3 sentences about what was done last session>",
  "decisions": [
    { "what": "<decision taken>", "why": "<reasoning>" }
  ],
  "discarded": [
    { "what": "<approach>", "why": "<reason it was discarded>" }
  ],
  "pending": [
    "<specific actionable task>",
    "<specific actionable task>"
  ],
  "blockers": [],
  "notes": ""
}
```

## Quality rules

- Every `pending` item must be actionable. If it is vague, make it specific before saving.
- Every `decisions` entry must have a `why`. Without reasoning it has no value.
- If the file exceeds ~60 lines, prune stale decisions and completed context before saving.
- The file must be useful to a Claude session that has never seen this codebase — write for that reader.
- **NEVER include credentials, passwords, API keys, connection strings, or sensitive file paths in any field.** If a project uses a password-protected file or service, note only that credentials are required and where to find them (e.g. "Ver gestor de contraseñas") — never the credential itself.
