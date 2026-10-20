//! Closing out a project: confirm the deliverable and where the final version lives, decide
//! what happens to each open task, keep a wrap-up record, write CLOSEOUT.md into the project
//! folder (committed and tagged on its own, never sweeping in other changes), and mark the
//! project done.

use crate::ai::{self, Ai};
use crate::config::expand_home;
use crate::jev;
use crate::logic::{self, s};
use crate::progress;
use crate::store::{Doc, Store};
use crate::util::now_iso;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Map, Value};
use std::path::Path;
use std::process::Command;

/// What to do with one open task when its project closes.
#[derive(Debug, PartialEq)]
pub enum Fate {
    Done,
    Move(String),
    Drop,
}

impl Fate {
    fn read(action: &str, to: Option<&str>) -> Option<Fate> {
        match (action, to) {
            ("done", _) => Some(Fate::Done),
            ("drop", _) => Some(Fate::Drop),
            ("move", Some(to)) if !to.is_empty() => Some(Fate::Move(to.to_string())),
            _ => None,
        }
    }
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().map_err(|_| anyhow!("git isn't installed"))?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn is_repo(dir: &Path) -> bool {
    git(dir, &["rev-parse", "--is-inside-work-tree"]).map_or(false, |o| o == "true")
}

fn folder_of(p: &Map<String, Value>) -> Option<std::path::PathBuf> {
    s(p, "folder").map(expand_home).filter(|d| d.is_dir())
}

/// Everything the close-out window needs: open tasks with a suggested fate, the folder's git
/// state, and a drafted wrap-up.
pub async fn plan(store: &Store, ai: Option<&Ai>, id: &str, dirs: &[String]) -> Result<Value> {
    let p = store.get("projects", id)?.ok_or_else(|| anyhow!("There's no project with id {id}."))?;
    let projects = store.list("projects")?;
    let items = store.list("items")?;
    let mine: Vec<&Doc> = items.iter().filter(|(_, m)| s(m, "project") == Some(id)).collect();
    let open: Vec<&Doc> = mine.iter().copied().filter(|(_, m)| logic::is_open(m)).collect();
    let done = mine.len() - open.len();
    let others: Vec<(String, String)> = logic::ordered(&projects)
        .into_iter()
        .filter(|(pid, q)| pid != id && s(q, "status") != Some("done"))
        .map(|(pid, q)| (pid.clone(), s(&q, "name").unwrap_or(&pid).to_string()))
        .collect();

    // Loose ends: by default they wait in the Inbox; Jev suggests better when there's a key.
    let mut fates: Vec<Value> = open
        .iter()
        .map(|(tid, m)| json!({ "id": tid, "text": s(m, "text"), "action": "move", "to": "inbox", "why": null }))
        .collect();
    let mut notes = Vec::new();
    if let (Some(j), false) = (ai.and_then(|a| a.jev.as_ref()), open.is_empty()) {
        let texts: Vec<String> = open.iter().map(|(_, m)| s(m, "text").unwrap_or("").to_string()).collect();
        let state = json!({ "project": logic::project_facts(id, &p, &[]), "open_tasks": texts, "other_projects": others.iter().map(|(i, n)| json!({"id": i, "name": n})).collect::<Vec<_>>() });
        match j.loose_ends(state, texts.len(), &others).await {
            Ok(ends) => {
                for (f, (still, home)) in fates.iter_mut().zip(ends) {
                    match (still, home) {
                        (Some(p), _) if p < jev::YES => {
                            f["action"] = json!("drop");
                            f["why"] = json!(format!("Jev: probably not needed now ({:.0}% still worth doing)", p * 100.0));
                        }
                        (_, Some(h)) if h.choice != "inbox" && h.confidence >= jev::MIN_CONFIDENCE => {
                            let name = others.iter().find(|(i, _)| *i == h.choice).map_or(h.choice.clone(), |(_, n)| n.clone());
                            f["to"] = json!(h.choice);
                            f["why"] = json!(format!("Jev: belongs with {name}"));
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => notes.push(format!("Jev couldn't suggest what to do with the open tasks: {e}")),
        }
    }

    // The folder: is it a repo, and what isn't committed (it won't be swept into the close-out commit).
    let folder = folder_of(&p);
    let repo = folder.as_deref().map_or(false, is_repo);
    let uncommitted: Vec<String> = match (&folder, repo) {
        (Some(d), true) => git(d, &["status", "--short", "--untracked-files=normal"]).map(|o| o.lines().map(str::to_string).collect()).unwrap_or_default(),
        _ => Vec::new(),
    };

    let summary = match ai.filter(|a| a.chat.is_some()) {
        Some(a) => {
            let prog = folder.as_deref().and_then(|d| progress::scan(d, dirs));
            match ai::closeout_summary(a, &p, &mine, prog.as_ref()).await {
                Ok(t) => Some(t),
                Err(e) => {
                    notes.push(format!("Couldn't draft the wrap-up: {e}"));
                    None
                }
            }
        }
        None => None,
    };

    Ok(json!({
        "deliverable": s(&p, "deliverable"),
        "done": done,
        "open": fates,
        "others": others.iter().map(|(i, n)| json!({"id": i, "name": n})).collect::<Vec<_>>(),
        "folder": s(&p, "folder"),
        "repo": repo,
        "uncommitted": uncommitted.iter().take(8).collect::<Vec<_>>(),
        "uncommitted_count": uncommitted.len(),
        "tag": folder.as_deref().filter(|_| repo).map(free_tag),
        "summary": summary,
        "notes": notes,
    }))
}

/// `closeout-<today>`, or `-2`, `-3`… if that tag is taken.
fn free_tag(dir: &Path) -> String {
    let base = format!("closeout-{}", &now_iso()[..10]);
    let taken = |t: &str| git(dir, &["rev-parse", "-q", "--verify", &format!("refs/tags/{t}")]).is_ok();
    (1..).map(|n| if n == 1 { base.clone() } else { format!("{base}-{n}") }).find(|t| !taken(t)).unwrap()
}

/// What the person confirmed in the close-out window.
pub struct Confirmed {
    pub deliverable: Option<String>,
    /// The final version: a path (stored as `~/…`) or a web link.
    pub link: Option<String>,
    pub summary: String,
    pub fates: Vec<(String, Fate)>,
    /// Write CLOSEOUT.md, commit just that file, and tag the result.
    pub commit: bool,
}

impl Confirmed {
    pub fn read(body: &Map<String, Value>, link: Option<String>) -> Confirmed {
        let text = |k: &str| body.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
        let fates = body
            .get("tasks")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|t| {
                        let id = t.get("id")?.as_str()?.to_string();
                        Some((id, Fate::read(t.get("action")?.as_str()?, t.get("to").and_then(Value::as_str))?))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Confirmed {
            deliverable: text("deliverable"),
            link,
            summary: text("summary").unwrap_or_default(),
            fates,
            commit: body.get("commit").and_then(Value::as_bool).unwrap_or(false),
        }
    }
}

/// Close the project out. Returns the project and a sentence on what happened.
pub fn apply(store: &Store, id: &str, c: Confirmed) -> Result<(Map<String, Value>, String)> {
    let p = store.get("projects", id)?.ok_or_else(|| anyhow!("There's no project with id {id}."))?;
    let name = s(&p, "name").unwrap_or(id).to_string();
    let projects = store.list("projects")?;
    let items = store.list("items")?;
    let now = now_iso();
    let today = &now[..10];
    let text_of = |tid: &str| items.iter().find(|(i, _)| i == tid).and_then(|(_, m)| s(m, "text")).unwrap_or(tid).to_string();

    // Loose ends
    let mut patches = Vec::new();
    let mut ends = Vec::new();
    let mut newly_done = 0;
    for (tid, fate) in &c.fates {
        if !items.iter().any(|(i, m)| i == tid && s(m, "project") == Some(id) && logic::is_open(m)) {
            continue;
        }
        if *fate == Fate::Done {
            newly_done += 1;
        }
        let mut patch = Map::new();
        patch.insert("updated".into(), json!(now));
        match fate {
            Fate::Done | Fate::Drop => {
                for (k, v) in [("status", json!("done")), ("stage", json!("done")), ("done_at", json!(now))] {
                    patch.insert(k.into(), v);
                }
                if *fate == Fate::Drop {
                    patch.insert("dropped".into(), json!(true));
                }
                ends.push(format!("- {}: {}", if *fate == Fate::Drop { "Dropped" } else { "Done" }, text_of(tid)));
            }
            Fate::Move(to) => {
                let to = if to == "inbox" || projects.iter().any(|(pid, _)| pid == to) { to.clone() } else { "inbox".into() };
                ends.push(format!("- Moved to {}: {}", logic::project_name(&projects, &to), text_of(tid)));
                patch.insert("project".into(), json!(to));
                patch.insert("triage".into(), Value::Null);
            }
        }
        patches.push((tid.clone(), patch));
    }
    store.merge_many("items", &patches)?;

    // CLOSEOUT.md, committed and tagged on its own
    let deliverable = c.deliverable.clone().or_else(|| s(&p, "deliverable").map(str::to_string));
    let done_count = items.iter().filter(|(_, m)| s(m, "project") == Some(id) && !logic::is_open(m) && m.get("dropped").is_none()).count() + newly_done;
    let mut notes = Vec::new();
    let mut tag = None;
    if let Some(dir) = folder_of(&p) {
        let mut md = format!("# {name}: close-out\n\nClosed {today}.\n\n");
        md.push_str(&format!("**Deliverable:** {}  \n", deliverable.as_deref().unwrap_or("_not recorded_")));
        md.push_str(&format!("**Final version:** {}\n\n", c.link.as_deref().unwrap_or("_not linked_")));
        if !c.summary.is_empty() {
            md.push_str(&format!("{}\n\n", c.summary));
        }
        md.push_str(&format!("## Loose ends\n\n{}\n\n", if ends.is_empty() { "None: every task was finished.".to_string() } else { ends.join("\n") }));
        md.push_str(&format!("## Record\n\n- {done_count} tasks done.\n- Tracked in Work Desk as `{id}`.\n"));
        match std::fs::write(dir.join("CLOSEOUT.md"), &md) {
            Ok(()) => notes.push("Wrote CLOSEOUT.md.".to_string()),
            Err(e) => notes.push(format!("Couldn't write CLOSEOUT.md: {e}.")),
        }
        if c.commit && is_repo(&dir) {
            let t = free_tag(&dir);
            let r = git(&dir, &["add", "CLOSEOUT.md"])
                .and_then(|_| git(&dir, &["commit", "-q", "-m", &format!("Close out {name}"), "--", "CLOSEOUT.md"]))
                .and_then(|_| git(&dir, &["tag", "-a", &t, "-m", &format!("{name} closed out {today}")]));
            match r {
                Ok(_) => {
                    notes.push(format!("Committed it and tagged {t}."));
                    tag = Some(t);
                }
                Err(e) => notes.push(format!("Couldn't commit and tag: {e}.")),
            }
        }
    }

    // The project itself
    let mut patch = Map::new();
    patch.insert("status".into(), json!("done"));
    patch.insert("closed_at".into(), json!(now));
    patch.insert("updated".into(), json!(now));
    if let Some(d) = &c.deliverable {
        patch.insert("deliverable".into(), json!(d));
    }
    patch.insert("closeout".into(), json!({ "at": now, "summary": c.summary, "link": c.link, "tag": tag }));
    if let Some(link) = &c.link {
        let mut files = p.get("files").and_then(Value::as_array).cloned().unwrap_or_default();
        if !files.iter().any(|f| f.get("path").and_then(Value::as_str) == Some(link.as_str())) {
            let name = link.trim_end_matches('/').rsplit('/').next().unwrap_or(link).to_string();
            let kind = if link.starts_with("http") { "link" } else if expand_home(link).is_dir() { "folder" } else { "file" };
            files.push(json!({ "path": link, "name": name, "kind": kind, "added": now }));
            patch.insert("files".into(), Value::Array(files));
        }
    }
    let doc = store.merge("projects", id, &patch)?.unwrap_or_default();
    let n = patches.len();
    let mut msg = format!("{name} is closed out.");
    if n > 0 {
        msg.push_str(&format!(" {n} loose end{} handled.", if n == 1 { "" } else { "s" }));
    }
    if !notes.is_empty() {
        msg.push(' ');
        msg.push_str(&notes.join(" "));
    }
    Ok((doc, msg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closes_out_tasks_folder_and_project() {
        let d = std::env::temp_dir().join(format!("workdesk-test-closeout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let repo = d.join("talk");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join("slides.md"), "# Slides").unwrap();
        let has_git = Command::new("git").args(["init", "-q"]).current_dir(&repo).status().map_or(false, |s| s.success());
        if has_git {
            Command::new("git").args(["add", "-A"]).current_dir(&repo).status().unwrap();
            Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "Slides"]).current_dir(&repo).status().unwrap();
            std::fs::write(repo.join("draft-notes.md"), "not committed").unwrap();
        }
        let store = Store::open(&d.join("workdesk.json"), None).unwrap();
        let put = |c: &str, id: &str, v: Value| store.put(c, id, v.as_object().unwrap()).unwrap();
        put("projects", "talk", json!({"name": "Lab talk", "deliverable": "20-minute talk", "folder": repo.display().to_string(), "status": "active"}));
        put("projects", "tox", json!({"name": "Tox"}));
        put("items", "a", json!({"text": "Make slides", "project": "talk", "status": "done"}));
        put("items", "b", json!({"text": "Send slides to Eric", "project": "talk", "status": "open"}));
        put("items", "c", json!({"text": "Rerun GSEA", "project": "talk", "status": "open"}));
        put("items", "d", json!({"text": "Old idea", "project": "talk", "status": "open"}));

        let body = json!({"summary": "Gave the talk.", "commit": true, "tasks": [
            {"id": "b", "action": "done"}, {"id": "c", "action": "move", "to": "tox"}, {"id": "d", "action": "drop"}, {"id": "zzz", "action": "done"}]});
        let c = Confirmed::read(body.as_object().unwrap(), Some("~/Talks/final.key".into()));
        let (doc, msg) = apply(&store, "talk", c).unwrap();
        assert_eq!(doc["status"], "done");
        assert_eq!(doc["closeout"]["summary"], "Gave the talk.");
        assert!(doc["files"].as_array().unwrap().iter().any(|f| f["path"] == "~/Talks/final.key"));
        assert!(msg.starts_with("Lab talk is closed out. 3 loose ends handled."), "{msg}");
        let get = |id: &str| store.get("items", id).unwrap().unwrap();
        assert_eq!((get("b")["status"].as_str(), get("c")["project"].as_str(), get("d")["dropped"].as_bool()), (Some("done"), Some("tox"), Some(true)));
        let md = std::fs::read_to_string(repo.join("CLOSEOUT.md")).unwrap();
        assert!(md.contains("**Deliverable:** 20-minute talk") && md.contains("- Moved to Tox: Rerun GSEA") && md.contains("- 2 tasks done."), "{md}");
        if has_git {
            let tag = doc["closeout"]["tag"].as_str().unwrap().to_string();
            assert!(tag.starts_with("closeout-"));
            assert_eq!(git(&repo, &["log", "-1", "--format=%s", &tag]).unwrap(), "Close out Lab talk");
            // the close-out commit holds CLOSEOUT.md only; other changes are left as they were
            assert_eq!(git(&repo, &["show", "--name-only", "--format=", "HEAD"]).unwrap(), "CLOSEOUT.md");
            assert!(git(&repo, &["status", "--short"]).unwrap().contains("draft-notes.md"));
            assert_eq!(free_tag(&repo), format!("{tag}-2"));
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}
