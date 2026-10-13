//! DeepSeek integration (OpenAI-compatible chat completions API).

use crate::config::Config;
use crate::logic;
use crate::store::{Doc, Store};
use crate::util::*;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Map, Value};
use std::time::Duration;

#[derive(Clone)]
pub struct Ai {
    key: String,
    model: String,
    base: String,
    http: reqwest::Client,
}

pub struct Filed {
    pub project: String,
    pub text: String,
    pub due: Option<String>,
}

const FILE_SYSTEM: &str = "You file quick sticky notes into a researcher's project tracker.\n\
Pick the single project the note belongs to, using the project names, goals and example tasks. \
Researchers abbreviate: 'NH' means natural history, 'tox' means toxicology, and people's first names \
often point to a collaborator on one project. If no project clearly fits, use \"inbox\".\n\
Rewrite the note as one short, concrete task in the user's own words. Keep names, numbers, gene and \
variant names, and technical terms exactly as written. Drop filler such as 'remember to' or 'I need to'. \
Remove the deadline phrase from the task text once you have captured it as a date.\n\
If the note implies a deadline ('by Friday', 'next Tuesday', 'end of the month', 'tomorrow'), convert it \
to an ISO date relative to today. A bare weekday means the next occurrence of that day on or after today. \
Otherwise use null.\n\
Respond with a JSON object only: {\"project_id\": string, \"task\": string, \"due\": string or null}.";

const BRIEF_SYSTEM: &str = "You are a calm, practical executive assistant for a researcher who struggles \
with executive function and with getting things fully finished. Using their task list, decide what \
matters today. Prefer finishing work that is already in progress or in review over starting new work. \
Respect deadlines. Point out anything overdue, stalled, or at risk of missing a deadline. Keep every \
line short, specific and kind; use the exact task wording so they can find it.\n\
Respond with a JSON object only:\n\
{\"headline\": one sentence about today,\n \"focus\": up to 3 objects {\"task\": exact task text, \"project\": project name, \"why\": short reason},\n \"watch\": up to 3 short warnings,\n \"quick_wins\": up to 3 tasks likely to take under 30 minutes,\n \"first_step\": one tiny concrete action to start in the next 5 minutes}";

impl Ai {
    pub fn from_config(cfg: &Config, http: &reqwest::Client) -> Option<Ai> {
        Some(Ai { key: cfg.key()?, model: cfg.model(), base: cfg.base_url(), http: http.clone() })
    }

    pub fn http_client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(90))
            .connect_timeout(Duration::from_secs(15))
            .user_agent(concat!("workdesk/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("http client")
    }

    async fn chat_json(&self, system: &str, user: &str, max_tokens: u32) -> Result<Value> {
        let body = json!({
            "model": self.model,
            "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
            "response_format": {"type": "json_object"},
            "temperature": 0.2,
            "max_tokens": max_tokens,
            "stream": false
        });
        let resp = self
            .http
            .post(format!("{}/chat/completions", self.base))
            .bearer_auth(&self.key)
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow!("Could not reach DeepSeek: {e}"))?;
        let status = resp.status();
        let text = resp.text().await?;
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if !status.is_success() {
            let msg = v["error"]["message"].as_str().map(str::to_string).unwrap_or_else(|| text.chars().take(200).collect());
            match status.as_u16() {
                401 => bail!("DeepSeek rejected the API key. Check it with `desk key`."),
                402 => bail!("DeepSeek says the account balance is too low."),
                429 => bail!("DeepSeek is rate limiting requests. Try again in a minute."),
                _ => bail!("DeepSeek returned {status}: {msg}"),
            }
        }
        let content = v["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow!("DeepSeek sent an empty reply"))?;
        let trimmed = content.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
        serde_json::from_str(trimmed).map_err(|_| anyhow!("DeepSeek's reply was not valid JSON"))
    }

    /// Decide which project a note belongs to, tidy its wording, and pull out a due date.
    pub async fn file_note(&self, note: &str, projects: &[Doc], items: &[Doc]) -> Result<Filed> {
        let today = today_days();
        let user = format!(
            "Today is {} {}.\n\nProjects:\n{}\nNote: \"{}\"",
            weekday_name(today),
            iso_from_days(today),
            logic::classify_context(projects, items),
            note.replace('"', "'")
        );
        let v = self.chat_json(FILE_SYSTEM, &user, 300).await?;
        let pid = v["project_id"].as_str().unwrap_or("inbox");
        let project = if projects.iter().any(|(id, _)| id == pid) { pid.to_string() } else { "inbox".to_string() };
        let text = v["task"].as_str().map(str::trim).filter(|t| !t.is_empty()).unwrap_or(note).to_string();
        let due = v["due"].as_str().filter(|d| parse_date(d).is_some()).map(|d| d[..10].to_string());
        Ok(Filed { project, text, due })
    }

    pub async fn brief(&self, projects: &[Doc], items: &[Doc]) -> Result<Value> {
        let user = logic::brief_context(projects, items);
        self.chat_json(BRIEF_SYSTEM, &user, 900).await
    }
}

/// Outcome of filing a new note, used by both the page and `desk add`.
pub struct Captured {
    pub id: String,
    pub doc: Map<String, Value>,
    pub notice: Option<String>,
}

/// Create an item. When `project` is "auto", DeepSeek chooses the project
/// (falling back to the Inbox with a notice if it can't).
pub async fn capture(store: &Store, ai: Option<&Ai>, mut doc: Map<String, Value>) -> Result<Captured> {
    let now = now_iso();
    let text = logic::s(&doc, "text").map(str::trim).unwrap_or("").to_string();
    if text.is_empty() {
        bail!("The note is empty.");
    }
    doc.insert("text".into(), json!(text));
    let mut notice = None;
    if logic::s(&doc, "project").map_or(true, |p| p == "auto") {
        match ai {
            None => {
                doc.insert("project".into(), json!("inbox"));
                notice = Some("Saved to the Inbox. Add a DeepSeek key to have notes filed automatically.".into());
            }
            Some(ai) => {
                let projects = store.list("projects")?;
                let items = store.list("items")?;
                match ai.file_note(&text, &projects, &items).await {
                    Ok(f) => {
                        doc.insert("project".into(), json!(f.project));
                        doc.insert("text".into(), json!(f.text));
                        doc.insert("original".into(), json!(text));
                        if let Some(d) = f.due {
                            if logic::s(&doc, "due").is_none() {
                                doc.insert("due".into(), json!(d));
                            }
                        }
                        if f.project == "inbox" {
                            notice = Some("DeepSeek couldn't tell which project this is, so it's in the Inbox.".into());
                        }
                    }
                    Err(e) => {
                        doc.insert("project".into(), json!("inbox"));
                        notice = Some(format!("Saved to the Inbox. {e}"));
                    }
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

/// File every open Inbox note. Returns (moved, still_in_inbox, first_error).
pub async fn sort_inbox(store: &Store, ai: &Ai) -> Result<(usize, usize, Option<String>)> {
    let projects = store.list("projects")?;
    let items = store.list("items")?;
    let inbox: Vec<&Doc> = items.iter().filter(|(_, m)| logic::s(m, "project") == Some("inbox") && logic::is_open(m)).collect();
    let (mut moved, mut left, mut err) = (0, 0, None);
    for (id, m) in inbox {
        let text = logic::s(m, "text").unwrap_or("");
        match ai.file_note(text, &projects, &items).await {
            Ok(f) if f.project != "inbox" => {
                let mut patch = Map::new();
                patch.insert("project".into(), json!(f.project));
                patch.insert("updated".into(), json!(now_iso()));
                if let (Some(d), None) = (f.due, logic::s(m, "due")) {
                    patch.insert("due".into(), json!(d));
                }
                store.merge("items", id, &patch)?;
                moved += 1;
            }
            Ok(_) => left += 1,
            Err(e) => {
                left += 1;
                err.get_or_insert(e.to_string());
            }
        }
    }
    Ok((moved, left, err))
}
