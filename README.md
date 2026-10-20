# Work Desk

Your projects, kanban board, timeline and sticky notes as a Mac app (**Work Desk.app**) and a `desk`
command. Work Desk files your notes into the right project, tells you when a note doesn't fit any of them,
writes a start-of-day brief, drafts prompts you can hand to Claude Code, and gives every new project its
own git-tracked work folder.

Open it as its own window, or in your browser with `desk`. Either way it runs only on your computer.
Everything — every project and task — is kept in one readable file, `workdesk.json`, which you can keep
in Google Drive (see below). Nothing else leaves your Mac except the text sent to the AI service whose
key you add, when Work Desk files a note or writes a brief.

## Install (once)

```bash
cd workdesk
./build.sh
```

If Rust isn't installed, the script tells you the one command to install it.
It installs the `desk` command and puts **Work Desk.app** in your Applications folder (`~/Applications`).
Then:

```bash
desk key <your API key>   # or paste it under "AI keys" on the page
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
- Apps opened from the Dock don't see variables set in your shell, so the app ignores `WORKDESK_API_KEY`,
  `TYPESAFE_API_KEY` and `WORKDESK_FILE`. Use `desk key` and `desk drive` / `desk use-file`, which save to
  your settings.

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
- Your API keys are **not** in the synced file. Each Mac keeps its own keys in
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
| `desk add "tox plots for Sonia by Friday"` | Files a sticky note; Work Desk picks the project and the due date (no confirm step in the terminal) |
| `desk add -p nh "check MRC 5 crossing code"` | Files it under a project yourself (an id or part of a name) |
| `desk today` | What's overdue, due soon, stalled and next, in the terminal |
| `desk brief` | A plan for today |
| `desk sort` | Files everything waiting in the Inbox |
| `desk projects` | Lists projects and their ids |
| `desk new "CHARM IND" -g "IND package"` | Adds a project and creates its work folder (`--no-folder` to skip) |
| `desk key <key>` | Saves an API key; Work Desk works out which service it's for |
| `desk prompt "power sweep"` | Drafts a prompt for Claude Code about a task, prints it and copies it |
| `desk drive` | Moves your data into Google Drive and uses it from there |
| `desk use-file "<path>"` | Uses a data file in any other folder |
| `desk merge "<file>"` | Folds in another copy (e.g. a Drive conflict copy), newest edits win |
| `desk export backup.json` / `desk import backup.json` | Backups |
| `desk where` | Shows the data file, backups, settings, project folder and AI in use |

On the page:
- **Desk**: Start of day (Brief me), Needs attention, Today, Next up, the sticky note, and your projects.
  Use **+ New project** and **Edit** to set goals and deadlines.
- **Board**: To do / In progress / Review / Done across all projects, with project filters.
- **Timeline**: Gantt bars, deadlines, and **Draft schedule**.
- **Project pages**: click a project name on the Desk to open its own page — overview, its four lanes,
  notes, linked files and what's coming up. Each page has its own address (`#/p/<project id>`).
- **Task details**: click any card, or the **›** button on a Desk row, for notes, dates, stage and files.
- **Finishing a task**: **Mark done** in task details, the ✓ on a Board card, a Desk checkbox, or dragging
  to Done. Confetti bursts from where you clicked, cannons fire from both corners, a short chime plays,
  and you get a count of today's wins. Clearing a project, or 5 or 10 in a day, gets a bigger show.
  **Sound on/off** is at the bottom of the page; reduced motion (System Settings) gets a small burst.

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

## Filing notes

“Sort it for me” in any note box sends the note to the AI. It tidies the wording, catches dates like
“by Friday”, and decides whether the note clearly belongs to one of your projects. Sharing a topic isn't
enough: a note about CHARM's off-target analysis doesn't belong to RNAseq just because RNAseq also does
off-target work.

Nothing is saved until you confirm. A small window shows the tidied task, the suggested project and the
due date; change any of them, then press Enter (**File under …**) or **Cancel** to go back to the note.
When no project clearly fits, it says why, picks the Inbox, and can start the project it suggests.

Choosing a project yourself skips the question, but the note is still tidied into a task and its date
caught. Your original wording is kept with the task either way.

Notes waiting in the **Inbox** show the reason, and two buttons:

- **Create “CHARM IND”**: opens the new-project form with the suggested name. Saving it creates the
  project (and its folder) and moves every waiting note with the same suggestion into it.
- **File under RNAseq**: when there's a close match, files it there anyway.

**Sort inbox** re-checks everything waiting there. With no key yet, notes simply wait in the Inbox.

### Jev (optional)

Add a [TypeSafe](https://typesafe.ai) key as well and Jev makes every decision that can be put as a
question with set answers. Jev doesn't write text: it answers with calibrated probabilities, code acts on
them, and the chat model writes with Jev's answers in front of it.

| When | Jev decides | What happens with it |
|---|---|---|
| Filing a note | Is it about one of your projects? Which one? Does it hold several to-dos? | Below 50% on either project question, it's suggested for the Inbox. Several to-dos: the chat model splits it into tasks. |
| Filing a note | Does it repeat an open task in that project? | The confirm window warns you. |
| The brief | For each open task: how big is it, is it waiting on someone, how much does it matter to the goal, is it too vague to start? | Work Desk adds up the work due by each deadline against your hours, picks today's tasks (least room to spare first), quick wins, and what's stuck; the chat model writes the brief from that plan. |
| Drafting a prompt | What kind of work is it? Is it too vague? Which linked files does it need? How far along is it, judging from the project folder? Which scripts and notebooks there are part of it? | The prompt is shaped for that kind of work, starts by agreeing what done looks like if it's vague, names only the files that matter, and focuses on the scripts that belong to the task. |

Task judgments are kept on each task (`jev` in the data file) and only redone when the task's wording,
notes or project change, so the first brief takes a few seconds longer and later ones don't. Task details
show Jev's size estimate, and flag tasks that look stuck. The bars are constants in `src/jev.rs` and
`src/logic.rs`; they're starting points, so check them against how your own tasks come out. A Jev key on
its own files notes but can't write the brief or prompts.

Set your hours under **AI keys** (or `desk hours 5`). The plan assumes 4 hours of project work on a
weekday unless you change it.

## Prompts for Claude

Open any task and click **Draft prompt** under **Prompt for Claude**. Work Desk writes a prompt for Claude
Code from the task, its notes and linked files, the project's goal, overview and other open tasks, and
**what's already in the project folder**: the scripts in `scripts/` and `notebooks/` (newest first, with the
top of each file, and whether a notebook has been run), recent commits, and uncommitted changes. So the
prompt points Claude at the work to build on and says what's left, instead of starting over. It covers the
goal, the context Claude needs, what to do, how to check the work, and what to report back. Anything it
doesn't know is left as a `[placeholder]` for you to fill in.

- Edit it in the box; your edits are saved on the task.
- **Copy**, then **Open <folder> in VS Code** and paste it into Claude Code there.
- When the folder changes after the prompt was written (a script saved, a new commit), the task says
  what changed. **Update** revises the prompt for the progress, keeping your edits; **Start over** writes
  a new one. `desk prompt` updates too (`--fresh` to start over).
- To read other folders as well (say `analysis/` or `code/`), set `progress_dirs` in config.json, e.g.
  `"progress_dirs": ["scripts", "notebooks", "analysis"]`.
- For writing tasks (paper sections, IND narrative), the prompt asks Claude to start on an outline with
  you rather than draft the whole document.
- To open folders in another editor, set `editor` in config.json to its app name (e.g. `"Cursor"`).

## New projects get a work folder

**+ New project**, `desk new`, and **Create “…”** in the Inbox all create
`~/Documents/Projects/Repos/<project-name>/`, make it a git repo with a first commit, and link it as the
project folder:

```
charm-ind/
├── README.md        name, goal and people, from the project
├── CLAUDE.md        the same context, for Claude Code
├── .gitignore       keeps data/ and output/ out of git
├── data/            raw inputs (not committed)
├── src/             reusable code
├── scripts/         one-off analyses
├── notebooks/       exploratory notebooks
├── output/          generated results (not committed)
├── display_items/   figures and tables
├── drafts/          manuscripts, IND sections, reports
└── notes/           meeting notes
```

If a folder with that name already exists, Work Desk links it and leaves its contents alone. Untick
**Create a project folder** (or use `desk new --no-folder`) for projects that don't need one.

## Settings

- **API keys**: click **AI keys** at the top of the page. Paste a chat model key in the first box; Work
  Desk works out which service it's for (DeepSeek, OpenAI, Anthropic, Google Gemini or OpenRouter), checks
  it works, and picks a sensible model. Paste a TypeSafe key in the **Jev** box. From the terminal:
  `desk key <key>` (or `desk key --provider jev <key>`); `desk key --forget chat` or `--forget jev`
  removes one.
- **Model**: `desk model <name>` uses a different model from the same service.
- **Another OpenAI-compatible service**: set `base_url` and `model` in
  `~/Library/Application Support/WorkDesk/config.json`, then `desk key --provider custom <key>`.
- **Project folders**: set `projects_root` in the same file to create them somewhere other than
  `~/Documents/Projects/Repos`.
- **Environment**: `WORKDESK_API_KEY` and `TYPESAFE_API_KEY` override the saved keys (terminal only).
- **Port**: `desk open --port 8080`.

## Notes

- Releases are tagged `v0.1.0`, `v0.2.0`, … on `main`; `CHANGELOG.md` says what changed. New work happens
  on a branch and is merged when it's done.
- This copy is separate from the Work Desk page on claude.ai. Changes here don't show up there.
- The browser version's server only answers on `localhost` and rejects requests from other websites.
  The desktop app doesn't open a port at all: its window talks to Work Desk directly.
- To change the app icon, edit and rerun `macos/make_icon.py` (needs Pillow).
- `ui/index.html` is generated from `ui/source.html` (the claude.ai page) by `ui/build_ui.py`, which
  also splices in `ui/project.js`, `ui/celebrate.js` and `ui/project.css` (project pages, task details,
  linked files, and the confetti). Edit those files, not `index.html`.
