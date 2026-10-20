//! Which AI service an API key belongs to. Work Desk takes a key for any of these
//! and works the service out from the key itself, trying the key against each
//! candidate when its shape alone can't tell (DeepSeek and OpenAI keys both start
//! with `sk-`, and TypeSafe doesn't document a prefix for Jev keys).

use anyhow::{bail, Result};
use serde_json::Value;

pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    /// OpenAI-compatible API root; `/chat/completions` and `/models` hang off it.
    pub base: &'static str,
    /// Used unless the service's model list no longer has it (see `pick_model`).
    pub model: &'static str,
    /// Words that all appear in the name of a good everyday model on this service.
    pub prefer: &'static [&'static str],
}

pub const PROVIDERS: &[Provider] = &[
    Provider { id: "deepseek", name: "DeepSeek", base: "https://api.deepseek.com", model: "deepseek-chat", prefer: &["deepseek-chat"] },
    Provider { id: "openai", name: "OpenAI", base: "https://api.openai.com/v1", model: "gpt-4.1-mini", prefer: &["gpt-", "mini"] },
    Provider { id: "anthropic", name: "Anthropic", base: "https://api.anthropic.com/v1", model: "claude-sonnet-5-5", prefer: &["claude-", "sonnet"] },
    Provider {
        id: "gemini",
        name: "Google Gemini",
        base: "https://generativelanguage.googleapis.com/v1beta/openai",
        model: "gemini-2.5-flash",
        prefer: &["gemini-", "flash"],
    },
    Provider { id: "openrouter", name: "OpenRouter", base: "https://openrouter.ai/api/v1", model: "deepseek/deepseek-chat", prefer: &["deepseek/deepseek-chat"] },
    // Anything else OpenAI-compatible: set `base_url` and `model` in config.json.
    Provider { id: "custom", name: "your AI service", base: "", model: "", prefer: &[] },
];

/// TypeSafe's API root. `TYPESAFE_API_BASE` points Work Desk at another one (e.g. a local stand-in for tests).
pub fn jev_base() -> String {
    std::env::var("TYPESAFE_API_BASE").ok().filter(|b| !b.trim().is_empty()).unwrap_or_else(|| "https://api.typesafe.ai/v1".into())
}
pub const JEV_MODEL: &str = "jev-latest";

/// Services a pasted key is matched against, for messages.
pub const SUPPORTED: &str = "DeepSeek, OpenAI, Anthropic, Google Gemini, OpenRouter or TypeSafe (Jev)";

pub fn by_id(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// The service a key's prefix gives away, if it does.
pub fn by_prefix(key: &str) -> Option<&'static Provider> {
    let id = if key.starts_with("sk-ant-") {
        "anthropic"
    } else if key.starts_with("sk-or-") {
        "openrouter"
    } else if key.starts_with("AIza") {
        "gemini"
    } else if ["sk-proj-", "sk-svcacct-", "sk-admin-"].iter().any(|p| key.starts_with(p)) {
        "openai"
    } else {
        return None;
    };
    by_id(id)
}

pub enum Identified {
    Chat { provider: &'static Provider, model: String },
    Jev,
}

/// Add the service's auth headers to a request.
pub fn auth(req: reqwest::RequestBuilder, provider: &Provider, key: &str) -> reqwest::RequestBuilder {
    let req = req.bearer_auth(key);
    if provider.id == "anthropic" {
        req.header("x-api-key", key).header("anthropic-version", "2023-06-01")
    } else {
        req
    }
}

/// The service's model list: Some(ids) if it accepts the key, None if it turns the key down.
async fn list_models(http: &reqwest::Client, provider: &Provider, key: &str) -> Result<Option<Vec<String>>> {
    let resp = auth(http.get(format!("{}/models", provider.base)), provider, key).send().await?;
    match resp.status().as_u16() {
        200..=299 => {
            let v: Value = resp.json().await.unwrap_or(Value::Null);
            let ids = v["data"].as_array().map(|a| a.iter().filter_map(|m| m["id"].as_str().map(str::to_string)).collect());
            Ok(Some(ids.unwrap_or_default()))
        }
        400 | 401 | 403 => Ok(None),
        s => bail!("{} answered {s}", provider.name),
    }
}

/// Whether TypeSafe accepts the key. Listing models needs the key and costs nothing.
async fn jev_accepts(http: &reqwest::Client, key: &str) -> Result<bool> {
    let resp = http.get(format!("{}/models", jev_base())).bearer_auth(key).send().await?;
    match resp.status().as_u16() {
        200..=299 => Ok(true),
        401 | 403 => Ok(false),
        s => bail!("TypeSafe answered {s}"),
    }
}

/// Work out which service `key` is for. `forced` skips the guessing ("jev" or a provider id).
pub async fn identify(http: &reqwest::Client, key: &str, forced: Option<&str>) -> Result<Identified> {
    if key.is_empty() {
        bail!("The key is empty.");
    }
    if forced == Some("jev") {
        return Ok(Identified::Jev);
    }
    if let Some(id) = forced {
        let Some(p) = by_id(id) else { bail!("Unknown service '{id}'. Use one of: deepseek, openai, anthropic, gemini, openrouter, custom, jev.") };
        if p.id == "custom" {
            return Ok(Identified::Chat { provider: p, model: String::new() });
        }
        let ids = list_models(http, p, key).await.ok().flatten().unwrap_or_default();
        return Ok(Identified::Chat { provider: p, model: pick_model(p, &ids) });
    }
    if let Some(p) = by_prefix(key) {
        return match list_models(http, p, key).await {
            Ok(Some(ids)) => Ok(Identified::Chat { provider: p, model: pick_model(p, &ids) }),
            Ok(None) => bail!("This looks like a {} key, but {} didn't accept it. Check that you copied all of it.", p.name, p.name),
            // The list is only for choosing a model; the test call that follows checks the key.
            Err(_) => Ok(Identified::Chat { provider: p, model: p.model.to_string() }),
        };
    }
    // The key's shape doesn't say. Try the likely services in turn.
    let chat_first = key.starts_with("sk-");
    let order: &[&str] = if chat_first { &["deepseek", "openai", "jev"] } else { &["jev", "deepseek", "openai"] };
    let mut first_err = None;
    for id in order {
        let found = if *id == "jev" {
            jev_accepts(http, key).await.map(|ok| ok.then_some(Identified::Jev))
        } else {
            let p = by_id(id).unwrap();
            list_models(http, p, key).await.map(|r| r.map(|ids| Identified::Chat { provider: p, model: pick_model(p, &ids) }))
        };
        match found {
            Ok(Some(x)) => return Ok(x),
            Ok(None) => {}
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
    }
    match first_err {
        Some(e) => bail!("Couldn't check the key ({e}). Are you online?"),
        None => bail!("None of {SUPPORTED} accepted this key. For another OpenAI-compatible service, run `desk key --provider custom <key>` after setting base_url and model in config.json."),
    }
}

/// The provider's default model if the service still lists it, otherwise its newest
/// everyday model (e.g. the highest-numbered "gpt-…-mini").
pub fn pick_model(p: &Provider, ids: &[String]) -> String {
    const SKIP: &[&str] = &[
        "audio", "realtime", "tts", "transcribe", "image", "search", "embed", "live", "preview", "exp", "lite", "nano", "vision", "instruct", "thinking",
        "codex",
    ];
    let ids: Vec<String> = ids.iter().map(|i| i.trim_start_matches("models/").to_string()).collect();
    if ids.is_empty() || ids.iter().any(|i| i == p.model) {
        return p.model.to_string();
    }
    let mut ok: Vec<&String> = ids
        .iter()
        .filter(|i| {
            let l = i.to_lowercase();
            p.prefer.iter().all(|w| l.contains(w)) && !SKIP.iter().any(|w| l.contains(w))
        })
        .collect();
    ok.sort_by(|a, b| version(b).cmp(&version(a)).then(a.len().cmp(&b.len())));
    ok.first().map(|s| s.to_string()).unwrap_or_else(|| p.model.to_string())
}

/// The numbers in a model name, without a trailing date: "gpt-4.1-mini-2025-04-14" → [4, 1].
fn version(id: &str) -> Vec<u64> {
    let mut nums: Vec<&str> = id.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).collect();
    if let Some(i) = nums.iter().position(|s| s.len() >= 4) {
        nums.truncate(i);
    }
    nums.iter().filter_map(|s| s.parse().ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes() {
        assert_eq!(by_prefix("sk-ant-api03-xyz").unwrap().id, "anthropic");
        assert_eq!(by_prefix("sk-or-v1-abc").unwrap().id, "openrouter");
        assert_eq!(by_prefix("AIzaSyAbc").unwrap().id, "gemini");
        assert_eq!(by_prefix("sk-proj-abc").unwrap().id, "openai");
        // DeepSeek and old OpenAI keys look alike: these get tested against each service
        assert!(by_prefix("sk-0123456789abcdef").is_none());
        assert!(by_prefix("ts_live_abc").is_none());
    }

    #[test]
    fn picks_models() {
        let openai = by_id("openai").unwrap();
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(pick_model(openai, &s(&["gpt-4.1-mini", "gpt-5-mini"])), "gpt-4.1-mini");
        assert_eq!(
            pick_model(openai, &s(&["gpt-5-mini-2026-01-01", "gpt-5-mini", "gpt-4o-mini-audio-preview", "gpt-5.2-nano", "o4-mini"])),
            "gpt-5-mini"
        );
        let gemini = by_id("gemini").unwrap();
        assert_eq!(pick_model(gemini, &s(&["models/gemini-3.0-flash", "models/gemini-3.0-flash-lite", "models/gemini-3.0-pro"])), "gemini-3.0-flash");
        assert_eq!(pick_model(gemini, &[]), "gemini-2.5-flash");
    }
}
