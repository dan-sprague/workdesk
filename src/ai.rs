//! The AI side of Work Desk.
//!
//! - A chat model (any OpenAI-compatible service; see provider.rs) tidies each note,
//!   catches its due date, judges which project it belongs to, and writes the brief.
//! - Jev (TypeSafe), when there's a key for it, makes the project call instead: it
//!   returns calibrated probabilities, so a note it isn't sure about waits in the Inbox.

use crate::config::Config;
use crate::logic;
use crate::provider::{self, Identified, Provider};
use crate::store::{Doc, Store};
use crate::util::*;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Map, Value};
use std::time::Duration;

/// Jev answers two questions about each note, and both must clear these bars for it to be filed.
/// They're starting points; check them against how your own notes get filed.
/// Probability that the note is about one of the projects at all:
pub const JEV_MIN_FIT: f64 = 0.5;
/// Confidence in which project (0 when probability is spread evenly over them, 1 when it's all on one):
pub const JEV_MIN_CONFIDENCE: f64 = 0.5;

#[derive(Clone)]
pub struct Chat {
    key: String,
    model: String,
    base: String,
    provider: &'static Provider,
    http: reqwest::Client,
}

#[derive(Clone)]
pub struct Jev {
    key: String,
    http: reqwest::Client,
}

/// Whatever AI there are keys for: a chat model, Jev, or both.
#[derive(Clone)]
pub struct Ai {
    pub chat: Option<Chat>,
    pub jev: Option<Jev>,
}

/// Where a note went. `triage` is set when it went to the Inbox because no project clearly fit.
pub struct Filed {
    pub project: String,
    pub text: String,
    pub due: Option<String>,
    pub triage: Option<Triage>,
    /// One of the two AIs failed and the other decided alone.
    pub warning: Option<String>,
}

/// Why a note is waiting in the Inbox, and what to do with it.
#[derive(Debug, PartialEq)]
pub struct Triage {
    pub reason: String,
    /// A name for the project the note seems to belong to, if it isn't one of the existing ones.
    pub suggest: Option<String>,
    /// The nearest existing project.
    pub closest: Option<String>,
    /// "jev" or "model": which AI made the call.
    pub by: &'static str,
}

impl Triage {
    pub fn to_json(&self) -> Value {
        json!({ "reason": self.reason, "suggest": self.suggest, "closest": self.closest, "by": self.by })
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Fit {
    Clear,
    Unsure,
    None,
}

/// The chat model's reading of a note.
struct ChatFiling {
    project: Option<String>,
    fit: Fit,
    reason: Option<String>,
    new_project: Option<String>,
    task: Option<String>,
    due: Option<String>,
}

/// Jev's reading of a note: which project, with every project ranked by probability,
/// and the probability that the note belongs to any of them.
struct JevPick {
    choice: String,
    confidence: f64,
    ranked: Vec<(String, f64)>,
    fits: Option<f64>,
}

/// How a note becomes a task, shared by filing and by tidying a note whose project is already chosen.
macro_rules! rewrite_rules {
    () => {
        "Rewrite the note as one short, concrete task in the user's own words. Keep names, numbers, gene and \
variant names, and technical terms exactly as written. Drop filler such as 'remember to' or 'I need to'. \
Remove the deadline phrase from the task text once you have captured it as a date.\n\
If the note implies a deadline ('by Friday', 'next Tuesday', 'end of the month', 'tomorrow'), convert it \
to an ISO date relative to today. A bare weekday means the next occurrence of that day on or after today. \
Otherwise use null.\n"
    };
}

const TIDY_SYSTEM: &str = concat!(
    "You turn a quick sticky note into a task for a researcher's project tracker. The project is already chosen.\n",
    rewrite_rules!(),
    "Respond with a JSON object only: {\"task\": string, \"due\": string or null}."
);

const FILE_SYSTEM: &str = concat!("You file quick sticky notes into a researcher's project tracker.\n\
First decide whether the note belongs to one of the listed projects. It belongs to a project only if it is \
about that project's own work: its named study, paper, collaborators, data or deliverables. Sharing a topic or \
method is not enough; a note about off-target analysis does not belong to a project just because that project \
also does off-target analysis. Names in the note that appear nowhere in the projects (a program, drug, study, \
grant, partner, or a filing such as an IND) usually mean a project the user hasn't added yet.\n\
Researchers abbreviate: 'NH' means natural history, 'tox' means toxicology, and a person's first name often \
points to a collaborator listed on one project.\n\
Set fit to \"clear\" when one project obviously fits, \"unsure\" when it could be several or the match is only \
topical, and \"none\" when no project fits. Set project_id to the closest project's id even when unsure, or \
\"inbox\" if none is close. Unless fit is \"clear\", set new_project to a short name (2 to 4 words, using names \
from the note) for the project it seems to belong to, or null if it is a stray to-do. Set reason to one short \
sentence naming the words that decided it.\n",
    rewrite_rules!(),
    "Respond with a JSON object only: {\"project_id\": string, \"fit\": \"clear\" | \"unsure\" | \"none\", \
\"reason\": string, \"new_project\": string or null, \"task\": string, \"due\": string or null}."
);

// TypeSafe's docs advise a separate yes/no question over a "none of the above" option.
const JEV_FITS: &str = "Is `note` about the work of one of the researcher's `projects`: that project's own study, \
paper, collaborators, data or deliverables? Sharing a topic or method with a project is not enough, and a \
program, drug, study or filing named in the note that no project mentions points to work that isn't listed.";
const JEV_WHICH: &str = "Which of the researcher's `projects` is `note` about? The options are the projects' ids.";

const BRIEF_SYSTEM: &str = "You are a calm, practical executive assistant for a researcher who struggles \
with executive function and with getting things fully finished. Using their task list, decide what \
matters today. Prefer finishing work that is already in progress or in review over starting new work. \
Respect deadlines. Point out anything overdue, stalled, or at risk of missing a deadline. Keep every \
line short, specific and kind; use the exact task wording so they can find it.\n\
Respond with a JSON object only:\n\
{\"headline\": one sentence about today,\n \"focus\": up to 3 objects {\"task\": exact task text, \"project\": project name, \"why\": short reason},\n \"watch\": up to 3 short warnings,\n \"quick_wins\": up to 3 tasks likely to take under 30 minutes,\n \"first_step\": one tiny concrete action to start in the next 5 minutes}";

const PROMPT_SYSTEM: &str = "You write a prompt that a researcher will paste into Claude Code, an AI agent \
running inside their project folder with access to its files, a terminal and git. Write it in the researcher's \
voice, addressed to Claude, as plain text with short headings or numbered lists. No greeting or sign-off.\n\
Cover, briefly:\n\
1. The goal: what this task should produce and why it matters to the project.\n\
2. Context Claude needs: the project's goal, the files and folders that matter (use the paths given; they are \
relative to the project folder when inside it), deadlines, and how the result will be used. Leave out people \
and details that don't affect the work.\n\
3. What to do: concrete steps or deliverables, and where outputs go. In folders with the usual layout, results \
go in output/, figures and tables in display_items/, drafts in drafts/, and raw data in data/ is never changed.\n\
4. How to check the work: what to verify, which tests or sanity checks to run, and to stop and ask before \
anything destructive or when a choice is ambiguous.\n\
5. What to report back when done.\n\
Use only the facts given. When something important is unknown, put a short placeholder in [square brackets] \
for the researcher to fill in instead of guessing. If the task is mainly writing prose (a paper section, IND \
narrative, grant or reviewer response), ask Claude to start by working out an outline with the researcher, \
not to draft the whole document. Keep the prompt as short as the task allows.\n\
Respond with a JSON object only: {\"prompt\": string}.";

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(90))
        .connect_timeout(Duration::from_secs(15))
        .user_agent(concat!("workdesk/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("http client")
}

/// Loosen a request a service turned down for using a parameter its model doesn't take
/// (reasoning models want `max_completion_tokens` and no `temperature`). True if anything changed.
fn relax(body: &mut Value, msg: &str) -> bool {
    let m = msg.to_lowercase();
    let obj = body.as_object_mut().unwrap();
    let mut changed = false;
    if m.contains("temperature") {
        changed |= obj.remove("temperature").is_some();
    }
    if m.contains("max_tokens") {
        if let Some(n) = obj.remove("max_tokens").and_then(|n| n.as_u64()) {
            obj.insert("max_completion_tokens".into(), json!((n * 10).max(2000)));
            changed = true;
        }
    }
    if m.contains("response_format") {
        changed |= obj.remove("response_format").is_some();
    }
    changed
}

impl Chat {
    fn new(key: String, provider: &'static Provider, base: String, model: String, http: &reqwest::Client) -> Chat {
        Chat { key, model, base, provider, http: http.clone() }
    }

    /// One chat completion; returns the reply text.
    async fn complete(&self, system: &str, user: &str, max_tokens: u32) -> Result<String> {
        let name = self.provider.name;
        let mut body = json!({
            "model": self.model,
            "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
            "response_format": {"type": "json_object"},
            "temperature": 0.2,
            "max_tokens": max_tokens,
            "stream": false
        });
        for attempt in 0.. {
            let req = provider::auth(self.http.post(format!("{}/chat/completions", self.base)), self.provider, &self.key);
            let resp = req.json(&body).send().await.map_err(|e| anyhow!("Couldn't reach {name}: {e}"))?;
            let status = resp.status();
            let text = resp.text().await?;
            let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            if status.is_success() {
                return Ok(v["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string());
            }
            let msg = v["error"]["message"]
                .as_str()
                .or_else(|| v[0]["error"]["message"].as_str())
                .map(str::to_string)
                .unwrap_or_else(|| text.chars().take(200).collect());
            if status.as_u16() == 400 && attempt < 3 && relax(&mut body, &msg) {
                continue;
            }
            match status.as_u16() {
                401 | 403 => bail!("{name} turned down the API key. Paste a new one under AI keys, or run `desk key`."),
                402 => bail!("{name} says the account balance is too low."),
                404 => bail!("{name} doesn't know the model '{}'. Change it with `desk model <name>`. ({msg})", self.model),
                429 => bail!("{name} is limiting requests. Try again in a minute."),
                _ => bail!("{name} returned {status}: {msg}"),
            }
        }
        unreachable!()
    }

    async fn chat_json(&self, system: &str, user: &str, max_tokens: u32) -> Result<Value> {
        let name = self.provider.name;
        let content = self.complete(system, user, max_tokens).await?;
        if content.trim().is_empty() {
            bail!("{name} sent an empty reply");
        }
        let trimmed = content.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
        serde_json::from_str(trimmed).map_err(|_| anyhow!("{name}'s reply wasn't valid JSON"))
    }

    /// Tidy a note into a task for a project that's already chosen: (task, due).
    async fn tidy(&self, note: &str, project: &str) -> Result<(Option<String>, Option<String>)> {
        let today = today_days();
        let user = format!("Today is {} {}.\nProject: {project}\nNote: \"{}\"", weekday_name(today), iso_from_days(today), note.replace('"', "'"));
        let v = self.chat_json(TIDY_SYSTEM, &user, 300).await?;
        let f = read_filing(&v, &[]);
        Ok((f.task, f.due))
    }

    async fn file(&self, note: &str, projects: &[Doc], items: &[Doc]) -> Result<ChatFiling> {
        let today = today_days();
        let user = format!(
            "Today is {} {}.\n\nProjects:\n{}\nNote: \"{}\"",
            weekday_name(today),
            iso_from_days(today),
            logic::classify_context(projects, items),
            note.replace('"', "'")
        );
        let v = self.chat_json(FILE_SYSTEM, &user, 400).await?;
        Ok(read_filing(&v, projects))
    }
}

fn text_field(v: &Value, k: &str) -> Option<String> {
    v[k].as_str().map(str::trim).filter(|t| !t.is_empty() && *t != "null").map(str::to_string)
}

fn read_filing(v: &Value, projects: &[Doc]) -> ChatFiling {
    let project = v["project_id"].as_str().filter(|pid| projects.iter().any(|(id, _)| id == pid)).map(str::to_string);
    let fit = match v["fit"].as_str() {
        _ if project.is_none() => Fit::None,
        Some("clear") => Fit::Clear,
        Some("none") => Fit::None,
        _ => Fit::Unsure,
    };
    ChatFiling {
        project,
        fit,
        reason: text_field(v, "reason"),
        new_project: text_field(v, "new_project"),
        task: text_field(v, "task"),
        due: v["due"].as_str().filter(|d| parse_date(d).is_some()).map(|d| d[..10].to_string()),
    }
}

impl Jev {
    async fn pick(&self, note: &str, projects: &[Doc], items: &[Doc]) -> Result<JevPick> {
        let ordered = logic::ordered(projects);
        let criteria: Map<String, Value> = ordered.iter().map(|(pid, p)| (pid.clone(), json!(logic::s(p, "name").unwrap_or(pid)))).collect();
        let facts: Vec<Value> = ordered.iter().map(|(pid, p)| logic::project_facts(pid, p, items)).collect();
        let body = json!({
            "model": provider::JEV_MODEL,
            "state": {"note": note, "projects": facts},
            "questions": {
                "fits": {"type": "noul", "instructions": JEV_FITS},
                "project": {"type": "choice", "instructions": JEV_WHICH, "criteria": criteria}
            }
        });
        // Busy or rate-limited: TypeSafe asks for a short wait before retrying.
        let mut tries = 0;
        let (status, v) = loop {
            let resp = self.http.post(provider::JEV_URL).bearer_auth(&self.key).json(&body).send().await.map_err(|e| anyhow!("Couldn't reach Jev: {e}"))?;
            let status = resp.status().as_u16();
            if matches!(status, 429 | 529) && tries < 1 {
                tries += 1;
                tokio::time::sleep(Duration::from_millis(1500)).await;
                continue;
            }
            break (status, resp.json::<Value>().await.unwrap_or(Value::Null));
        };
        match status {
            200..=299 => {}
            401 | 403 => bail!("TypeSafe turned down the Jev key."),
            429 => bail!("Jev is limiting requests."),
            529 => bail!("Jev is overloaded right now."),
            s => bail!("Jev returned {s}: {}", v.to_string().chars().take(200).collect::<String>()),
        }
        let mut pick = read_pick(&v["answers"]["project"]).ok_or_else(|| anyhow!("Jev's answer was missing the project choice"))?;
        pick.fits = v["answers"]["fits"]["noul"].as_f64();
        Ok(pick)
    }
}

fn read_pick(a: &Value) -> Option<JevPick> {
    let choice = a["choice"].as_str()?.to_string();
    let mut ranked: Vec<(String, f64)> = a["probabilities"]
        .as_object()
        .map(|m| m.iter().filter_map(|(k, p)| Some((k.clone(), p.as_f64()?))).collect())
        .unwrap_or_default();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    Some(JevPick { choice, confidence: a["confidence"].as_f64().unwrap_or(0.0), ranked, fits: None })
}

/// Combine the two readings of a note. Jev's calibrated pick decides the project when there is
/// one; the chat model always supplies the tidied wording, the due date and any new-project name.
fn decide(note: &str, projects: &[Doc], chat: Option<ChatFiling>, jev: Option<JevPick>) -> Filed {
    let name = |id: &str| logic::project_name(projects, id);
    let (text, due, suggest) = match &chat {
        Some(c) => (c.task.clone().unwrap_or_else(|| note.to_string()), c.due.clone(), c.new_project.clone()),
        None => (note.to_string(), None, None),
    };
    let chat_doubts = chat.as_ref().map_or(true, |c| c.fit != Fit::Clear);
    let chat_reason = chat.as_ref().and_then(|c| c.reason.clone()).filter(|_| chat_doubts);
    let inbox = |reason: String, closest: Option<String>, by| Filed {
        project: "inbox".into(),
        text: text.clone(),
        due: due.clone(),
        triage: Some(Triage { reason, suggest: suggest.clone(), closest, by }),
        warning: None,
    };
    if let Some(j) = jev {
        let known = |id: &str| projects.iter().any(|(pid, _)| pid == id);
        let close: Vec<&String> = j.ranked.iter().map(|(id, _)| id).filter(|id| known(id)).take(2).collect();
        let fits = j.fits.map_or(true, |p| p >= JEV_MIN_FIT);
        if fits && known(&j.choice) && j.confidence >= JEV_MIN_CONFIDENCE {
            return Filed { project: j.choice, text, due, triage: None, warning: None };
        }
        let reason = if !fits {
            chat_reason.unwrap_or_else(|| "None of your projects clearly covers this.".into())
        } else if close.len() == 2 {
            format!("It could be {} or {}.", name(close[0]), name(close[1]))
        } else {
            "Jev wasn't sure which project this is.".into()
        };
        return inbox(reason, close.first().map(|s| s.to_string()), "jev");
    }
    let c = chat.as_ref().expect("decide needs at least one reading");
    match (&c.project, c.fit) {
        (Some(pid), Fit::Clear) => Filed { project: pid.clone(), text, due, triage: None, warning: None },
        (closest, _) => inbox(chat_reason.unwrap_or_else(|| "Work Desk couldn't tell which project this belongs to.".into()), closest.clone(), "model"),
    }
}

impl Ai {
    pub fn from_config(cfg: &Config, http: &reqwest::Client) -> Option<Ai> {
        let chat = cfg.chat().map(|c| Chat::new(c.key, c.provider, c.base, c.model, http));
        let jev = cfg.jev_key().map(|key| Jev { key, http: http.clone() });
        (chat.is_some() || jev.is_some()).then_some(Ai { chat, jev })
    }

    /// Decide which project a note belongs to (or that none clearly does), tidy its wording,
    /// and pull out a due date.
    pub async fn file_note(&self, note: &str, projects: &[Doc], items: &[Doc]) -> Result<Filed> {
        let chat = async {
            match &self.chat {
                Some(c) => Some(c.file(note, projects, items).await),
                None => None,
            }
        };
        let jev = async {
            match &self.jev {
                Some(j) => Some(j.pick(note, projects, items).await),
                None => None,
            }
        };
        let (chat, jev) = tokio::join!(chat, jev);
        let mut warning = None;
        let (chat, jev) = match (chat, jev) {
            (Some(Err(e)), None) | (None, Some(Err(e))) | (Some(Err(e)), Some(Err(_))) => return Err(e),
            (Some(Err(e)), Some(Ok(j))) => {
                warning = Some(format!("Kept your wording: {e}"));
                (None, Some(j))
            }
            (Some(Ok(c)), Some(Err(e))) => {
                warning = Some(format!("Jev didn't answer, so the chat model decided alone: {e}"));
                (Some(c), None)
            }
            (c, j) => (c.and_then(Result::ok), j.and_then(Result::ok)),
        };
        if chat.is_none() && jev.is_none() {
            bail!("No AI key yet.");
        }
        let mut filed = decide(note, projects, chat, jev);
        filed.warning = warning;
        Ok(filed)
    }

    fn writer(&self, what: &str) -> Result<&Chat> {
        self.chat
            .as_ref()
            .ok_or_else(|| anyhow!("{what} needs a chat model. Add a key from DeepSeek, OpenAI, Anthropic, Google Gemini or OpenRouter."))
    }

    pub async fn brief(&self, projects: &[Doc], items: &[Doc]) -> Result<Value> {
        self.writer("The daily brief")?.chat_json(BRIEF_SYSTEM, &logic::brief_context(projects, items), 900).await
    }
}

/// Draft a prompt for Claude Code from a task and its project, and keep it on the task.
pub async fn draft_prompt(store: &Store, ai: &Ai, id: &str) -> Result<String> {
    let item = store.get("items", id)?.ok_or_else(|| anyhow!("There's no task with id {id}."))?;
    let context = logic::prompt_context(id, &item, &store.list("projects")?, &store.list("items")?);
    let v = ai.writer("Drafting a prompt")?.chat_json(PROMPT_SYSTEM, &context, 1500).await?;
    let prompt = text_field(&v, "prompt").ok_or_else(|| anyhow!("The draft came back empty. Try again."))?;
    let mut patch = Map::new();
    patch.insert("prompt".into(), json!(prompt));
    patch.insert("updated".into(), json!(now_iso()));
    store.merge("items", id, &patch)?;
    Ok(prompt)
}

/// Save a pasted key wherever it belongs, after checking that it works. Returns the new
/// settings and a sentence saying what happened. `forced` names the service instead of guessing.
pub async fn connect(mut cfg: Config, http: &reqwest::Client, key: &str, forced: Option<&str>) -> Result<(Config, String)> {
    let key = key.trim().to_string();
    match provider::identify(http, &key, forced).await? {
        Identified::Jev => {
            cfg.jev_key = Some(key);
            Ok((cfg, "Saved your Jev key. Jev now checks which project each note belongs to.".into()))
        }
        Identified::Chat { provider, model } => {
            let (base, model) = if provider.id == "custom" {
                match (cfg.base_url.clone().filter(|b| !b.is_empty()), cfg.model.clone().filter(|m| !m.is_empty())) {
                    (Some(b), Some(m)) => (b.trim_end_matches('/').to_string(), m),
                    _ => bail!("For another service, first set base_url and model in config.json (see `desk where`)."),
                }
            } else {
                (provider.base.to_string(), model)
            };
            let chat = Chat::new(key.clone(), provider, base, model.clone(), http);
            chat.complete("Reply with the JSON object {\"ok\": true}.", "Key check", 50).await?;
            if provider.id != "custom" {
                cfg.base_url = None;
                cfg.model = (model != provider.model).then(|| model.clone());
            }
            cfg.api_key = Some(key);
            cfg.provider = Some(provider.id.into());
            Ok((cfg, format!("Saved your {} key. Work Desk will use {model}.", provider.name)))
        }
    }
}

/// Outcome of filing a new note, used by both the page and `desk add`.
pub struct Captured {
    pub id: String,
    pub doc: Map<String, Value>,
    pub notice: Option<String>,
}

/// Create an item. When `project` is "auto", Work Desk chooses the project
/// (falling back to the Inbox with a notice if it can't). When the project is already
/// chosen, the chat model still tidies the note into a task, unless `tidy: false`
/// says the text has already been through it (the page's confirm step).
pub async fn capture(store: &Store, ai: Option<&Ai>, mut doc: Map<String, Value>) -> Result<Captured> {
    let now = now_iso();
    let text = logic::s(&doc, "text").map(str::trim).unwrap_or("").to_string();
    if text.is_empty() {
        bail!("The note is empty.");
    }
    doc.insert("text".into(), json!(text));
    let tidy = doc.remove("tidy").and_then(|v| v.as_bool()).unwrap_or(true);
    let mut notice = None;
    let chosen = logic::s(&doc, "project").filter(|p| *p != "auto").map(str::to_string);
    if let (Some(pid), true, Some(chat)) = (&chosen, tidy, ai.and_then(|a| a.chat.as_ref())) {
        let projects = store.list("projects")?;
        match chat.tidy(&text, &logic::project_name(&projects, pid)).await {
            Ok((task, due)) => {
                if let Some(t) = task.filter(|t| *t != text) {
                    doc.insert("text".into(), json!(t));
                    doc.insert("original".into(), json!(text));
                }
                if let (Some(d), None) = (due, logic::s(&doc, "due")) {
                    doc.insert("due".into(), json!(d));
                }
            }
            Err(e) => notice = Some(format!("Saved as typed. {e}")),
        }
    }
    if chosen.is_none() {
        doc.insert("project".into(), json!("inbox"));
        match ai {
            None => notice = Some("Saved to the Inbox. Add an API key under AI keys to have notes filed for you.".into()),
            Some(ai) => {
                let projects = store.list("projects")?;
                let items = store.list("items")?;
                match ai.file_note(&text, &projects, &items).await {
                    Ok(f) => {
                        doc.insert("project".into(), json!(f.project));
                        if f.text != text {
                            doc.insert("text".into(), json!(f.text));
                            doc.insert("original".into(), json!(text));
                        }
                        if let (Some(d), None) = (&f.due, logic::s(&doc, "due")) {
                            doc.insert("due".into(), json!(d));
                        }
                        if let Some(t) = &f.triage {
                            doc.insert("triage".into(), t.to_json());
                            notice = Some(format!("Not sure where this goes. {} It's in the Inbox.", t.reason));
                        } else if let Some(w) = &f.warning {
                            notice = Some(format!("Filed under {}. {w}", logic::project_name(&projects, &f.project)));
                        }
                    }
                    Err(e) => notice = Some(format!("Saved to the Inbox. {e}")),
                }
            }
        }
    }
    for (k, v) in [("status", json!("open")), ("stage", json!("todo")), ("kind", json!("task"))] {
        doc.entry(k).or_insert(v);
    }
    doc.entry("created").or_insert(json!(now));
    doc.insert("updated".into(), json!(now));
    doc.remove("id");
    let id = store.new_id("t");
    store.put("items", &id, &doc)?;
    Ok(Captured { id, doc, notice })
}

/// File every open Inbox note. Notes that still don't clearly fit stay, with a fresh reason.
/// Returns (moved, still_in_inbox, first_error).
pub async fn sort_inbox(store: &Store, ai: &Ai) -> Result<(usize, usize, Option<String>)> {
    let projects = store.list("projects")?;
    let items = store.list("items")?;
    let inbox: Vec<&Doc> = items.iter().filter(|(_, m)| logic::s(m, "project") == Some("inbox") && logic::is_open(m)).collect();
    let (mut moved, mut left, mut err) = (0, 0, None);
    for (id, m) in inbox {
        let text = logic::s(m, "text").unwrap_or("");
        match ai.file_note(text, &projects, &items).await {
            Ok(f) => {
                let mut patch = Map::new();
                patch.insert("updated".into(), json!(now_iso()));
                match &f.triage {
                    None => {
                        patch.insert("project".into(), json!(f.project));
                        patch.insert("triage".into(), Value::Null);
                        if let (Some(d), None) = (&f.due, logic::s(m, "due")) {
                            patch.insert("due".into(), json!(d));
                        }
                        moved += 1;
                    }
                    Some(t) => {
                        patch.insert("triage".into(), t.to_json());
                        left += 1;
                    }
                }
                store.merge("items", id, &patch)?;
            }
            Err(e) => {
                left += 1;
                err.get_or_insert(e.to_string());
            }
        }
    }
    Ok((moved, left, err))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projects() -> Vec<Doc> {
        let p = |id: &str, name: &str| (id.to_string(), json!({ "name": name }).as_object().unwrap().clone());
        vec![p("rnaseq", "RNAseq"), p("tox", "Tox")]
    }

    fn chat(v: Value) -> Option<ChatFiling> {
        Some(read_filing(&v, &projects()))
    }

    fn jev(fits: f64, choice: &str, conf: f64, probs: Value) -> Option<JevPick> {
        let mut p = read_pick(&json!({ "type": "choice", "choice": choice, "confidence": conf, "probabilities": probs }))?;
        p.fits = Some(fits);
        Some(p)
    }

    const NOTE: &str = "CHARM off-target notes for the IND";

    #[test]
    fn chat_alone() {
        let f = decide(NOTE, &projects(), chat(json!({"project_id": "rnaseq", "fit": "clear", "task": "Tidy", "due": "2026-11-02"})), None);
        assert_eq!((f.project.as_str(), f.text.as_str(), f.due.as_deref()), ("rnaseq", "Tidy", Some("2026-11-02")));
        assert!(f.triage.is_none());

        let f = decide(
            NOTE,
            &projects(),
            chat(json!({"project_id": "rnaseq", "fit": "unsure", "reason": "Mentions CHARM.", "new_project": "CHARM IND", "task": "Tidy", "due": null})),
            None,
        );
        assert_eq!(f.project, "inbox");
        assert_eq!(
            f.triage,
            Some(Triage { reason: "Mentions CHARM.".into(), suggest: Some("CHARM IND".into()), closest: Some("rnaseq".into()), by: "model" })
        );

        // a project id that doesn't exist is never trusted
        let f = decide(NOTE, &projects(), chat(json!({"project_id": "charm", "fit": "clear", "task": "Tidy"})), None);
        assert_eq!(f.project, "inbox");
        assert_eq!(f.triage.unwrap().closest, None);
    }

    #[test]
    fn jev_decides_the_project() {
        let clear = chat(json!({"project_id": "rnaseq", "fit": "clear", "reason": "Off-target.", "task": "Tidy"}));
        // confident: filed, with the chat model's wording
        let f = decide(NOTE, &projects(), clear, jev(0.9, "tox", 0.8, json!({"tox": 0.9, "rnaseq": 0.1})));
        assert_eq!((f.project.as_str(), f.text.as_str()), ("tox", "Tidy"));

        // doesn't fit any project: Inbox, and the chat model's confident reason isn't shown as the reason
        let clear = chat(json!({"project_id": "rnaseq", "fit": "clear", "reason": "Off-target.", "new_project": null, "task": "Tidy"}));
        let f = decide(NOTE, &projects(), clear, jev(0.2, "rnaseq", 0.9, json!({"rnaseq": 0.95, "tox": 0.05})));
        let t = f.triage.unwrap();
        assert_eq!((t.reason.as_str(), t.closest.as_deref(), t.by), ("None of your projects clearly covers this.", Some("rnaseq"), "jev"));

        // torn between two
        let f = decide(NOTE, &projects(), None, jev(0.9, "rnaseq", 0.06, json!({"rnaseq": 0.53, "tox": 0.47})));
        assert_eq!(f.text, NOTE);
        assert_eq!(f.triage.unwrap().reason, "It could be RNAseq or Tox.");
    }

    #[test]
    fn relaxes_reasoning_model_params() {
        let mut b = json!({"temperature": 0.2, "max_tokens": 300, "response_format": {}});
        assert!(relax(&mut b, "Unsupported parameter: 'max_tokens' is not supported with this model. Use 'max_completion_tokens' instead."));
        assert_eq!(b["max_completion_tokens"], 3000);
        assert!(relax(&mut b, "Unsupported value: 'temperature' does not support 0.2"));
        assert!(b.get("temperature").is_none());
        assert!(!relax(&mut b, "Something else"));
    }
}
