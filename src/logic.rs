//! Shared task logic: stages, staleness, and the text summaries used by the
//! terminal commands and by the prompts sent to DeepSeek.

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

/// Project list given to DeepSeek when filing a note.
pub fn classify_context(projects: &[Doc], items: &[Doc]) -> String {
    let mut out = String::new();
    for (pid, p) in ordered(projects) {
        out.push_str(&format!("- id: {pid}\n  name: {}\n", s(&p, "name").unwrap_or(&pid)));
        if let Some(g) = s(&p, "goal") {
            out.push_str(&format!("  goal: {g}\n"));
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

/// Full picture of open work, given to DeepSeek for the daily brief.
pub fn brief_context(projects: &[Doc], items: &[Doc]) -> String {
    let today = today_days();
    let mut out = format!("Today is {} {}.\n\n", weekday_name(today), iso_from_days(today));
    let mut push_tasks = |out: &mut String, pid: &str| {
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

    let inbox = open.iter().filter(|(_, m)| s(m, "project") == Some("inbox")).count();
    if inbox > 0 {
        out.push_str(&format!("\n{}\n", c.yellow(&format!("{inbox} note(s) waiting in the Inbox. Run `desk sort` to file them."))));
    }
    out
}
