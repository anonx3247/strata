//! `strata ui`: a local, read-only viewer for a codebase.
//!
//! A tiny HTTP server (std only) serves one HTML page and a JSON snapshot of
//! the codebase. The snapshot is rebuilt on every request, so edits made by
//! agents through the CLI appear in the browser within a second or two.

use crate::ast::*;
use crate::store::{self, Codebase};
use crate::{types, verify, view};
use serde_json::{json, Value as Json};
use std::collections::{BTreeSet, HashMap};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;

const PAGE: &str = include_str!("ui/index.html");

pub enum Target {
    Db(PathBuf),
    File(PathBuf),
}

fn snapshot(target: &Target) -> Json {
    let (program, log, cache, label) = match target {
        Target::Db(root) => match Codebase::open(root) {
            Ok(cb) => {
                let log = cb.log();
                let cache = cb.load_verdicts();
                (cb.program, log, cache, root.display().to_string())
            }
            Err(e) => return json!({"fatal": e}),
        },
        Target::File(f) => {
            let src = std::fs::read_to_string(f).unwrap_or_default();
            match crate::parser::parse_program(&src) {
                Ok(p) => (p, vec![], HashMap::new(), f.display().to_string()),
                Err(e) => return json!({"fatal": format!("{}:{e}", f.display())}),
            }
        }
    };
    let p = &program;
    let mut rep = types::check(p);
    let (env, _) = types::Env::new(p);
    rep.errors.extend(verify::static_refinements(p, &env));

    let graph = store::graph(p);
    let mut holes: HashMap<String, Vec<&types::HoleInfo>> = HashMap::new();
    for h in &rep.holes {
        holes.entry(h.decl.clone()).or_default().push(h);
    }
    let mut verdicts: HashMap<String, verify::Verdict> = HashMap::new();
    for d in &p.decls {
        if let Some(v) = cache.get(&store::closure_hash(p, &d.name())) {
            verdicts.insert(d.name(), v.clone());
        }
    }
    let status = view::Status {
        holes: holes.iter().map(|(k, v)| (k.clone(), v.iter().map(|h| h.name.clone()).collect())).collect(),
        verdicts: HashMap::new(),
    };
    // For each definition: index of the last log entry that touched it.
    let mut last_change: HashMap<String, usize> = HashMap::new();
    for (i, e) in log.iter().enumerate() {
        for c in e["changes"].as_array().into_iter().flatten() {
            if let Some(n) = c["name"].as_str() {
                last_change.insert(n.to_string(), i);
            }
        }
    }

    let decls: Vec<Json> = p
        .decls
        .iter()
        .map(|d| {
            let name = d.name();
            let one: BTreeSet<String> = [name.clone()].into();
            let render = |lvl| view::render(p, &one, lvl, &status).trim_end().to_string();
            let effects = match &d.kind {
                DeclKind::Fn(f) => f.effects.clone(),
                _ => vec![],
            };
            let label = match &d.kind {
                DeclKind::Test(t) => t.name.clone(),
                _ => name.clone(),
            };
            json!({
                "name": name,
                "label": label,
                "kind": d.kind_str(),
                "doc": d.doc.join("\n"),
                "hash": store::self_hash(d),
                "interface_hash": store::interface_hash(d),
                "effects": effects,
                "deps": graph.get(&name).cloned().unwrap_or_default(),
                "used_by": graph.iter().filter(|(_, ds)| ds.contains(&name)).map(|(n, _)| n.clone()).collect::<Vec<_>>(),
                "holes": holes.get(&name).cloned().unwrap_or_default(),
                "verdict": verdicts.get(&name),
                "errors": rep.errors.iter().filter(|e| e.decl == name).map(|e| e.msg.clone()).collect::<Vec<_>>(),
                "last_change": last_change.get(&name),
                "text": { "1": render(1), "2": render(2), "3": render(3) },
            })
        })
        .collect();
    json!({
        "label": label,
        "decls": decls,
        "errors": rep.errors,
        "log": log,
    })
}

fn respond(mut s: TcpStream, status: &str, ctype: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(body);
}

pub fn serve(target: Target, port: u16) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("cannot listen on port {port}: {e}"))?;
    println!("Strata viewer on http://127.0.0.1:{port}  (Ctrl-C to stop)");
    for stream in listener.incoming().flatten() {
        let mut line = String::new();
        if BufReader::new(&stream).read_line(&mut line).is_err() {
            continue;
        }
        let path = line.split_whitespace().nth(1).unwrap_or("/").split('?').next().unwrap_or("/");
        match path {
            "/" | "/index.html" => respond(stream, "200 OK", "text/html; charset=utf-8", PAGE.as_bytes()),
            "/api/state" => {
                let body = snapshot(&target).to_string();
                respond(stream, "200 OK", "application/json", body.as_bytes())
            }
            _ => respond(stream, "404 Not Found", "text/plain", b"not found"),
        }
    }
    Ok(())
}
