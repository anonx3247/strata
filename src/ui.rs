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
                DeclKind::Fn(f) => f.effects.iter().map(crate::printer::ty).collect(),
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
    // ---- structure for the type map ----
    let declared: std::collections::HashSet<String> = p
        .decls
        .iter()
        .filter(|d| matches!(d.kind, DeclKind::Type(_) | DeclKind::Effect(_)))
        .map(|d| d.name())
        .collect();
    let refs_of = |t: &TypeExpr| -> Vec<String> {
        let mut m = Vec::new();
        crate::flow::mentions(t, &mut m);
        m.into_iter().filter(|n| declared.contains(n)).collect()
    };
    let field_json = |name: Option<&str>, t: &TypeExpr| json!({"name": name, "ty": crate::printer::ty(t), "refs": refs_of(t)});
    let mut structure = Vec::new();
    for d in &p.decls {
        // functions whose first parameter is this record/sum type act as its methods
        let has_methods = matches!(&d.kind, DeclKind::Type(t) if !matches!(t.body, TypeBody::Alias(_)));
        let methods: Vec<Json> = p
            .decls
            .iter()
            .filter(|_| has_methods)
            .filter_map(|m| match &m.kind {
                DeclKind::Fn(f) if f.params.first().and_then(|x| crate::flow::head(&x.ty)).as_deref() == Some(&d.name()) => {
                    Some(json!({"name": f.name, "sig": crate::printer::fn_signature(f).lines().next().unwrap_or("").trim_start_matches("fn ").to_string()}))
                }
                _ => None,
            })
            .collect();
        match &d.kind {
            DeclKind::Type(t) => {
                let (kind, base, fields, variants) = match &t.body {
                    TypeBody::Alias(a) => (if t.distinct { "distinct" } else { "alias" }, Some(json!({"ty": crate::printer::ty(a), "refs": refs_of(a)})), vec![], vec![]),
                    TypeBody::Record(fs) => ("record", None, fs.iter().map(|f| field_json(Some(&f.name), &f.ty)).collect(), vec![]),
                    TypeBody::Sum(vs) => (
                        "sum",
                        None,
                        vec![],
                        vs.iter()
                            .map(|v| {
                                let fs: Vec<Json> = match &v.payload {
                                    Payload::None => vec![],
                                    Payload::Positional(ts) => ts.iter().map(|t| field_json(None, t)).collect(),
                                    Payload::Record(fs) => fs.iter().map(|f| field_json(Some(&f.name), &f.ty)).collect(),
                                };
                                json!({"name": v.name, "fields": fs})
                            })
                            .collect(),
                    ),
                };
                structure.push(json!({
                    "name": t.name, "kind": kind, "params": t.params, "doc": d.doc.join("\n"),
                    "refinement": t.refinement.as_ref().map(|r| crate::printer::expr(r, 0)),
                    "base": base, "fields": fields, "variants": variants, "methods": methods,
                }));
            }
            DeclKind::Effect(e) => {
                let ops: Vec<Json> = e
                    .ops
                    .iter()
                    .map(|o| {
                        let mut r = Vec::new();
                        for p in &o.params {
                            r.extend(refs_of(&p.ty));
                        }
                        r.extend(refs_of(&o.ret));
                        r.dedup();
                        let params: Vec<String> = o.params.iter().map(|p| format!("{}: {}", p.name, crate::printer::ty(&p.ty))).collect();
                        json!({
                            "name": o.name,
                            "sig": format!("{}({}) -> {}", o.name, params.join(", "), crate::printer::ty(&o.ret)),
                            "refs": r,
                            "contract": o.requires.iter().map(|x| format!("requires {}", crate::printer::expr(x, 0)))
                                .chain(o.ensures.iter().map(|x| format!("ensures {}", crate::printer::expr(x, 0)))).collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                structure.push(json!({"name": e.name, "kind": "effect", "params": e.params, "doc": d.doc.join("\n"), "ops": ops, "methods": []}));
            }
            _ => {}
        }
    }

    // ---- execution outlines for the flow map ----
    let fn_names: std::collections::HashSet<String> = p.decls.iter().filter(|d| matches!(d.kind, DeclKind::Fn(_))).map(|d| d.name()).collect();
    let outliner = crate::flow::Outliner { fns: &fn_names };
    let mut flows = serde_json::Map::new();
    let mut called: std::collections::HashSet<String> = std::collections::HashSet::new();
    for d in &p.decls {
        let (body, sig) = match &d.kind {
            DeclKind::Fn(f) => (&f.body, json!({"effects": f.effects.iter().map(crate::printer::ty).collect::<Vec<_>>(), "sig": crate::printer::fn_signature(f).lines().next().unwrap_or("").to_string(), "hole": matches!(f.body, Expr::Hole(_))})),
            DeclKind::Test(t) => (&t.body, json!({"effects": [], "sig": format!("test {:?}", t.name), "hole": false})),
            _ => continue,
        };
        let steps = outliner.steps(body);
        if matches!(d.kind, DeclKind::Fn(_)) {
            let s = serde_json::to_string(&steps).unwrap_or_default();
            for n in &fn_names {
                if *n != d.name() && s.contains(&format!("\"f\":\"{n}\"")) {
                    called.insert(n.clone());
                }
            }
        }
        flows.insert(d.name(), json!({"steps": steps, "info": sig}));
    }
    // entry points: functions nothing else uses (calls, contracts or refinements; tests don't count)
    let used: std::collections::HashSet<String> = p
        .decls
        .iter()
        .filter(|d| !matches!(d.kind, DeclKind::Test(_)))
        .flat_map(|d| graph.get(&d.name()).cloned().unwrap_or_default().into_iter().filter(move |n| *n != d.name()))
        .chain(called.iter().cloned())
        .collect();
    let entries: Vec<String> = p
        .decls
        .iter()
        .filter(|d| matches!(d.kind, DeclKind::Fn(_)) && !used.contains(&d.name()))
        .map(|d| d.name())
        .collect();

    json!({
        "label": label,
        "decls": decls,
        "structure": structure,
        "flows": flows,
        "entries": entries,
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
