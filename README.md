# Work Desk

Your projects, kanban board, timeline and sticky notes as a Mac app (**Work Desk.app**) and a `desk`
command. DeepSeek files your notes into the right project and writes a start-of-day brief.

Open it as its own window, or in your browser with `desk`. Either way it runs only on your computer.
Everything — every project and task — is kept in one readable file, `workdesk.json`, which you can keep
in Google Drive (see below). Nothing else leaves your Mac except the text sent to DeepSeek when it files
a note or writes a brief.

## Install (once)

```bash
cd workdesk
./build.sh
```

If Rust isn't installed, the script tells you the one command to install it.
It installs the `desk` command and puts **Work Desk.app** in your Applications folder (`~/Applications`).
Then:

```bash
desk key sk-your-deepseek-key   # or paste it into the page
```

and open **Work Desk** from Spotlight, Launchpad or Finder. Drag it to the Dock to keep it there.
To use your browser instead, run `desk`.

It starts with your 7 projects and 15 Natural History tasks, copied from the Work Desk on claude.ai
on Oct 13.

## The desktop app

Work Desk.app is the same page in its own window, with the usual Mac menus: ⌘C/⌘V/⌘Z in the note
boxes, ⌘R to reload, ⌘+ / ⌘− / ⌘0 to zoom, ⌘W or ⌘Q to close. It uses the same data file, key and
settings as `desk`. The app and a browser tab can be open at the same time: changes made in one appear
in the other within a few seconds.

- `desk app` opens the same window from the terminal.
- After a `git pull`, run `./build.sh` again to update both the app and the `desk` command.
- If the app can't read your data file, it says why in a dialog and quits instead of starting empty.
- Apps opened from the Dock don't see variables set in your shell, so the app ignores `DEEPSEEK_API_KEY`
  and `WORKDESK_FILE`. Use `desk key` and `desk drive` / `desk use-file`, which save to your settings.

## Keep it in Google Drive

```bash
desk drive
```

This finds your Google Drive folder (Google Drive for desktop must be installed and signed in), copies your
data to `My Drive/WorkDesk/workdesk.json`, and uses that file from then on. On a second Mac, install
Work Desk and run `desk drive` there too: it picks up the same file instead of starting fresh.

- To use another synced folder (Dropbox, iCloud, a lab share): `desk use-file "<folder or file>"`.
- Edits from another computer appear on the open page within a few seconds of Drive syncing them.
- Each save writes a temporary file and then swaps it in, so Drive never uploads a half-written file.
  If Drive ever delivers a broken copy, Work Desk keeps showing the last good version and saves a copy of
  the broken file next to it before replacing it.
- If two computers save at nearly the same moment, Drive may create a conflict copy such as
  `workdesk (1).json`. Fold it back in with `desk merge "<path to the copy>"`. For each task and project,
  the most recently changed version is kept.
- Your DeepSeek key is **not** in the synced file. Each Mac keeps its own key in
  `~/Library/Application Support/WorkDesk/config.json`.
- A dated backup of the data is saved locally once a day in `~/Library/Application Support/WorkDesk/backups/`.
- `desk where` shows which file is in use.

### The file

```json
{
  "format": "workdesk/1",
  "saved_at": "2026-10-13T17:02:00Z",
  "saved_by": "your-mac",
  "projects": {
    "nh-paper": { "name": "Natural History Harmonization & Paper", "deadline": "2026-10-31",
                  "goal": "...", "tier": "major", "status": "active", "order": 1 }
  },
  "items": {
    "nh-01": { "text": "Calculate certainty of when ...", "project": "nh-paper",
               "stage": "todo", "status": "open", "start": "2026-10-20", "due": "2026-10-24",
               "created": "...", "updated": "..." }
  }
}
```

Keys are sorted, so the file is easy to read and compare. You can edit it by hand while Work Desk is
running; it notices the change.

## Everyday use

| Command | What it does |
|---|---|
| `desk` | Opens the Work Desk in your browser (leave the terminal window open) |
| `desk app` | Opens it in its own window, like Work Desk.app |
| `desk add "tox plots for Sonia by Friday"` | Files a sticky note; DeepSeek picks the project and the due date |
| `desk add -p nh "check MRC 5 crossing code"` | Files it under a project yourself (an id or part of a name) |
| `desk today` | What's overdue, due soon, stalled and next, in the terminal |
| `desk brief` | DeepSeek's plan for today |
| `desk sort` | Files everything waiting in the Inbox |
| `desk projects` | Lists projects and their ids |
| `desk drive` | Moves your data into Google Drive and uses it from there |
| `desk use-file "<path>"` | Uses a data file in any other folder |
| `desk merge "<file>"` | Folds in another copy (e.g. a Drive conflict copy), newest edits win |
| `desk export backup.json` / `desk import backup.json` | Backups |
| `desk where` | Shows the data file, backups and settings locations |

On the page:
- **Desk**: Start of day (Brief me), Needs attention, Today, Next up, the sticky note, and your projects.
  Use **+ New project** and **Edit** to set goals and deadlines.
- **Board**: To do / In progress / Review / Done across all projects, with project filters.
- **Timeline**: Gantt bars, deadlines, and **Draft schedule**.
- **Project pages**: click a project name on the Desk to open its own page — overview, its four lanes,
  notes, linked files and what's coming up. Each page has its own address (`#/p/<project id>`).
- **Task details**: click any card, or the **›** button on a Desk row, for notes, dates, stage and files.

## Linking files

Tasks and projects can point at the real work: scripts, drafts, figures, data folders and web links.

- **Link files…** / **Link folder…** opens a Mac file chooser; or paste a path or URL into the box.
- In the desktop app you can also **drag files from Finder** onto a task card, onto a project page, or
  onto an open task.
- Clicking a linked file opens it in whatever app usually opens it; **Show** reveals it in Finder.
- Paths under your home folder are stored as `~/…`, so they still work on another Mac with the same
  folders. A file that isn't on this computer is shown struck through and marked, rather than silently
  doing nothing.
- A project can also have one **project folder**, shown in Details.
- Unlinking removes the link only. Work Desk never moves, changes or deletes the file itself.
- For safety, Work Desk will only open a file that something in your data actually links to.

“Sort it for me” in any note box sends the note to DeepSeek. If it can't tell where a note goes, or there's
no key yet, the note waits in the Inbox.

## Settings

- Key: `desk key …`, or set `DEEPSEEK_API_KEY`, which takes precedence.
- Model: defaults to `deepseek-chat`. To change it, edit `model` in
  `~/Library/Application Support/WorkDesk/config.json`.
- Port: `desk open --port 8080`.

## Notes

- This copy is separate from the Work Desk page on claude.ai. Changes here don't show up there.
- The browser version's server only answers on `localhost` and rejects requests from other websites.
  The desktop app doesn't open a port at all: its window talks to Work Desk directly.
- To change the app icon, edit and rerun `macos/make_icon.py` (needs Pillow).
- `ui/index.html` is generated from `ui/source.html` (the claude.ai page) by `ui/build_ui.py`, which
  also splices in `ui/project.js` and `ui/project.css` (project pages, task details and linked files).
  Edit those three files, not `index.html`.
