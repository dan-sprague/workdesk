//! Jev, TypeSafe's System One model: narrow judgments with calibrated probabilities.
//!
//! Work Desk asks Jev wherever a decision can be put as a question with set answers:
//! which project a note is about and whether it fits any, whether it holds several
//! to-dos, whether it repeats an open task, how big each task is, whether it's waiting
//! on someone, how much it matters, whether it's too vague, what kind of work it is,
//! and which linked files it needs. Code acts on the answers (thresholds, the plan),
//! and the chat model gets them as facts when it writes.
//!
//! Request and answer shapes follow docs.typesafe.ai (api, primitives, state).

use crate::logic::{self, s};
use crate::provider;
use crate::store::{Doc, Store};
use crate::util::{iso_from_days, now_iso, today_days};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Map, Value};
use std::time::Duration;

/// Bars for acting on a judgment. They're starting points; check them against your own tasks.
/// Probability that a note is about one of the projects at all:
pub const MIN_FIT: f64 = 0.5;
/// Confidence in which project (0 when probability is spread evenly over them, 1 when it's all on one):
pub const MIN_CONFIDENCE: f64 = 0.5;
/// A yes/no judgment counts as yes from here.
pub const YES: f64 = 0.5;

#[derive(Clone)]
pub struct Jev {
    key: String,
    http: reqwest::Client,
}

/// A Choice answer, with every option ranked by probability.
pub struct Pick {
    pub choice: String,
    pub confidence: f64,
    pub ranked: Vec<(String, f64)>,
}

pub fn read_pick(a: &Value) -> Option<Pick> {
    let choice = a["choice"].as_str()?.to_string();
    let mut ranked: Vec<(String, f64)> = a["probabilities"]
        .as_object()
        .map(|m| m.iter().filter_map(|(k, p)| Some((k.clone(), p.as_f64()?))).collect())
        .unwrap_or_default();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    Some(Pick { choice, confidence: a["confidence"].as_f64().unwrap_or(0.0), ranked })
}

/// The answers to one request, by question id.
pub struct Answers(Value);

impl Answers {
    pub fn noul(&self, id: &str) -> Option<f64> {
        self.0[id]["noul"].as_f64()
    }
    pub fn choice(&self, id: &str) -> Option<Pick> {
        read_pick(&self.0[id])
    }
    /// A Score's probability-weighted level, counting from 0.
    pub fn score(&self, id: &str) -> Option<f64> {
        self.0[id]["score"].as_f64()
    }
    /// A Score's probability for each level, lowest first.
    pub fn levels(&self, id: &str) -> Option<Vec<f64>> {
        let m = self.0[id]["probabilities"].as_object()?;
        let mut out = vec![0.0; m.len()];
        for (k, p) in m {
            *out.get_mut(k.parse::<usize>().ok()?)? = p.as_f64()?;
        }
        Some(out)
    }
}

fn noul(instructions: &str) -> Value {
    json!({ "type": "noul", "instructions": instructions })
}
fn choice(instructions: &str, criteria: Map<String, Value>) -> Value {
    json!({ "type": "choice", "instructions": instructions, "criteria": criteria })
}
fn score(instructions: &str, levels: &[&str]) -> Value {
    json!({ "type": "score", "instructions": instructions, "criteria": levels })
}

fn pct(p: f64) -> String {
    format!("{:.0}%", p * 100.0)
}

fn clip(t: &str, n: usize) -> String {
    t.chars().take(n).collect()
}

// ---------- notes ----------

// TypeSafe's docs advise a separate yes/no question over a "none of the above" option.
const NOTE_FITS: &str = "Is `note` about the work of one of the researcher's `projects`: that project's own study, \
paper, collaborators, data or deliverables? Sharing a topic or method with a project is not enough, and a \
program, drug, study or filing named in the note that no project mentions points to work that isn't listed.";
const NOTE_WHICH: &str = "Which of the researcher's `projects` is `note` about? The options are the projects' ids.";
const NOTE_SEVERAL: &str = "Does `note` hold more than one separate to-do, such as several action items from a \
meeting, rather than one piece of work?";
const DUP_SAME: &str = "Is `note` the same piece of work as one of `open_tasks`, even if it's worded differently? \
A related or follow-up task doesn't count.";
const DUP_WHICH: &str = "Which of `open_tasks` is closest to `note`? The options are the tasks' ids.";

/// Jev's reading of a new note.
#[derive(Default)]
pub struct NoteReading {
    /// Probability that the note is about one of the projects.
    pub fits: Option<f64>,
    pub pick: Option<Pick>,
    /// Probability that the note holds several separate to-dos.
    pub several: Option<f64>,
}

impl NoteReading {
    /// The same judgments, written out for the chat model.
    pub fn facts(&self, projects: &[Doc]) -> String {
        let mut out = String::from("Jev's judgments about this note (calibrated probabilities from a separate classifier):\n");
        if let Some(f) = self.fits {
            out.push_str(&format!("- It is about one of the listed projects: {}\n", pct(f)));
        }
        if let Some(p) = &self.pick {
            let top: Vec<String> =
                p.ranked.iter().take(3).map(|(id, pr)| format!("{} ({id}) {}", logic::project_name(projects, id), pct(*pr))).collect();
            out.push_str(&format!("- Most likely project: {}\n", top.join(", ")));
        }
        if let Some(s) = self.several {
            out.push_str(&format!("- It holds several separate to-dos: {}\n", pct(s)));
        }
        out
    }
}

// ---------- tasks ----------

const EFFORT: &str = "How much of the researcher's own working time will `task` take to finish, judging from \
`task.text`, `task.notes` and `project`?";
const EFFORT_LEVELS: [&str; 5] = [
    "A few minutes: a quick message, check, small edit or lookup",
    "An hour or two of focused work",
    "About half a day",
    "A day or two of work",
    "A week or more: a project in itself",
];
/// Rough hours for each effort level, for adding up work against deadlines.
pub const EFFORT_HOURS: [f64; 5] = [0.25, 1.5, 4.0, 12.0, 40.0];
const WAITING: &str = "Is `task` waiting on someone other than the researcher (a reply, data, a review, a signature \
or a decision) before it can move forward?";
const MATTERS: &str = "How much does finishing `task` matter to `project.goal`?";
const MATTERS_LEVELS: [&str; 4] = [
    "It doesn't serve the goal: housekeeping or nice to have",
    "It helps the goal, but the goal is reachable without it",
    "The goal needs it done",
    "The goal can't be reached without it, and other work is waiting on it",
];
const VAGUE: &str = "Is `task` too vague to start without first deciding what it means or what done looks like?";

/// Changes when a task's wording, notes or project change, so its judgments are redone then.
pub fn signature(m: &Map<String, Value>) -> String {
    let mut h: u64 = 0xcbf29ce484222325; // FNV-1a: stable across builds
    for k in ["text", "notes", "project"] {
        for b in s(m, k).unwrap_or("").bytes().chain([0u8]) {
            h = (h ^ b as u64).wrapping_mul(0x100000001b3);
        }
    }
    format!("{h:016x}")
}

fn task_state(m: &Map<String, Value>, project: Option<&Map<String, Value>>) -> Value {
    json!({
        "task": { "text": s(m, "text"), "notes": s(m, "notes").map(|n| clip(n, 3000)), "stage": logic::stage_label(logic::stage(m)), "due": s(m, "due") },
        "project": project.map(|p| json!({ "name": s(p, "name"), "goal": s(p, "goal") })),
        "today": iso_from_days(today_days()),
    })
}

// ---------- prompts ----------

const KIND: &str = "What kind of work is `task`?";
const KINDS: [(&str, &str); 6] = [
    ("analysis", "Writing or running code or an analysis"),
    ("writing", "Writing or revising prose: a paper section, IND narrative, grant, reviewer response or email"),
    ("figures", "Making or fixing figures or tables"),
    ("data", "Finding, cleaning or organizing data"),
    ("reading", "Reading or reviewing literature or someone else's work"),
    ("admin", "Coordinating, scheduling, ordering or other admin"),
];
/// At most this many linked files get their own "is it needed?" question.
const MAX_FILES: usize = 20;

/// Jev's reading of a task before a prompt is drafted for it.
pub struct PromptReading {
    pub kind: Option<Pick>,
    pub vague: Option<f64>,
    /// Each linked file and the probability that the task needs it.
    pub files: Vec<(String, f64)>,
}

impl PromptReading {
    pub fn facts(&self) -> String {
        let mut out = String::from("\nJev's judgments about this task (calibrated probabilities from a separate classifier):\n");
        if let Some(k) = &self.kind {
            let top: Vec<String> = k.ranked.iter().take(2).map(|(id, p)| format!("{id} {}", pct(*p))).collect();
            out.push_str(&format!("- Kind of work: {}\n", top.join(", ")));
        }
        if let Some(v) = self.vague {
            out.push_str(&format!("- Too vague to start without first deciding what done looks like: {}\n", pct(v)));
        }
        let (need, skip): (Vec<_>, Vec<_>) = self.files.iter().partition(|(_, p)| *p >= YES);
        if !need.is_empty() {
            out.push_str(&format!("- Linked files likely needed: {}\n", need.iter().map(|(f, p)| format!("{f} ({})", pct(*p))).collect::<Vec<_>>().join(", ")));
        }
        if !skip.is_empty() {
            out.push_str(&format!("- Linked files likely not needed: {}\n", skip.iter().map(|(f, _)| f.as_str()).collect::<Vec<_>>().join(", ")));
        }
        out
    }
}

impl Jev {
    pub fn new(key: String, http: &reqwest::Client) -> Jev {
        Jev { key, http: http.clone() }
    }

    /// One request: several questions about the same state, answered in parallel.
    pub async fn ask(&self, state: Value, questions: Map<String, Value>) -> Result<Answers> {
        let body = json!({ "model": provider::JEV_MODEL, "state": state, "questions": questions });
        // Busy or rate-limited: TypeSafe asks for a short wait before retrying.
        let mut tries = 0;
        let (status, v) = loop {
            let resp = self.http.post(format!("{}/systemone", provider::jev_base())).bearer_auth(&self.key).json(&body).send().await.map_err(|e| anyhow!("Couldn't reach Jev: {e}"))?;
            let status = resp.status().as_u16();
            if matches!(status, 429 | 529) && tries < 1 {
                tries += 1;
                tokio::time::sleep(Duration::from_millis(1500)).await;
                continue;
            }
            break (status, resp.json::<Value>().await.unwrap_or(Value::Null));
        };
        match status {
            200..=299 => Ok(Answers(v["answers"].clone())),
            401 | 403 => bail!("TypeSafe turned down the Jev key."),
            429 => bail!("Jev is limiting requests."),
            529 => bail!("Jev is overloaded right now."),
            s => bail!("Jev returned {s}: {}", clip(&v.to_string(), 200)),
        }
    }

    /// Which project a note is about, whether it fits any, and whether it holds several
    /// to-dos. With no projects, only the last.
    pub async fn read_note(&self, note: &str, projects: &[Doc], items: &[Doc]) -> Result<NoteReading> {
        let ordered = logic::ordered(projects);
        let mut q = Map::new();
        q.insert("several".into(), noul(NOTE_SEVERAL));
        if !ordered.is_empty() {
            let criteria = ordered.iter().map(|(pid, p)| (pid.clone(), json!(s(p, "name").unwrap_or(pid)))).collect();
            q.insert("fits".into(), noul(NOTE_FITS));
            q.insert("project".into(), choice(NOTE_WHICH, criteria));
        }
        let facts: Vec<Value> = ordered.iter().map(|(pid, p)| logic::project_facts(pid, p, items)).collect();
        let a = self.ask(json!({ "note": note, "projects": facts }), q).await?;
        Ok(NoteReading { fits: a.noul("fits"), pick: a.choice("project"), several: a.noul("several") })
    }

    /// The open task a note repeats, if Jev thinks it repeats one: (id, probability).
    pub async fn duplicate_of(&self, note: &str, open: &[(String, String)]) -> Result<Option<(String, f64)>> {
        let open = &open[..open.len().min(200)];
        if open.is_empty() {
            return Ok(None);
        }
        let tasks: Vec<Value> = open.iter().map(|(id, t)| json!({ "id": id, "text": t })).collect();
        let criteria = open.iter().map(|(id, t)| (id.clone(), json!(clip(t, 200)))).collect();
        let mut q = Map::new();
        q.insert("same".into(), noul(DUP_SAME));
        q.insert("which".into(), choice(DUP_WHICH, criteria));
        let a = self.ask(json!({ "note": note, "open_tasks": tasks }), q).await?;
        let same = a.noul("same").unwrap_or(0.0);
        Ok(a.choice("which").filter(|_| same >= YES).map(|p| (p.choice, same)))
    }

    /// Size, waiting, importance and vagueness for one task, ready to keep on it as `jev`.
    pub async fn judge_task(&self, m: &Map<String, Value>, project: Option<&Map<String, Value>>) -> Result<Value> {
        let mut q = Map::new();
        q.insert("effort".into(), score(EFFORT, &EFFORT_LEVELS));
        q.insert("waiting".into(), noul(WAITING));
        q.insert("vague".into(), noul(VAGUE));
        if project.and_then(|p| s(p, "goal")).is_some() {
            q.insert("matters".into(), score(MATTERS, &MATTERS_LEVELS));
        }
        let a = self.ask(task_state(m, project), q).await?;
        let hours = a.levels("effort").map(|ps| ps.iter().zip(EFFORT_HOURS).map(|(p, h)| p * h).sum::<f64>());
        Ok(json!({
            "sig": signature(m),
            "at": now_iso(),
            "effort": a.score("effort"),
            "hours": hours.map(|h| (h * 10.0).round() / 10.0),
            "waiting": a.noul("waiting"),
            "vague": a.noul("vague"),
            "matters": a.score("matters"),
        }))
    }

    /// Judge every open task whose judgments are missing or out of date, eight at a time,
    /// and keep the answers on the tasks. Returns (judged, first_error).
    pub async fn judge_open_tasks(&self, store: &Store) -> Result<(usize, Option<String>)> {
        let projects = store.list("projects")?;
        let stale: Vec<(String, Map<String, Value>, Option<Map<String, Value>>)> = store
            .list("items")?
            .into_iter()
            .filter(|(_, m)| logic::is_open(m))
            .filter(|(_, m)| m.get("jev").and_then(|j| j.get("sig")).and_then(Value::as_str) != Some(signature(m).as_str()))
            .map(|(id, m)| {
                let p = s(&m, "project").and_then(|pid| projects.iter().find(|(q, _)| q == pid)).map(|(_, p)| p.clone());
                (id, m, p)
            })
            .collect();
        let (mut patches, mut err) = (Vec::new(), None);
        for chunk in stale.chunks(8) {
            let mut set = tokio::task::JoinSet::new();
            for (id, m, p) in chunk.iter().cloned() {
                let jev = self.clone();
                set.spawn(async move { (id, jev.judge_task(&m, p.as_ref()).await) });
            }
            while let Some(done) = set.join_next().await {
                match done {
                    Ok((id, Ok(v))) => {
                        let mut patch = Map::new();
                        patch.insert("jev".into(), v);
                        patches.push((id, patch));
                    }
                    Ok((_, Err(e))) => {
                        err.get_or_insert(e.to_string());
                    }
                    Err(e) => {
                        err.get_or_insert(e.to_string());
                    }
                }
            }
        }
        store.merge_many("items", &patches)?;
        Ok((patches.len(), err))
    }

    /// What kind of work a task is, whether it's vague, and which of `files` it needs.
    pub async fn read_for_prompt(&self, m: &Map<String, Value>, project: Option<&Map<String, Value>>, files: &[String]) -> Result<PromptReading> {
        let files = &files[..files.len().min(MAX_FILES)];
        let mut state = task_state(m, project);
        state["files"] = json!(files);
        let mut q = Map::new();
        q.insert("kind".into(), choice(KIND, KINDS.iter().map(|(k, d)| (k.to_string(), json!(d))).collect()));
        q.insert("vague".into(), noul(VAGUE));
        for i in 0..files.len() {
            q.insert(format!("file_{i}"), noul(&format!("Will working on `task` need the file or folder `files[{i}]`?")));
        }
        let a = self.ask(state, q).await?;
        let files = files.iter().enumerate().filter_map(|(i, f)| Some((f.clone(), a.noul(&format!("file_{i}"))?))).collect();
        Ok(PromptReading { kind: a.choice("kind"), vague: a.noul("vague"), files })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_answers() {
        // shapes from docs.typesafe.ai/api
        let a = Answers(json!({
            "department": {"type": "choice", "choice": "technical", "confidence": 1.0, "probabilities": {"technical": 1.0, "billing": 0.0}},
            "frustration": {"type": "score", "score": 0.67, "confidence": 0.5, "probabilities": {"0": 0.33, "1": 0.67, "2": 0.0}},
            "is_urgent": {"type": "noul", "noul": 0.99}
        }));
        assert_eq!(a.noul("is_urgent"), Some(0.99));
        assert_eq!(a.choice("department").unwrap().ranked[0], ("technical".to_string(), 1.0));
        assert_eq!(a.score("frustration"), Some(0.67));
        assert_eq!(a.levels("frustration"), Some(vec![0.33, 0.67, 0.0]));
        assert!(a.noul("missing").is_none());
    }

    #[test]
    fn signature_follows_wording() {
        let m = |t: &str| json!({"text": t, "project": "tox"}).as_object().unwrap().clone();
        assert_eq!(signature(&m("a")), signature(&m("a")));
        assert_ne!(signature(&m("a")), signature(&m("b")));
        let mut moved = m("a");
        moved.insert("stage".into(), json!("doing"));
        assert_eq!(signature(&m("a")), signature(&moved));
    }
}
