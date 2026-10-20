//! Projects: creating them, their work folders, and the framework that gives each one a
//! finish line. Every project has a kind (paper, presentation, report, …); the kind decides
//! the folder layout and the usual stages its milestones follow. A work folder is set up as
//! a git repo with a first commit, under `projects_root` (~/Documents/Projects/Repos).

use crate::ai::NewTask;
use crate::config::{tilde, Config};
use crate::logic;
use crate::store::Store;
use crate::util::now_iso;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Map, Value};
use std::path::Path;
use std::process::Command;

/// A kind of project: what it's for, how its folder is laid out, and its usual stages.
pub struct Kind {
    pub id: &'static str,
    pub label: &'static str,
    /// For Jev's options and the chat model.
    pub about: &'static str,
    pub folders: &'static [(&'static str, &'static str)],
    pub stages: &'static str,
}

const DATA: (&str, &str) = ("data/", "Raw inputs. Not committed; list where each file came from in data/README.md.");
const SRC: (&str, &str) = ("src/", "Reusable code.");
const SCRIPTS: (&str, &str) = ("scripts/", "One-off analyses that use `src/`.");
const NOTEBOOKS: (&str, &str) = ("notebooks/", "Exploratory notebooks (Jupyter, Pluto, R Markdown).");
const OUTPUT: (&str, &str) = ("output/", "Generated results. Not committed: anything here can be made again from the code.");
const DISPLAY: (&str, &str) = ("display_items/", "Figures and tables.");
const NOTES: (&str, &str) = ("notes/", "Meeting notes, decisions and feedback.");
const REFS: (&str, &str) = ("references/", "Papers, citations and examples to follow.");

pub const KINDS: &[Kind] = &[
    Kind {
        id: "paper",
        label: "Research paper",
        about: "A research paper or manuscript: analyses, figures and a written paper to submit or post as a preprint",
        folders: &[DATA, SRC, SCRIPTS, NOTEBOOKS, OUTPUT, DISPLAY, ("drafts/", "The manuscript and its versions."), REFS, NOTES],
        stages: "pick the target journal and outline the story; finish the analyses; make the figures and tables; draft \
                 methods and results; draft the introduction and discussion; co-author review; submit or post the preprint",
    },
    Kind {
        id: "analysis",
        label: "Analysis or pipeline",
        about: "Code, a data analysis or a pipeline whose result is findings or a tool rather than (yet) a written paper",
        folders: &[DATA, SRC, SCRIPTS, NOTEBOOKS, OUTPUT, DISPLAY, NOTES],
        stages: "pin down the question and the data; get the data in shape; build the analysis; check it with sanity \
                 checks and tests; summarize the findings; document it or hand it off",
    },
    Kind {
        id: "presentation",
        label: "Presentation",
        about: "A talk, slide deck or poster for a lab meeting, conference, committee or collaborators",
        folders: &[
            ("slides/", "The deck or poster and its versions."),
            ("figures/", "Figures and images for the slides."),
            DATA,
            ("scripts/", "Code that makes the figures."),
            ("notes/", "Speaker notes and feedback."),
        ],
        stages: "settle the audience, length and main message; storyboard the slides; make the figures; build the \
                 slides; rehearse and get feedback; give it",
    },
    Kind {
        id: "report",
        label: "Report or regulatory document",
        about: "A written report, white paper, draft document or regulatory section such as an IND module, usually \
                following a template or required format",
        folders: &[
            ("drafts/", "The document and its versions."),
            ("templates/", "Required formats and examples to follow."),
            DATA,
            ("scripts/", "Code for the tables and figures."),
            DISPLAY,
            REFS,
            NOTES,
        ],
        stages: "confirm the audience, template and required sections; outline; gather the data, tables and figures; \
                 write the first full draft; review with stakeholders; finalize and submit",
    },
    Kind {
        id: "grant",
        label: "Grant or fellowship",
        about: "A grant, fellowship or other funding application",
        folders: &[
            ("drafts/", "Aims, research strategy and the other sections."),
            ("budget/", "Budget and justification."),
            ("letters/", "Letters of support and biosketches."),
            REFS,
            NOTES,
        ],
        stages: "read the call and confirm eligibility and the deadline; draft the specific aims; get feedback on the \
                 aims; write the research strategy; budget, biosketches and letters; internal review; submit",
    },
    Kind {
        id: "task",
        label: "Simple task",
        about: "A small piece of work or an errand: a few steps, with no files or analysis to keep",
        folders: &[("notes/", "Notes.")],
        stages: "one to three concrete steps to get it done",
    },
    Kind {
        id: "other",
        label: "Other",
        about: "Any other kind of project",
        folders: &[DATA, SRC, SCRIPTS, NOTEBOOKS, OUTPUT, DISPLAY, ("drafts/", "Manuscripts, reports and other writing."), NOTES],
        stages: "the few concrete steps from where it stands now to the deliverable",
    },
];

/// The project's kind; "other" (the general research layout) when it has none.
pub fn kind_of(id: Option<&str>) -> &'static Kind {
    let id = id.unwrap_or("other");
    KINDS.iter().find(|k| k.id == id).unwrap_or(&KINDS[KINDS.len() - 1])
}

/// The kinds, for the page.
pub fn kinds_json() -> Value {
    Value::Array(
        KINDS
            .iter()
            .map(|k| json!({ "id": k.id, "label": k.label, "folders": k.folders.iter().map(|f| f.0).collect::<Vec<_>>() }))
            .collect(),
    )
}

pub struct Created {
    pub id: String,
    pub doc: Map<String, Value>,
    /// What happened with the folder and milestones, for the person who asked.
    pub notice: Option<String>,
}

/// "CHARM IND" → "charm-ind". Empty if the name has no letters or digits.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').chars().take(40).collect::<String>().trim_end_matches('-').to_string()
}

/// Milestones as sent by the page: `[{text, due}]`.
pub fn read_milestones(v: Option<&Value>) -> Vec<NewTask> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|m| {
                    let text = m.get("text").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty())?.to_string();
                    let due = m.get("due").and_then(Value::as_str).filter(|d| crate::util::parse_date(d).is_some()).map(|d| d[..10].to_string());
                    Some(NewTask { text, due })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn add_milestones(store: &Store, pid: &str, milestones: &[NewTask]) -> Result<usize> {
    let now = now_iso();
    for m in milestones {
        let mut d = Map::new();
        for (k, v) in [("text", json!(m.text)), ("project", json!(pid)), ("status", json!("open")), ("stage", json!("todo")), ("kind", json!("task"))] {
            d.insert(k.into(), v);
        }
        if let Some(due) = &m.due {
            d.insert("due".into(), json!(due));
        }
        d.insert("milestone".into(), json!(true));
        d.insert("created".into(), json!(now));
        d.insert("updated".into(), json!(now));
        store.put("items", &store.new_id("t"), &d)?;
    }
    Ok(milestones.len())
}

fn milestones_note(n: usize) -> String {
    format!("Added {n} milestone task{}.", if n == 1 { "" } else { "s" })
}

/// Make (or link) a project's work folder, laid out for its kind. Sets `folder` on `doc` when
/// there is one afterwards, and says what happened.
fn make_folder(cfg: &Config, doc: &mut Map<String, Value>, id: &str) -> String {
    let name = logic::s(doc, "name").unwrap_or(id).to_string();
    let base = slug(&name);
    let dir = cfg.projects_root().join(if base.is_empty() { id } else { &base });
    let shown = tilde(&dir);
    if dir.exists() {
        if !dir.is_dir() {
            return format!("Didn't create a folder: there's a file at {shown}.");
        }
        doc.insert("folder".into(), json!(shown));
        return format!("Linked the folder that was already at {shown}. Nothing in it was changed.");
    }
    match scaffold(&dir, &name, doc, id) {
        Ok(git) => {
            doc.insert("folder".into(), json!(shown));
            match git {
                Ok(()) => format!("Created {shown} as a git repo."),
                Err(e) => format!("Created {shown}, but couldn't set up git: {e}"),
            }
        }
        Err(e) => format!("Couldn't create {shown}: {e:#}"),
    }
}

/// Add a project. `want_folder`: create its work folder. `move_items`: notes to move into it,
/// e.g. the Inbox notes that suggested it. `milestones`: tasks to start it with.
pub fn create(
    store: &Store,
    cfg: &Config,
    mut doc: Map<String, Value>,
    want_folder: bool,
    move_items: &[String],
    milestones: &[NewTask],
) -> Result<Created> {
    let name = logic::s(&doc, "name").map(str::trim).unwrap_or("").to_string();
    if name.is_empty() {
        bail!("The project needs a name.");
    }
    let projects = store.list("projects")?;
    let taken = |id: &str| matches!(id, "inbox" | "auto" | "all") || projects.iter().any(|(pid, _)| pid == id);
    let base = slug(&name);
    let id = if base.is_empty() {
        store.new_id("p")
    } else {
        (1..).map(|n| if n == 1 { base.clone() } else { format!("{base}-{n}") }).find(|id| !taken(id)).unwrap()
    };

    let now = now_iso();
    doc.remove("id");
    doc.insert("name".into(), json!(name));
    for (k, v) in [("status", "active"), ("tier", "standard")] {
        if logic::s(&doc, k).is_none() {
            doc.insert(k.into(), json!(v));
        }
    }
    if doc.get("order").and_then(Value::as_f64).is_none() {
        let last = projects.iter().filter_map(|(_, p)| p.get("order").and_then(Value::as_f64)).fold(0.0, f64::max);
        doc.insert("order".into(), json!(last as i64 + 1));
    }
    doc.insert("created".into(), json!(now));
    doc.insert("updated".into(), json!(now));

    let mut notes = Vec::new();
    if want_folder && logic::s(&doc, "folder").is_none() {
        notes.push(make_folder(cfg, &mut doc, &id));
    }
    store.put("projects", &id, &doc)?;
    let mut patch = Map::new();
    patch.insert("project".into(), json!(id));
    patch.insert("triage".into(), Value::Null);
    patch.insert("updated".into(), json!(now));
    for item in move_items {
        store.merge("items", item, &patch)?;
    }
    if add_milestones(store, &id, milestones)? > 0 {
        notes.push(milestones_note(milestones.len()));
    }
    Ok(Created { id, doc, notice: (!notes.is_empty()).then(|| notes.join(" ")) })
}

/// Give an existing project its work folder, laid out for its kind.
pub fn add_folder(store: &Store, cfg: &Config, id: &str) -> Result<Created> {
    let mut doc = store.get("projects", id)?.ok_or_else(|| anyhow!("There's no project with id {id}."))?;
    if let Some(f) = logic::s(&doc, "folder") {
        bail!("This project already has a folder: {f}");
    }
    let notice = make_folder(cfg, &mut doc, id);
    if let Some(f) = doc.get("folder").cloned() {
        let mut patch = Map::new();
        patch.insert("folder".into(), f);
        patch.insert("updated".into(), json!(now_iso()));
        store.merge("projects", id, &patch)?;
    }
    Ok(Created { id: id.to_string(), doc, notice: Some(notice) })
}

/// Apply a framework the person confirmed to an existing project: its kind, deliverable,
/// deadline and overview (whichever of those `fields` fills in), milestone tasks, and a folder.
pub fn apply_framework(store: &Store, cfg: &Config, id: &str, fields: &Map<String, Value>, milestones: &[NewTask], want_folder: bool) -> Result<Created> {
    let mut doc = store.get("projects", id)?.ok_or_else(|| anyhow!("There's no project with id {id}."))?;
    let mut patch = Map::new();
    for k in ["type", "role", "deliverable", "deadline", "summary"] {
        if let Some(v) = fields.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()) {
            patch.insert(k.into(), json!(v));
            doc.insert(k.into(), json!(v));
        }
    }
    let mut notes = Vec::new();
    if want_folder && logic::s(&doc, "folder").is_none() {
        notes.push(make_folder(cfg, &mut doc, id));
        if let Some(f) = doc.get("folder") {
            patch.insert("folder".into(), f.clone());
        }
    }
    patch.insert("updated".into(), json!(now_iso()));
    store.merge("projects", id, &patch)?;
    if add_milestones(store, id, milestones)? > 0 {
        notes.push(milestones_note(milestones.len()));
    }
    Ok(Created { id: id.to_string(), doc, notice: (!notes.is_empty()).then(|| notes.join(" ")) })
}

const GITIGNORE: &str = "\
.DS_Store
.venv/
__pycache__/
.ipynb_checkpoints/
.Rhistory
.Rproj.user/

# Raw data and generated results stay on this computer
data/*
!data/README.md
output/*
!output/.gitkeep
";

/// Write the project's layout into `dir` (which must not exist yet), then make it a git repo.
/// The outer error is for the folder; the inner one is for git, which is allowed to fail.
fn scaffold(dir: &Path, name: &str, doc: &Map<String, Value>, id: &str) -> Result<Result<()>> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let kind = kind_of(logic::s(doc, "type"));
    let has = |f: &str| kind.folders.iter().any(|(d, _)| *d == f);
    let goal = logic::s(doc, "goal");
    let deliverable = logic::s(doc, "deliverable");
    let context = logic::s(doc, "context");
    let today = &now_iso()[..10];

    let mut readme = format!("# {name}\n\n{}\n", goal.unwrap_or("_Goal: not written yet._"));
    readme.push_str(&format!("\n**Kind:** {}  \n**Deliverable:** {}\n", kind.label, deliverable.unwrap_or("_not set yet_")));
    if let Some(c) = context {
        readme.push_str(&format!("\n## People and context\n\n{c}\n"));
    }
    readme.push_str("\n## Layout\n\n| Folder | What goes there |\n|---|---|\n");
    for (folder, what) in kind.folders {
        readme.push_str(&format!("| `{folder}` | {what} |\n"));
    }
    readme.push_str(&format!("\nStarted {today}. Tracked in Work Desk as `{id}`.\n"));

    let mut claude = format!("# {name}\n\n");
    for (label, v) in [("Goal", goal), ("Deliverable", deliverable), ("Context", context)] {
        if let Some(v) = v {
            claude.push_str(&format!("{label}: {v}\n\n"));
        }
    }
    claude.push_str(&format!("This is a {} project. Folders:\n", kind.label.to_lowercase()));
    for (folder, what) in kind.folders {
        claude.push_str(&format!("- `{folder}`: {what}\n"));
    }
    if has("data/") {
        claude.push_str("\nNever change raw data in `data/` in place; write results somewhere else.\n");
    }

    for (folder, _) in kind.folders {
        std::fs::create_dir_all(dir.join(folder))?;
        if *folder != "data/" {
            std::fs::write(dir.join(folder).join(".gitkeep"), "")?;
        }
    }
    std::fs::write(dir.join("README.md"), readme)?;
    std::fs::write(dir.join("CLAUDE.md"), claude)?;
    std::fs::write(dir.join(".gitignore"), GITIGNORE)?;
    if has("data/") {
        std::fs::write(dir.join("data/README.md"), "# Data\n\nRaw inputs. Not committed to git. For each file, note where it came from and when.\n")?;
    }
    Ok(git_init(dir, name))
}

fn git_init(dir: &Path, name: &str) -> Result<()> {
    let run = |args: &[&str]| -> Result<()> {
        let out = Command::new("git").args(args).current_dir(dir).output().context("git isn't installed")?;
        if !out.status.success() {
            bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(())
    };
    run(&["init", "-q", "-b", "main"])?;
    run(&["add", "-A"])?;
    run(&["commit", "-q", "-m", &format!("Start {name}")])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("workdesk-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("CHARM IND"), "charm-ind");
        assert_eq!(slug("  Natural History Harmonization & Paper "), "natural-history-harmonization-paper");
        assert_eq!(slug("ΔΔ"), "");
    }

    #[test]
    fn creates_project_folder_and_moves_notes() {
        let d = tmp("proj");
        let store = Store::open(&d.join("workdesk.json"), None).unwrap();
        store.put("projects", "charm-ind", &json!({"name": "Old"}).as_object().unwrap().clone()).unwrap();
        store.put("items", "t1", &json!({"text": "x", "project": "inbox", "triage": {"reason": "r"}}).as_object().unwrap().clone()).unwrap();
        let mut cfg = Config::default();
        cfg.projects_root = Some(d.join("Repos").display().to_string());
        let doc = json!({"name": "CHARM IND", "goal": "File the IND"}).as_object().unwrap().clone();

        let c = create(&store, &cfg, doc.clone(), true, &["t1".into()], &[]).unwrap();
        assert_eq!(c.id, "charm-ind-2");
        let dir = d.join("Repos/charm-ind");
        for f in ["README.md", "CLAUDE.md", ".gitignore", "data/README.md", "scripts/.gitkeep", "notebooks/.gitkeep", "display_items/.gitkeep", "notes/.gitkeep"] {
            assert!(dir.join(f).exists(), "{f}");
        }
        assert!(std::fs::read_to_string(dir.join("README.md")).unwrap().contains("File the IND"));
        assert_eq!(c.doc["folder"], json!(tilde(&dir)));
        if Command::new("git").arg("--version").output().is_ok() {
            assert!(dir.join(".git").is_dir(), "{:?}", c.notice);
            let log = Command::new("git").args(["log", "--format=%s"]).current_dir(&dir).output().unwrap();
            assert_eq!(String::from_utf8_lossy(&log.stdout).trim(), "Start CHARM IND");
        }
        let item = store.get("items", "t1").unwrap().unwrap();
        assert_eq!(item["project"], "charm-ind-2");
        assert!(item.get("triage").is_none());

        // an existing folder is linked, never written to
        std::fs::write(dir.join("notes/mine.md"), "keep").unwrap();
        let c = create(&store, &cfg, doc, true, &[], &[]).unwrap();
        assert_eq!(c.id, "charm-ind-3");
        assert!(c.notice.unwrap().starts_with("Linked"));
        assert_eq!(std::fs::read_to_string(dir.join("notes/mine.md")).unwrap(), "keep");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn frameworks_shape_the_folder_and_add_milestones() {
        let d = tmp("framework");
        let store = Store::open(&d.join("workdesk.json"), None).unwrap();
        store.put("projects", "talk", &json!({"name": "Lab meeting talk"}).as_object().unwrap().clone()).unwrap();
        let mut cfg = Config::default();
        cfg.projects_root = Some(d.join("Repos").display().to_string());

        let fields = json!({"type": "presentation", "deliverable": "20-minute lab meeting talk", "deadline": "2026-11-12", "summary": ""});
        let ms = read_milestones(Some(&json!([{"text": "Storyboard the slides", "due": "2026-11-03"}, {"text": " "}, {"text": "Rehearse", "due": "bad"}])));
        assert_eq!(ms.len(), 2);
        let c = apply_framework(&store, &cfg, "talk", fields.as_object().unwrap(), &ms, true).unwrap();
        let p = store.get("projects", "talk").unwrap().unwrap();
        assert_eq!((p["type"].as_str(), p["deliverable"].as_str(), p["deadline"].as_str()), (Some("presentation"), Some("20-minute lab meeting talk"), Some("2026-11-12")));
        assert!(p.get("summary").is_none(), "blank fields are left alone");
        let dir = d.join("Repos/lab-meeting-talk");
        assert!(dir.join("slides/.gitkeep").exists() && dir.join("figures").is_dir() && !dir.join("notebooks").exists());
        assert!(std::fs::read_to_string(dir.join("README.md")).unwrap().contains("**Deliverable:** 20-minute lab meeting talk"));
        let tasks: Vec<_> = store.list("items").unwrap().into_iter().filter(|(_, m)| m["project"] == "talk").collect();
        assert_eq!(tasks.len(), 2);
        assert!(tasks.iter().any(|(_, m)| m["text"] == "Storyboard the slides" && m["due"] == "2026-11-03"));
        assert!(c.notice.unwrap().contains("Added 2 milestone tasks."));

        // and a folder for a project that has none, on its own
        store.put("projects", "mri", &json!({"name": "Brain MRI"}).as_object().unwrap().clone()).unwrap();
        add_folder(&store, &cfg, "mri").unwrap();
        assert!(d.join("Repos/brain-mri/notebooks").is_dir());
        assert!(store.get("projects", "mri").unwrap().unwrap()["folder"].as_str().unwrap().ends_with("brain-mri"));
        assert!(add_folder(&store, &cfg, "mri").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
