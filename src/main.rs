//! `desk` — Work Desk on your own computer.
//!
//!   desk                 open the Work Desk in your browser
//!   desk app             open it in its own window (what Work Desk.app runs)
//!   desk add "<note>"    file a sticky note (Work Desk picks the project)
//!   desk new "<name>"    add a project, with its own git-tracked work folder
//!   desk today           what needs doing today
//!   desk brief           a start-of-day plan
//!   desk sort            file everything waiting in the Inbox
//!   desk key <KEY>       save an API key (any supported service, or Jev)

mod ai;
mod config;
#[cfg(target_os = "macos")]
mod desktop;
mod logic;
mod project;
mod provider;
mod server;
mod store;
mod util;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use config::Config;
use serde_json::{json, Map, Value};
use std::io::IsTerminal;
use std::path::Path;
use std::sync::{Arc, RwLock};
use store::Store;

const SEED: &str = include_str!("../seed.json");

#[derive(Parser)]
#[command(name = "desk", version, about = "Work Desk: projects, kanban, timeline and sticky notes")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Open the Work Desk in your browser (the default)
    Open {
        /// Port for the local server
        #[arg(long, default_value_t = 7477)]
        port: u16,
        /// Start the server without opening a browser tab
        #[arg(long)]
        no_browser: bool,
    },
    /// Open the Work Desk in its own window (this is what Work Desk.app runs)
    #[cfg(target_os = "macos")]
    App,
    /// Add a sticky note; Work Desk picks the project unless you pass --project
    Add {
        /// Project id (see `desk projects`), or "inbox"
        #[arg(short, long)]
        project: Option<String>,
        /// Lane: todo, doing or review
        #[arg(short, long, default_value = "todo")]
        stage: String,
        /// The note, e.g. "Tox: analyze Sonia's data by Friday"
        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
    },
    /// Show what needs doing today
    Today,
    /// Get a start-of-day plan
    Brief,
    /// File every note waiting in the Inbox
    Sort,
    /// List projects and their ids
    Projects,
    /// Add a project and create its work folder (a git repo) in ~/Documents/Projects/Repos
    New {
        /// The project's name, e.g. "CHARM IND"
        #[arg(required = true, num_args = 1..)]
        name: Vec<String>,
        /// What the project is for (helps Work Desk file notes)
        #[arg(short, long)]
        goal: Option<String>,
        /// Don't create a folder
        #[arg(long)]
        no_folder: bool,
    },
    /// Save an API key. Work Desk works out which service it's for: DeepSeek, OpenAI,
    /// Anthropic, Google Gemini, OpenRouter, or TypeSafe's Jev (checks which project a note fits)
    Key {
        /// The key
        #[arg(required_unless_present = "forget")]
        key: Option<String>,
        /// Say which service instead of letting Work Desk work it out
        /// (deepseek, openai, anthropic, gemini, openrouter, custom, jev)
        #[arg(long)]
        provider: Option<String>,
        /// Remove a saved key: "chat" or "jev"
        #[arg(long, conflicts_with = "key")]
        forget: Option<String>,
    },
    /// Use a different model from the same service, e.g. `desk model deepseek-reasoner`
    Model { name: String },
    /// Keep your data in Google Drive (moves it to My Drive/WorkDesk/workdesk.json)
    Drive {
        /// Folder inside My Drive
        #[arg(long, default_value = "WorkDesk")]
        folder: String,
    },
    /// Keep your data in a specific file or folder (e.g. any synced folder)
    UseFile { path: String },
    /// Combine another copy of workdesk.json (such as a Google Drive conflict copy),
    /// keeping whichever version of each task or project is newest
    Merge { file: String },
    /// Export everything as JSON (to a file, or print it)
    Export { file: Option<String> },
    /// Import a JSON export (adds new entries, replaces ones with the same id)
    Import { file: String },
    /// Show where your data is kept
    Where,
}

fn open_store(cfg: &Config) -> Result<Store> {
    std::fs::create_dir_all(config::data_dir()).with_context(|| format!("creating {}", config::data_dir().display()))?;
    let path = cfg.data_file();
    let store = Store::open(&path, Some(config::backup_dir()))?;
    if !store.exists_on_disk() {
        let seed: Value = serde_json::from_str(SEED)?;
        store.import(&seed, false)?;
    }
    Ok(store)
}

/// Point Work Desk at a new data file, copying the current data there if it doesn't exist yet.
fn switch_to(mut cfg: Config, dest: std::path::PathBuf, paint: &logic::Paint) -> Result<()> {
    let dest = if dest.is_dir() || (!dest.exists() && dest.extension().is_none()) { dest.join("workdesk.json") } else { dest };
    if std::env::var("WORKDESK_FILE").map_or(false, |v| !v.trim().is_empty()) {
        println!("{}", paint.yellow("Note: WORKDESK_FILE is set in your shell and will override this setting."));
    }
    let current = cfg.data_file();
    if dest == current {
        println!("Already using {}", dest.display());
        return Ok(());
    }
    if dest.exists() {
        let s = Store::open(&dest, None)?;
        let (p, i) = s.counts();
        println!("Using the existing file at {} ({p} projects, {i} tasks).", paint.cyan(&dest.display().to_string()));
        if current.exists() {
            println!(
                "Your previous data is still at {}.\nTo combine it in, run:  desk merge \"{}\"",
                current.display(),
                current.display()
            );
        }
    } else {
        let cur = open_store(&cfg)?;
        cur.copy_to(&dest)?;
        let (p, i) = cur.counts();
        println!("Copied {p} projects and {i} tasks to {}", paint.cyan(&dest.display().to_string()));
    }
    cfg.data_file = Some(dest.display().to_string());
    cfg.save()?;
    println!("Work Desk now reads and writes that file. If `desk` is already running, stop it (Ctrl+C) and start it again.");
    Ok(())
}

fn need_ai(cfg: &Config, http: &reqwest::Client) -> Result<ai::Ai> {
    match ai::Ai::from_config(cfg, http) {
        Some(a) => Ok(a),
        None => bail!("No API key yet. Run `desk key <your key>` first."),
    }
}

/// True when this binary is the one inside Work Desk.app (…/Work Desk.app/Contents/MacOS/).
fn in_app_bundle() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    let mut up = exe.ancestors().skip(1).map(Path::file_name);
    matches!(
        (up.next().flatten(), up.next().flatten(), up.next().flatten()),
        (Some(m), Some(c), Some(app)) if m == "MacOS" && c == "Contents" && app.to_string_lossy().ends_with(".app")
    )
}

/// Started from Finder or the Dock there is no terminal to print to, so errors get a dialog.
fn alert(msg: &str) {
    let quote = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    let script = format!("display alert {} message {} as critical", quote("Work Desk couldn't start"), quote(msg));
    let _ = std::process::Command::new("osascript").args(["-e", &script]).status();
}

fn main() {
    // Older macOS passes a -psn_… argument to apps opened from Finder.
    let cli = Cli::parse_from(std::env::args_os().filter(|a| !a.to_string_lossy().starts_with("-psn_")));
    let bundled = in_app_bundle();
    let cmd = match cli.cmd {
        Some(cmd) => cmd,
        #[cfg(target_os = "macos")]
        None if bundled => Cmd::App,
        None => Cmd::Open { port: 7477, no_browser: false },
    };
    let rt = tokio::runtime::Runtime::new().expect("starting the async runtime");
    let result = match cmd {
        #[cfg(target_os = "macos")]
        Cmd::App => run_app(rt),
        cmd => rt.block_on(run(cmd)),
    };
    if let Err(e) = result {
        eprintln!("desk: {e:#}");
        if bundled {
            alert(&format!("{e:#}"));
        }
        std::process::exit(1);
    }
}

#[cfg(target_os = "macos")]
fn run_app(rt: tokio::runtime::Runtime) -> Result<()> {
    let app = rt.block_on(async {
        let cfg = Config::load();
        let store = Arc::new(open_store(&cfg)?);
        anyhow::Ok(server::App { store, cfg: Arc::new(RwLock::new(cfg)), http: ai::http_client(), picker: None })
    })?;
    desktop::run(app, rt)
}

async fn run(cmd: Cmd) -> Result<()> {
    let paint = logic::Paint::new(std::io::stdout().is_terminal());
    let cfg = Config::load();
    let http = ai::http_client();

    match cmd {
        #[cfg(target_os = "macos")]
        Cmd::App => unreachable!("handled in main"),
        Cmd::Open { port, no_browser } => {
            let store = Arc::new(open_store(&cfg)?);
            let app = server::App { store, cfg: Arc::new(RwLock::new(cfg)), http, picker: None };
            server::serve(app, port, !no_browser).await?;
        }
        Cmd::Add { project, stage, text } => {
            let store = open_store(&cfg)?;
            let text = text.join(" ");
            if !matches!(stage.as_str(), "todo" | "doing" | "review") {
                bail!("--stage must be todo, doing or review");
            }
            let projects = store.list("projects")?;
            let pid = match project {
                Some(p) if p == "inbox" => p,
                Some(p) => {
                    let found = projects.iter().find(|(id, m)| {
                        id.eq_ignore_ascii_case(&p) || logic::s(m, "name").map_or(false, |n| n.to_lowercase().contains(&p.to_lowercase()))
                    });
                    match found {
                        Some((id, _)) => id.clone(),
                        None => bail!("No project matches '{p}'. Run `desk projects` to see the ids."),
                    }
                }
                None => "auto".to_string(),
            };
            let ai = ai::Ai::from_config(&cfg, &http);
            let mut doc = Map::new();
            doc.insert("text".into(), json!(text));
            doc.insert("project".into(), json!(pid));
            doc.insert("stage".into(), json!(stage));
            let c = ai::capture(&store, ai.as_ref(), doc).await?;
            let proj = logic::project_name(&projects, logic::s(&c.doc, "project").unwrap_or("inbox"));
            let mut line = format!("{} {}  →  {}", paint.bold("Added:"), logic::s(&c.doc, "text").unwrap_or(""), paint.cyan(&proj));
            if let Some(d) = logic::s(&c.doc, "due") {
                line.push_str(&format!("  {}", paint.yellow(&format!("due {}", util::short_date(d)))));
            }
            println!("{line}");
            if let Some(n) = c.notice {
                println!("{}", paint.dim(&n));
            }
        }
        Cmd::Today => {
            let store = open_store(&cfg)?;
            print!("{}", logic::today_report(&store.list("projects")?, &store.list("items")?, &paint));
        }
        Cmd::Brief => {
            let store = open_store(&cfg)?;
            let ai = need_ai(&cfg, &http)?;
            eprintln!("{}", paint.dim("Thinking…"));
            let v = ai.brief(&store.list("projects")?, &store.list("items")?).await?;
            print_brief(&v, &paint);
        }
        Cmd::Sort => {
            let store = open_store(&cfg)?;
            let ai = need_ai(&cfg, &http)?;
            let (moved, left, err) = ai::sort_inbox(&store, &ai).await?;
            println!("Filed {moved} note(s). {left} left in the Inbox.");
            if let Some(e) = err {
                println!("{}", paint.dim(&e));
            }
        }
        Cmd::Projects => {
            let store = open_store(&cfg)?;
            let items = store.list("items")?;
            for (id, p) in logic::ordered(&store.list("projects")?) {
                let open = items.iter().filter(|(_, m)| logic::s(m, "project") == Some(id.as_str()) && logic::is_open(m)).count();
                let mut line = format!("{:<18} {}  {}", paint.cyan(&id), logic::s(&p, "name").unwrap_or(""), paint.dim(&format!("{open} open")));
                if let Some(dl) = logic::s(&p, "deadline") {
                    line.push_str(&format!("  {}", paint.yellow(&format!("deadline {}", util::short_date(dl)))));
                }
                println!("{line}");
            }
        }
        Cmd::Key { key, provider, forget } => {
            let mut cfg = cfg;
            match (key, forget.as_deref()) {
                (Some(key), _) => {
                    eprintln!("{}", paint.dim("Checking the key…"));
                    let (new, msg) = ai::connect(cfg, &http, &key, provider.as_deref()).await?;
                    new.save()?;
                    println!("{msg}");
                }
                (None, Some("jev")) => {
                    cfg.jev_key = None;
                    cfg.save()?;
                    println!("Removed the Jev key.");
                }
                (None, Some("chat")) => {
                    (cfg.api_key, cfg.provider, cfg.model, cfg.base_url) = (None, None, None, None);
                    cfg.save()?;
                    println!("Removed the chat model's key.");
                }
                _ => bail!("--forget takes \"chat\" or \"jev\""),
            }
        }
        Cmd::Model { name } => {
            let mut cfg = cfg;
            cfg.model = Some(name.trim().to_string()).filter(|m| !m.is_empty());
            cfg.save()?;
            match cfg.chat() {
                Some(c) => println!("Work Desk will use {} from {}.", c.model, c.provider.name),
                None => println!("Saved. Add a key with `desk key` to use it."),
            }
        }
        Cmd::New { name, goal, no_folder } => {
            let store = open_store(&cfg)?;
            let mut doc = Map::new();
            doc.insert("name".into(), json!(name.join(" ")));
            if let Some(g) = goal {
                doc.insert("goal".into(), json!(g));
            }
            let c = project::create(&store, &cfg, doc, !no_folder, &[])?;
            println!("{} {}  {}", paint.bold("Added:"), logic::s(&c.doc, "name").unwrap_or(""), paint.cyan(&c.id));
            if let Some(n) = c.notice {
                println!("{n}");
            }
        }
        Cmd::Export { file } => {
            let store = open_store(&cfg)?;
            let s = serde_json::to_string_pretty(&store.export()?)?;
            match file {
                Some(f) => {
                    std::fs::write(&f, s)?;
                    println!("Exported to {f}");
                }
                None => println!("{s}"),
            }
        }
        Cmd::Import { file } => {
            let store = open_store(&cfg)?;
            let v: Value = serde_json::from_str(&std::fs::read_to_string(&file)?).context("reading the export")?;
            let (a, u, _) = store.import(&v, false)?;
            println!("Imported: {a} new, {u} replaced.");
        }
        Cmd::Merge { file } => {
            let store = open_store(&cfg)?;
            let path = config::expand_home(&file);
            if path == store.path() {
                bail!("That's the file Work Desk is already using.");
            }
            let v: Value = serde_json::from_str(&std::fs::read_to_string(&path)?).context("reading that file")?;
            let (a, u, k) = store.import(&v, true)?;
            println!("Merged into {}: {a} added, {u} updated with newer versions, {k} unchanged.", store.path().display());
            println!("{}", paint.dim("You can delete the other copy now."));
        }
        Cmd::Drive { folder } => {
            let drives = config::find_google_drive();
            match drives.len() {
                0 => bail!(
                    "Couldn't find Google Drive on this Mac. Install Google Drive for desktop and sign in, \
                     or point Work Desk at any synced folder with:  desk use-file \"<folder>\""
                ),
                1 => switch_to(cfg, drives[0].join(&folder), &paint)?,
                _ => {
                    println!("Found more than one Google Drive:");
                    for d in &drives {
                        println!("  {}", d.display());
                    }
                    bail!("Pick one with:  desk use-file \"<one of the folders above>/{folder}\"");
                }
            }
        }
        Cmd::UseFile { path } => {
            switch_to(cfg, config::expand_home(&path), &paint)?;
        }
        Cmd::Where => {
            let store = open_store(&cfg)?;
            let (p, i) = store.counts();
            println!("Data file:  {}  ({p} projects, {i} tasks)", store.path().display());
            println!("Backups:    {}", config::backup_dir().display());
            println!("Settings:   {}  (API keys stay here, not in the data file)", config::data_dir().join("config.json").display());
            println!("Projects:   {}  (new project folders)", config::tilde(&cfg.projects_root()));
            match cfg.chat() {
                Some(c) => println!("AI:         {} · {}", c.provider.name, c.model),
                None => println!("AI:         no key yet (desk key <your key>)"),
            }
            println!("Jev:        {}", if cfg.jev_key().is_some() { "on: checks which project each note fits" } else { "off" });
        }
    }
    Ok(())
}

fn print_brief(v: &Value, c: &logic::Paint) {
    if let Some(h) = v["headline"].as_str() {
        println!("\n{}\n", c.bold(h));
    }
    if let Some(f) = v["focus"].as_array().filter(|a| !a.is_empty()) {
        println!("{}", c.cyan(&c.bold("Focus")));
        for (i, x) in f.iter().enumerate() {
            println!("  {}. {}  {}", i + 1, x["task"].as_str().unwrap_or(""), c.dim(x["project"].as_str().unwrap_or("")));
            if let Some(w) = x["why"].as_str() {
                println!("     {}", c.dim(w));
            }
        }
        println!();
    }
    if let Some(w) = v["watch"].as_array().filter(|a| !a.is_empty()) {
        println!("{}", c.yellow(&c.bold("Watch out")));
        for x in w {
            println!("  • {}", x.as_str().unwrap_or(""));
        }
        println!();
    }
    if let Some(q) = v["quick_wins"].as_array().filter(|a| !a.is_empty()) {
        println!("{}", c.bold("Quick wins"));
        for x in q {
            println!("  • {}", x.as_str().unwrap_or(""));
        }
        println!();
    }
    if let Some(s) = v["first_step"].as_str() {
        println!("{} {}\n", c.violet(&c.bold("Start here:")), s);
    }
}
