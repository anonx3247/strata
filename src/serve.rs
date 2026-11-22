//! `strata serve`: an HTTP server that calls the program's `handle` for each
//! request (`fn handle(req: Request) -> Response`, from `std.http`), or runs
//! it as an app (`init`, `update` and `view`, the Elm architecture): each
//! page load starts a session whose model the server keeps, the page's small
//! script sends back the number of the handler the user acted on (`on_click`,
//! a form, a timer), and the server answers with the view of the updated
//! model.
//!
//! The program is compiled once, with a few functions the server calls
//! (`strata_serve_*`, written here as Strata and checked with the program),
//! and runs in memory (the JIT) for as long as the server does. A session's
//! model stays in compiled code's memory between requests. Requests are read
//! here and handed over as `std.http.Request` values (`native::write`), one
//! at a time; a fault is a 500 and leaves the session as it was.

use crate::ast::{Decl, DeclKind, Expr, FnDecl, Param, Program, Symbol, TypeExpr};
use crate::llvm::{jit, native, Arg, CallEntry, Shape, Shapes};
use crate::types::Env;
use serde_json::Value as Json;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use crate::fx::HashMap;
use std::time::Instant;

const REQUEST: &str = "std.http.Request";
const RESPONSE: &str = "std.http.Response";
/// The largest body read; past it the request is refused.
const MAX_BODY: usize = 16 << 20;

/// A request as read from the connection.
struct Request {
    method: String,
    path: String,
    query: BTreeMap<String, String>,
    headers: BTreeMap<String, String>,
    body: String,
}

type Reply = (u16, Vec<(String, String)>, String);

fn text(status: u16, body: String) -> Reply {
    (status, vec![("content-type".into(), "text/plain; charset=utf-8".into())], body)
}

/// A 4xx or 5xx, also said on the server's terminal.
fn error(status: u16, why: String) -> Reply {
    eprintln!("error: {why}");
    text(status, why)
}

fn fault_text(f: &jit::Fault) -> String {
    let stack: Vec<&str> = f.stack.split(" -> ").filter(|s| !s.is_empty() && !s.starts_with("strata_serve_")).collect();
    let stack = if stack.is_empty() { String::new() } else { format!("\n    stack: {}", stack.join(" -> ")) };
    format!("fault ({}): {}{stack}", f.kind, f.message)
}

/// Check that `handle` has a signature the server can call: the request's
/// position among its parameters, and the capabilities' types.
fn handler(env: &Env) -> Result<(FnDecl, usize), String> {
    let f = env
        .fns
        .get(&Symbol::new("handle"))
        .ok_or("no `handle` function to serve: write `fn handle(req: Request) -> Response` (with `use std.http.{Request, Response}`)")?
        .clone();
    let usage = "`handle` takes a `Request` (std.http) and may ask for `Console` and capabilities (`Disk`, `Process`, or the program's own)";
    let mut request = None;
    for (i, p) in f.params.iter().enumerate() {
        let ty = crate::printer::ty(&p.ty);
        if ty == REQUEST && request.is_none() {
            request = Some(i);
        } else if crate::cmd::capability(env, &ty).is_none() {
            return Err(format!("{usage}. Its parameter `{}` has type `{ty}`", p.name));
        }
    }
    let Some(request) = request else { return Err(format!("{usage}: exactly one `Request`")) };
    let ret = crate::printer::ty(&f.ret);
    if ret != RESPONSE {
        return Err(format!("`handle` must return a `Response` (std.http), not `{ret}`"));
    }
    Ok((f, request))
}

/// The functions of an app: `fn init() -> Model`, `fn update(model: Model,
/// msg: Msg) -> Model` and `fn view(model: Model) -> Html[Msg]`; `init` and
/// `update` may also ask for capabilities, after their other parameters.
struct AppFns {
    init: FnDecl,
    update: FnDecl,
    model: TypeExpr,
    msg: TypeExpr,
}

fn app_fns(env: &Env) -> Result<AppFns, String> {
    let usage = "an app is `fn init() -> Model`, `fn update(model: Model, msg: Msg) -> Model` and `fn view(model: Model) -> Html[Msg]` (std.web), for your own `Model` and `Msg` types; `init` and `update` may also ask for `Console` and capabilities (`Disk`, `Process`, `Codebase`, or the program's own), after their other parameters";
    let get = |n: &str| env.fns.get(&Symbol::new(n)).cloned().ok_or(format!("no `{n}` function: {usage}"));
    let (init, update, view) = (get("init")?, get("update")?, get("view")?);
    let ty = crate::printer::ty;
    let model = ty(&init.ret);
    for (f, skip) in [(&init, 0), (&update, 2)] {
        for p in &f.params[skip.min(f.params.len())..] {
            let t = ty(&p.ty);
            if crate::cmd::capability(env, &t).is_none() {
                return Err(format!("`{}`'s parameter `{}` is a `{t}`, not a capability: {usage}", f.name, p.name));
            }
        }
    }
    let [m, msg, ..] = &update.params[..] else { return Err(format!("`update` takes the model and a message: {usage}")) };
    if ty(&m.ty) != model || ty(&update.ret) != model {
        return Err(format!("`update` takes and returns the model `{model}` that `init` returns: {usage}"));
    }
    let [vm] = &view.params[..] else { return Err(format!("`view` takes the model: {usage}")) };
    let html = format!("std.web.Html[{}]", ty(&msg.ty));
    if ty(&vm.ty) != model || ty(&view.ret) != html {
        return Err(format!("`view` takes the model `{model}` and returns `{html}`: {usage}"));
    }
    let msg = msg.ty.clone();
    Ok(AppFns { model: init.ret.clone(), init, update, msg })
}

/// A function of the server's, in the program's root module.
fn wrapper(name: &str, params: Vec<(&str, TypeExpr)>, ret: TypeExpr, body: &str) -> Result<Decl, String> {
    let body = crate::parser::parse_expr(body).map_err(|e| format!("strata serve's `{name}`: {e}"))?;
    let params = params.into_iter().map(|(n, ty)| Param { name: Symbol::new(n), ty, mutable: false, comptime: false }).collect();
    let f = FnDecl { doc: vec![], comments: vec![], name: Symbol::new(name), type_params: vec![], bounds: vec![], params, ret, requires: vec![], ensures: vec![], terminates: false, body, comptime: false, derived: false };
    Ok(Decl { comments: vec![], doc: vec![], kind: DeclKind::Fn(f), module: Symbol::new(""), public: false })
}

fn named(n: &str, args: Vec<TypeExpr>) -> TypeExpr {
    TypeExpr::Named(n.into(), args)
}

/// The capabilities `f` asks for from parameter `from` on: names, types and
/// how they are passed (the runtime's, lent).
fn caps(f: &FnDecl, from: usize) -> Vec<(String, TypeExpr)> {
    f.params[from.min(f.params.len())..].iter().enumerate().map(|(i, p)| (format!("cap_{i}"), p.ty.clone())).collect()
}

/// What `strata serve` runs, compiled: the calls, by name, and their shapes.
struct Server {
    calls: Vec<CallEntry>,
    shapes: Vec<Vec<Shape>>,
    named: Shapes,
}

impl Server {
    fn index(&self, name: &str) -> usize {
        self.calls.iter().position(|c| c.name == name).expect("a call strata serve made")
    }
}

/// The program, with the server's functions, compiled.
fn compile(p: &Program, extra: Vec<Decl>, calls: Vec<CallEntry>) -> Result<(String, Server), String> {
    let mut with = p.clone();
    with.decls.extend(extra);
    let checked = crate::full_checked(&with);
    if let Some(e) = checked.report.errors.iter().find(|e| e.decl.starts_with("strata_serve_")) {
        return Err(format!("the program can't be served: {}: {}", e.decl, e.msg));
    }
    if !checked.report.ok() {
        let e = &checked.report.errors[0];
        return Err(format!("the program can't be served: {}: {}", e.decl, e.msg));
    }
    let (ir, shapes, named) = crate::llvm::compile_calls(&checked.compiled, &calls).map_err(|e| format!("the program can't be compiled yet: {e}"))?;
    Ok((ir, Server { calls, shapes, named }))
}

pub fn serve(p: &Program, env: &Env, port: u16, name: &str) -> Result<(), String> {
    let handle = env.fns.contains_key(&Symbol::new("handle")) || !env.fns.contains_key(&Symbol::new("view"));
    let (ir, server, mut mode) = if handle { handle_server(p, env)? } else { app_server(p, env, name)? };
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("cannot listen on port {port}: {e}"))?;
    jit::with_calls(&ir, server.calls.len(), |calls| {
        println!("Serving on http://127.0.0.1:{port}  (Ctrl-C to stop)");
        let run = Run { calls, server: &server };
        for stream in listener.incoming().flatten() {
            let start = Instant::now();
            let (line, status) = match read_request(&stream) {
                Ok(req) => {
                    let line = format!("{} {}", req.method, req.path);
                    let head = req.method == "HEAD";
                    let (status, headers, body) = match &mut mode {
                        Mode::Handle { request, caps } => run.handle(req, *request, *caps),
                        Mode::App(app) => app.request(&run, &req),
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
    })
}

/// What `strata serve` runs: a request handler, or an app.
enum Mode {
    /// `handle`, with the position of its `Request` among its parameters and
    /// how many there are.
    Handle { request: usize, caps: usize },
    App(App),
}

/// `handle`, called as it is, its result dropped by `strata_serve_drop_response`.
fn handle_server(p: &Program, env: &Env) -> Result<(String, Server, Mode), String> {
    let (f, request) = handler(env)?;
    let args = (0..f.params.len()).map(|i| if i == request { Arg::Given } else { Arg::Lent }).collect();
    let calls = vec![CallEntry { name: "handle".into(), args }, CallEntry { name: "strata_serve_drop_response".into(), args: vec![Arg::Moved] }];
    let extra = vec![wrapper("strata_serve_drop_response", vec![("r", named(RESPONSE, vec![]))], TypeExpr::Unit, "()")?];
    let (ir, server) = compile(p, extra, calls)?;
    Ok((ir, server, Mode::Handle { request, caps: f.params.len() }))
}

/// The calls of an app: `init`; the page of a model; the message of a page's
/// handler; `update`; and dropping what the host holds.
fn app_server(p: &Program, env: &Env, title: &str) -> Result<(String, Server, Mode), String> {
    let a = app_fns(env)?;
    let (m, msg) = (a.model.clone(), a.msg.clone());
    let opt_msg = named("Option", vec![msg.clone()]);
    let string = || named("String", vec![]);
    let init_caps = caps(&a.init, 0);
    let update_caps = caps(&a.update, 2);
    let names = |cs: &[(String, TypeExpr)]| cs.iter().map(|(n, _)| format!(", {n}")).collect::<String>();
    let mut update_params = vec![("model", m.clone()), ("msg", opt_msg.clone())];
    update_params.extend(update_caps.iter().map(|(n, t)| (n.as_str(), t.clone())));
    let extra = vec![
        wrapper("strata_serve_page", vec![("model", m.clone())], string(), "std.web.render(view(model))")?,
        wrapper(
            "strata_serve_message",
            vec![("model", m.clone()), ("handler", named("Int", vec![])), ("value", string()), ("fields", named("Dict", vec![string(), string()]))],
            opt_msg.clone(),
            "std.web.dispatch(view(model), handler, value, fields)",
        )?,
        wrapper(
            "strata_serve_update",
            update_params,
            m.clone(),
            &format!("match msg {{ Some(m) => update(model, m{}), None => model }}", names(&update_caps)),
        )?,
        wrapper("strata_serve_drop_model", vec![("model", m.clone())], TypeExpr::Unit, "()")?,
        wrapper("strata_serve_drop_text", vec![("text", string())], TypeExpr::Unit, "()")?,
        wrapper("strata_serve_drop_message", vec![("msg", opt_msg)], TypeExpr::Unit, "()")?,
    ];
    let lent = |n: usize| vec![Arg::Lent; n];
    let calls = vec![
        CallEntry { name: "init".into(), args: lent(init_caps.len()) },
        CallEntry { name: "strata_serve_page".into(), args: lent(1) },
        CallEntry { name: "strata_serve_message".into(), args: vec![Arg::Lent, Arg::Given, Arg::Given, Arg::Given] },
        CallEntry { name: "strata_serve_update".into(), args: lent(2 + update_caps.len()) },
        CallEntry { name: "strata_serve_drop_model".into(), args: vec![Arg::Moved] },
        CallEntry { name: "strata_serve_drop_text".into(), args: vec![Arg::Moved] },
        CallEntry { name: "strata_serve_drop_message".into(), args: vec![Arg::Moved] },
    ];
    let (ir, server) = compile(p, extra, calls)?;
    let app = App { title: title.to_string(), init_caps: init_caps.len(), update_caps: update_caps.len(), sessions: HashMap::default(), order: vec![] };
    Ok((ir, server, Mode::App(app)))
}

/// The compiled server, running.
struct Run<'a, 'ctx> {
    calls: &'a jit::Calls<'ctx>,
    server: &'a Server,
}

impl Run<'_, '_> {
    /// Call `name` with these slots: its result's slot, or the fault's text.
    /// What it printed goes to the terminal.
    fn call(&self, name: &str, args: &[i64]) -> Result<i64, String> {
        let (r, printed) = self.calls.call(self.server.index(name), args);
        print!("{printed}");
        let _ = std::io::stdout().flush();
        r.map_err(|f| fault_text(&f))
    }

    /// Drop a value the host holds, by the call that takes it.
    fn drop(&self, name: &str, slot: i64) {
        if let Err(e) = self.call(name, &[slot]) {
            eprintln!("error: dropping a value: {e}");
        }
    }

    /// The string a call returned, read and dropped.
    fn take_text(&self, slot: i64) -> Result<String, String> {
        let s = unsafe { native::read(&Shape::Str, &self.server.named, slot) };
        self.drop("strata_serve_drop_text", slot);
        match s? {
            Expr::Str(s) => Ok(s),
            other => Err(format!("expected a string, got {}", crate::printer::literal(&other))),
        }
    }

    /// The shape of argument `k` of call `name` (0: its result).
    fn shape(&self, name: &str, k: usize) -> &Shape {
        &self.server.shapes[self.server.index(name)][k]
    }

    /// Run `handle` on a request: the response's status, headers and body.
    /// A fault, or a response the server can't send, is a 500 whose body
    /// says why.
    fn handle(&self, req: Request, request: usize, n: usize) -> Reply {
        let dict = |kvs: BTreeMap<String, String>| Expr::Dict(vec![], kvs.into_iter().map(|(k, v)| (Expr::Str(k), Expr::Str(v))).collect());
        let fields = vec![
            ("method", Expr::Str(req.method)),
            ("path", Expr::Str(req.path)),
            ("query", dict(req.query)),
            ("headers", dict(req.headers)),
            ("body", Expr::Str(req.body)),
            ("params", dict(BTreeMap::new())),
        ];
        let value = Expr::Ctor(Symbol::new(REQUEST), crate::ast::CtorArgs::Record(fields.into_iter().map(|(n, e)| (Symbol::new(n), e)).collect()));
        let slot = match native::write(self.shape("handle", request + 1), &self.server.named, &value) {
            Ok(s) => s,
            Err(e) => return error(500, format!("the request doesn't make a `Request`: {e}")),
        };
        // the capabilities are the runtime's: no value
        let mut args = vec![0; n];
        args[request] = slot;
        let r = match self.call("handle", &args) {
            Ok(r) => r,
            Err(e) => return error(500, e),
        };
        let v = unsafe { native::read(self.shape("handle", 0), &self.server.named, r) };
        self.drop("strata_serve_drop_response", r);
        match v.and_then(|v| response(&v)) {
            Ok(reply) => reply,
            Err(e) => error(500, e),
        }
    }
}

/// The parts of a `Response` value.
fn response(v: &Expr) -> Result<Reply, String> {
    let crate::ast::Expr::Ctor(_, crate::ast::CtorArgs::Record(fs)) = v else { return Err("the response is not a `Response`".into()) };
    let field = |n: &str| fs.iter().find(|(f, _)| f.as_str() == n).map(|(_, e)| e);
    let status = match field("status") {
        Some(Expr::Int(n)) => u16::try_from(*n).map_err(|_| format!("the response's status {n} is not one"))?,
        _ => return Err("the response has no status".into()),
    };
    let body = match field("body") {
        Some(Expr::Str(s)) => s.clone(),
        _ => String::new(),
    };
    let mut headers = Vec::new();
    if let Some(Expr::Dict(_, kvs)) = field("headers") {
        for (k, v) in kvs {
            let (Expr::Str(k), Expr::Str(v)) = (k, v) else { continue };
            if k.is_empty() || !k.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)) {
                return Err(format!("the response's header name {k:?} is not a valid header name"));
            }
            if v.contains(['\r', '\n']) {
                return Err(format!("the response's header `{k}` holds a line break"));
            }
            // the server writes these itself
            if !matches!(k.to_lowercase().as_str(), "content-length" | "connection" | "transfer-encoding") {
                headers.push((k.to_lowercase(), v.clone()));
            }
        }
    }
    Ok((status, headers, body))
}

// ---- apps ----

/// The script every app page loads: it sends the handlers the user acts on,
/// one at a time, and puts the new view in place.
const APP_JS: &str = include_str!("serve/app.js");

/// The most sessions kept; past it, the oldest is dropped (its page reloads).
const MAX_SESSIONS: usize = 1000;

/// An app's sessions: each one's model, held in compiled code's memory, and
/// the page it was last sent.
struct App {
    /// The page's title: the program's name.
    title: String,
    /// How many capabilities `init` and `update` ask for.
    init_caps: usize,
    update_caps: usize,
    sessions: HashMap<String, (i64, String)>,
    /// Session ids, oldest first.
    order: Vec<String>,
}

impl App {
    /// The model's view, as HTML.
    fn render(run: &Run, model: i64) -> Result<String, String> {
        let page = run.call("strata_serve_page", &[model])?;
        run.take_text(page)
    }

    fn request(&mut self, run: &Run, req: &Request) -> Reply {
        match (req.method.as_str(), req.path.as_str()) {
            ("GET" | "HEAD", "/") => {
                let model = match run.call("init", &vec![0; self.init_caps]) {
                    Ok(m) => m,
                    Err(e) => return error(500, e),
                };
                let body = match App::render(run, model) {
                    Ok(b) => b,
                    Err(e) => {
                        run.drop("strata_serve_drop_model", model);
                        return error(500, e);
                    }
                };
                let id = format!("{:032x}", rand::random::<u128>());
                self.keep(run, id.clone(), model, body.clone());
                let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;");
                let page = format!(
                    "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{}</title><script src=\"/_strata/app.js\" defer></script></head><body><div id=\"strata-app\" data-session=\"{id}\">{body}</div></body></html>\n",
                    esc(&self.title)
                );
                (200, vec![("content-type".into(), "text/html; charset=utf-8".into())], page)
            }
            ("GET" | "HEAD", "/_strata/app.js") => (200, vec![("content-type".into(), "text/javascript; charset=utf-8".into())], APP_JS.to_string()),
            ("POST", "/_strata/msg") => self.message(run, &req.body),
            _ => text(404, "not found".into()),
        }
    }

    /// A handler of the page the user sees, and what they entered:
    /// `std.web.dispatch` makes the message from them, `update` the new model.
    fn message(&mut self, run: &Run, body: &str) -> Reply {
        let Ok(sent) = serde_json::from_str::<Json>(body) else { return error(400, "the message is not JSON".into()) };
        let id = sent["session"].as_str().unwrap_or("").to_string();
        // the server restarted, or the session was dropped: the page reloads
        let Some((model, shown)) = self.sessions.get(&id) else { return text(410, "this session has ended: reload the page".into()) };
        let model = *model;
        let Some(handler) = sent["handler"].as_i64() else { return error(400, "the handler is not a number".into()) };
        let value = Expr::Str(sent["value"].as_str().unwrap_or("").to_string());
        let mut fields = vec![];
        if let Some(o) = sent.get("fields") {
            let Some(o) = o.as_object() else { return error(400, "the form's fields are not an object".into()) };
            for (k, v) in o {
                let Some(v) = v.as_str() else { return error(400, format!("the form's field `{k}` is not a string")) };
                fields.push((Expr::Str(k.clone()), Expr::Str(v.to_string())));
            }
        }
        let fields = Expr::Dict(vec![], fields);
        let named = &run.server.named;
        let given = (|| -> Result<Vec<i64>, String> {
            let name = "strata_serve_message";
            Ok(vec![
                model,
                native::write(run.shape(name, 2), named, &Expr::Int(handler))?,
                native::write(run.shape(name, 3), named, &value)?,
                native::write(run.shape(name, 4), named, &fields)?,
            ])
        })();
        let given = match given {
            Ok(g) => g,
            Err(e) => return error(400, format!("the message: {e}")),
        };
        let msg = match run.call("strata_serve_message", &given) {
            Ok(m) => m,
            Err(e) => return error(500, e),
        };
        if is_none(run.shape("strata_serve_message", 0), named, msg) {
            run.drop("strata_serve_drop_message", msg);
            return error(400, format!("the page has no handler {handler}"));
        }
        let mut args = vec![model, msg];
        args.extend(vec![0; self.update_caps]);
        let new = run.call("strata_serve_update", &args);
        run.drop("strata_serve_drop_message", msg);
        // a fault: the model stays as it was
        let new = match new {
            Ok(n) => n,
            Err(e) => return error(500, e),
        };
        let body = match App::render(run, new) {
            Ok(b) => b,
            Err(e) => {
                run.drop("strata_serve_drop_model", new);
                return error(500, e);
            }
        };
        // the page the user sees changes to the new model's either way
        let same = body == *shown;
        run.drop("strata_serve_drop_model", model);
        self.sessions.insert(id, (new, body.clone()));
        // nothing to show (a timer's check found nothing new): the page stays
        if same {
            return (204, vec![], String::new());
        }
        (200, vec![("content-type".into(), "text/html; charset=utf-8".into())], body)
    }

    fn keep(&mut self, run: &Run, id: String, model: i64, shown: String) {
        if self.order.len() >= MAX_SESSIONS {
            let old = self.order.remove(0);
            if let Some((m, _)) = self.sessions.remove(&old) {
                run.drop("strata_serve_drop_model", m);
            }
        }
        self.order.push(id.clone());
        self.sessions.insert(id, (model, shown));
    }
}

/// Whether the `Option` in `slot` is `None`: its block's tag.
fn is_none(s: &Shape, shapes: &Shapes, slot: i64) -> bool {
    let Shape::Named(k) = s else { return false };
    let none = shapes.named.get(k).and_then(|cs| cs.iter().find(|c| c.name == "None")).map(|c| c.tag);
    none.is_some_and(|t| unsafe { *(slot as *const i64) } == t)
}

// ---- HTTP ----

/// A request read from the connection, or the status and reason to refuse
/// it with (0: the connection closed before it sent anything).
fn read_request(stream: &TcpStream) -> Result<Request, (u16, String)> {
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
    let mut headers = BTreeMap::new();
    loop {
        let mut h = String::new();
        r.read_line(&mut h).map_err(|_| bad("unreadable headers"))?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        let (k, v) = h.split_once(':').ok_or_else(|| bad("malformed header"))?;
        headers.insert(k.trim().to_lowercase(), v.trim().to_string());
    }
    let len: usize = match headers.get("content-length") {
        Some(n) => n.parse().map_err(|_| bad("bad content-length"))?,
        None => 0,
    };
    if len > MAX_BODY {
        return Err((413, "the body is too large".into()));
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body).map_err(|_| bad("the body is shorter than its content-length"))?;
    let body = String::from_utf8(body).map_err(|_| bad("the body is not UTF-8"))?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let query = query.split('&').filter(|kv| !kv.is_empty()).map(|kv| kv.split_once('=').unwrap_or((kv, ""))).map(|(k, v)| (url_decode(k), url_decode(v))).collect();
    Ok(Request { method: method.to_string(), path: url_decode(path), query, headers, body })
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
