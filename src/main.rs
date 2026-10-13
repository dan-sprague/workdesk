//! `desk` — Work Desk on your own computer.
//!
//!   desk                 open the Work Desk in your browser
//!   desk add "<note>"    file a sticky note (DeepSeek picks the project)
//!   desk today           what needs doing today
//!   desk brief           a start-of-day plan from DeepSeek
//!   desk sort            file everything waiting in the Inbox
//!   desk key <KEY>       save your DeepSeek API key

mod ai;
mod config;
mod logic;
mod server;
mod store;
mod util;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use config::Config;
use serde_json::{json, Map, Value};
use std::io::IsTerminal;
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
    /// Add a sticky note; DeepSeek picks the project unless you pass --project
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
    /// Ask DeepSeek for a start-of-day plan
    Brief,
    /// File every note waiting in the Inbox
    Sort,
    /// List projects and their ids
    Projects,
    /// Save your DeepSeek API key
    Key {
        /// The key, starting with sk-
        key: String,
    },
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
        None => bail!("No DeepSeek key yet. Run `desk key <your key>` first."),
    }
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("desk: {e:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    let paint = logic::Paint::new(std::io::stdout().is_terminal());
    let cfg = Config::load();
    let http = ai::Ai::http_client();

    match cli.cmd.unwrap_or(Cmd::Open { port: 7477, no_browser: false }) {
        Cmd::Open { port, no_browser } => {
            let store = Arc::new(open_store(&cfg)?);
            let app = server::App { store, cfg: Arc::new(RwLock::new(cfg)), http };
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
            eprintln!("{}", paint.dim("Asking DeepSeek…"));
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
        Cmd::Key { key } => {
            let mut cfg = cfg;
            cfg.deepseek_key = Some(key.trim().to_string());
            cfg.save()?;
            println!("Saved. Notes will now be filed by DeepSeek ({}).", cfg.model());
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
            println!("Settings:   {}  (DeepSeek key stays here, not in the data file)", config::data_dir().join("config.json").display());
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
