//! `strata ui`: a local, read-only viewer for a codebase.
//!
//! A tiny HTTP server (std only) serves one HTML page and a JSON snapshot of
//! the codebase. The page polls it; the snapshot is rebuilt when a file under
//! the target changed (and only then), so edits made by agents through the
//! CLI appear in the browser within a second or two.

use crate::ast::*;
use crate::store::{self, Codebase};
use crate::{types, verify, view};
use serde_json::{json, Value as Json};
use crate::fx::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;

const PAGE: &str = include_str!("ui/index.html");

pub enum Target {
    Db(PathBuf),
    File(PathBuf),
}

/// Drop the declaration's doc comment (the `///` lines above it, which may
/// follow `//` comments); the viewer shows it as the description instead.
fn strip_doc(text: &str) -> String {
    let mut head = true;
    let mut out = Vec::new();
    for l in text.lines() {
        let t = l.trim_start();
        head = head && (t.is_empty() || t.starts_with("//"));
        if !(head && t.starts_with("///")) {
            out.push(l);
        }
    }
    out.join("\n")
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
        Target::File(f) => match crate::read_sources(&f.to_string_lossy()) {
            Ok(p) => (p, vec![], HashMap::default(), f.display().to_string()),
            Err(e) => return json!({"fatal": e}),
        },
    };
    let p = &program;
    // modules: work on qualified names, report under each definition's qname
    // resolve once (it expands derives, which runs code): everything below works from it
    let resolved = crate::resolve::resolve(p);
    let graph = store::graph_of(p, &resolved.program);
    let rp = &resolved.program.clone();
    let qname_of: HashMap<String, String> = rp.decls.iter().zip(&p.decls).map(|(r, d)| (r.name(), d.qname())).collect();
    let types::Checked { report: mut rep, program: elaborated, env, compiled } = types::check_resolved(resolved);
    rep.errors.extend(verify::refinements_in(&elaborated, &compiled, &env));

    let mut holes: HashMap<String, Vec<&types::HoleInfo>> = HashMap::default();
    for h in &rep.holes {
        holes.entry(qname_of.get(&h.decl).cloned().unwrap_or(h.decl.clone())).or_default().push(h);
    }
    let mut verdicts: HashMap<String, verify::Verdict> = HashMap::default();
    let code_hashes: HashMap<String, String> = p.decls.iter().map(|d| (d.qname(), store::code_hash(d))).collect();
    for d in p.decls.iter().filter(|_| !cache.is_empty()) {
        if let Some(v) = cache.get(&store::closure_hash_in(&graph, &code_hashes, &d.qname())) {
            verdicts.insert(d.qname(), v.clone());
        }
    }
    let status = view::Status {
        holes: holes.iter().map(|(k, v)| (k.clone(), v.iter().map(|h| h.name.clone()).collect())).collect(),
        verdicts: HashMap::default(),
    };
    // For each definition: index of the last log entry that touched it.
    let mut last_change: HashMap<String, usize> = HashMap::default();
    for (i, e) in log.iter().enumerate() {
        for c in e["changes"].as_array().into_iter().flatten() {
            if let Some(n) = c["name"].as_str() {
                last_change.insert(n.to_string(), i);
            }
        }
    }

    // An impl block is not a dependency of its trait or its type, nor a reference
    // to them: it is how that type implements that trait. Keep that relation apart.
    let impl_of: HashMap<String, (String, String)> = rp
        .decls
        .iter()
        .zip(&p.decls)
        .filter_map(|(r, d)| match &r.kind {
            DeclKind::Impl(i) => Some((d.qname(), (i.trait_name.to_string(), crate::types::texpr_head(&i.for_type).to_string()))),
            _ => None,
        })
        .collect();
    // who refers to each definition (the graph read backwards)
    let mut referrers: HashMap<&str, Vec<&String>> = HashMap::default();
    for (n, ds) in &graph {
        for dep in ds {
            referrers.entry(dep.as_str()).or_default().push(n);
        }
    }
    let shown = crate::resolve::written_form(p);
    // a union's members inline (the default) or each on its own: each card's
    // code in the view it differs in, and the types the inline view folds
    // into a union (hidden there)
    let (inline, folded, separate) = member_views(&shown);
    // mocks and their implementations: hidden in the viewer unless shown
    let mocks = crate::view::mock_decls(p);
    let decls: Vec<Json> = p
        .decls
        .iter()
        .zip(&shown.decls)
        .map(|(d, sd)| {
            let name = d.qname();
            // a card already shows its module, so drop the `module m { ... }` wrapper;
            // the doc comment is shown as the description, so drop it from the code too
            let render = |lvl| strip_doc(view::render_decl(sd, d, lvl, &status, &graph).trim_end());
            let (iface, source) = (render(1), render(2));
            let own = impl_of.get(&name);
            let deps: Vec<String> = graph
                .get(&name)
                .into_iter()
                .flatten()
                .filter(|n| own.is_none_or(|(tr, ty)| *n != tr && *n != ty))
                .cloned()
                .collect();
            let is_own_impl = |n: &String| impl_of.get(n).is_some_and(|(tr, ty)| *tr == name || *ty == name);
            let mut references: Vec<String> = referrers.get(name.as_str()).into_iter().flatten().filter(|n| !is_own_impl(n)).map(|n| (*n).clone()).collect();
            let mut impls: Vec<String> = impl_of.keys().filter(|n| is_own_impl(n)).cloned().collect();
            // the maps are unordered; sort so the lists don't reshuffle on every refresh
            references.sort();
            impls.sort();
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
                "capability": matches!(&d.kind, DeclKind::Trait(t) if t.capability),
                "mock": mocks.contains(&d.qname()),
                "doc": d.doc.join("\n"),
                "hash": store::self_hash(d),
                "interface_hash": store::interface_hash(d),
                "deps": deps,
                "refs": references,
                "impls": impls,
                "impl_of": own.map(|(tr, ty)| json!({"trait": tr, "type": ty})),
                "holes": holes.get(&name).cloned().unwrap_or_default(),
                "verdict": verdicts.get(&name),
                "errors": rep.errors.iter().filter(|e| qname_of.get(&e.decl).unwrap_or(&e.decl) == &name || e.decl == name).map(|e| e.msg.clone()).collect::<Vec<_>>(),
                "last_change": last_change.get(&name),
                "has_body": iface != source,
                "text": { "1": iface, "2": source },
                "view_inline": inline.get(&name),
                "view_separate": separate.get(&name),
                "folded_into": folded.get(&name),
            })
        })
        .collect();
    // ---- structure for the type map ----
    let declared: crate::fx::HashSet<String> = rp
        .decls
        .iter()
        .filter(|d| matches!(d.kind, DeclKind::Type(_) | DeclKind::Trait(_)))
        .map(|d| d.name())
        .collect();
    let refs_of = |t: &TypeExpr| -> Vec<String> {
        let mut m = Vec::new();
        mentions(t, &mut m);
        m.into_iter().filter(|n| declared.contains(n)).collect()
    };
    let field_json = |name: Option<&str>, t: &TypeExpr| json!({"name": name, "ty": crate::printer::ty(t), "refs": refs_of(t)});
    let mut structure = Vec::new();
    // (the prelude's code, added to every program, isn't the program's)
    let own = |d: &&Decl| d.module != crate::types::PRELUDE_MODULE;
    for d in rp.decls.iter().filter(own) {
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
                                    Payload::Type(_) => vec![],
                                };
                                json!({"name": v.name, "fields": fs})
                            })
                            .collect(),
                    ),
                };
                structure.push(json!({
                    "name": t.name, "kind": kind, "params": t.params, "doc": d.doc.join("\n"),
                    "refinement": t.refinement.as_ref().map(|r| crate::printer::expr(r, 0)),
                    "base": base, "fields": fields, "variants": variants,
                }));
            }
            _ => {}
        }
    }

    // ---- traits for the type map ----
    for d in rp.decls.iter().filter(own) {
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
            let mut implementors: Vec<String> = env.impls.iter().filter(|i| i.trait_name == t.name).map(|i| crate::printer::ty(&i.for_type)).collect();
            implementors.sort();
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
        let mut traits: Vec<String> = env.impls.iter().filter(|i| crate::types::texpr_head(&i.for_type) == name).map(|i| i.trait_name.to_string()).collect();
        traits.sort();
        s["impls"] = json!(traits);
    }

    json!({
        "label": label,
        "decls": decls,
        "structure": structure,
        "errors": rep.errors,
        "log": log,
    })
}

fn parse_query(q: &str) -> HashMap<String, String> {
    let decode = |s: &str| {
        let b = s.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'+' => out.push(b' '),
                b'%' if i + 2 < b.len() => {
                    out.push(u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("3f"), 16).unwrap_or(b'?'));
                    i += 2;
                }
                c => out.push(c),
            }
            i += 1;
        }
        String::from_utf8_lossy(&out).to_string()
    };
    q.split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (decode(k), decode(v))).filter(|(_, v)| !v.is_empty()).collect()
}

/// The structural diff for the viewer's Changes view: patches of the
/// codebase's log, or git history of the source files.
/// On an error (a base that doesn't exist or doesn't parse), the options
/// still come back, so the viewer can always offer another range.
fn changes(target: &Target, q: &HashMap<String, String>) -> Json {
    let num = |k: &str| q.get(k).and_then(|v| v.parse::<usize>().ok());
    let base = q.get("base").map(|s| s.as_str());
    match target {
        Target::Db(root) => match Codebase::open(root) {
            Ok(cb) => match crate::history::log_range(&cb, num("from"), num("to")).and_then(|r| r.diff(num("step"))) {
                Ok(j) => j,
                Err(e) => json!({"error": e, "options": crate::history::log_options(&cb, num("from"), num("to"))}),
            },
            Err(e) => json!({"error": e}),
        },
        Target::File(f) => match crate::history::git_range(f, base).and_then(|r| r.diff(num("step"))) {
            Ok(j) => j,
            Err(e) => json!({"error": e, "options": crate::history::git_options(f, base)}),
        },
    }
}

fn respond(mut s: TcpStream, status: &str, ctype: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(body);
}

/// A fingerprint of what a snapshot is made from: every file under the
/// target (path, size, modification time), so the viewer rebuilds the
/// snapshot only when something changed, not on every poll.
fn fingerprint(target: &Target) -> u64 {
    use std::hash::{Hash, Hasher};
    fn walk(p: &std::path::Path, h: &mut impl Hasher, depth: usize) {
        let Ok(md) = std::fs::metadata(p) else { return };
        if md.is_dir() {
            if depth > 32 {
                return;
            }
            let Ok(rd) = std::fs::read_dir(p) else { return };
            let mut es: Vec<_> = rd.flatten().map(|e| e.path()).collect();
            es.sort();
            for e in es {
                // build output and version control don't make the program
                let skip = e.file_name().and_then(|n| n.to_str()).is_some_and(|n| matches!(n, ".git" | "target" | "node_modules"));
                if !skip {
                    walk(&e, h, depth + 1);
                }
            }
        } else {
            p.hash(h);
            md.len().hash(h);
            md.modified().ok().hash(h);
        }
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    match target {
        Target::Db(root) => walk(root, &mut h, 0),
        Target::File(f) => walk(f, &mut h, 0),
    }
    h.finish()
}

/// The last snapshot and the fingerprint it was made at.
type Cache = std::sync::Mutex<Option<(u64, std::sync::Arc<String>)>>;

/// The snapshot as JSON text, rebuilt only if the files changed since the last one.
fn cached_snapshot(target: &Target, cache: &Cache) -> (u64, std::sync::Arc<String>) {
    // held while building, so polls that arrive meanwhile wait for this snapshot
    // instead of building their own
    let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
    let fp = fingerprint(target);
    if let Some((f, body)) = c.as_ref() {
        if *f == fp {
            return (fp, body.clone());
        }
    }
    let body = std::sync::Arc::new(snapshot(target).to_string());
    *c = Some((fp, body.clone()));
    (fp, body)
}

fn respond_etag(mut s: TcpStream, etag: u64, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nETag: \"{etag:x}\"\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(body);
}

fn handle(stream: TcpStream, target: &Target, cache: &Cache) {
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    // the headers: only `If-None-Match` matters
    let mut if_none_match = None;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.trim().eq_ignore_ascii_case("if-none-match") {
                if_none_match = Some(v.trim().trim_matches('"').to_string());
            }
        }
    }
    let target_path = line.split_whitespace().nth(1).unwrap_or("/");
    let (path, query) = target_path.split_once('?').unwrap_or((target_path, ""));
    match path {
        "/api/changes" => {
            let body = changes(target, &parse_query(query)).to_string();
            respond(stream, "200 OK", "application/json", body.as_bytes())
        }
        "/" | "/index.html" => respond(stream, "200 OK", "text/html; charset=utf-8", PAGE.as_bytes()),
        "/api/state" => {
            let (etag, body) = cached_snapshot(target, cache);
            if if_none_match.as_deref() == Some(format!("{etag:x}").as_str()) {
                let mut s = stream;
                let _ = s.write_all(format!("HTTP/1.1 304 Not Modified\r\nETag: \"{etag:x}\"\r\nConnection: close\r\n\r\n").as_bytes());
            } else {
                respond_etag(stream, etag, body.as_bytes())
            }
        }
        _ => respond(stream, "404 Not Found", "text/plain", b"not found"),
    }
}

pub fn serve(target: Target, port: u16) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("cannot listen on port {port}: {e}"))?;
    println!("Strata viewer on http://127.0.0.1:{port}  (Ctrl-C to stop)");
    let shared = std::sync::Arc::new((target, Cache::default()));
    for stream in listener.incoming().flatten() {
        // one thread per connection: a snapshot being rebuilt doesn't hold up the page or the Changes view
        let shared = shared.clone();
        let spawned = std::thread::Builder::new().stack_size(64 << 20).spawn(move || handle(stream, &shared.0, &shared.1));
        if let Err(e) = spawned {
            eprintln!("viewer: cannot start a thread: {e}");
        }
    }
    Ok(())
}

/// The two views of unions' members (`resolve::members_view`) for the
/// viewer, by definition: the code of each definition the inline view shows
/// differently, the union each type it folds goes into, and the code of
/// each union the separate view shows with its members before it.
fn member_views(shown: &Program) -> (HashMap<String, String>, HashMap<String, String>, HashMap<String, String>) {
    use crate::resolve::{members_view, Members};
    let text = |d: &Decl| crate::printer::decl(d).trim_end().to_string();
    let before: HashMap<String, String> = shown.decls.iter().map(|d| (d.qname(), text(d))).collect();
    let (mut inline, mut folded, mut separate) = (HashMap::default(), HashMap::default(), HashMap::default());
    let iv = members_view(shown, Members::Inline);
    let kept: crate::fx::HashSet<String> = iv.decls.iter().map(|d| d.qname()).collect();
    for d in &iv.decls {
        let t = text(d);
        if before.get(&d.qname()) != Some(&t) {
            inline.insert(d.qname(), t);
        }
        // the types it declares now that were on their own
        if let DeclKind::Type(td) = &d.kind {
            if let TypeBody::Sum(vs) = &td.body {
                for v in vs.iter().filter(|v| !matches!(v.payload, Payload::Type(_))) {
                    let q = if d.module.is_empty() { v.name.to_string() } else { format!("{}.{}", d.module, crate::names::short(&v.name)) };
                    if before.contains_key(&q) && !kept.contains(&q) {
                        folded.insert(q, d.qname());
                    }
                }
            }
        }
    }
    let mut pending: Vec<String> = vec![];
    for d in &members_view(shown, Members::Separate).decls {
        let t = text(d);
        if !before.contains_key(&d.qname()) {
            pending.push(t);
            continue;
        }
        if !pending.is_empty() || before.get(&d.qname()) != Some(&t) {
            pending.push(t);
            separate.insert(d.qname(), pending.join("\n\n"));
        }
        pending.clear();
    }
    (inline, folded, separate)
}

/// Every named type mentioned in a type expression.
fn mentions(t: &TypeExpr, out: &mut Vec<String>) {
    match t {
        TypeExpr::Named(n, args) => {
            if !out.contains(&n.to_string()) {
                out.push(n.to_string());
            }
            args.iter().for_each(|a| mentions(a, out));
        }
        TypeExpr::Fn(ps, r) => {
            ps.iter().for_each(|a| mentions(a, out));
            mentions(r, out);
        }
        TypeExpr::Unit => {}
        TypeExpr::Refined(b, _) => mentions(b, out),
        TypeExpr::Union(ms) => ms.iter().for_each(|m| mentions(m, out)),
    }
}
