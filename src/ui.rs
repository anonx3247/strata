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

fn unwrap_module(text: &str) -> String {
    match text.strip_prefix("module ").and_then(|t| t.split_once(" {\n")) {
        Some((_, rest)) if rest.ends_with('}') => rest[..rest.len() - 1]
            .lines()
            .map(|l| l.strip_prefix("  ").unwrap_or(l))
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end()
            .to_string(),
        _ => text.to_string(),
    }
}

/// Drop the leading `///` lines (the declaration's doc comment).
fn strip_doc(text: &str) -> String {
    text.lines().skip_while(|l| l.trim_start().starts_with("///")).collect::<Vec<_>>().join("\n")
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
        Target::File(f) => match crate::read_sources(&f.to_string_lossy(), None) {
            Ok(p) => (p, vec![], HashMap::new(), f.display().to_string()),
            Err(e) => return json!({"fatal": e}),
        },
    };
    let p = &program;
    // modules: work on qualified names, report under each definition's qname
    let resolved = crate::resolve::resolve(p);
    let rp = &resolved.program;
    let qname_of: HashMap<String, String> = rp.decls.iter().zip(&p.decls).map(|(r, d)| (r.name(), d.qname())).collect();
    let mut rep = types::check(p);
    let (env, _) = types::Env::new(p);
    rep.errors.extend(verify::static_refinements(p, &env));

    let graph = store::graph(p);
    let mut holes: HashMap<String, Vec<&types::HoleInfo>> = HashMap::new();
    for h in &rep.holes {
        holes.entry(qname_of.get(&h.decl).cloned().unwrap_or(h.decl.clone())).or_default().push(h);
    }
    let mut verdicts: HashMap<String, verify::Verdict> = HashMap::new();
    for d in &p.decls {
        if let Some(v) = cache.get(&store::closure_hash(p, &d.qname())) {
            verdicts.insert(d.qname(), v.clone());
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
            let name = d.qname();
            let one: BTreeSet<String> = [name.clone()].into();
            // a card already shows its module, so drop the `module m { ... }` wrapper;
            // the doc comment is shown as the description, so drop it from the code too
            let render = |lvl| strip_doc(&unwrap_module(view::render(p, &one, lvl, &status).trim_end()));
            let (iface, source) = (render(1), render(3));
            let effects = match &d.kind {
                _ => Vec::<String>::new(),
            };
            let label = match &d.kind {
                DeclKind::Test(t) => t.name.clone(),
                _ => d.name(),
            };
            json!({
                "name": name,
                "module": d.module,
                "public": d.public,
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
                "errors": rep.errors.iter().filter(|e| qname_of.get(&e.decl).unwrap_or(&e.decl) == &name || e.decl == name).map(|e| e.msg.clone()).collect::<Vec<_>>(),
                "last_change": last_change.get(&name),
                "has_body": iface != source,
                "text": { "1": iface, "2": render(2), "3": source },
            })
        })
        .collect();
    // ---- structure for the type map ----
    let declared: std::collections::HashSet<String> = rp
        .decls
        .iter()
        .filter(|d| matches!(d.kind, DeclKind::Type(_) | DeclKind::Trait(_)))
        .map(|d| d.name())
        .collect();
    let refs_of = |t: &TypeExpr| -> Vec<String> {
        let mut m = Vec::new();
        crate::flow::mentions(t, &mut m);
        m.into_iter().filter(|n| declared.contains(n)).collect()
    };
    let field_json = |name: Option<&str>, t: &TypeExpr| json!({"name": name, "ty": crate::printer::ty(t), "refs": refs_of(t)});
    let mut structure = Vec::new();
    for d in &rp.decls {
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
            _ => {}
        }
    }

    // ---- traits for the type map ----
    for d in &rp.decls {
        if let DeclKind::Trait(t) = &d.kind {
            let mut refs = Vec::new();
            for m in &t.methods {
                for p in &m.decl.params {
                    refs.extend(refs_of(&p.ty));
                }
                refs.extend(refs_of(&m.decl.ret));
            }
            refs.sort();
            refs.dedup();
            let implementors: Vec<String> = env.impls.iter().filter(|i| i.trait_name == t.name).map(|i| crate::printer::ty(&i.for_type)).collect();
            structure.push(json!({
                "name": t.name, "kind": "trait", "params": t.params, "doc": d.doc.join("\n"),
                "methods": t.methods.iter().map(|m| json!({"name": m.decl.name, "sig": crate::printer::fn_signature(&m.decl).lines().next().unwrap_or("").trim_start_matches("fn ").to_string(), "default": m.default})).collect::<Vec<_>>(),
                "laws": t.laws.iter().map(|l| json!({"name": l.name, "text": crate::printer::expr(&l.body, 0)})).collect::<Vec<_>>(),
                "supers": t.supers.iter().map(crate::types::texpr_head).collect::<Vec<_>>(),
                "refs": refs, "implementors": implementors,
            }));
        }
    }
    // which traits each type implements
    for s in structure.iter_mut() {
        let name = s["name"].as_str().unwrap_or("").to_string();
        let traits: Vec<String> = env.impls.iter().filter(|i| crate::types::texpr_head(&i.for_type) == name).map(|i| i.trait_name.clone()).collect();
        s["impls"] = json!(traits);
    }

    // ---- execution outlines for the flow map ----
    let fn_names: std::collections::HashSet<String> = rp.decls.iter().filter(|d| matches!(d.kind, DeclKind::Fn(_))).map(|d| d.name()).collect();
    let mut methods: HashMap<String, (String, Vec<crate::flow::Target>)> = HashMap::new();
    for (m, tr) in &env.method_trait {
        let targets = env
            .impls
            .iter()
            .filter(|i| &i.trait_name == tr)
            .map(|i| crate::flow::Target { ty: crate::printer::ty(&i.for_type), key: format!("{}::{m}", i.name()) })
            .collect();
        methods.insert(m.clone(), (tr.clone(), targets));
    }
    let outliner = crate::flow::Outliner { fns: &fn_names, methods: &methods };
    let mut flows = serde_json::Map::new();
    // implementations get outlines too, keyed `impl T for X::method`
    for i in &env.impls {
        let Some(tr) = env.traits.get(&i.trait_name) else { continue };
        for tm in &tr.methods {
            let decl = i.methods.iter().find(|m| m.name == tm.decl.name).unwrap_or(&tm.decl);
            if !i.methods.iter().any(|m| m.name == tm.decl.name) && !tm.default {
                continue;
            }
            flows.insert(
                format!("{}::{}", i.name(), tm.decl.name),
                json!({"steps": outliner.steps(&decl.body), "info": {"effects": [], "sig": crate::printer::fn_signature(decl).lines().next().unwrap_or(""), "hole": false}}),
            );
        }
    }
    let mut called: std::collections::HashSet<String> = std::collections::HashSet::new();
    for d in &rp.decls {
        let (body, sig) = match &d.kind {
            DeclKind::Fn(f) => (&f.body, json!({"effects": [], "sig": crate::printer::fn_signature(f).lines().next().unwrap_or("").to_string(), "hole": matches!(f.body, Expr::Hole(_))})),
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
        flows.insert(qname_of[&d.name()].clone(), json!({"steps": steps, "info": sig}));
    }
    // entry points: functions nothing else uses (calls, contracts or refinements; tests don't count)
    let used: std::collections::HashSet<String> = rp
        .decls
        .iter()
        .filter(|d| !matches!(d.kind, DeclKind::Test(_)))
        .flat_map(|d| graph.get(&d.name()).cloned().unwrap_or_default().into_iter().filter(move |n| *n != d.name()))
        .chain(called.iter().cloned())
        .collect();
    let entries: Vec<String> = rp
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
