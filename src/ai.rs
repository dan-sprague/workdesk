//! The AI side of Work Desk.
//!
//! - Jev (TypeSafe; see jev.rs), when there's a key for it, makes the structured calls:
//!   which project, whether a note fits any or repeats a task, task sizes and so on.
//! - A chat model (any OpenAI-compatible service; see provider.rs) does the writing: it
//!   tidies notes into tasks, catches due dates, writes the brief and drafts prompts,
//!   with Jev's judgments in front of it. Without Jev it makes those calls itself.

use crate::config::Config;
use crate::jev::{self, Jev, NoteReading};
use crate::logic;
use crate::progress::{self, Progress};
use crate::provider::{self, Identified, Provider};
use crate::store::{Doc, Store};
use crate::util::*;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Map, Value};
use std::time::Duration;

#[derive(Clone)]
pub struct Chat {
    key: String,
    model: String,
    base: String,
    provider: &'static Provider,
    http: reqwest::Client,
}

/// Whatever AI there are keys for: a chat model, Jev, or both.
#[derive(Clone)]
pub struct Ai {
    pub chat: Option<Chat>,
    pub jev: Option<Jev>,
}

/// One task made from a note.
#[derive(Clone, Debug, PartialEq)]
pub struct NewTask {
    pub text: String,
    pub due: Option<String>,
}

/// Where a note went. `triage` is set when it went to the Inbox because no project clearly fit.
pub struct Filed {
    pub project: String,
    /// Usually one; several when the note held several separate to-dos.
    pub tasks: Vec<NewTask>,
    pub triage: Option<Triage>,
    /// One of the two AIs failed and the other decided alone.
    pub warning: Option<String>,
    /// An open task Jev thinks this repeats: (id, text, probability).
    pub duplicate: Option<(String, String, f64)>,
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

impl Filed {
    pub fn duplicate_json(&self) -> Value {
        match &self.duplicate {
            Some((id, text, p)) => json!({ "id": id, "text": text, "p": p }),
            None => Value::Null,
        }
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
    tasks: Vec<NewTask>,
}

/// How a note becomes tasks, shared by filing and by tidying a note whose project is already chosen.
macro_rules! rewrite_rules {
    () => {
        "Rewrite the note as a short, concrete task in the user's own words. If it holds several separate to-dos \
(such as action items from a meeting), make each its own task; otherwise make exactly one. Keep names, numbers, \
gene and variant names, and technical terms exactly as written. Drop filler such as 'remember to' or 'I need to'. \
Remove a deadline phrase from the task text once you have captured it as a date.\n\
If a task has a deadline ('by Friday', 'next Tuesday', 'end of the month', 'tomorrow'), convert it to an ISO \
date relative to today. A bare weekday means the next occurrence of that day on or after today. Otherwise use null.\n\
When the message includes Jev's judgments, Jev is a calibrated classifier: if it says the note holds several \
to-dos at 50% or more, split it; below that, keep it as one task unless the note plainly lists separate items.\n"
    };
}

const TIDY_SYSTEM: &str = concat!(
    "You turn a quick sticky note into tasks for a researcher's project tracker. The project is already chosen.\n",
    rewrite_rules!(),
    "Respond with a JSON object only: {\"tasks\": [{\"task\": string, \"due\": string or null}]}."
);

const FILE_SYSTEM: &str = concat!(
    "You file quick sticky notes into a researcher's project tracker.\n\
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
sentence naming the words that decided it.\n\
When the message includes Jev's judgments about the project, they decide it: below 50% that the note fits any \
project means fit \"none\"; otherwise use Jev's most likely project. Write reason and new_project to agree.\n",
    rewrite_rules!(),
    "Respond with a JSON object only: {\"project_id\": string, \"fit\": \"clear\" | \"unsure\" | \"none\", \
\"reason\": string, \"new_project\": string or null, \"tasks\": [{\"task\": string, \"due\": string or null}]}."
);

const BRIEF_SYSTEM: &str = "You are a calm, practical executive assistant for a researcher who struggles \
with executive function and with getting things fully finished. Using their task list, decide what \
matters today. Prefer finishing work that is already in progress or in review over starting new work. \
Respect deadlines. Point out anything overdue, stalled, or at risk of missing a deadline. Keep every \
line short, specific and kind; use the exact task wording so they can find it.\n\
When the message includes a plan that Work Desk computed from Jev's judgments (task sizes, waiting, \
importance, vagueness), build on it: take focus from its Today list in that order, quick wins from its \
Quick wins, and warnings from At risk, Waiting and Too vague. Don't invent other time estimates.\n\
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
When the message includes progress in the project folder (scripts, notebooks, commits, uncommitted changes), \
use it to see what has already been done: point Claude at the existing scripts and notebooks to build on rather \
than starting over, say what's left, and have Claude begin by checking the current state of those files and any \
uncommitted changes before changing anything. Don't call something done unless the progress shows it.\n\
When the message includes the current prompt, revise that prompt for the progress shown instead of writing a new \
one: keep the researcher's wording and edits unless the progress makes them out of date, mark finished steps as \
done or drop them, and update what comes next.\n\
When the message includes Jev's judgments about the task, use them: shape the prompt for the kind of work Jev \
names; if Jev puts the task at 50% or more too vague, have Claude first agree with the researcher on what done \
looks like before changing anything; name only the linked files Jev says are likely needed, and focus on the \
scripts and notebooks Jev says are part of this task.\n\
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

    /// Tidy a note into tasks for a project that's already chosen.
    async fn tidy(&self, note: &str, project: &str, jev_facts: Option<&str>) -> Result<Vec<NewTask>> {
        let today = today_days();
        let user = format!(
            "Today is {} {}.\nProject: {project}\nNote: \"{}\"\n{}",
            weekday_name(today),
            iso_from_days(today),
            note.replace('"', "'"),
            jev_facts.unwrap_or("")
        );
        let v = self.chat_json(TIDY_SYSTEM, &user, 800).await?;
        Ok(read_filing(&v, &[]).tasks)
    }

    async fn file(&self, note: &str, projects: &[Doc], items: &[Doc], jev_facts: Option<&str>) -> Result<ChatFiling> {
        let today = today_days();
        let user = format!(
            "Today is {} {}.\n\nProjects:\n{}\nNote: \"{}\"\n{}",
            weekday_name(today),
            iso_from_days(today),
            logic::classify_context(projects, items),
            note.replace('"', "'"),
            jev_facts.unwrap_or("")
        );
        let v = self.chat_json(FILE_SYSTEM, &user, 900).await?;
        Ok(read_filing(&v, projects))
    }
}

fn text_field(v: &Value, k: &str) -> Option<String> {
    v[k].as_str().map(str::trim).filter(|t| !t.is_empty() && *t != "null").map(str::to_string)
}

fn due_field(v: &Value) -> Option<String> {
    v["due"].as_str().filter(|d| parse_date(d).is_some()).map(|d| d[..10].to_string())
}

fn read_filing(v: &Value, projects: &[Doc]) -> ChatFiling {
    let project = v["project_id"].as_str().filter(|pid| projects.iter().any(|(id, _)| id == pid)).map(str::to_string);
    let fit = match v["fit"].as_str() {
        _ if project.is_none() => Fit::None,
        Some("clear") => Fit::Clear,
        Some("none") => Fit::None,
        _ => Fit::Unsure,
    };
    // {"tasks": [...]}, or the older single {"task", "due"}
    let mut tasks: Vec<NewTask> = v["tasks"]
        .as_array()
        .map(|a| a.iter().filter_map(|t| Some(NewTask { text: text_field(t, "task")?, due: due_field(t) })).collect())
        .unwrap_or_default();
    if tasks.is_empty() {
        if let Some(text) = text_field(v, "task") {
            tasks.push(NewTask { text, due: due_field(v) });
        }
    }
    ChatFiling { project, fit, reason: text_field(v, "reason"), new_project: text_field(v, "new_project"), tasks }
}

/// Combine the two readings of a note. Jev's calibrated judgments decide the project when there
/// are some; the chat model supplies the tasks' wording, due dates and any new-project name.
fn decide(note: &str, projects: &[Doc], chat: Option<ChatFiling>, jev: Option<&NoteReading>) -> Filed {
    let name = |id: &str| logic::project_name(projects, id);
    let (tasks, suggest) = match &chat {
        Some(c) if !c.tasks.is_empty() => (c.tasks.clone(), c.new_project.clone()),
        Some(c) => (vec![NewTask { text: note.to_string(), due: None }], c.new_project.clone()),
        None => (vec![NewTask { text: note.to_string(), due: None }], None),
    };
    let chat_doubts = chat.as_ref().map_or(true, |c| c.fit != Fit::Clear);
    let chat_reason = chat.as_ref().and_then(|c| c.reason.clone()).filter(|_| chat_doubts);
    let filed = |project: String, triage| Filed { project, tasks: tasks.clone(), triage, warning: None, duplicate: None };
    if let Some(pick) = jev.and_then(|j| j.pick.as_ref()) {
        let known = |id: &str| projects.iter().any(|(pid, _)| pid == id);
        let close: Vec<&String> = pick.ranked.iter().map(|(id, _)| id).filter(|id| known(id)).take(2).collect();
        let fits = jev.and_then(|j| j.fits).map_or(true, |p| p >= jev::MIN_FIT);
        if fits && known(&pick.choice) && pick.confidence >= jev::MIN_CONFIDENCE {
            return filed(pick.choice.clone(), None);
        }
        let reason = if !fits {
            chat_reason.unwrap_or_else(|| "None of your projects clearly covers this.".into())
        } else if close.len() == 2 {
            format!("It could be {} or {}.", name(close[0]), name(close[1]))
        } else {
            "Jev wasn't sure which project this is.".into()
        };
        let closest = close.first().map(|s| s.to_string());
        return filed("inbox".into(), Some(Triage { reason, suggest, closest, by: "jev" }));
    }
    match chat.as_ref().map(|c| (&c.project, c.fit)) {
        Some((Some(pid), Fit::Clear)) => filed(pid.clone(), None),
        other => {
            let reason = chat_reason.unwrap_or_else(|| "Work Desk couldn't tell which project this belongs to.".into());
            let closest = other.and_then(|(p, _)| p.clone());
            filed("inbox".into(), Some(Triage { reason, suggest, closest, by: "model" }))
        }
    }
}

impl Ai {
    pub fn from_config(cfg: &Config, http: &reqwest::Client) -> Option<Ai> {
        let chat = cfg.chat().map(|c| Chat::new(c.key, c.provider, c.base, c.model, http));
        let jev = cfg.jev_key().map(|key| Jev::new(key, http));
        (chat.is_some() || jev.is_some()).then_some(Ai { chat, jev })
    }

    /// Decide which project a note belongs to (or that none clearly does), turn it into tasks
    /// with due dates, and check whether it repeats an open task. Jev answers first; the chat
    /// model writes with Jev's answers in front of it.
    pub async fn file_note(&self, note: &str, projects: &[Doc], items: &[Doc]) -> Result<Filed> {
        let mut warning = None;
        let reading = match &self.jev {
            Some(j) => match j.read_note(note, projects, items).await {
                Ok(r) => Some(r),
                Err(e) if self.chat.is_some() => {
                    warning = Some(format!("Jev didn't answer, so the chat model decided alone: {e}"));
                    None
                }
                Err(e) => return Err(e),
            },
            None => None,
        };
        let facts = reading.as_ref().map(|r| r.facts(projects));
        let chat = match &self.chat {
            Some(c) => match c.file(note, projects, items, facts.as_deref()).await {
                Ok(f) => Some(f),
                Err(e) if reading.is_some() => {
                    warning = Some(format!("Kept your wording: {e}"));
                    None
                }
                Err(e) => return Err(e),
            },
            None => None,
        };
        if chat.is_none() && reading.is_none() {
            bail!("No AI key yet.");
        }
        let mut filed = decide(note, projects, chat, reading.as_ref());
        filed.warning = warning;
        if let (Some(j), [only], false) = (&self.jev, filed.tasks.as_slice(), filed.project == "inbox") {
            let open: Vec<(String, String)> = items
                .iter()
                .filter(|(_, m)| logic::s(m, "project") == Some(filed.project.as_str()) && logic::is_open(m))
                .filter_map(|(id, m)| Some((id.clone(), logic::s(m, "text")?.to_string())))
                .collect();
            if let Ok(Some((id, p))) = j.duplicate_of(&only.text, &open).await {
                let text = open.iter().find(|(i, _)| *i == id).map(|(_, t)| t.clone()).unwrap_or_default();
                filed.duplicate = Some((id, text, p));
            }
        }
        Ok(filed)
    }

    fn writer(&self, what: &str) -> Result<&Chat> {
        self.chat
            .as_ref()
            .ok_or_else(|| anyhow!("{what} needs a chat model. Add a key from DeepSeek, OpenAI, Anthropic, Google Gemini or OpenRouter."))
    }

    /// The start-of-day brief. With Jev, every open task is sized and judged first (only the
    /// ones that changed), Work Desk works out a plan from that, and the chat model writes it up.
    pub async fn brief(&self, store: &Store, hours_per_day: f64) -> Result<Value> {
        let chat = self.writer("The daily brief")?;
        let mut trouble = None;
        if let Some(j) = &self.jev {
            match j.judge_open_tasks(store).await {
                Ok((_, e)) => trouble = e,
                Err(e) => trouble = Some(e.to_string()),
            }
        }
        let projects = store.list("projects")?;
        let items = store.list("items")?;
        let mut context = logic::brief_context(&projects, &items);
        if let Some(plan) = logic::plan_text(&projects, &items, hours_per_day) {
            context.push('\n');
            context.push_str(&plan);
        }
        let mut v = chat.chat_json(BRIEF_SYSTEM, &context, 900).await?;
        if let Some(t) = trouble {
            v["note"] = json!(format!("Jev couldn't judge some tasks: {t}"));
        }
        Ok(v)
    }
}

/// What's been done so far in a task's project folder (`dirs`, e.g. scripts/ and notebooks/).
pub fn task_progress(projects: &[Doc], item: &Map<String, Value>, dirs: &[String]) -> Option<Progress> {
    let pid = logic::s(item, "project")?;
    let folder = projects.iter().find(|(p, _)| p == pid).and_then(|(_, p)| logic::s(p, "folder"))?;
    progress::scan(&crate::config::expand_home(folder), dirs)
}

/// Whether the project folder has moved on since a task's prompt was drafted: what changed.
pub fn prompt_outdated(store: &Store, id: &str, dirs: &[String]) -> Result<Vec<String>> {
    let item = store.get("items", id)?.ok_or_else(|| anyhow!("There's no task with id {id}."))?;
    if logic::s(&item, "prompt").is_none() {
        return Ok(Vec::new());
    }
    let stamp = item.get("prompt_stamp").and_then(Value::as_i64).unwrap_or(0);
    Ok(task_progress(&store.list("projects")?, &item, dirs).map(|p| p.changed_since(stamp)).unwrap_or_default())
}

/// Draft a prompt for Claude Code from a task, its project, and what's already been done in the
/// project folder, and keep it on the task. Unless `fresh`, an existing prompt is revised for the
/// progress since, keeping the researcher's edits.
pub async fn draft_prompt(store: &Store, ai: &Ai, id: &str, dirs: &[String], fresh: bool) -> Result<String> {
    let chat = ai.writer("Drafting a prompt")?;
    let item = store.get("items", id)?.ok_or_else(|| anyhow!("There's no task with id {id}."))?;
    let projects = store.list("projects")?;
    let mut context = logic::prompt_context(id, &item, &projects, &store.list("items")?);
    let progress = task_progress(&projects, &item, dirs);
    if let Some(p) = &progress {
        context.push_str(&p.text());
    }
    if let Some(j) = &ai.jev {
        let project = logic::s(&item, "project").and_then(|pid| projects.iter().find(|(p, _)| p == pid)).map(|(_, p)| p);
        let folder = project.and_then(|p| logic::s(p, "folder"));
        let files: Vec<String> = [Some(&item), project]
            .into_iter()
            .flatten()
            .flat_map(logic::file_paths)
            .map(|f| logic::from_folder(f, folder))
            .collect();
        if let Ok(r) = j.read_for_prompt(&item, project, &files, progress.as_ref()).await {
            context.push_str(&r.facts());
        }
    }
    if let (false, Some(current)) = (fresh, logic::s(&item, "prompt")) {
        context.push_str(&format!("\nThe current prompt, to revise (the researcher may have edited it):\n<<<\n{current}\n>>>\n"));
    }
    let v = chat.chat_json(PROMPT_SYSTEM, &context, 1800).await?;
    let prompt = text_field(&v, "prompt").ok_or_else(|| anyhow!("The draft came back empty. Try again."))?;
    let mut patch = Map::new();
    patch.insert("prompt".into(), json!(prompt));
    patch.insert("prompt_stamp".into(), json!(progress.as_ref().map_or(0, Progress::latest)));
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
            Ok((cfg, "Saved your Jev key. Jev now makes the structured calls: projects, task sizes, duplicates and more.".into()))
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
}

/// Create items from a note: usually one, several when the note held several to-dos.
/// When `project` is "auto", Work Desk chooses the project (falling back to the Inbox with a
/// notice if it can't). When the project is already chosen, the note is still tidied into
/// tasks, unless `tidy: false` says the text has already been through that (the page's
/// confirm step). Returns the items and a notice for the person, if any.
pub async fn capture(store: &Store, ai: Option<&Ai>, mut doc: Map<String, Value>) -> Result<(Vec<Captured>, Option<String>)> {
    let now = now_iso();
    let text = logic::s(&doc, "text").map(str::trim).unwrap_or("").to_string();
    if text.is_empty() {
        bail!("The note is empty.");
    }
    doc.insert("text".into(), json!(text));
    let tidy = doc.remove("tidy").and_then(|v| v.as_bool()).unwrap_or(true);
    let mut notes: Vec<String> = Vec::new();
    let mut tasks = vec![NewTask { text: text.clone(), due: None }];
    let chosen = logic::s(&doc, "project").filter(|p| *p != "auto").map(str::to_string);
    if let (Some(pid), true, Some(ai)) = (&chosen, tidy, ai) {
        if let Some(chat) = &ai.chat {
            let projects = store.list("projects")?;
            // Jev's only question here is whether the note holds several to-dos.
            let facts = match &ai.jev {
                Some(j) => j.read_note(&text, &[], &[]).await.ok().map(|r| r.facts(&projects)),
                None => None,
            };
            match chat.tidy(&text, &logic::project_name(&projects, pid), facts.as_deref()).await {
                Ok(t) if !t.is_empty() => tasks = t,
                Ok(_) => {}
                Err(e) => notes.push(format!("Saved as typed. {e}")),
            }
        }
    }
    if chosen.is_none() {
        doc.insert("project".into(), json!("inbox"));
        match ai {
            None => notes.push("Saved to the Inbox. Add an API key under AI keys to have notes filed for you.".into()),
            Some(ai) => {
                let projects = store.list("projects")?;
                let items = store.list("items")?;
                match ai.file_note(&text, &projects, &items).await {
                    Ok(f) => {
                        doc.insert("project".into(), json!(f.project));
                        if let Some(t) = &f.triage {
                            doc.insert("triage".into(), t.to_json());
                            notes.push(format!("Not sure where this goes. {} It's in the Inbox.", t.reason));
                        } else if let Some(w) = &f.warning {
                            notes.push(format!("Filed under {}. {w}", logic::project_name(&projects, &f.project)));
                        }
                        if let Some((_, t, p)) = &f.duplicate {
                            notes.push(format!("Jev thinks this may repeat “{t}” ({:.0}%).", p * 100.0));
                        }
                        tasks = f.tasks;
                    }
                    Err(e) => notes.push(format!("Saved to the Inbox. {e}")),
                }
            }
        }
    }
    if tasks.len() > 1 {
        notes.push(format!("Split into {} tasks.", tasks.len()));
    }
    for (k, v) in [("status", json!("open")), ("stage", json!("todo")), ("kind", json!("task"))] {
        doc.entry(k).or_insert(v);
    }
    doc.entry("created").or_insert(json!(now));
    doc.insert("updated".into(), json!(now));
    doc.remove("id");
    let mut out = Vec::new();
    for t in tasks {
        let mut d = doc.clone();
        if t.text != text {
            d.insert("text".into(), json!(t.text));
            d.entry("original").or_insert(json!(text));
        }
        if let (Some(due), None) = (&t.due, logic::s(&d, "due")) {
            d.insert("due".into(), json!(due));
        }
        let id = store.new_id("t");
        store.put("items", &id, &d)?;
        out.push(Captured { id, doc: d });
    }
    Ok((out, (!notes.is_empty()).then(|| notes.join(" "))))
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
                        if let (Some(d), None) = (f.tasks.first().and_then(|t| t.due.as_ref()), logic::s(m, "due")) {
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

    fn jev(fits: f64, choice: &str, conf: f64, probs: Value) -> NoteReading {
        let pick = jev::read_pick(&json!({ "type": "choice", "choice": choice, "confidence": conf, "probabilities": probs }));
        NoteReading { fits: Some(fits), pick, several: Some(0.1) }
    }

    fn one(text: &str, due: Option<&str>) -> Vec<NewTask> {
        vec![NewTask { text: text.into(), due: due.map(str::to_string) }]
    }

    const NOTE: &str = "CHARM off-target notes for the IND";

    #[test]
    fn chat_alone() {
        let f = decide(NOTE, &projects(), chat(json!({"project_id": "rnaseq", "fit": "clear", "tasks": [{"task": "Tidy", "due": "2026-11-02"}]})), None);
        assert_eq!((f.project.as_str(), f.tasks), ("rnaseq", one("Tidy", Some("2026-11-02"))));
        assert!(f.triage.is_none());

        let f = decide(
            NOTE,
            &projects(),
            chat(json!({"project_id": "rnaseq", "fit": "unsure", "reason": "Mentions CHARM.", "new_project": "CHARM IND", "tasks": [{"task": "Tidy", "due": null}]})),
            None,
        );
        assert_eq!(f.project, "inbox");
        assert_eq!(
            f.triage,
            Some(Triage { reason: "Mentions CHARM.".into(), suggest: Some("CHARM IND".into()), closest: Some("rnaseq".into()), by: "model" })
        );

        // a project id that doesn't exist is never trusted; the older single-task reply still reads
        let f = decide(NOTE, &projects(), chat(json!({"project_id": "charm", "fit": "clear", "task": "Tidy"})), None);
        assert_eq!((f.project.as_str(), f.tasks), ("inbox", one("Tidy", None)));
        assert_eq!(f.triage.unwrap().closest, None);
    }

    #[test]
    fn meeting_notes_split() {
        let f = decide(
            NOTE,
            &projects(),
            chat(json!({"project_id": "rnaseq", "fit": "clear", "tasks": [{"task": "Table the knockup data", "due": null}, {"task": "Ask Plasmidsaurus about flow cells", "due": "2026-10-24"}]})),
            None,
        );
        assert_eq!(f.tasks.len(), 2);
        assert_eq!(f.tasks[1].due.as_deref(), Some("2026-10-24"));
    }

    #[test]
    fn jev_decides_the_project() {
        let clear = chat(json!({"project_id": "rnaseq", "fit": "clear", "reason": "Off-target.", "tasks": [{"task": "Tidy"}]}));
        // confident: filed, with the chat model's wording
        let f = decide(NOTE, &projects(), clear, Some(&jev(0.9, "tox", 0.8, json!({"tox": 0.9, "rnaseq": 0.1}))));
        assert_eq!((f.project.as_str(), f.tasks), ("tox", one("Tidy", None)));

        // doesn't fit any project: Inbox, and the chat model's confident reason isn't shown as the reason
        let clear = chat(json!({"project_id": "rnaseq", "fit": "clear", "reason": "Off-target.", "new_project": null, "tasks": [{"task": "Tidy"}]}));
        let f = decide(NOTE, &projects(), clear, Some(&jev(0.2, "rnaseq", 0.9, json!({"rnaseq": 0.95, "tox": 0.05}))));
        let t = f.triage.unwrap();
        assert_eq!((t.reason.as_str(), t.closest.as_deref(), t.by), ("None of your projects clearly covers this.", Some("rnaseq"), "jev"));

        // torn between two, and no chat model: the note is kept as typed
        let f = decide(NOTE, &projects(), None, Some(&jev(0.9, "rnaseq", 0.06, json!({"rnaseq": 0.53, "tox": 0.47}))));
        assert_eq!(f.tasks, one(NOTE, None));
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
