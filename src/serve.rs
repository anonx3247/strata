//! `strata serve`: an HTTP server that calls the program's `handle` for each
//! request (`fn handle(req: Request) -> Response`, from `std.http`), or runs
//! it as an app (`init`, `update` and `view`, the Elm architecture): each
//! page load starts a session whose model the server keeps, the page's small
//! script sends back the messages its elements carry (`on_click(msg)`), and
//! the server answers with the view of the updated model.
//!
//! Requests are read here, turned into a `std.http.Request` through the same
//! JSON reading as `strata run`'s arguments, and the `Response` value is
//! written back. One request at a time, each with a fresh interpreter, so
//! nothing a request does is seen by the next.

use crate::ast::{FnDecl, Symbol, TypeExpr};
use crate::interp::{self, Interp, Value};
use crate::types::Env;
use serde_json::{json, Map, Value as Json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use crate::fx::HashMap;
use std::time::Instant;

const REQUEST: &str = "std.http.Request";
const RESPONSE: &str = "std.http.Response";
/// The largest body read; past it the request is refused.
const MAX_BODY: usize = 16 << 20;

/// Check that `handle` exists and has a signature the server can call.
fn handler(env: &Env) -> Result<FnDecl, String> {
    let f = env
        .fns
        .get(&Symbol::new("handle"))
        .ok_or("no `handle` function to serve: write `fn handle(req: Request) -> Response` (with `use std.http.{Request, Response}`)")?
        .clone();
    let usage = "`handle` takes a `Request` (std.http) and may ask for `Console` and capabilities (`Disk`, `Process`, or the program's own)";
    let mut requests = 0;
    for p in &f.params {
        let ty = crate::printer::ty(&p.ty);
        if ty == REQUEST {
            requests += 1;
        } else if crate::cmd::capability(env, &ty).is_none() {
            return Err(format!("{usage}. Its parameter `{}` has type `{ty}`", p.name));
        }
    }
    if requests != 1 {
        return Err(format!("{usage}: exactly one `Request`"));
    }
    let ret = crate::printer::ty(&f.ret);
    if ret != RESPONSE {
        return Err(format!("`handle` must return a `Response` (std.http), not `{ret}`"));
    }
    Ok(f)
}

/// What `strata serve` runs: a request handler, or an app.
enum Mode {
    Handle(FnDecl),
    App(App),
}

pub fn serve(env: &Env, port: u16, name: &str) -> Result<(), String> {
    let mut mode = if env.fns.contains_key(&Symbol::new("handle")) || !env.fns.contains_key(&Symbol::new("view")) {
        Mode::Handle(handler(env)?)
    } else {
        Mode::App(App::new(env, name)?)
    };
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("cannot listen on port {port}: {e}"))?;
    println!("Serving on http://127.0.0.1:{port}  (Ctrl-C to stop)");
    for stream in listener.incoming().flatten() {
        let start = Instant::now();
        let (line, status) = match read_request(&stream) {
            Ok(req) => {
                let line = format!("{} {}", req["method"].as_str().unwrap_or("?"), req["path"].as_str().unwrap_or("?"));
                let head = req["method"] == "HEAD";
                let (status, headers, body) = match &mut mode {
                    Mode::Handle(f) => call(env, f, &req),
                    Mode::App(app) => app.request(env, &req),
                };
                write_response(&stream, status, &headers, &body, head);
                (line, status)
            }
            Err((0, _)) => continue,
            Err((status, why)) => {
                write_response(&stream, status, &[("content-type".into(), "text/plain; charset=utf-8".into())], &why, false);
                (format!("({why})"), status)
            }
        };
        println!("{line} {status} ({} ms)", start.elapsed().as_millis());
    }
    Ok(())
}

// ---- apps ----

/// The script every app page loads: it sends the messages of the elements
/// the user acts on, one at a time, and puts the new view in place.
const APP_JS: &str = include_str!("serve/app.js");

/// The most sessions kept; past it, the oldest is dropped (its page reloads).
const MAX_SESSIONS: usize = 1000;

type Reply = (u16, Vec<(String, String)>, String);

/// An app: `fn init() -> Model`, `fn update(model: Model, msg: Msg) -> Model`
/// and `fn view(model: Model) -> Html[Msg]`, and the sessions' models.
struct App {
    /// The page's title: the program's name.
    title: String,
    msg: TypeExpr,
    sessions: HashMap<String, Value>,
    /// Session ids, oldest first.
    order: Vec<String>,
}

fn text(status: u16, body: String) -> Reply {
    (status, vec![("content-type".into(), "text/plain; charset=utf-8".into())], body)
}

fn fault_text(f: &interp::Fault) -> String {
    let stack = if f.stack.is_empty() { String::new() } else { format!("\n    stack: {}", crate::names::show_stack(&f.stack)) };
    format!("fault ({:?}): {}{stack}", f.kind, f.msg)
}

impl App {
    /// Check the three functions' signatures.
    fn new(env: &Env, name: &str) -> Result<App, String> {
        let usage = "an app is `fn init() -> Model`, `fn update(model: Model, msg: Msg) -> Model` and `fn view(model: Model) -> Html[Msg]` (std.web), for your own `Model` and `Msg` types";
        let get = |n: &str| env.fns.get(&Symbol::new(n)).cloned().ok_or(format!("no `{n}` function: {usage}"));
        let (init, update, view) = (get("init")?, get("update")?, get("view")?);
        let ty = crate::printer::ty;
        let model = ty(&init.ret);
        if !init.params.is_empty() {
            return Err(format!("`init` takes no parameters: {usage}"));
        }
        let [m, msg] = &update.params[..] else { return Err(format!("`update` takes the model and a message: {usage}")) };
        if ty(&m.ty) != model || ty(&update.ret) != model {
            return Err(format!("`update` takes and returns the model `{model}` that `init` returns: {usage}"));
        }
        let [vm] = &view.params[..] else { return Err(format!("`view` takes the model: {usage}")) };
        let html = format!("std.web.Html[{}]", ty(&msg.ty));
        if ty(&vm.ty) != model || ty(&view.ret) != html {
            return Err(format!("`view` takes the model `{model}` and returns `{html}`: {usage}"));
        }
        Ok(App { title: name.to_string(), msg: msg.ty.clone(), sessions: HashMap::default(), order: vec![] })
    }

    fn interp<'a>(env: &'a Env) -> Interp<'a> {
        let mut it = Interp::new(env, 1);
        it.output = interp::Output::Terminal;
        it
    }

    /// The model's view, as HTML.
    fn render(env: &Env, model: &Value) -> Result<String, String> {
        let mut it = App::interp(env);
        let html = it.call_fn("view", vec![model.clone()]).map_err(|f| fault_text(&f))?;
        match it.call_fn("std.web.render", vec![html]).map_err(|f| fault_text(&f))? {
            Value::Str(s) => Ok(s),
            v => Err(format!("`render` returned {}", interp::show(&v))),
        }
    }

    fn request(&mut self, env: &Env, req: &Json) -> Reply {
        let error = |status: u16, why: String| {
            eprintln!("error: {why}");
            text(status, why)
        };
        match (req["method"].as_str().unwrap_or(""), req["path"].as_str().unwrap_or("")) {
            ("GET" | "HEAD", "/") => {
                let model = match App::interp(env).call_fn("init", vec![]) {
                    Ok(m) => m,
                    Err(f) => return error(500, fault_text(&f)),
                };
                let body = match App::render(env, &model) {
                    Ok(b) => b,
                    Err(e) => return error(500, e),
                };
                let id = format!("{:032x}", rand::random::<u128>());
                self.keep(id.clone(), model);
                let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;");
                let page = format!(
                    "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{}</title><script src=\"/_strata/app.js\" defer></script></head><body><div id=\"strata-app\" data-session=\"{id}\">{body}</div></body></html>\n",
                    esc(&self.title)
                );
                (200, vec![("content-type".into(), "text/html; charset=utf-8".into())], page)
            }
            ("GET" | "HEAD", "/_strata/app.js") => (200, vec![("content-type".into(), "text/javascript; charset=utf-8".into())], APP_JS.to_string()),
            ("POST", "/_strata/msg") => {
                let Ok(sent) = serde_json::from_str::<Json>(req["body"].as_str().unwrap_or("")) else { return error(400, "the message is not JSON".into()) };
                let id = sent["session"].as_str().unwrap_or("").to_string();
                // the server restarted, or the session was dropped: the page reloads
                let Some(model) = self.sessions.get(&id).cloned() else { return text(410, "this session has ended: reload the page".into()) };
                let mut it = App::interp(env);
                let msg = match sent.get("handler") {
                    // a message the page holds, as JSON
                    None => match it.from_json(&self.msg, &sent["msg"], "msg") {
                        Ok(m) => m,
                        Err(e) => return error(400, format!("not a message of this app: {e}")),
                    },
                    // a handler of the page the user sees, and what they entered:
                    // `std.web.dispatch` makes the message
                    Some(h) => {
                        let Some(id) = h.as_i64() else { return error(400, "the handler is not a number".into()) };
                        let value = Value::Str(sent["value"].as_str().unwrap_or("").to_string());
                        let fields_ty = TypeExpr::Named("Dict".into(), vec![TypeExpr::Named("String".into(), vec![]), TypeExpr::Named("String".into(), vec![])]);
                        let fields = match it.from_json(&fields_ty, sent.get("fields").unwrap_or(&json!({})), "fields") {
                            Ok(f) => f,
                            Err(e) => return error(400, format!("the form's fields: {e}")),
                        };
                        let page = match it.call_fn("view", vec![model.clone()]) {
                            Ok(p) => p,
                            Err(f) => return error(500, fault_text(&f)),
                        };
                        match it.call_fn("std.web.dispatch", vec![page, Value::Int(id), value, fields]) {
                            Ok(Value::Variant(c, interp::Fields::Pos(xs))) if c == "Some" && xs.len() == 1 => xs[0].clone(),
                            Ok(_) => return error(400, format!("the page has no handler {id}")),
                            Err(f) => return error(500, fault_text(&f)),
                        }
                    }
                };
                let model = match it.call_fn("update", vec![model, msg]) {
                    Ok(m) => m,
                    Err(f) => return error(500, fault_text(&f)),
                };
                let body = match App::render(env, &model) {
                    Ok(b) => b,
                    Err(e) => return error(500, e),
                };
                self.sessions.insert(id, model);
                (200, vec![("content-type".into(), "text/html; charset=utf-8".into())], body)
            }
            _ => text(404, "not found".into()),
        }
    }

    fn keep(&mut self, id: String, model: Value) {
        if self.order.len() >= MAX_SESSIONS {
            let old = self.order.remove(0);
            self.sessions.remove(&old);
        }
        self.order.push(id.clone());
        self.sessions.insert(id, model);
    }
}

/// Run `handle` on a request (as JSON): the response's status, headers and
/// body. A fault, or a response the server can't send, is a 500 whose body
/// says why.
fn call(env: &Env, f: &FnDecl, req: &Json) -> (u16, Vec<(String, String)>, String) {
    let error = |why: String| {
        eprintln!("error: {why}");
        (500, vec![("content-type".to_string(), "text/plain; charset=utf-8".to_string())], why)
    };
    let mut it = Interp::new(env, 1);
    it.host = Some(interp::Host { args: vec![] });
    it.output = interp::Output::Terminal;
    let mut args = Vec::new();
    for p in &f.params {
        let ty = crate::printer::ty(&p.ty);
        match crate::cmd::capability(env, &ty) {
            Some(v) => args.push(v),
            None => match it.from_json(&p.ty, req, &p.name) {
                Ok(v) => args.push(v),
                Err(e) => return error(format!("the request doesn't make a `Request`: {e}")),
            },
        }
    }
    match it.call_fn("handle", args) {
        Ok(v) => response(&v).unwrap_or_else(error),
        Err(fault) => {
            let stack = if fault.stack.is_empty() { String::new() } else { format!("\n    stack: {}", crate::names::show_stack(&fault.stack)) };
            error(format!("fault ({:?}): {}{stack}", fault.kind, fault.msg))
        }
    }
}

/// The parts of a `Response` value.
fn response(v: &Value) -> Result<(u16, Vec<(String, String)>, String), String> {
    let j = interp::to_json(v);
    let status = j["status"].as_u64().and_then(|n| u16::try_from(n).ok()).ok_or("the response has no status")?;
    let body = j["body"].as_str().unwrap_or("").to_string();
    let mut headers = Vec::new();
    for (k, v) in j["headers"].as_object().cloned().unwrap_or_default() {
        let v = v.as_str().unwrap_or("").to_string();
        if k.is_empty() || !k.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)) {
            return Err(format!("the response's header name {k:?} is not a valid header name"));
        }
        if v.contains(['\r', '\n']) {
            return Err(format!("the response's header `{k}` holds a line break"));
        }
        // the server writes these itself
        if !matches!(k.to_lowercase().as_str(), "content-length" | "connection" | "transfer-encoding") {
            headers.push((k.to_lowercase(), v));
        }
    }
    Ok((status, headers, body))
}

/// A request read from the connection, as the JSON of a `std.http.Request`;
/// or the status and reason to refuse it with.
fn read_request(stream: &TcpStream) -> Result<Json, (u16, String)> {
    let bad = |why: &str| (400, why.to_string());
    let mut r = BufReader::new(stream);
    let mut line = String::new();
    // a connection closed before it sent anything (a browser's preconnect)
    if r.read_line(&mut line).map_err(|_| bad("unreadable request"))? == 0 {
        return Err((0, String::new()));
    }
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else { return Err(bad("malformed request line")) };
    if !matches!(method, "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS") {
        return Err((501, format!("method {method} is not supported")));
    }
    let mut headers = Map::new();
    loop {
        let mut h = String::new();
        r.read_line(&mut h).map_err(|_| bad("unreadable headers"))?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        let (k, v) = h.split_once(':').ok_or_else(|| bad("malformed header"))?;
        headers.insert(k.trim().to_lowercase(), json!(v.trim()));
    }
    let len: usize = match headers.get("content-length") {
        Some(n) => n.as_str().unwrap_or("").parse().map_err(|_| bad("bad content-length"))?,
        None => 0,
    };
    if len > MAX_BODY {
        return Err((413, "the body is too large".into()));
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body).map_err(|_| bad("the body is shorter than its content-length"))?;
    let body = String::from_utf8(body).map_err(|_| bad("the body is not UTF-8"))?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let query: Map<String, Json> = query.split('&').filter(|kv| !kv.is_empty()).map(|kv| kv.split_once('=').unwrap_or((kv, ""))).map(|(k, v)| (url_decode(k), json!(url_decode(v)))).collect();
    Ok(json!({ "method": method, "path": url_decode(path), "query": query, "headers": headers, "body": body, "params": {} }))
}

/// `%xx` escapes and `+` (a space) decoded.
pub fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => match std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                Some(c) => {
                    out.push(c);
                    i += 2;
                }
                None => out.push(b'%'),
            },
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        410 => "Gone",
        413 => "Content Too Large",
        422 => "Unprocessable Content",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        _ => "",
    }
}

/// The response; for a `HEAD` request (`head_only`), without the body.
fn write_response(mut s: &TcpStream, status: u16, headers: &[(String, String)], body: &str, head_only: bool) {
    let mut head = format!("HTTP/1.1 {status} {}\r\n", reason(status));
    if !headers.iter().any(|(k, _)| k == "content-type") && !body.is_empty() {
        head.push_str("content-type: text/plain; charset=utf-8\r\n");
    }
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str(&format!("content-length: {}\r\ncache-control: no-store\r\nconnection: close\r\n\r\n", body.len()));
    let _ = s.write_all(head.as_bytes());
    if !head_only {
        let _ = s.write_all(body.as_bytes());
    }
}
