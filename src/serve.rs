//! `strata serve`: an HTTP server that calls the program's `handle` for each
//! request (`fn handle(req: Request) -> Response`, from `std.http`).
//!
//! Requests are read here, turned into a `std.http.Request` through the same
//! JSON reading as `strata run`'s arguments, and the `Response` value is
//! written back. One request at a time, each with a fresh interpreter, so
//! nothing a request does is seen by the next.

use crate::ast::FnDecl;
use crate::interp::{self, Interp, Value};
use crate::types::Env;
use serde_json::{json, Map, Value as Json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Instant;

const REQUEST: &str = "std.http.Request";
const RESPONSE: &str = "std.http.Response";
/// The largest body read; past it the request is refused.
const MAX_BODY: usize = 16 << 20;

/// Check that `handle` exists and has a signature the server can call.
fn handler(env: &Env) -> Result<FnDecl, String> {
    let f = env
        .fns
        .get("handle")
        .ok_or("no `handle` function to serve: write `fn handle(req: Request) -> Response` (with `use std.http.{Request, Response}`)")?
        .clone();
    let usage = "`handle` takes a `Request` (std.http) and may ask for `Console`, `Files` and `Environment`";
    let mut requests = 0;
    for p in &f.params {
        let ty = crate::printer::ty(&p.ty);
        if ty == REQUEST {
            requests += 1;
        } else if crate::cmd::capability(&ty).is_none() {
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

pub fn serve(env: &Env, port: u16) -> Result<(), String> {
    let f = handler(env)?;
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("cannot listen on port {port}: {e}"))?;
    println!("Serving on http://127.0.0.1:{port}  (Ctrl-C to stop)");
    for stream in listener.incoming().flatten() {
        let start = Instant::now();
        let (line, status) = match read_request(&stream) {
            Ok(req) => {
                let line = format!("{} {}", req["method"].as_str().unwrap_or("?"), req["path"].as_str().unwrap_or("?"));
                let head = req["method"] == "HEAD";
                let (status, headers, body) = call(env, &f, &req);
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
        match crate::cmd::capability(&ty) {
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
            let stack = if fault.stack.is_empty() { String::new() } else { format!("\n    stack: {}", fault.stack.join(" -> ")) };
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
