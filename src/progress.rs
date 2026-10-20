//! What's already been done in a project folder, so a prompt for Claude builds on it:
//! the scripts and notebooks there (newest first, with the top of each), recent commits,
//! and uncommitted changes.

use crate::util::iso_from_days;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;
use std::time::UNIX_EPOCH;

/// Folders read for progress unless `progress_dirs` in config.json says otherwise.
pub const DEFAULT_DIRS: &[&str] = &["scripts", "notebooks"];

const CODE: &[&str] = &["py", "jl", "r", "R", "rmd", "Rmd", "qmd", "ipynb", "sh", "m", "sql", "nf", "smk", "do", "sas", "stan"];
const SKIP_DIRS: &[&str] = &["__pycache__", ".ipynb_checkpoints", "node_modules", ".venv", "venv", ".git"];
const MAX_FILES: usize = 40;
const MAX_LINES: usize = 12;

pub struct Script {
    /// Relative to the project folder.
    pub path: String,
    pub modified: i64,
    /// The top of the file; for a notebook, its headings and the first line of each code cell.
    pub summary: Vec<String>,
    /// Notebooks only: whether it has saved outputs (it has been run).
    pub has_run: Option<bool>,
}

pub struct Progress {
    /// Which folders were read, e.g. ["scripts", "notebooks"].
    pub dirs: Vec<String>,
    pub scripts: Vec<Script>,
    /// (time, "2026-10-18 Add the 3-component fit"), newest first.
    pub commits: Vec<(i64, String)>,
    /// `git status --short` lines.
    pub uncommitted: Vec<String>,
}

fn secs(md: &std::fs::Metadata) -> i64 {
    md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs() as i64)
}

fn date(secs: i64) -> String {
    iso_from_days(secs.div_euclid(86400))
}

fn clip(t: &str, n: usize) -> String {
    let t = t.trim_end();
    if t.chars().count() > n {
        format!("{}…", t.chars().take(n).collect::<String>())
    } else {
        t.to_string()
    }
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<(std::path::PathBuf, i64)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        let Ok(md) = e.metadata() else { continue };
        let path = e.path();
        if md.is_dir() {
            if depth < 3 {
                walk(&path, depth + 1, out);
            }
        } else if path.extension().and_then(|x| x.to_str()).map_or(false, |x| CODE.contains(&x)) {
            out.push((path, secs(&md)));
        }
    }
}

/// The headings and the first line of each code cell of a Jupyter notebook, and whether it has outputs.
fn notebook(text: &str) -> (Vec<String>, bool) {
    let v: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let mut lines = Vec::new();
    let mut run = false;
    for cell in v["cells"].as_array().into_iter().flatten() {
        let src: String = match &cell["source"] {
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            Value::String(s) => s.clone(),
            _ => String::new(),
        };
        run |= cell["outputs"].as_array().map_or(false, |o| !o.is_empty());
        let pick = match cell["cell_type"].as_str() {
            Some("markdown") => src.lines().find(|l| l.trim_start().starts_with('#')),
            Some("code") => src.lines().find(|l| !l.trim().is_empty()),
            _ => None,
        };
        if let Some(l) = pick {
            if lines.len() < MAX_LINES {
                lines.push(clip(l, 140));
            }
        }
    }
    (lines, run)
}

fn top_of(path: &Path) -> (Vec<String>, Option<bool>) {
    use std::io::Read;
    let mut buf = Vec::new();
    let limit = if path.extension().map_or(false, |x| x == "ipynb") { 4 << 20 } else { 16 << 10 };
    if std::fs::File::open(path).and_then(|f| f.take(limit).read_to_end(&mut buf)).is_err() {
        return (Vec::new(), None);
    }
    let text = String::from_utf8_lossy(&buf);
    if path.extension().map_or(false, |x| x == "ipynb") {
        let (lines, run) = notebook(&text);
        return (lines, Some(run));
    }
    (text.lines().filter(|l| !l.trim().is_empty()).take(MAX_LINES).map(|l| clip(l, 140)).collect(), None)
}

fn git(folder: &Path, args: &[&str]) -> Vec<String> {
    Command::new("git")
        .arg("-C")
        .arg(folder)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().map(str::to_string).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
}

/// Read a project folder. None if it isn't there.
pub fn scan(folder: &Path, dirs: &[String]) -> Option<Progress> {
    if !folder.is_dir() {
        return None;
    }
    let mut found = Vec::new();
    for d in dirs {
        walk(&folder.join(d), 0, &mut found);
    }
    found.sort_by(|a, b| b.1.cmp(&a.1));
    let scripts = found
        .into_iter()
        .take(MAX_FILES)
        .map(|(p, modified)| {
            let (summary, has_run) = top_of(&p);
            let path = p.strip_prefix(folder).unwrap_or(&p).display().to_string();
            Script { path, modified, summary, has_run }
        })
        .collect();
    let commits = git(folder, &["log", "-n", "15", "--date=short", "--format=%ct %ad %s"])
        .into_iter()
        .filter_map(|l| {
            let (t, rest) = l.split_once(' ')?;
            Some((t.parse().ok()?, clip(rest, 160)))
        })
        .collect();
    let uncommitted = git(folder, &["status", "--short", "--untracked-files=all"]).into_iter().take(30).collect();
    Some(Progress { dirs: dirs.to_vec(), scripts, commits, uncommitted })
}

impl Progress {
    /// The newest change: a script or notebook saved, or a commit.
    pub fn latest(&self) -> i64 {
        let s = self.scripts.iter().map(|s| s.modified).max().unwrap_or(0);
        let c = self.commits.first().map_or(0, |c| c.0);
        s.max(c)
    }

    /// What changed after `stamp` (seconds), for "this prompt is out of date".
    pub fn changed_since(&self, stamp: i64) -> Vec<String> {
        let mut out: Vec<String> = self.scripts.iter().filter(|s| s.modified > stamp).map(|s| s.path.clone()).collect();
        let n = self.commits.iter().filter(|c| c.0 > stamp).count();
        if n > 0 {
            out.push(format!("{n} new commit{}", if n == 1 { "" } else { "s" }));
        }
        out
    }

    /// For the chat model.
    pub fn text(&self) -> String {
        let dirs = self.dirs.iter().map(|d| format!("{d}/")).collect::<Vec<_>>().join(", ");
        let mut out = format!("\nProgress in the project folder ({dirs}; newest first):\n");
        if self.scripts.is_empty() {
            out.push_str(&format!("- No scripts or notebooks in {dirs} yet.\n"));
        }
        for s in &self.scripts {
            let run = match s.has_run {
                Some(true) => ", has run outputs",
                Some(false) => ", not run",
                None => "",
            };
            out.push_str(&format!("- {} (changed {}{run})\n", s.path, date(s.modified)));
            for l in &s.summary {
                out.push_str(&format!("    {l}\n"));
            }
        }
        if !self.commits.is_empty() {
            out.push_str("Recent commits:\n");
            for (_, c) in &self.commits {
                out.push_str(&format!("- {c}\n"));
            }
        }
        if !self.uncommitted.is_empty() {
            out.push_str("Uncommitted changes (git status):\n");
            for l in &self.uncommitted {
                out.push_str(&format!("- {l}\n"));
            }
        }
        out
    }

    /// For Jev's state.
    pub fn state(&self) -> Value {
        json!({
            "scripts": self.scripts.iter().map(|s| json!({
                "path": s.path, "changed": date(s.modified), "top_of_file": s.summary, "has_run": s.has_run
            })).collect::<Vec<_>>(),
            "recent_commits": self.commits.iter().map(|c| &c.1).collect::<Vec<_>>(),
            "uncommitted": self.uncommitted,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_scripts_notebooks_and_git() {
        let d = std::env::temp_dir().join(format!("workdesk-test-progress-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("scripts/fits")).unwrap();
        std::fs::create_dir_all(d.join("notebooks/.ipynb_checkpoints")).unwrap();
        std::fs::create_dir_all(d.join("data")).unwrap();
        std::fs::write(d.join("scripts/fits/mixture.jl"), "# Fit the 3-component mixture\n\nusing Turing\n").unwrap();
        std::fs::write(d.join("scripts/notes.txt"), "not code").unwrap();
        std::fs::write(d.join("data/raw.py"), "# outside the folders read").unwrap();
        let nb = json!({"cells": [
            {"cell_type": "markdown", "source": ["# PIP calibration\n", "first look"]},
            {"cell_type": "code", "source": ["\n", "df = load()\n"], "outputs": [{"output_type": "stream"}]}
        ]});
        std::fs::write(d.join("notebooks/calib.ipynb"), nb.to_string()).unwrap();
        std::fs::write(d.join("notebooks/.ipynb_checkpoints/calib-checkpoint.ipynb"), "{}").unwrap();
        let has_git = Command::new("git").args(["init", "-q"]).current_dir(&d).status().map_or(false, |s| s.success());
        if has_git {
            Command::new("git").args(["add", "scripts"]).current_dir(&d).status().unwrap();
            Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "Add the mixture fit"]).current_dir(&d).status().unwrap();
        }

        let dirs: Vec<String> = DEFAULT_DIRS.iter().map(|s| s.to_string()).collect();
        let p = scan(&d, &dirs).unwrap();
        let mut paths: Vec<&str> = p.scripts.iter().map(|s| s.path.as_str()).collect();
        paths.sort();
        assert_eq!(paths, ["notebooks/calib.ipynb", "scripts/fits/mixture.jl"]);
        let nb = p.scripts.iter().find(|s| s.path.ends_with("ipynb")).unwrap();
        assert_eq!((nb.summary.clone(), nb.has_run), (vec!["# PIP calibration".to_string(), "df = load()".to_string()], Some(true)));
        let text = p.text();
        assert!(text.contains("    # Fit the 3-component mixture"), "{text}");
        if has_git {
            assert!(text.contains("Add the mixture fit"), "{text}");
            assert!(p.uncommitted.iter().any(|l| l.contains("notebooks/calib.ipynb")), "{:?}", p.uncommitted);
        }
        assert!(p.changed_since(p.latest()).is_empty());
        assert!(!p.changed_since(0).is_empty());
        assert!(scan(&d.join("missing"), &dirs).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
