//! The Claude Code sessions saved for this repository: each is a JSON-lines
//! file in `~/.claude/projects/<folder>/`, the folder named after the
//! session's working directory (`/` and `.` as `-`). The viewer lists those
//! of the repository and its worktrees, shows one as a conversation (its
//! lines turned into the ACP events a live session gives), and resumes one
//! through the agent (`session/load`).

use crate::fx::HashMap;
use serde_json::{json, Value as Json};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Where Claude Code keeps the sessions of a working directory.
fn project_dir(cwd: &Path) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let name: String = cwd.to_string_lossy().chars().map(|c| if c == '/' || c == '.' { '-' } else { c }).collect();
    Some(Path::new(&home).join(".claude").join("projects").join(name))
}

/// What the list shows of a session file, kept while the file doesn't change.
static META: Mutex<Option<HashMap<PathBuf, (u64, u64, Option<Json>)>>> = Mutex::new(None);

fn text_of(content: &Json) -> String {
    match content {
        Json::String(s) => s.clone(),
        Json::Array(xs) => xs.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// A user's text as they typed it: without the reminders the harness adds,
/// and a slash command as `/name args`. None: nothing the user typed.
fn user_text(raw: &str) -> Option<String> {
    let mut t = raw.to_string();
    while let (Some(a), Some(b)) = (t.find("<system-reminder>"), t.find("</system-reminder>")) {
        if b < a {
            break;
        }
        t.replace_range(a..b + "</system-reminder>".len(), "");
    }
    let tag = |t: &str, name: &str| -> Option<String> {
        let (open, close) = (format!("<{name}>"), format!("</{name}>"));
        let a = t.find(&open)? + open.len();
        let b = t[a..].find(&close)? + a;
        Some(t[a..b].trim().to_string())
    };
    if let Some(cmd) = tag(&t, "command-name") {
        let args = tag(&t, "command-args").unwrap_or_default();
        return Some(if args.is_empty() { cmd } else { format!("{cmd} {args}") });
    }
    let t = t.trim();
    if t.is_empty() || t.starts_with("<local-command") || t.starts_with("Caveat:") || t.starts_with("<task-notification>") {
        return None;
    }
    Some(t.to_string())
}

fn meta_of(file: &Path) -> Option<Json> {
    let text = std::fs::read_to_string(file).ok()?;
    let (mut custom, mut ai, mut first, mut cwd, mut branch, mut started) = (None::<String>, None::<String>, None::<String>, None::<String>, None::<String>, None::<String>);
    let mut messages = 0;
    for line in text.lines() {
        // the large lines (requests, snapshots) say nothing the list needs
        if line.len() > 200_000 {
            continue;
        }
        let Ok(j) = serde_json::from_str::<Json>(line) else { continue };
        match j["type"].as_str() {
            Some("custom-title") => custom = j["customTitle"].as_str().map(String::from).or(custom),
            Some("ai-title") => ai = j["aiTitle"].as_str().map(String::from).or(ai),
            Some("user" | "assistant") if j["isSidechain"] != true && j["isMeta"] != true => {
                cwd = cwd.or(j["cwd"].as_str().map(String::from));
                branch = j["gitBranch"].as_str().map(String::from).or(branch);
                started = started.or(j["timestamp"].as_str().map(String::from));
                if j["type"] == "user" {
                    if let Some(t) = user_text(&text_of(&j["message"]["content"])) {
                        messages += 1;
                        first = first.or(Some(t));
                    }
                } else {
                    messages += 1;
                }
            }
            _ => {}
        }
    }
    if messages == 0 {
        return None;
    }
    let title = custom.or(ai).or(first.map(|f| f.lines().next().unwrap_or("").chars().take(80).collect())).unwrap_or_default();
    let id = file.file_stem()?.to_string_lossy().to_string();
    Some(json!({"id": id, "title": title, "cwd": cwd, "branch": branch, "started": started, "messages": messages}))
}

/// The saved sessions of these working directories, the latest first.
pub fn list(cwds: &[PathBuf]) -> Json {
    let mut out = vec![];
    let mut cache = META.lock().unwrap_or_else(|e| e.into_inner());
    let cache = cache.get_or_insert_with(HashMap::default);
    for cwd in cwds {
        let Some(dir) = project_dir(cwd) else { continue };
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(md) = e.metadata() else { continue };
            let mtime = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
            let fresh = cache.get(&p).is_some_and(|(m, l, _)| *m == mtime && *l == md.len());
            if !fresh {
                cache.insert(p.clone(), (mtime, md.len(), meta_of(&p)));
            }
            if let Some((_, _, Some(m))) = cache.get(&p) {
                let mut m = m.clone();
                m["updated"] = json!(mtime);
                m["dir"] = json!(cwd);
                out.push(m);
            }
        }
    }
    out.sort_by(|a, b| b["updated"].as_u64().cmp(&a["updated"].as_u64()));
    Json::Array(out)
}

fn tool_kind(name: &str) -> &'static str {
    match name {
        "Bash" | "BashOutput" | "KillShell" => "execute",
        "Read" => "read",
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => "edit",
        "Grep" | "Glob" | "LS" => "search",
        "WebFetch" | "WebSearch" => "fetch",
        "Task" | "Agent" => "think",
        _ => "other",
    }
}

/// A tool use as an ACP `tool_call`.
fn tool_call(b: &Json) -> Json {
    let name = b["name"].as_str().unwrap_or("tool");
    let input = &b["input"];
    let path = input["file_path"].as_str().or(input["path"].as_str()).or(input["notebook_path"].as_str());
    let title = match name {
        "Bash" => input["command"].as_str().unwrap_or("").lines().next().unwrap_or("").to_string(),
        _ => match path.or(input["pattern"].as_str()).or(input["url"].as_str()).or(input["query"].as_str()).or(input["description"].as_str()) {
            Some(x) => format!("{name} {x}"),
            None => name.to_string(),
        },
    };
    let content = match name {
        "Edit" => json!([{"type": "diff", "path": path, "oldText": input["old_string"], "newText": input["new_string"]}]),
        "Write" => json!([{"type": "diff", "path": path, "oldText": null, "newText": input["content"]}]),
        _ => json!([]),
    };
    json!({"sessionUpdate": "tool_call", "toolCallId": b["id"], "title": title, "kind": tool_kind(name), "status": "pending",
           "rawInput": input, "content": content, "locations": path.map(|p| json!([{"path": p}])).unwrap_or(json!([]))})
}

fn update(u: Json) -> Json {
    json!({"kind": "update", "update": u})
}

/// A saved session as the events a live one gives (`/api/history/<id>`).
pub fn transcript(cwds: &[PathBuf], id: &str) -> Json {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return json!({"error": "not a session id"});
    }
    let Some(file) = cwds.iter().filter_map(|c| project_dir(c)).map(|d| d.join(format!("{id}.jsonl"))).find(|f| f.is_file()) else {
        return json!({"error": format!("no saved session {id} for this repository")});
    };
    let Ok(text) = std::fs::read_to_string(&file) else { return json!({"error": "can't read the session"}) };
    let mut events = vec![];
    // tool uses edited in place: a Write's content shows as a diff, not again as output
    let mut edits = crate::fx::HashSet::default();
    for line in text.lines() {
        if line.len() > 2_000_000 {
            continue;
        }
        let Ok(j) = serde_json::from_str::<Json>(line) else { continue };
        if j["isSidechain"] == true || j["isMeta"] == true {
            continue;
        }
        let c = &j["message"]["content"];
        match j["type"].as_str() {
            Some("user") => {
                if let Some(t) = c.as_str().and_then(user_text) {
                    events.push(json!({"kind": "user", "text": t}));
                }
                for b in c.as_array().into_iter().flatten() {
                    match b["type"].as_str() {
                        Some("text") => {
                            if let Some(t) = b["text"].as_str().and_then(user_text) {
                                events.push(json!({"kind": "user", "text": t}));
                            }
                        }
                        Some("tool_result") => {
                            let id = b["tool_use_id"].as_str().unwrap_or("");
                            let status = if b["is_error"] == true { "failed" } else { "completed" };
                            let mut u = json!({"sessionUpdate": "tool_call_update", "toolCallId": id, "status": status});
                            if !edits.contains(id) || status == "failed" {
                                let out = text_of(&b["content"]);
                                let out: String = out.chars().take(20_000).collect();
                                u["content"] = json!([{"type": "content", "content": {"type": "text", "text": out}}]);
                            }
                            events.push(update(u));
                        }
                        _ => {}
                    }
                }
            }
            Some("assistant") => {
                let mid = &j["message"]["id"];
                for b in c.as_array().into_iter().flatten() {
                    match b["type"].as_str() {
                        Some("text") => events.push(update(json!({"sessionUpdate": "agent_message_chunk", "messageId": mid, "content": {"type": "text", "text": b["text"]}}))),
                        Some("thinking") if b["thinking"].as_str().is_some_and(|t| !t.trim().is_empty()) => {
                            events.push(update(json!({"sessionUpdate": "agent_thought_chunk", "messageId": mid, "content": {"type": "text", "text": b["thinking"]}})))
                        }
                        Some("tool_use") if b["name"] == "TodoWrite" => {
                            let entries: Vec<Json> = b["input"]["todos"].as_array().into_iter().flatten().map(|t| json!({"content": t["content"], "status": t["status"], "priority": "medium"})).collect();
                            events.push(update(json!({"sessionUpdate": "plan", "entries": entries})));
                        }
                        Some("tool_use") => {
                            if matches!(b["name"].as_str(), Some("Edit" | "Write")) {
                                edits.insert(b["id"].as_str().unwrap_or("").to_string());
                            }
                            events.push(update(tool_call(b)));
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    let meta = meta_of(&file).unwrap_or(json!({}));
    json!({"id": id, "meta": meta, "list": events})
}
