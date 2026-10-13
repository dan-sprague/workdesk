//! Local web server: serves the Work Desk page and a small JSON API.
//! Binds to 127.0.0.1 only.

use crate::ai::{self, Ai};
use crate::config::Config;
use crate::store::{check_collection, Store};
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
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

const PAGE: &str = include_str!("../ui/index.html");

#[derive(Clone)]
pub struct App {
    pub store: Arc<Store>,
    pub cfg: Arc<RwLock<Config>>,
    pub http: reqwest::Client,
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
    Json(json!({
        "ai": cfg.key().is_some(),
        "model": cfg.model(),
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
        let c = ai::capture(&app.store, ai.as_ref(), doc).await.map_err(|e| bad(&e.to_string()))?;
        let mut out = with_id(&c.id, c.doc);
        if let Some(n) = c.notice {
            out["notice"] = json!(n);
        }
        return Ok(Json(out));
    }
    doc.remove("id");
    doc.insert("created".into(), json!(now_iso()));
    doc.insert("updated".into(), json!(now_iso()));
    let id = app.store.new_id("p");
    app.store.put(&coll, &id, &doc)?;
    Ok(Json(with_id(&id, doc)))
}

async fn update(State(app): State<App>, Path((coll, id)): Path<(String, String)>, Json(body): Json<Value>) -> ApiResult {
    check_collection(&coll).map_err(|e| ApiError(StatusCode::NOT_FOUND, e.to_string()))?;
    let Value::Object(mut patch) = body else { return Err(bad("Expected a JSON object")) };
    patch.entry("updated").or_insert(json!(now_iso()));
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

async fn sort_inbox(State(app): State<App>) -> ApiResult {
    let ai = app.ai().ok_or_else(|| bad("Add your DeepSeek key first."))?;
    let (moved, left, err) = ai::sort_inbox(&app.store, &ai).await?;
    Ok(Json(json!({ "moved": moved, "left": left, "error": err })))
}

async fn brief(State(app): State<App>) -> ApiResult {
    let ai = app.ai().ok_or_else(|| bad("Add your DeepSeek key first."))?;
    let projects = app.store.list("projects")?;
    let items = app.store.list("items")?;
    let v = ai.brief(&projects, &items).await.map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e.to_string()))?;
    Ok(Json(v))
}

async fn settings(State(app): State<App>, Json(body): Json<Value>) -> ApiResult {
    let mut cfg = app.cfg.write().unwrap();
    if let Some(k) = body.get("key").and_then(|v| v.as_str()) {
        let k = k.trim();
        if k.is_empty() {
            return Err(bad("The key is empty."));
        }
        cfg.deepseek_key = Some(k.to_string());
    }
    if let Some(m) = body.get("model").and_then(|v| v.as_str()) {
        cfg.model = Some(m.trim().to_string()).filter(|m| !m.is_empty());
    }
    cfg.save()?;
    Ok(Json(json!({ "ai": cfg.key().is_some(), "model": cfg.model() })))
}

pub fn router(app: App) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/status", get(status))
        .route("/api/rev", get(rev))
        .route("/api/settings", post(settings))
        .route("/api/sort-inbox", post(sort_inbox))
        .route("/api/brief", post(brief))
        .route("/api/:coll", get(list).post(create))
        .route("/api/:coll/:id", patch(update).delete(remove))
        .layer(middleware::from_fn(guard))
        .with_state(app)
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
        println!("Tip: run `desk key <your DeepSeek key>` (or paste it on the page) to turn on auto-filing.");
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
