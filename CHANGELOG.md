# Changelog

Releases are tagged `vX.Y.Z` on `main`.

## 0.3.0

- **Any API key.** Paste a key from DeepSeek, OpenAI, Anthropic, Google Gemini or OpenRouter (or a
  TypeSafe key for Jev) and Work Desk works out which service it's for, checks it and picks a model.
  `desk key`, `desk model`, and **AI keys** on the page. Settings from 0.2 carry over.
- **Confirm before filing.** “Sort it for me” suggests a project and waits for you to confirm or change
  it (and the tidied text and due date) before saving.
- **Fit check.** A note that only shares a topic with a project goes to the Inbox with the reason,
  a suggested new project and the closest existing one, with buttons to create the project or file it
  anyway. With a Jev key, Jev's calibrated pick makes the project call.
- Notes sent to a project you chose yourself are tidied into a task too, with the date caught.
- **Jev in every structured decision.** With a TypeSafe key, Jev decides the project and whether a
  note fits any, spots notes that hold several to-dos (which become separate tasks) or repeat an open
  task, sizes every open task and judges whether it's waiting, vague or critical, and reads tasks before
  prompts are drafted. Work Desk plans the day from those judgments against your hours
  (`desk hours`), and the chat model writes with them in front of it.
- **Project work folders.** New projects get `~/Documents/Projects/Repos/<name>` with a standard
  research layout, set up as a git repo with a first commit. `desk new`.
- **Prompts for Claude.** Draft, edit and copy a Claude Code prompt from any task; open the project
  folder in VS Code. `desk prompt`.
- The app no longer says DeepSeek: it's Work Desk, using whichever AI you connect.

## 0.2.0

- Work Desk.app: the page in its own macOS window.
- Project pages, task details, and linked files and folders.

## 0.1.0

- Projects, kanban board, timeline and sticky notes, kept in one `workdesk.json` that can live in
  Google Drive. DeepSeek filing and a daily brief. The `desk` command.
