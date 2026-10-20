//! Shared task logic: stages, staleness, and the text summaries used by the
//! terminal commands and by the prompts sent to the AI.

use crate::config::expand_home;
use crate::store::Doc;
use crate::util::*;
use serde_json::{Map, Value};

/// Days without change before a task counts as stalled (matches the page).
pub const STALE_DOING: i64 = 5;
pub const STALE_REVIEW: i64 = 3;
pub const STALE_TODO: i64 = 14;

pub fn s<'a>(m: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    m.get(k).and_then(|v| v.as_str()).filter(|x| !x.is_empty())
}

pub fn is_open(m: &Map<String, Value>) -> bool {
    s(m, "status") != Some("done")
}

pub fn stage(m: &Map<String, Value>) -> &'static str {
    if !is_open(m) {
        return "done";
    }
    match s(m, "stage") {
        Some("doing") => "doing",
        Some("review") => "review",
        _ => "todo",
    }
}

pub fn stage_label(st: &str) -> &'static str {
    match st {
        "doing" => "In progress",
        "review" => "Review",
        "done" => "Done",
        _ => "To do",
    }
}

pub fn touched_secs(m: &Map<String, Value>) -> i64 {
    ["created", "updated", "done_at"].iter().filter_map(|k| s(m, k).and_then(parse_ts)).max().unwrap_or(0)
}

pub fn idle_days(m: &Map<String, Value>) -> i64 {
    let t = touched_secs(m);
    if t == 0 {
        0
    } else {
        (now_secs() - t).max(0) / 86400
    }
}

pub fn stale_limit(st: &str) -> i64 {
    match st {
        "doing" => STALE_DOING,
        "review" => STALE_REVIEW,
        _ => STALE_TODO,
    }
}

pub fn days_until(iso: &str) -> Option<i64> {
    parse_date(iso).map(|d| d - today_days())
}

pub fn relative(n: i64) -> String {
    match n {
        n if n < -1 => format!("{} days late", -n),
        -1 => "1 day late".into(),
        0 => "today".into(),
        1 => "tomorrow".into(),
        n => format!("in {n} days"),
    }
}

pub fn ordered(projects: &[Doc]) -> Vec<Doc> {
    let mut p = projects.to_vec();
    p.sort_by_key(|(_, m)| m.get("order").and_then(|v| v.as_f64()).map(|f| f as i64).unwrap_or(99));
    p
}

pub fn project_name(projects: &[Doc], id: &str) -> String {
    if id == "inbox" {
        return "Inbox".into();
    }
    projects
        .iter()
        .find(|(pid, _)| pid == id)
        .and_then(|(_, m)| s(m, "name"))
        .unwrap_or(id)
        .to_string()
}

fn is_active(m: &Map<String, Value>) -> bool {
    matches!(s(m, "status"), Some("active") | Some("wrapping") | None)
}

fn sort_key(m: &Map<String, Value>) -> (u8, String) {
    let st = match stage(m) {
        "doing" => 0,
        "review" => 1,
        _ => 2,
    };
    (st, s(m, "due").unwrap_or("9999").to_string())
}

/// The single best next step for each active project: in-progress work first,
/// then review, then the earliest-due to-do.
pub fn next_steps<'a>(projects: &[Doc], items: &'a [Doc]) -> Vec<(String, &'a Doc)> {
    let mut out = Vec::new();
    for (pid, p) in ordered(projects) {
        if !is_active(&p) {
            continue;
        }
        let mut open: Vec<&Doc> = items.iter().filter(|(_, m)| s(m, "project") == Some(pid.as_str()) && is_open(m)).collect();
        open.sort_by_key(|(_, m)| sort_key(m));
        if let Some(d) = open.first() {
            out.push((pid.clone(), *d));
        }
    }
    out
}

/// Project list given to the chat model when filing a note.
pub fn classify_context(projects: &[Doc], items: &[Doc]) -> String {
    let mut out = String::new();
    for (pid, p) in ordered(projects) {
        out.push_str(&format!("- id: {pid}\n  name: {}\n", s(&p, "name").unwrap_or(&pid)));
        if let Some(g) = s(&p, "goal") {
            out.push_str(&format!("  goal: {g}\n"));
        }
        if let Some(d) = s(&p, "deliverable") {
            out.push_str(&format!("  deliverable: {d}\n"));
        }
        if let Some(c) = s(&p, "context") {
            out.push_str(&format!("  context: {c}\n"));
        }
        let tasks: Vec<&str> = items
            .iter()
            .filter(|(_, m)| s(m, "project") == Some(pid.as_str()))
            .filter_map(|(_, m)| s(m, "text"))
            .take(8)
            .collect();
        if !tasks.is_empty() {
            out.push_str("  example tasks:\n");
            for t in tasks {
                out.push_str(&format!("    - {t}\n"));
            }
        }
    }
    out
}

/// One project as named facts, for Jev's `state`.
pub fn project_facts(pid: &str, p: &Map<String, Value>, items: &[Doc]) -> Value {
    let mut out = Map::new();
    out.insert("id".into(), Value::from(pid));
    out.insert("name".into(), Value::from(s(p, "name").unwrap_or(pid)));
    for k in ["goal", "deliverable", "context"] {
        if let Some(v) = s(p, k) {
            out.insert(k.into(), Value::from(v.chars().take(600).collect::<String>()));
        }
    }
    let tasks: Vec<&str> = items.iter().filter(|(_, m)| s(m, "project") == Some(pid)).filter_map(|(_, m)| s(m, "text")).take(6).collect();
    if !tasks.is_empty() {
        out.insert("example_tasks".into(), Value::from(tasks));
    }
    Value::Object(out)
}

/// A linked path as Claude Code will see it from the project folder.
pub fn from_folder(path: &str, folder: Option<&str>) -> String {
    if let Some(f) = folder.map(|f| f.trim_end_matches('/')) {
        if path == f {
            return ".".into();
        }
        if let Some(rest) = path.strip_prefix(f).and_then(|r| r.strip_prefix('/')) {
            return rest.to_string();
        }
    }
    path.to_string()
}

pub fn file_paths(m: &Map<String, Value>) -> Vec<&str> {
    m.get("files").and_then(Value::as_array).map(|a| a.iter().filter_map(|f| f["path"].as_str()).collect()).unwrap_or_default()
}

/// What's at the top of the project folder, e.g. "data/, drafts/, README.md".
fn folder_listing(folder: &str) -> Option<String> {
    let mut names: Vec<String> = std::fs::read_dir(expand_home(folder))
        .ok()?
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            (!n.starts_with('.')).then(|| if e.path().is_dir() { format!("{n}/") } else { n })
        })
        .collect();
    names.sort();
    names.truncate(60);
    Some(names.join(", "))
}

/// Everything known about one task and its project, for drafting a prompt for Claude Code.
pub fn prompt_context(id: &str, item: &Map<String, Value>, projects: &[Doc], items: &[Doc]) -> String {
    let today = today_days();
    let pid = s(item, "project").unwrap_or("inbox");
    let project = projects.iter().find(|(p, _)| p == pid).map(|(_, m)| m);
    let folder = project.and_then(|p| s(p, "folder"));
    let clip = |t: &str, n: usize| t.chars().take(n).collect::<String>();
    let mut out = format!("Today is {} {}.\n\nTask: {}\n", weekday_name(today), iso_from_days(today), s(item, "text").unwrap_or(""));
    if let Some(o) = s(item, "original") {
        out.push_str(&format!("The note it came from: {}\n", clip(o, 2000)));
    }
    if let Some(n) = s(item, "notes") {
        out.push_str(&format!("Task notes: {}\n", clip(n, 2000)));
    }
    let mut line = format!("Stage: {}", stage_label(stage(item)));
    if let Some(d) = s(item, "due") {
        if let Some(n) = days_until(d) {
            line.push_str(&format!(" | due {} ({})", short_date(d), relative(n)));
        }
    }
    out.push_str(&line);
    out.push('\n');
    let tf = file_paths(item);
    if !tf.is_empty() {
        out.push_str("Files linked to this task:\n");
        for f in tf {
            out.push_str(&format!("  - {}\n", from_folder(f, folder)));
        }
    }
    let Some(p) = project else {
        out.push_str("\nThe task isn't in a project yet.\n");
        return out;
    };
    out.push_str(&format!("\nProject: {}\n", s(p, "name").unwrap_or(pid)));
    for (k, label, n) in [("goal", "Goal", 1000), ("summary", "Overview", 2500), ("context", "Context", 1000), ("notes", "Project notes", 2000)] {
        if let Some(v) = s(p, k) {
            out.push_str(&format!("{label}: {}\n", clip(v, n)));
        }
    }
    if let Some(dl) = s(p, "deadline") {
        if let Some(n) = days_until(dl) {
            out.push_str(&format!("Project deadline: {} ({})\n", short_date(dl), relative(n)));
        }
    }
    match folder {
        Some(f) => {
            out.push_str(&format!("Project folder (Claude Code runs here): {f}\n"));
            if let Some(l) = folder_listing(f) {
                out.push_str(&format!("Top of the folder: {l}\n"));
            }
        }
        None => out.push_str("The project has no folder yet.\n"),
    }
    let pf = file_paths(p);
    if !pf.is_empty() {
        out.push_str("Files linked to the project:\n");
        for f in pf.iter().take(20) {
            out.push_str(&format!("  - {}\n", from_folder(f, folder)));
        }
    }
    let mut others: Vec<&Doc> = items.iter().filter(|(i, m)| i != id && s(m, "project") == Some(pid) && is_open(m)).collect();
    others.sort_by_key(|(_, m)| sort_key(m));
    if !others.is_empty() {
        out.push_str("Other open tasks in the project:\n");
        for (_, m) in others.iter().take(12) {
            out.push_str(&format!("  - [{}] {}\n", stage_label(stage(m)), s(m, "text").unwrap_or("")));
        }
    }
    out
}

/// One of Jev's judgments kept on a task (see jev.rs): hours, waiting, vague, matters, effort.
fn judged(m: &Map<String, Value>, k: &str) -> Option<f64> {
    m.get("jev")?.get(k)?.as_f64()
}

/// Bars for flagging a task from Jev's yes/no judgments.
const WAITING_FLAG: f64 = 0.6;
const VAGUE_FLAG: f64 = 0.6;
/// Hours assumed for a task Jev hasn't sized yet.
const UNSIZED_HOURS: f64 = 2.0;

fn hours_label(h: f64) -> String {
    if h < 1.0 {
        format!("{:.0} min", (h * 60.0).max(5.0))
    } else if h < 10.0 {
        format!("{} h", format!("{h:.1}").trim_end_matches(".0"))
    } else {
        format!("{:.0} h", h)
    }
}

/// Jev's judgments as a few words for a task line.
fn judged_label(m: &Map<String, Value>) -> String {
    let mut out = String::new();
    if let Some(h) = judged(m, "hours") {
        out.push_str(&format!(" | ~{} (Jev)", hours_label(h)));
    }
    if judged(m, "waiting").map_or(false, |p| p >= WAITING_FLAG) {
        out.push_str(" | probably waiting on someone");
    }
    if judged(m, "vague").map_or(false, |p| p >= VAGUE_FLAG) {
        out.push_str(" | vague");
    }
    if judged(m, "matters").map_or(false, |x| x >= 2.5) {
        out.push_str(" | critical to the project goal");
    }
    out
}

/// 0 for Monday … 6 for Sunday (1970-01-01 was a Thursday).
fn weekday_index(days: i64) -> i64 {
    (days + 3).rem_euclid(7)
}

/// Working days (Monday to Friday) from today through `day`, counting today.
fn workdays_through(today: i64, day: i64) -> i64 {
    (today..=day).filter(|d| weekday_index(*d) < 5).count() as i64
}

/// A plan worked out in code from Jev's judgments: deadlines that don't fit the working time
/// left, today's tasks (least room to spare first), quick wins, and what's stuck. The chat
/// model writes the brief from it. None until Jev has sized some tasks.
pub fn plan_text(projects: &[Doc], items: &[Doc], hours_per_day: f64) -> Option<String> {
    let open: Vec<&Doc> = items.iter().filter(|(_, m)| is_open(m) && s(m, "project") != Some("inbox")).collect();
    if !open.iter().any(|(_, m)| judged(m, "hours").is_some()) {
        return None;
    }
    let today = today_days();
    let hours = |m: &Map<String, Value>| judged(m, "hours").unwrap_or(UNSIZED_HOURS);
    let stuck = |m: &Map<String, Value>| {
        judged(m, "waiting").map_or(false, |p| p >= WAITING_FLAG) || judged(m, "vague").map_or(false, |p| p >= VAGUE_FLAG)
    };
    let label = |m: &Map<String, Value>| format!("{} ({})", s(m, "text").unwrap_or(""), project_name(projects, s(m, "project").unwrap_or("inbox")));

    // Deadlines, soonest first: the work due by each date against the working time until then.
    let mut dated: Vec<(&Doc, i64)> = open.iter().filter_map(|d| Some((*d, parse_date(s(&d.1, "due")?)?))).collect();
    dated.sort_by_key(|(_, due)| *due);
    let mut days: Vec<(i64, f64, f64)> = Vec::new(); // (due, work due by then, time available)
    let mut cum = 0.0;
    for (d, due) in &dated {
        cum += hours(&d.1);
        let avail = workdays_through(today, *due) as f64 * hours_per_day;
        match days.last_mut() {
            Some(last) if last.0 == *due => last.1 = cum,
            _ => days.push((*due, cum, avail)),
        }
    }
    let at_risk: Vec<String> = days
        .iter()
        .filter(|(due, work, avail)| *due >= today && work > avail)
        .take(4)
        .map(|(due, work, avail)| {
            format!("- {}: about {} of work due by then, about {} of working time", short_date(&iso_from_days(*due)), hours_label(*work), hours_label(*avail))
        })
        .collect();
    // A task's room to spare is the tightest gap at its deadline or any later one.
    let mut room: Vec<f64> = days.iter().map(|(_, w, a)| a - w).collect();
    for i in (0..room.len().saturating_sub(1)).rev() {
        room[i] = room[i].min(room[i + 1]);
    }
    let room_of = |id: &str| {
        let due = dated.iter().find(|(d, _)| d.0 == id)?.1;
        days.iter().position(|(d, _, _)| *d == due).map(|i| room[i])
    };

    // Today: tight deadlines, then work already started, then other deadlines, then the rest
    // by how much it matters. Waiting and vague tasks are listed separately instead.
    let rank = |(id, m): &&Doc| {
        let r = room_of(id);
        let started = matches!(stage(m), "doing" | "review");
        let tier = match r {
            Some(r) if r < hours_per_day * 2.0 => 0,
            _ if started => 1,
            Some(_) => 2,
            None => 3,
        };
        let within = r.unwrap_or(0.0) - judged(m, "matters").unwrap_or(1.0) * 100.0;
        (tier, (within * 10.0) as i64)
    };
    let mut ready: Vec<&Doc> = open.iter().copied().filter(|(_, m)| !stuck(m)).collect();
    ready.sort_by_key(|d| rank(d));
    let mut today_list = Vec::new();
    let mut used = 0.0;
    for (id, m) in &ready {
        if today_list.len() >= 6 || (used >= hours_per_day && !today_list.is_empty()) {
            break;
        }
        used += hours(m).min(hours_per_day);
        let mut line = format!("- {} · ~{}", label(m), hours_label(hours(m)));
        if let Some(d) = s(m, "due") {
            line.push_str(&format!(" · due {}", short_date(d)));
            if let Some(r) = room_of(id) {
                line.push_str(&if r < 0.0 { format!(", {} short", hours_label(-r)) } else { format!(", {} to spare", hours_label(r)) });
            }
        }
        if matches!(stage(m), "doing" | "review") {
            line.push_str(&format!(" · {}", stage_label(stage(m)).to_lowercase()));
        }
        today_list.push((id.clone(), line));
    }
    let mut quick: Vec<&Doc> = ready
        .iter()
        .copied()
        .filter(|(id, m)| judged(m, "hours").map_or(false, |h| h <= 0.75) && !today_list.iter().any(|(t, _)| t == id))
        .collect();
    quick.sort_by(|a, b| judged(&b.1, "matters").unwrap_or(1.0).total_cmp(&judged(&a.1, "matters").unwrap_or(1.0)));
    let waiting: Vec<String> =
        open.iter().filter(|(_, m)| judged(m, "waiting").map_or(false, |p| p >= WAITING_FLAG)).take(5).map(|(_, m)| format!("- {}", label(m))).collect();
    let vague: Vec<String> = open
        .iter()
        .filter(|(_, m)| judged(m, "vague").map_or(false, |p| p >= VAGUE_FLAG))
        .filter(|(_, m)| s(m, "due").is_some() || stage(m) != "todo" || judged(m, "matters").map_or(false, |x| x >= 2.0))
        .take(3)
        .map(|(_, m)| format!("- {}", label(m)))
        .collect();

    let mut out = format!(
        "Plan worked out by Work Desk from Jev's judgments (sizes are Jev's estimates; about {} of project work a day; unsized tasks count as {}):\n",
        hours_label(hours_per_day),
        hours_label(UNSIZED_HOURS)
    );
    let mut section = |title: &str, lines: &[String]| {
        if !lines.is_empty() {
            out.push_str(&format!("{title}:\n{}\n", lines.join("\n")));
        }
    };
    section("At risk", &at_risk);
    section("Today, in this order", &today_list.into_iter().map(|(_, l)| l).collect::<Vec<_>>());
    section("Quick wins", &quick.iter().take(3).map(|(_, m)| format!("- {} · ~{}", label(m), hours_label(hours(m)))).collect::<Vec<_>>());
    section("Waiting on someone", &waiting);
    section("Too vague to start (decide what done looks like first)", &vague);
    Some(out)
}

/// Full picture of open work, given to the chat model for the daily brief.
pub fn brief_context(projects: &[Doc], items: &[Doc]) -> String {
    let today = today_days();
    let mut out = format!("Today is {} {}.\n\n", weekday_name(today), iso_from_days(today));
    let push_tasks = |out: &mut String, pid: &str| {
        let mut open: Vec<&Doc> = items.iter().filter(|(_, m)| s(m, "project") == Some(pid) && is_open(m)).collect();
        open.sort_by_key(|(_, m)| sort_key(m));
        if open.is_empty() {
            out.push_str("  (no open tasks)\n");
        }
        for (_, m) in open.iter().take(30) {
            let mut line = format!("  * [{}] {}", stage_label(stage(m)), s(m, "text").unwrap_or(""));
            if let Some(d) = s(m, "due") {
                if let Some(n) = days_until(d) {
                    line.push_str(&format!(" | due {} ({})", short_date(d), relative(n)));
                }
            } else {
                line.push_str(" | no due date");
            }
            let idle = idle_days(m);
            if idle > 0 {
                line.push_str(&format!(" | untouched {idle} days"));
            }
            line.push_str(&judged_label(m));
            out.push_str(&line);
            out.push('\n');
        }
    };
    for (pid, p) in ordered(projects) {
        if !is_active(&p) {
            continue;
        }
        out.push_str(&format!("Project: {}", s(&p, "name").unwrap_or(&pid)));
        if s(&p, "tier") == Some("major") {
            out.push_str(" (major)");
        }
        if let Some(dl) = s(&p, "deadline") {
            if let Some(n) = days_until(dl) {
                out.push_str(&format!(" | deadline {} ({})", short_date(dl), relative(n)));
            }
        }
        out.push('\n');
        if let Some(g) = s(&p, "goal") {
            out.push_str(&format!("  goal: {g}\n"));
        }
        match s(&p, "deliverable") {
            Some(d) => out.push_str(&format!("  deliverable: {d}\n")),
            None => out.push_str("  deliverable: none set yet\n"),
        }
        push_tasks(&mut out, &pid);
        out.push('\n');
    }
    let inbox = items.iter().filter(|(_, m)| s(m, "project") == Some("inbox") && is_open(m)).count();
    if inbox > 0 {
        out.push_str(&format!("Inbox: {inbox} unsorted notes.\n"));
    }
    out
}

pub struct Paint {
    on: bool,
}
impl Paint {
    pub fn new(on: bool) -> Self {
        Self { on }
    }
    fn wrap(&self, code: &str, t: &str) -> String {
        if self.on {
            format!("\x1b[{code}m{t}\x1b[0m")
        } else {
            t.to_string()
        }
    }
    pub fn bold(&self, t: &str) -> String {
        self.wrap("1", t)
    }
    pub fn dim(&self, t: &str) -> String {
        self.wrap("2", t)
    }
    pub fn red(&self, t: &str) -> String {
        self.wrap("31", t)
    }
    pub fn yellow(&self, t: &str) -> String {
        self.wrap("33", t)
    }
    pub fn cyan(&self, t: &str) -> String {
        self.wrap("36", t)
    }
    pub fn violet(&self, t: &str) -> String {
        self.wrap("35", t)
    }
}

/// The `desk today` report.
pub fn today_report(projects: &[Doc], items: &[Doc], c: &Paint) -> String {
    let today = today_days();
    let mut out = String::new();
    let (_, m, d) = civil_from_days(today);
    let mon = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][(m - 1) as usize];
    out.push_str(&format!("{}  {}\n", c.bold("WORK DESK"), c.dim(&format!("{}, {mon} {d}", weekday_name(today)))));

    let open: Vec<&Doc> = items.iter().filter(|(_, m)| is_open(m)).collect();
    let name = |pid: Option<&str>| project_name(projects, pid.unwrap_or("inbox"));
    let line = |m: &Map<String, Value>, extra: String| {
        format!("  • {}  {}{}\n", s(m, "text").unwrap_or(""), c.dim(&name(s(m, "project"))), extra)
    };

    let mut overdue: Vec<&&Doc> = open.iter().filter(|(_, m)| s(m, "due").and_then(days_until).map_or(false, |n| n < 0)).collect();
    overdue.sort_by_key(|(_, m)| s(m, "due").unwrap_or("").to_string());
    if !overdue.is_empty() {
        out.push_str(&format!("\n{}\n", c.red(&c.bold("Overdue"))));
        for (_, m) in overdue {
            let n = days_until(s(m, "due").unwrap()).unwrap();
            out.push_str(&line(m, format!("  {}", c.red(&relative(n)))));
        }
    }

    let mut soon: Vec<&&Doc> = open
        .iter()
        .filter(|(_, m)| s(m, "due").and_then(days_until).map_or(false, |n| (0..=2).contains(&n)))
        .collect();
    soon.sort_by_key(|(_, m)| s(m, "due").unwrap_or("").to_string());
    if !soon.is_empty() {
        out.push_str(&format!("\n{}\n", c.yellow(&c.bold("Due in the next 2 days"))));
        for (_, m) in soon {
            let n = days_until(s(m, "due").unwrap()).unwrap();
            out.push_str(&line(m, format!("  {}", c.yellow(&relative(n)))));
        }
    }

    let stalled: Vec<&&Doc> = open
        .iter()
        .filter(|(_, m)| stage(m) != "todo" && idle_days(m) >= stale_limit(stage(m)))
        .collect();
    if !stalled.is_empty() {
        out.push_str(&format!("\n{}\n", c.yellow(&c.bold("Stalled"))));
        for (_, m) in stalled {
            out.push_str(&line(m, format!("  {}", c.yellow(&format!("{} for {} days", stage_label(stage(m)), idle_days(m))))));
        }
    }

    let nexts = next_steps(projects, items);
    if !nexts.is_empty() {
        out.push_str(&format!("\n{}\n", c.cyan(&c.bold("Next up, one per project"))));
        for (_, (_, m)) in nexts {
            let tagx = match stage(m) {
                "doing" => format!("  {}", c.cyan("in progress")),
                "review" => format!("  {}", c.violet("review")),
                _ => String::new(),
            };
            out.push_str(&line(m, tagx));
        }
    }

    let mut dl_lines = Vec::new();
    for (pid, p) in ordered(projects) {
        if let Some(dl) = s(&p, "deadline") {
            if let Some(n) = days_until(dl) {
                let po: Vec<&&Doc> = open.iter().filter(|(_, m)| s(m, "project") == Some(pid.as_str())).collect();
                let undated = po.iter().filter(|(_, m)| s(m, "due").is_none()).count();
                let mut t = format!("  • {}  {}  {} open", s(&p, "name").unwrap_or(&pid), short_date(dl), po.len());
                if undated > 0 {
                    t.push_str(&format!(", {undated} without dates"));
                }
                let when = if n < 0 { c.red(&format!("{} days past", -n)) } else if n <= 14 { c.yellow(&format!("{n} days left")) } else { format!("{n} days left") };
                dl_lines.push(format!("{t}  {when}\n"));
            }
        }
    }
    if !dl_lines.is_empty() {
        out.push_str(&format!("\n{}\n", c.bold("Deadlines")));
        for l in dl_lines {
            out.push_str(&l);
        }
    }

    let empty: Vec<String> = ordered(projects)
        .into_iter()
        .filter(|(pid, p)| is_active(p) && !items.iter().any(|(_, m)| s(m, "project") == Some(pid.as_str())))
        .map(|(pid, p)| s(&p, "name").unwrap_or(&pid).to_string())
        .collect();
    if !empty.is_empty() {
        out.push_str(&format!("\n{}\n", c.dim(&format!("No tasks yet: {}", empty.join(", ")))));
    }

    let undelivered: Vec<String> = ordered(projects)
        .into_iter()
        .filter(|(_, p)| is_active(p) && s(p, "deliverable").is_none())
        .map(|(pid, p)| s(&p, "name").unwrap_or(&pid).to_string())
        .collect();
    if !undelivered.is_empty() {
        out.push_str(&format!("\n{}\n", c.yellow(&format!("No deliverable yet: {}. Set one on the project's page.", undelivered.join(", ")))));
    }

    let inbox = open.iter().filter(|(_, m)| s(m, "project") == Some("inbox")).count();
    if inbox > 0 {
        out.push_str(&format!("\n{}\n", c.yellow(&format!("{inbox} note(s) waiting in the Inbox. Run `desk sort` to file them."))));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc(id: &str, v: Value) -> Doc {
        (id.to_string(), v.as_object().unwrap().clone())
    }

    /// The next Monday on or after today, so the working-day count is predictable.
    fn monday() -> i64 {
        let t = today_days();
        t + (7 - weekday_index(t)) % 7
    }

    #[test]
    fn plan_needs_judgments() {
        let projects = vec![doc("p", json!({"name": "P"}))];
        let items = vec![doc("a", json!({"text": "A", "project": "p"}))];
        assert!(plan_text(&projects, &items, 4.0).is_none());
    }

    #[test]
    fn plan_flags_deadlines_that_dont_fit() {
        let projects = vec![doc("p", json!({"name": "Paper"}))];
        // due next Monday: 40 h of work, and at most five 4 h working days before then
        let due = iso_from_days(monday());
        let items = vec![
            doc("big", json!({"text": "Write methods", "project": "p", "due": due, "jev": {"hours": 40.0, "waiting": 0.1, "vague": 0.1}})),
            doc("quick", json!({"text": "Email Sonia", "project": "p", "jev": {"hours": 0.25, "waiting": 0.05, "vague": 0.0, "matters": 2.0}})),
            doc("stuck", json!({"text": "Get Eric's sign-off", "project": "p", "jev": {"hours": 0.5, "waiting": 0.9, "vague": 0.1}})),
            doc("fuzzy", json!({"text": "Think about the model", "project": "p", "stage": "doing", "jev": {"hours": 4.0, "waiting": 0.0, "vague": 0.8}})),
        ];
        let plan = plan_text(&projects, &items, 4.0).unwrap();
        assert!(plan.contains("At risk:\n- "), "{plan}");
        let today = plan.split("Today, in this order:\n").nth(1).unwrap();
        assert!(today.starts_with("- Write methods (Paper) · ~40 h · due"), "{plan}");
        assert!(plan.contains("Quick wins:\n- Email Sonia (Paper) · ~15 min"), "{plan}");
        assert!(today.contains("short"), "{plan}");
        assert!(plan.contains("Waiting on someone:\n- Get Eric's sign-off (Paper)"), "{plan}");
        assert!(plan.contains("Too vague to start (decide what done looks like first):\n- Think about the model (Paper)"), "{plan}");
        // stuck tasks never land in Today
        assert!(!today.split("Quick wins:").next().unwrap().contains("Get Eric"), "{plan}");
    }

    #[test]
    fn counts_working_days() {
        let m = monday();
        assert_eq!(workdays_through(m, m + 4), 5);
        assert_eq!(workdays_through(m, m + 6), 5);
        assert_eq!(workdays_through(m + 5, m + 6), 0);
    }
}
