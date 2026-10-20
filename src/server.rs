//! Local web server: serves the Work Desk page and a small JSON API.
//! Binds to 127.0.0.1 only.

use crate::ai::{self, Ai};
use crate::config::{expand_home, tilde, Config};
use crate::project;
use crate::store::{check_collection, Store, COLLECTIONS};
use crate::util::now_iso;
use anyhow::Result;
use axum::{
    extract::{Path, Request, State},
    http::{header, Method, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::{Path as FsPath, PathBuf};
use std::sync::{Arc, RwLock};

const PAGE: &str = include_str!("../ui/index.html");

/// A request for the Mac's file chooser. The desktop window answers these with its own
/// dialog; the browser version falls back to AppleScript's `choose file`.
pub struct PickRequest {
    pub folder: bool,
    pub start: Option<PathBuf>,
    pub reply: tokio::sync::oneshot::Sender<Vec<PathBuf>>,
}
pub type Picker = Arc<dyn Fn(PickRequest) + Send + Sync>;

#[derive(Clone)]
pub struct App {
    pub store: Arc<Store>,
    pub cfg: Arc<RwLock<Config>>,
    pub http: reqwest::Client,
    pub picker: Option<Picker>,
}

impl App {
    fn ai(&self) -> Option<Ai> {
        Ai::from_config(&self.cfg.read().unwrap(), &self.http)
    }
}

pub struct ApiError(StatusCode, String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}
impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    }
}
fn bad(msg: &str) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.to_string())
}
type ApiResult = Result<Json<Value>, ApiError>;

fn with_id(id: &str, mut m: Map<String, Value>) -> Value {
    m.insert("id".into(), json!(id));
    Value::Object(m)
}

/// Blocks other websites from talking to this server: the Host must be local
/// (stops DNS rebinding) and writes need a custom header (forces a CORS preflight
/// that this server never approves).
async fn guard(req: Request, next: Next) -> Response {
    let host_ok = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(|h| {
            let h = h.rsplit_once(':').map(|(a, _)| a).unwrap_or(h);
            h == "127.0.0.1" || h == "localhost"
        })
        .unwrap_or(false);
    if !host_ok {
        return (StatusCode::FORBIDDEN, "Work Desk only answers on localhost").into_response();
    }
    if req.method() != Method::GET && req.headers().get("x-desk").is_none() {
        return (StatusCode::FORBIDDEN, "Missing X-Desk header").into_response();
    }
    next.run(req).await
}

async fn index() -> impl IntoResponse {
    ([(header::CACHE_CONTROL, "no-store")], Html(PAGE))
}

async fn status(State(app): State<App>) -> Json<Value> {
    let cfg = app.cfg.read().unwrap();
    let chat = cfg.chat();
    Json(json!({
        "ai": chat.is_some() || cfg.jev_key().is_some(),
        "chat": chat.as_ref().map(|c| format!("{} · {}", c.provider.name, c.model)),
        "jev": cfg.jev_key().is_some(),
        "projects_root": tilde(&cfg.projects_root()),
        "editor": cfg.editor(),
        "hours_per_day": cfg.hours_per_day(),
        "data_file": app.store.path().display().to_string(),
        "data_error": app.store.last_error(),
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn rev(State(app): State<App>) -> Json<Value> {
    Json(json!({ "rev": app.store.rev() }))
}

async fn list(State(app): State<App>, Path(coll): Path<String>) -> ApiResult {
    check_collection(&coll).map_err(|e| ApiError(StatusCode::NOT_FOUND, e.to_string()))?;
    let docs = app.store.list(&coll)?;
    Ok(Json(Value::Array(docs.into_iter().map(|(id, m)| with_id(&id, m)).collect())))
}

async fn create(State(app): State<App>, Path(coll): Path<String>, Json(body): Json<Value>) -> ApiResult {
    check_collection(&coll).map_err(|e| ApiError(StatusCode::NOT_FOUND, e.to_string()))?;
    let Value::Object(mut doc) = body else { return Err(bad("Expected a JSON object")) };
    if coll == "items" {
        let ai = app.ai();
        let (mut made, notice) = ai::capture(&app.store, ai.as_ref(), doc).await.map_err(|e| bad(&e.to_string()))?;
        let first = made.remove(0);
        let mut out = with_id(&first.id, first.doc);
        if !made.is_empty() {
            out["also"] = Value::Array(made.into_iter().map(|c| with_id(&c.id, c.doc)).collect());
        }
        if let Some(n) = notice {
            out["notice"] = json!(n);
        }
        return Ok(Json(out));
    }
    // New projects get their own work folder unless the page says otherwise.
    let make_folder = doc.remove("make_folder").and_then(|v| v.as_bool()).unwrap_or(true);
    let moves: Vec<String> = doc
        .remove("move")
        .and_then(|v| v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()))
        .unwrap_or_default();
    let cfg = app.cfg.read().unwrap().clone();
    let store = app.store.clone();
    let c = tokio::task::spawn_blocking(move || project::create(&store, &cfg, doc, make_folder, &moves))
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| bad(&e.to_string()))?;
    let mut out = with_id(&c.id, c.doc);
    if let Some(n) = c.notice {
        out["notice"] = json!(n);
    }
    Ok(Json(out))
}

async fn update(State(app): State<App>, Path((coll, id)): Path<(String, String)>, Json(body): Json<Value>) -> ApiResult {
    check_collection(&coll).map_err(|e| ApiError(StatusCode::NOT_FOUND, e.to_string()))?;
    let Value::Object(mut patch) = body else { return Err(bad("Expected a JSON object")) };
    patch.entry("updated").or_insert(json!(now_iso()));
    // A note moved by hand no longer needs the Inbox's "where does this go" hint.
    if coll == "items" && patch.contains_key("project") {
        patch.entry("triage").or_insert(Value::Null);
    }
    match app.store.merge(&coll, &id, &patch)? {
        Some(doc) => Ok(Json(with_id(&id, doc))),
        None => Err(ApiError(StatusCode::NOT_FOUND, format!("No {coll} with id {id}"))),
    }
}

async fn remove(State(app): State<App>, Path((coll, id)): Path<(String, String)>) -> ApiResult {
    check_collection(&coll).map_err(|e| ApiError(StatusCode::NOT_FOUND, e.to_string()))?;
    let found = app.store.delete(&coll, &id)?;
    Ok(Json(json!({ "deleted": found })))
}

/// Where Work Desk would file a note, without saving it: the page asks you to confirm first.
async fn suggest(State(app): State<App>, Json(body): Json<Value>) -> ApiResult {
    let text = body.get("text").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()).ok_or_else(|| bad("The note is empty."))?;
    let ai = app.ai().ok_or_else(|| bad("Add an API key under AI keys to get suggestions."))?;
    let projects = app.store.list("projects")?;
    let items = app.store.list("items")?;
    let f = ai.file_note(text, &projects, &items).await.map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e.to_string()))?;
    Ok(Json(json!({
        "project": f.project,
        "tasks": f.tasks.iter().map(|t| json!({ "text": t.text, "due": t.due })).collect::<Vec<_>>(),
        "triage": f.triage.as_ref().map(|t| t.to_json()),
        "warning": f.warning,
        "duplicate": f.duplicate_json(),
    })))
}

async fn sort_inbox(State(app): State<App>) -> ApiResult {
    let ai = app.ai().ok_or_else(|| bad("Add an API key under AI keys first."))?;
    let (moved, left, err) = ai::sort_inbox(&app.store, &ai).await?;
    Ok(Json(json!({ "moved": moved, "left": left, "error": err })))
}

async fn brief(State(app): State<App>) -> ApiResult {
    let ai = app.ai().ok_or_else(|| bad("Add an API key under AI keys first."))?;
    let hours = app.cfg.read().unwrap().hours_per_day();
    let v = ai.brief(&app.store, hours).await.map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e.to_string()))?;
    Ok(Json(v))
}

/// Paste any key: Work Desk works out which service it's for, checks it, and saves it.
/// Also `{"forget": "jev"}` or `{"forget": "chat"}`, `{"model": "..."}` and `{"hours_per_day": 4}`.
async fn draft_prompt(State(app): State<App>, Json(body): Json<Value>) -> ApiResult {
    let id = body.get("id").and_then(Value::as_str).ok_or_else(|| bad("Which task?"))?;
    let ai = app.ai().ok_or_else(|| bad("Add an API key under AI keys first."))?;
    let prompt = ai::draft_prompt(&app.store, &ai, id).await.map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e.to_string()))?;
    Ok(Json(json!({ "prompt": prompt })))
}

/// The desktop window can't always reach the clipboard itself.
async fn copy(Json(body): Json<Value>) -> ApiResult {
    let text = body.get("text").and_then(Value::as_str).ok_or_else(|| bad("Nothing to copy."))?.to_string();
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        use std::io::Write;
        let mut child = std::process::Command::new("pbcopy").stdin(std::process::Stdio::piped()).spawn()?;
        child.stdin.take().unwrap().write_all(text.as_bytes())?;
        child.wait()?;
        Ok(())
    })
    .await
    .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("Couldn't copy: {e}")))?;
    Ok(Json(json!({ "copied": true })))
}

async fn settings(State(app): State<App>, Json(body): Json<Value>) -> ApiResult {
    let mut message = None;
    if let Some(k) = body.get("key").and_then(|v| v.as_str()) {
        let cfg = app.cfg.read().unwrap().clone();
        let forced = body.get("provider").and_then(|v| v.as_str()).filter(|p| !p.is_empty());
        let (new, msg) = ai::connect(cfg, &app.http, k, forced).await.map_err(|e| bad(&e.to_string()))?;
        *app.cfg.write().unwrap() = new;
        message = Some(msg);
    }
    let mut cfg = app.cfg.write().unwrap();
    match body.get("forget").and_then(|v| v.as_str()) {
        Some("jev") => cfg.jev_key = None,
        Some("chat") => (cfg.api_key, cfg.provider, cfg.model, cfg.base_url) = (None, None, None, None),
        _ => {}
    }
    if let Some(m) = body.get("model").and_then(|v| v.as_str()) {
        cfg.model = Some(m.trim().to_string()).filter(|m| !m.is_empty());
    }
    if let Some(h) = body.get("hours_per_day").and_then(Value::as_f64) {
        if !(h > 0.0 && h <= 24.0) {
            return Err(bad("Hours a day should be between 0 and 24."));
        }
        cfg.hours_per_day = Some(h);
    }
    cfg.save()?;
    Ok(Json(json!({ "message": message })))
}

// ---------------- linked files ----------------
// Tasks and projects keep `files: [{path, name}]`, where a path is `~/…`, an absolute
// path, or a web link. Projects can also have one `folder`.

fn is_link(p: &str) -> bool {
    p.starts_with("https://") || p.starts_with("http://")
}

/// Accept paths the way people paste them: quoted, with `\ ` escapes, or as file:// URLs.
fn clean_path(raw: &str) -> String {
    let mut p = raw.trim().trim_matches(|c| c == '"' || c == '\'').to_string();
    if let Some(rest) = p.strip_prefix("file://") {
        let rest = rest.strip_prefix("localhost").unwrap_or(rest);
        let bytes = rest.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                if let Ok(b) = u8::from_str_radix(&rest[i + 1..i + 3], 16) {
                    out.push(b);
                    i += 3;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        p = String::from_utf8_lossy(&out).into_owned();
    }
    if !is_link(&p) {
        p = p.replace("\\ ", " ");
        if p.len() > 1 {
            p = p.trim_end_matches('/').to_string();
        }
    }
    p
}

/// The form a path is stored and compared in: `~/…` when it is under the home folder.
fn normalize(raw: &str) -> String {
    let p = clean_path(raw);
    if is_link(&p) {
        p
    } else {
        tilde(&expand_home(&p))
    }
}

/// Everything the data links to. /api/open only opens these.
fn linked(store: &Store) -> anyhow::Result<HashSet<String>> {
    let mut out = HashSet::new();
    for coll in COLLECTIONS {
        for (_, doc) in store.list(coll)? {
            if let Some(Value::Array(files)) = doc.get("files") {
                out.extend(files.iter().filter_map(|f| f.get("path")?.as_str()).map(normalize));
            }
            if let Some(f) = doc.get("folder").and_then(Value::as_str).filter(|f| !f.is_empty()) {
                out.insert(normalize(f));
            }
        }
    }
    Ok(out)
}

fn display_name(p: &str) -> String {
    if is_link(p) {
        let rest = p.split_once("://").map(|x| x.1).unwrap_or(p);
        return rest.trim_end_matches('/').chars().take(60).collect();
    }
    FsPath::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.to_string())
}

/// For each path: its stored form, display name, and whether it is there on this computer.
async fn stat(Json(body): Json<Value>) -> ApiResult {
    let paths = body.get("paths").and_then(Value::as_array).ok_or_else(|| bad("Expected {\"paths\": [...]}"))?;
    let out: Vec<Value> = paths
        .iter()
        .filter_map(Value::as_str)
        .map(|raw| {
            let path = normalize(raw);
            if is_link(&path) {
                return json!({ "input": raw, "path": path, "name": display_name(&path), "link": true, "exists": true });
            }
            let full = expand_home(&path);
            let md = std::fs::metadata(&full).ok();
            json!({
                "input": raw,
                "path": path,
                "name": display_name(&path),
                "exists": md.is_some(),
                "dir": md.as_ref().map_or(false, |m| m.is_dir()),
                "absolute": full.is_absolute(),
            })
        })
        .collect();
    Ok(Json(Value::Array(out)))
}

async fn open_linked(State(app): State<App>, Json(body): Json<Value>) -> ApiResult {
    let raw = body.get("path").and_then(Value::as_str).ok_or_else(|| bad("Which file?"))?;
    let reveal = body.get("reveal").and_then(Value::as_bool).unwrap_or(false);
    let in_editor = body.get("editor").and_then(Value::as_bool).unwrap_or(false);
    let path = normalize(raw);
    if !linked(&app.store)?.contains(&path) {
        return Err(ApiError(StatusCode::FORBIDDEN, "Only files linked to a task or project can be opened from Work Desk.".into()));
    }
    if is_link(&path) {
        open_url(&path);
        return Ok(Json(json!({ "opened": path })));
    }
    let full = expand_home(&path);
    if !full.exists() {
        return Err(ApiError(StatusCode::NOT_FOUND, format!("{path} isn't on this computer. It may have been moved or renamed.")));
    }
    if in_editor {
        let editor = app.cfg.read().unwrap().editor();
        let out = std::process::Command::new("open").arg("-a").arg(&editor).arg(&full).output().map_err(|e| bad(&e.to_string()))?;
        if !out.status.success() {
            return Err(bad(&format!("Couldn't open it in {editor}. Set `editor` in config.json to the app you use.")));
        }
        return Ok(Json(json!({ "opened": path, "editor": editor })));
    }
    open_path(&full, reveal);
    Ok(Json(json!({ "opened": path })))
}

async fn pick(State(app): State<App>, Json(body): Json<Value>) -> ApiResult {
    let folder = body.get("folder").and_then(Value::as_bool).unwrap_or(false);
    let start = body
        .get("start")
        .and_then(Value::as_str)
        .map(|s| expand_home(&clean_path(s)))
        .map(|p| if p.is_dir() { p } else { p.parent().map(FsPath::to_path_buf).unwrap_or(p) })
        .filter(|p| p.is_dir())
        .or_else(|| dirs::document_dir().filter(|d| d.is_dir()));
    let paths = match &app.picker {
        Some(picker) => {
            let (reply, rx) = tokio::sync::oneshot::channel();
            picker(PickRequest { folder, start, reply });
            rx.await.unwrap_or_default()
        }
        None => tokio::task::spawn_blocking(move || choose_with_applescript(folder, start.as_deref()))
            .await
            .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??,
    };
    Ok(Json(json!({ "paths": paths.iter().map(|p| tilde(p)).collect::<Vec<_>>() })))
}

/// The browser version's file chooser. Cancelling returns no paths.
fn choose_with_applescript(folder: bool, start: Option<&FsPath>) -> Result<Vec<PathBuf>, ApiError> {
    if !cfg!(target_os = "macos") {
        return Err(bad("Choosing files needs a Mac. Paste the path instead."));
    }
    let what = if folder { "choose folder" } else { "choose file" };
    let script = format!(
        r#"on run argv
  activate
  if (count of argv) > 0 then
    set picked to {what} with prompt "Link to Work Desk" default location (POSIX file (item 1 of argv)) with multiple selections allowed
  else
    set picked to {what} with prompt "Link to Work Desk" with multiple selections allowed
  end if
  set out to ""
  repeat with f in picked
    set out to out & POSIX path of f & linefeed
  end repeat
  return out
end run"#
    );
    let mut cmd = std::process::Command::new("osascript");
    cmd.arg("-e").arg(script);
    if let Some(s) = start {
        cmd.arg(s);
    }
    let out = cmd.output().map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !out.status.success() {
        // -128 is "User canceled"
        let err = String::from_utf8_lossy(&out.stderr);
        return if err.contains("-128") { Ok(vec![]) } else { Err(ApiError(StatusCode::INTERNAL_SERVER_ERROR, err.trim().to_string())) };
    }
    Ok(String::from_utf8_lossy(&out.stdout).lines().filter(|l| !l.is_empty()).map(PathBuf::from).collect())
}

pub fn router(app: App) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/status", get(status))
        .route("/api/rev", get(rev))
        .route("/api/settings", post(settings))
        .route("/api/suggest", post(suggest))
        .route("/api/sort-inbox", post(sort_inbox))
        .route("/api/brief", post(brief))
        .route("/api/prompt", post(draft_prompt))
        .route("/api/copy", post(copy))
        .route("/api/stat", post(stat))
        .route("/api/open", post(open_linked))
        .route("/api/pick", post(pick))
        .route("/api/:coll", get(list).post(create))
        .route("/api/:coll/:id", patch(update).delete(remove))
        .layer(middleware::from_fn(guard))
        .with_state(app)
}

/// Open a file or folder with its default app, or show it in Finder.
pub fn open_path(path: &FsPath, reveal: bool) {
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        if reveal {
            c.arg("-R");
        }
        c.arg(path);
        c
    };
    #[cfg(not(target_os = "macos"))]
    let mut cmd = {
        let target = if reveal { path.parent().unwrap_or(path) } else { path };
        let mut c = std::process::Command::new("xdg-open");
        c.arg(target);
        c
    };
    let _ = cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
}

pub fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(not(target_os = "macos"))]
    let cmd = "xdg-open";
    let _ = std::process::Command::new(cmd)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

pub async fn serve(app: App, port: u16, open_browser: bool) -> Result<()> {
    let url = format!("http://localhost:{port}/");
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            println!("Work Desk is already running at {url}");
            if open_browser {
                open_url(&url);
            }
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };
    println!("Work Desk is running at {url}");
    println!("Data file: {}", app.store.path().display());
    if app.ai().is_none() {
        println!("Tip: run `desk key <your API key>` (or paste it on the page) to have notes filed for you.");
    }
    println!("Press Ctrl+C to stop.");
    if open_browser {
        open_url(&url);
    }
    axum::serve(listener, router(app))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
