//! Creating projects. A new project gets a short id made from its name and, unless
//! asked not to, its own work folder: the standard research layout, set up as a git
//! repo with a first commit, under `projects_root` (~/Documents/Projects/Repos).

use crate::config::{tilde, Config};
use crate::logic;
use crate::store::Store;
use crate::util::now_iso;
use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};
use std::path::Path;
use std::process::Command;

pub struct Created {
    pub id: String,
    pub doc: Map<String, Value>,
    /// What happened with the folder, for the person who asked.
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

/// Add a project. `make_folder`: create its work folder (see the module comment).
/// `move_items`: notes to move into it, e.g. the Inbox notes that suggested it.
pub fn create(store: &Store, cfg: &Config, mut doc: Map<String, Value>, make_folder: bool, move_items: &[String]) -> Result<Created> {
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

    let mut notice = None;
    if make_folder && logic::s(&doc, "folder").is_none() {
        let dir = cfg.projects_root().join(if base.is_empty() { &id } else { &base });
        let shown = tilde(&dir);
        if dir.exists() {
            if dir.is_dir() {
                doc.insert("folder".into(), json!(shown));
                notice = Some(format!("Linked the folder that was already at {shown}. Nothing in it was changed."));
            } else {
                notice = Some(format!("Didn't create a folder: there's a file at {shown}."));
            }
        } else {
            match scaffold(&dir, &name, &doc, &id) {
                Ok(git) => {
                    doc.insert("folder".into(), json!(shown));
                    notice = Some(match git {
                        Ok(()) => format!("Created {shown} as a git repo."),
                        Err(e) => format!("Created {shown}, but couldn't set up git: {e}"),
                    });
                }
                Err(e) => notice = Some(format!("Couldn't create {shown}: {e:#}")),
            }
        }
    }

    store.put("projects", &id, &doc)?;
    let mut patch = Map::new();
    patch.insert("project".into(), json!(id));
    patch.insert("triage".into(), Value::Null);
    patch.insert("updated".into(), json!(now));
    for item in move_items {
        store.merge("items", item, &patch)?;
    }
    Ok(Created { id, doc, notice })
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

const LAYOUT: &[(&str, &str)] = &[
    ("data/", "Raw inputs. Not committed; list where each file came from in data/README.md."),
    ("src/", "Reusable code."),
    ("scripts/", "One-off analyses that use `src/`."),
    ("notebooks/", "Exploratory notebooks (Jupyter, Pluto, R Markdown)."),
    ("output/", "Generated results. Not committed: anything here can be made again from the code."),
    ("display_items/", "Figures and tables for papers and talks."),
    ("drafts/", "Manuscripts, IND sections and reports."),
    ("notes/", "Meeting notes."),
];

/// Write the standard layout into `dir` (which must not exist yet), then make it a git repo.
/// The outer error is for the folder; the inner one is for git, which is allowed to fail.
fn scaffold(dir: &Path, name: &str, doc: &Map<String, Value>, id: &str) -> Result<Result<()>> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let goal = logic::s(doc, "goal");
    let context = logic::s(doc, "context");
    let today = &now_iso()[..10];

    let mut readme = format!("# {name}\n\n{}\n", goal.unwrap_or("_Goal: not written yet._"));
    if let Some(c) = context {
        readme.push_str(&format!("\n## People and context\n\n{c}\n"));
    }
    readme.push_str("\n## Layout\n\n| Folder | What goes there |\n|---|---|\n");
    for (folder, what) in LAYOUT {
        readme.push_str(&format!("| `{folder}` | {what} |\n"));
    }
    readme.push_str(&format!("\nStarted {today}. Tracked in Work Desk as `{id}`.\n"));

    let mut claude = format!("# {name}\n\n");
    if let Some(g) = goal {
        claude.push_str(&format!("Goal: {g}\n\n"));
    }
    if let Some(c) = context {
        claude.push_str(&format!("Context: {c}\n\n"));
    }
    claude.push_str(
        "- Raw data lives in `data/` and is not committed. Never change it in place; write results to `output/`.\n\
         - Reusable code goes in `src/`; scripts in `scripts/` and notebooks in `notebooks/` call it.\n\
         - Figures and tables for papers and talks go in `display_items/`; drafts in `drafts/`.\n",
    );

    let files = [
        ("README.md", readme),
        ("CLAUDE.md", claude),
        (".gitignore", GITIGNORE.to_string()),
        ("data/README.md", "# Data\n\nRaw inputs. Not committed to git. For each file, note where it came from and when.\n".to_string()),
    ];
    for (folder, _) in LAYOUT {
        std::fs::create_dir_all(dir.join(folder))?;
    }
    for (path, text) in files {
        std::fs::write(dir.join(path), text)?;
    }
    for folder in ["src", "scripts", "notebooks", "output", "display_items", "drafts", "notes"] {
        std::fs::write(dir.join(folder).join(".gitkeep"), "")?;
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

    #[test]
    fn slugs() {
        assert_eq!(slug("CHARM IND"), "charm-ind");
        assert_eq!(slug("  Natural History Harmonization & Paper "), "natural-history-harmonization-paper");
        assert_eq!(slug("ΔΔ"), "");
    }

    #[test]
    fn creates_project_folder_and_moves_notes() {
        let d = std::env::temp_dir().join(format!("workdesk-test-proj-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let store = Store::open(&d.join("workdesk.json"), None).unwrap();
        store.put("projects", "charm-ind", &json!({"name": "Old"}).as_object().unwrap().clone()).unwrap();
        store.put("items", "t1", &json!({"text": "x", "project": "inbox", "triage": {"reason": "r"}}).as_object().unwrap().clone()).unwrap();
        let mut cfg = Config::default();
        cfg.projects_root = Some(d.join("Repos").display().to_string());
        let doc = json!({"name": "CHARM IND", "goal": "File the IND"}).as_object().unwrap().clone();

        let c = create(&store, &cfg, doc.clone(), true, &["t1".into()]).unwrap();
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
        let c = create(&store, &cfg, doc, true, &[]).unwrap();
        assert_eq!(c.id, "charm-ind-3");
        assert!(c.notice.unwrap().starts_with("Linked"));
        assert_eq!(std::fs::read_to_string(dir.join("notes/mine.md")).unwrap(), "keep");
        let _ = std::fs::remove_dir_all(&d);
    }
}
