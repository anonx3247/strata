//! Agent sessions for the viewer's Agents mode, over ACP (the Agent Client
//! Protocol): JSON-RPC 2.0 on the agent's stdin and stdout, one message per
//! line. Each session is a subprocess; a thread reads what it says into a
//! list of events the page polls (`/api/agents/<id>?since=N`).
//!
//! The viewer doesn't offer file system or terminal capabilities, so the
//! agent uses its own tools; it asks before running them
//! (`session/request_permission`), and the page answers, or the session
//! allows them itself (auto-allow).

use crate::fx::HashMap;
use serde_json::{json, Value as Json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};

/// The agent run when nothing says otherwise: Claude Code over ACP.
pub const DEFAULT_AGENT: &str = "npx -y @agentclientprotocol/claude-agent-acp";

/// The agent command: `STRATA_UI_AGENT`, else the package's `[ui] agent`, else Claude Code.
pub fn agent_command(dir: &Path) -> String {
    if let Ok(c) = std::env::var("STRATA_UI_AGENT") {
        if !c.trim().is_empty() {
            return c;
        }
    }
    ui_rules(dir).agent.unwrap_or_else(|| DEFAULT_AGENT.to_string())
}

/// Whether new sessions allow their tools without asking (`[ui] auto_allow`;
/// by default they do: agents ask for little that a person would refuse).
pub fn auto_allow_default(dir: &Path) -> bool {
    ui_rules(dir).auto_allow.unwrap_or(true)
}

fn ui_rules(dir: &Path) -> crate::package::UiRules {
    crate::package::find(dir).ok().flatten().map(|p| p.ui).unwrap_or_default()
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[derive(Default)]
struct State {
    /// what happened, in order: `{kind, t, ...}`
    events: Vec<Json>,
    /// starting, ready, working, stopped, failed
    status: &'static str,
    session_id: Option<String>,
    next_rpc: u64,
    /// our requests waiting for an answer: id → method
    waiting: HashMap<u64, &'static str>,
    /// prompts sent before the session was ready, or while it worked
    queued: Vec<String>,
    /// permission requests not answered yet: their JSON-RPC ids
    asking: Vec<Json>,
    auto_allow: bool,
    /// a saved session to load (`session/load`) instead of a new one
    load: Option<String>,
}

pub struct Session {
    pub id: usize,
    pub title: String,
    pub cwd: PathBuf,
    pub command: String,
    started: u64,
    stdin: Mutex<Option<ChildStdin>>,
    child: Mutex<Option<Child>>,
    st: Mutex<State>,
}

#[derive(Default)]
pub struct Agents {
    sessions: Mutex<Vec<Arc<Session>>>,
}

impl State {
    fn push(&mut self, mut e: Json) {
        e["t"] = json!(now());
        self.events.push(e);
    }
}

impl Session {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Write a message to the agent. (A failure shows as the agent's exit.)
    fn send(&self, msg: &Json) {
        let mut w = self.stdin.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(w) = w.as_mut() {
            let _ = w.write_all(format!("{msg}\n").as_bytes()).and_then(|_| w.flush());
        }
    }

    /// Send a request; its answer is handled by `answered`.
    fn request(&self, st: &mut State, method: &'static str, params: Json) {
        st.next_rpc += 1;
        let id = st.next_rpc;
        st.waiting.insert(id, method);
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
    }

    fn prompt_now(&self, st: &mut State, text: String) {
        let Some(sid) = st.session_id.clone() else { return };
        st.status = "working";
        self.request(st, "session/prompt", json!({"sessionId": sid, "prompt": [{"type": "text", "text": text}]}));
    }

    /// A line the agent wrote.
    fn heard(&self, line: &str) {
        let Ok(msg) = serde_json::from_str::<Json>(line) else {
            self.lock().push(json!({"kind": "log", "text": line}));
            return;
        };
        let mut st = self.lock();
        match (msg.get("method").and_then(|m| m.as_str()), msg.get("id")) {
            (Some("session/update"), _) => {
                let u = msg["params"]["update"].clone();
                st.push(json!({"kind": "update", "update": u}));
            }
            (Some("session/request_permission"), Some(id)) => {
                let p = &msg["params"];
                let options = p["options"].as_array().cloned().unwrap_or_default();
                if st.auto_allow {
                    let pick = options.iter().find(|o| o["kind"] == "allow_once").or_else(|| options.iter().find(|o| o["kind"].as_str().is_some_and(|k| k.starts_with("allow"))));
                    if let Some(o) = pick {
                        let oid = o["optionId"].clone();
                        st.push(json!({"kind": "permission", "rpc": id, "toolCall": p["toolCall"], "options": options, "answer": oid, "auto": true}));
                        drop(st);
                        self.send(&json!({"jsonrpc": "2.0", "id": id, "result": {"outcome": {"outcome": "selected", "optionId": oid}}}));
                        return;
                    }
                }
                st.asking.push(id.clone());
                st.push(json!({"kind": "permission", "rpc": id, "toolCall": p["toolCall"], "options": options}));
            }
            (Some(m), Some(id)) => {
                // fs/*, terminal/*: not offered
                let m = m.to_string();
                drop(st);
                self.send(&json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("the viewer doesn't provide `{m}`")}}));
            }
            (Some(_), None) => {}
            (None, Some(id)) => {
                let method = id.as_u64().and_then(|n| st.waiting.remove(&n));
                if let Some(err) = msg.get("error") {
                    let text = err["message"].as_str().unwrap_or("error").to_string();
                    let data = err.get("data").map(|d| d.to_string()).unwrap_or_default();
                    st.push(json!({"kind": "error", "message": format!("{}: {text}{}", method.unwrap_or("request"), if data.is_empty() || data == "null" { String::new() } else { format!(" ({data})") })}));
                    match method {
                        Some("initialize" | "session/new" | "session/load") => st.status = "failed",
                        Some("session/prompt") => {
                            st.status = "ready";
                            self.next_queued(&mut st);
                        }
                        _ => {}
                    }
                    return;
                }
                let r = &msg["result"];
                match method {
                    Some("initialize") => {
                        st.push(json!({"kind": "agent", "info": r["agentInfo"], "protocolVersion": r["protocolVersion"]}));
                        let cwd = self.cwd.display().to_string();
                        match st.load.clone() {
                            // the agent replays the saved conversation, then it goes on
                            Some(id) if r["agentCapabilities"]["loadSession"] == true => {
                                st.push(json!({"kind": "loading", "sessionId": id}));
                                self.request(&mut st, "session/load", json!({"sessionId": id, "cwd": cwd, "mcpServers": []}));
                            }
                            load => {
                                if load.is_some() {
                                    st.push(json!({"kind": "error", "message": "this agent can't load a saved session: starting a new one"}));
                                }
                                self.request(&mut st, "session/new", json!({"cwd": cwd, "mcpServers": []}));
                            }
                        }
                    }
                    Some("session/load") => {
                        st.session_id = st.load.clone();
                        st.status = "ready";
                        let id = st.load.clone();
                        st.push(json!({"kind": "ready", "sessionId": id, "loaded": true}));
                        self.next_queued(&mut st);
                    }
                    Some("session/new") => {
                        st.session_id = r["sessionId"].as_str().map(|s| s.to_string());
                        st.status = "ready";
                        st.push(json!({"kind": "ready", "sessionId": r["sessionId"], "modes": r["modes"]}));
                        self.next_queued(&mut st);
                    }
                    Some("session/prompt") => {
                        st.status = "ready";
                        st.push(json!({"kind": "stop", "reason": r["stopReason"]}));
                        self.next_queued(&mut st);
                    }
                    _ => {}
                }
            }
            (None, None) => {}
        }
    }

    fn next_queued(&self, st: &mut State) {
        if st.status == "ready" && !st.queued.is_empty() {
            let text = st.queued.remove(0);
            self.prompt_now(st, text);
        }
    }

    pub fn prompt(&self, text: String) {
        let mut st = self.lock();
        if matches!(st.status, "stopped" | "failed") {
            st.push(json!({"kind": "error", "message": "this session has stopped: start a new one"}));
            return;
        }
        st.push(json!({"kind": "user", "text": text}));
        if st.status == "ready" {
            self.prompt_now(&mut st, text);
        } else {
            st.queued.push(text);
        }
    }

    /// Answer a permission request with one of its options (None: cancelled).
    pub fn answer(&self, rpc: &Json, option: Option<&str>) -> Result<(), String> {
        let mut st = self.lock();
        let Some(i) = st.asking.iter().position(|a| a == rpc) else { return Err("that request was already answered".into()) };
        st.asking.remove(i);
        let outcome = match option {
            Some(o) => json!({"outcome": "selected", "optionId": o}),
            None => json!({"outcome": "cancelled"}),
        };
        st.push(json!({"kind": "answer", "rpc": rpc, "answer": option}));
        drop(st);
        self.send(&json!({"jsonrpc": "2.0", "id": rpc, "result": {"outcome": outcome}}));
        Ok(())
    }

    /// Stop the turn: a `session/cancel`, and every open permission request cancelled.
    pub fn cancel(&self) {
        let (sid, asking) = {
            let mut st = self.lock();
            st.queued.clear();
            (st.session_id.clone(), std::mem::take(&mut st.asking))
        };
        for rpc in asking {
            self.lock().push(json!({"kind": "answer", "rpc": rpc, "answer": null}));
            self.send(&json!({"jsonrpc": "2.0", "id": rpc, "result": {"outcome": {"outcome": "cancelled"}}}));
        }
        if let Some(sid) = sid {
            self.send(&json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": sid}}));
        }
    }

    pub fn set_auto_allow(&self, on: bool) {
        self.lock().auto_allow = on;
    }

    /// End the agent: its whole process group (the shell, the agent, what
    /// the agent started: `npx` runs node, which runs more).
    pub fn stop(&self) {
        if let Some(c) = self.child.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            let group = format!("-{}", c.id());
            let killed = Command::new("kill").args(["-TERM", "--", &group]).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
            if !killed {
                let _ = c.kill();
            }
        }
    }

    pub fn summary(&self) -> Json {
        let st = self.lock();
        json!({"id": self.id, "title": self.title, "cwd": self.cwd, "command": self.command, "started": self.started,
               "status": st.status, "asking": st.asking.len(), "session_id": st.session_id.clone().or(st.load.clone()), "events": st.events.len(), "auto_allow": st.auto_allow,
               "last": st.events.last().and_then(|e| e["t"].as_u64())})
    }

    pub fn events_since(&self, since: usize) -> Json {
        let mut s = self.summary();
        let st = self.lock();
        let from = since.min(st.events.len());
        s["from"] = json!(from);
        s["list"] = Json::Array(st.events[from..].to_vec());
        s
    }
}

impl Agents {
    pub fn list(&self) -> Json {
        Json::Array(self.sessions.lock().unwrap_or_else(|e| e.into_inner()).iter().map(|s| s.summary()).collect())
    }

    pub fn get(&self, id: usize) -> Option<Arc<Session>> {
        self.sessions.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|s| s.id == id).cloned()
    }

    /// Start `command` in `cwd` and open an ACP session in it.
    pub fn start(&self, cwd: PathBuf, title: String, command: String, auto_allow: bool, load: Option<String>) -> Result<Arc<Session>, String> {
        let mut cmd = Command::new("sh");
        // a group of its own, so that stopping it stops everything it started
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        let mut child = cmd
            .arg("-c")
            .arg(&command)
            .current_dir(&cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot start `{command}`: {e}"))?;
        let (stdin, stdout, stderr) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
        let id = {
            let list = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
            list.iter().map(|s| s.id).max().unwrap_or(0) + 1
        };
        let s = Arc::new(Session {
            id,
            title,
            cwd,
            command,
            started: now(),
            stdin: Mutex::new(stdin),
            child: Mutex::new(Some(child)),
            st: Mutex::new(State { status: "starting", auto_allow, load, ..Default::default() }),
        });
        self.sessions.lock().unwrap_or_else(|e| e.into_inner()).push(s.clone());
        if let Some(out) = stdout {
            let s = s.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(out).lines() {
                    let Ok(line) = line else { break };
                    if !line.trim().is_empty() {
                        s.heard(&line);
                    }
                }
                // (not `wait` under the lock: `stop` takes it to kill the process)
                let code = loop {
                    match s.child.lock().unwrap_or_else(|e| e.into_inner()).as_mut().map(|c| c.try_wait()) {
                        Some(Ok(Some(st))) => break st.code(),
                        Some(Ok(None)) => {}
                        _ => break None,
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                };
                let mut st = s.lock();
                st.status = if st.session_id.is_some() { "stopped" } else { "failed" };
                st.asking.clear();
                st.push(json!({"kind": "exit", "code": code}));
            });
        }
        if let Some(err) = stderr {
            let s = s.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(err).lines() {
                    let Ok(line) = line else { break };
                    s.lock().push(json!({"kind": "log", "text": line}));
                }
            });
        }
        // no file system or terminal for the agent: it uses its own tools
        {
            let mut st = s.lock();
            s.request(
                &mut st,
                "initialize",
                json!({"protocolVersion": 1, "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
                       "clientInfo": {"name": "strata-ui", "title": "Strata viewer", "version": env!("CARGO_PKG_VERSION")}}),
            );
        }
        Ok(s)
    }

    /// Stop every agent (the viewer is closing).
    pub fn stop_all(&self) {
        for s in self.sessions.lock().unwrap_or_else(|e| e.into_inner()).iter() {
            s.stop();
        }
    }
}
