//! All Work Desk data lives in one JSON file (`workdesk.json`) so it can sit in a
//! synced folder such as Google Drive.
//!
//! - Every write goes to a temp file in the same folder, then is renamed over the
//!   real file, so a sync client never uploads a half-written file.
//! - Before every read or write the file's timestamp is checked; if another
//!   computer's change has synced in, it is reloaded first.
//! - The first save each day also copies the file to a local backups folder.
//!
//! File shape:
//! {
//!   "format": "workdesk/1",
//!   "saved_at": "2026-10-13T17:02:00Z",
//!   "saved_by": "my-mac",
//!   "projects": { "<id>": { ...project fields... } },
//!   "items":    { "<id>": { ...task fields... } }
//! }

use crate::util::{now_iso, parse_ts};
use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::SystemTime;

pub const COLLECTIONS: &[&str] = &["projects", "items"];
pub const FORMAT: &str = "workdesk/1";

pub type Doc = (String, Map<String, Value>);
type Colls = BTreeMap<String, Map<String, Value>>;

pub fn check_collection(coll: &str) -> Result<()> {
    if COLLECTIONS.contains(&coll) {
        Ok(())
    } else {
        bail!("Unknown collection '{coll}'")
    }
}

#[derive(PartialEq, Clone, Copy)]
struct Stamp {
    modified: SystemTime,
    len: u64,
}

struct Inner {
    colls: Colls,
    stamp: Option<Stamp>,
    rev: u64,
    last_error: Option<String>,
}

pub struct Store {
    path: PathBuf,
    backup_dir: Option<PathBuf>,
    inner: Mutex<Inner>,
    counter: AtomicU64,
}

fn empty_colls() -> Colls {
    COLLECTIONS.iter().map(|c| (c.to_string(), Map::new())).collect()
}

fn stamp_of(path: &Path) -> Option<Stamp> {
    let md = std::fs::metadata(path).ok()?;
    Some(Stamp { modified: md.modified().ok()?, len: md.len() })
}

fn parse_file(text: &str) -> Result<Colls> {
    let v: Value = serde_json::from_str(text).context("the data file is not valid JSON")?;
    let mut colls = empty_colls();
    for coll in COLLECTIONS {
        if let Some(Value::Object(docs)) = v.get(*coll) {
            let target = colls.get_mut(*coll).unwrap();
            for (id, doc) in docs {
                if let Value::Object(m) = doc {
                    target.insert(id.clone(), Value::Object(m.clone()));
                }
            }
        }
    }
    Ok(colls)
}

/// Latest timestamp on a document, used to decide which copy is newer.
pub fn doc_time(m: &Map<String, Value>) -> i64 {
    ["updated", "done_at", "created"]
        .iter()
        .filter_map(|k| m.get(*k).and_then(|v| v.as_str()).and_then(parse_ts))
        .max()
        .unwrap_or(0)
}

fn hostname() -> String {
    let mut buf = [0u8; 256];
    let ok = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) } == 0;
    if ok {
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        let h = String::from_utf8_lossy(&buf[..end]).to_string();
        if !h.is_empty() {
            return h.trim_end_matches(".local").to_string();
        }
    }
    "unknown".into()
}

impl Store {
    /// Open (or create on first save) the data file. `backup_dir` gets one copy per day.
    pub fn open(path: &Path, backup_dir: Option<PathBuf>) -> Result<Self> {
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
            }
        }
        let store = Self {
            path: path.to_path_buf(),
            backup_dir,
            inner: Mutex::new(Inner { colls: empty_colls(), stamp: None, rev: 1, last_error: None }),
            counter: AtomicU64::new(0),
        };
        {
            let mut inner = store.inner.lock().unwrap();
            if path.exists() {
                let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
                inner.colls = parse_file(&text).with_context(|| format!("reading {}", path.display()))?;
                inner.stamp = stamp_of(path);
            }
        }
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn exists_on_disk(&self) -> bool {
        self.path.exists()
    }

    /// Reload if the file changed on disk (e.g. Google Drive synced another computer's edit).
    fn refresh(&self, inner: &mut Inner) {
        let now = stamp_of(&self.path);
        if now.is_none() || now == inner.stamp {
            return;
        }
        match std::fs::read_to_string(&self.path).map_err(anyhow::Error::from).and_then(|t| parse_file(&t)) {
            Ok(colls) => {
                inner.colls = colls;
                inner.stamp = now;
                inner.rev += 1;
                inner.last_error = None;
            }
            Err(e) => {
                // Probably mid-sync. Keep what we have; try again next time.
                inner.last_error = Some(format!("Could not read {}: {e:#}", self.path.display()));
            }
        }
    }

    fn save(&self, inner: &mut Inner) -> Result<()> {
        // If the file on disk is unreadable, keep a copy before replacing it.
        if inner.last_error.is_some() && self.path.exists() {
            let bad = self.path.with_extension(format!("unreadable-{}.json", now_iso().replace(':', "-")));
            let _ = std::fs::copy(&self.path, bad);
        }
        let mut root = Map::new();
        root.insert("format".into(), json!(FORMAT));
        root.insert("saved_at".into(), json!(now_iso()));
        root.insert("saved_by".into(), json!(hostname()));
        for (coll, docs) in &inner.colls {
            root.insert(coll.clone(), Value::Object(docs.clone()));
        }
        let mut text = serde_json::to_string_pretty(&Value::Object(root))?;
        text.push('\n');

        let dir = self.path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let name = self.path.file_name().and_then(|n| n.to_str()).unwrap_or("workdesk.json");
        let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
        {
            let mut f = std::fs::File::create(&tmp).with_context(|| format!("writing {}", tmp.display()))?;
            f.write_all(text.as_bytes())?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, &self.path).with_context(|| format!("saving {}", self.path.display()))?;
        inner.stamp = stamp_of(&self.path);
        inner.rev += 1;
        inner.last_error = None;
        self.daily_backup(&text);
        Ok(())
    }

    fn daily_backup(&self, text: &str) {
        let Some(dir) = &self.backup_dir else { return };
        let now = now_iso();
        let file = dir.join(format!("workdesk-{}.json", &now[..10]));
        if file.exists() {
            return;
        }
        if std::fs::create_dir_all(dir).is_ok() {
            let _ = std::fs::write(&file, text);
        }
    }

    /// Changes whenever the data changes, here or on another computer.
    pub fn rev(&self) -> u64 {
        let mut inner = self.inner.lock().unwrap();
        self.refresh(&mut inner);
        inner.rev
    }

    pub fn last_error(&self) -> Option<String> {
        self.inner.lock().unwrap().last_error.clone()
    }

    pub fn list(&self, coll: &str) -> Result<Vec<Doc>> {
        check_collection(coll)?;
        let mut inner = self.inner.lock().unwrap();
        self.refresh(&mut inner);
        Ok(inner.colls[coll]
            .iter()
            .filter_map(|(id, v)| v.as_object().map(|m| (id.clone(), m.clone())))
            .collect())
    }

    pub fn get(&self, coll: &str, id: &str) -> Result<Option<Map<String, Value>>> {
        check_collection(coll)?;
        let mut inner = self.inner.lock().unwrap();
        self.refresh(&mut inner);
        Ok(inner.colls[coll].get(id).and_then(|v| v.as_object()).cloned())
    }

    pub fn put(&self, coll: &str, id: &str, doc: &Map<String, Value>) -> Result<()> {
        check_collection(coll)?;
        let mut inner = self.inner.lock().unwrap();
        self.refresh(&mut inner);
        let mut d = doc.clone();
        d.remove("id");
        inner.colls.get_mut(coll).unwrap().insert(id.to_string(), Value::Object(d));
        self.save(&mut inner)
    }

    /// Shallow-merge `patch` into an existing document. Returns None if it doesn't exist.
    pub fn merge(&self, coll: &str, id: &str, patch: &Map<String, Value>) -> Result<Option<Map<String, Value>>> {
        check_collection(coll)?;
        let mut inner = self.inner.lock().unwrap();
        self.refresh(&mut inner);
        let Some(Value::Object(doc)) = inner.colls.get_mut(coll).unwrap().get_mut(id) else { return Ok(None) };
        for (k, v) in patch {
            if k != "id" {
                doc.insert(k.clone(), v.clone());
            }
        }
        let out = doc.clone();
        self.save(&mut inner)?;
        Ok(Some(out))
    }

    pub fn delete(&self, coll: &str, id: &str) -> Result<bool> {
        check_collection(coll)?;
        let mut inner = self.inner.lock().unwrap();
        self.refresh(&mut inner);
        let found = inner.colls.get_mut(coll).unwrap().remove(id).is_some();
        if found {
            self.save(&mut inner)?;
        }
        Ok(found)
    }

    pub fn is_empty(&self) -> bool {
        let mut inner = self.inner.lock().unwrap();
        self.refresh(&mut inner);
        inner.colls.values().all(|c| c.is_empty())
    }

    pub fn counts(&self) -> (usize, usize) {
        let mut inner = self.inner.lock().unwrap();
        self.refresh(&mut inner);
        (inner.colls["projects"].len(), inner.colls["items"].len())
    }

    pub fn new_id(&self, prefix: &str) -> String {
        let n = self.counter.fetch_add(1, Ordering::SeqCst);
        let t = SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_micros()).unwrap_or(0);
        format!("{prefix}-{t:x}{n:x}")
    }

    /// Bring in documents from an export or another copy of the file.
    /// `newer_wins`: keep whichever copy of each document was changed most recently
    /// (for Google Drive conflict copies). Otherwise incoming documents replace ours.
    /// Returns (added, updated, kept).
    pub fn import(&self, data: &Value, newer_wins: bool) -> Result<(usize, usize, usize)> {
        let mut inner = self.inner.lock().unwrap();
        self.refresh(&mut inner);
        let (mut added, mut updated, mut kept) = (0, 0, 0);
        for coll in COLLECTIONS {
            let Some(Value::Object(docs)) = data.get(*coll) else { continue };
            let target = inner.colls.get_mut(*coll).unwrap();
            for (id, doc) in docs {
                let Value::Object(incoming) = doc else { continue };
                match target.get(id).and_then(|v| v.as_object()) {
                    None => {
                        target.insert(id.clone(), Value::Object(incoming.clone()));
                        added += 1;
                    }
                    Some(existing) if existing == incoming => kept += 1,
                    Some(existing) => {
                        if newer_wins && doc_time(existing) >= doc_time(incoming) {
                            kept += 1;
                        } else {
                            target.insert(id.clone(), Value::Object(incoming.clone()));
                            updated += 1;
                        }
                    }
                }
            }
        }
        if added + updated > 0 {
            self.save(&mut inner)?;
        }
        Ok((added, updated, kept))
    }

    pub fn export(&self) -> Result<Value> {
        let mut inner = self.inner.lock().unwrap();
        self.refresh(&mut inner);
        let mut out = Map::new();
        out.insert("format".into(), json!(FORMAT));
        out.insert("saved_at".into(), json!(now_iso()));
        for (coll, docs) in &inner.colls {
            out.insert(coll.clone(), Value::Object(docs.clone()));
        }
        Ok(Value::Object(out))
    }

    /// Write the current data to `dest` as a brand-new file (used when moving to Google Drive).
    pub fn copy_to(&self, dest: &Path) -> Result<()> {
        let data = self.export()?;
        let target = Store::open(dest, None)?;
        target.import(&data, false)?;
        if !dest.exists() {
            // nothing to import (empty desk): still create the file
            let mut inner = target.inner.lock().unwrap();
            target.save(&mut inner)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("workdesk-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn doc(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn save_reload_and_external_edit() {
        let d = tmpdir("a");
        let f = d.join("workdesk.json");
        let s = Store::open(&f, Some(d.join("backups"))).unwrap();
        assert!(s.is_empty());
        s.put("items", "t1", &doc(json!({"text":"a","updated":"2026-10-01T00:00:00Z"}))).unwrap();
        s.merge("items", "t1", &doc(json!({"stage":"doing"}))).unwrap();
        // reopen from disk
        let s2 = Store::open(&f, None).unwrap();
        assert_eq!(s2.get("items", "t1").unwrap().unwrap()["stage"], "doing");
        // another computer edits the file
        let r0 = s.rev();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        v["items"]["t2"] = json!({"text":"from drive"});
        v["extra"] = json!("ignored");
        std::fs::write(&f, serde_json::to_string_pretty(&v).unwrap() + "\n\n").unwrap();
        assert!(s.rev() > r0);
        assert_eq!(s.list("items").unwrap().len(), 2);
        // half-synced garbage is ignored, not wiped
        std::fs::write(&f, "{ \"items\": ").unwrap();
        assert_eq!(s.list("items").unwrap().len(), 2);
        assert!(s.last_error().is_some());
        s.put("items", "t3", &doc(json!({"text":"c"}))).unwrap();
        assert_eq!(Store::open(&f, None).unwrap().list("items").unwrap().len(), 3);
        assert!(d.join("backups").read_dir().unwrap().count() >= 1);
    }

    #[test]
    fn merge_keeps_newer() {
        let d = tmpdir("b");
        let s = Store::open(&d.join("workdesk.json"), None).unwrap();
        s.put("items", "x", &doc(json!({"text":"mine","updated":"2026-10-13T10:00:00Z"}))).unwrap();
        s.put("items", "y", &doc(json!({"text":"old","updated":"2026-10-01T10:00:00Z"}))).unwrap();
        let other = json!({"items":{
            "x":{"text":"theirs","updated":"2026-10-12T10:00:00Z"},
            "y":{"text":"new","updated":"2026-10-13T11:00:00Z"},
            "z":{"text":"added"}}});
        let (a, u, k) = s.import(&other, true).unwrap();
        assert_eq!((a, u, k), (1, 1, 1));
        assert_eq!(s.get("items", "x").unwrap().unwrap()["text"], "mine");
        assert_eq!(s.get("items", "y").unwrap().unwrap()["text"], "new");
    }
}
