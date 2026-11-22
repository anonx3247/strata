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
use std::path::{Path, PathBuf};

const PAGE: &str = include_str!("ui/index.html");

mod activity;
mod agents;
mod prs;
mod transcripts;

#[derive(Clone)]
pub enum Target {
    Db(PathBuf),
    File(PathBuf),
}

impl Target {
    fn path(&self) -> &Path {
        match self {
            Target::Db(p) | Target::File(p) => p,
        }
    }

    /// The same kind of target, somewhere else (another worktree).
    fn at(&self, path: PathBuf) -> Target {
        match self {
            Target::Db(_) => Target::Db(path),
            Target::File(_) => Target::File(path),
        }
    }
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
    // (resolved, std's definitions are named `std.io.Recorder`)
    let std_prefix = format!("{}.", crate::stdlib::NAME);
    // (nor what the compiler makes: `$where_0`, `Shape$own`)
    let is_std = |d: &Decl| d.name().starts_with(&std_prefix) || d.name().contains('$');
    let declared: crate::fx::HashSet<String> = rp
        .decls
        .iter()
        .filter(|d| matches!(d.kind, DeclKind::Type(_) | DeclKind::Trait(_)) && !is_std(d))
        .map(|d| d.name())
        .collect();
    let refs_of = |t: &TypeExpr| -> Vec<String> {
        let mut m = Vec::new();
        mentions(t, &mut m);
        m.into_iter().filter(|n| declared.contains(n)).collect()
    };
    let field_json = |name: Option<&str>, t: &TypeExpr| json!({"name": name, "ty": crate::printer::ty(t), "refs": refs_of(t)});
    let mut structure = Vec::new();
    // (the prelude's code, added to every program, isn't the program's; nor
    // is the standard library's, which resolving brings in: the map is the
    // program's types, whether it uses std's or not)
    let own = |d: &&Decl| d.module != crate::types::PRELUDE_MODULE && !is_std(d);
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
    fingerprint_of(target.path())
}

/// The fingerprint of the files under a folder (or of one file).
fn fingerprint_of(path: &Path) -> u64 {
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
    walk(path, &mut h, 0);
    h.finish()
}

/// The last snapshot of each target shown (the viewer's own, and the same
/// target in other worktrees) and the fingerprint it was made at.
type Cache = std::sync::Mutex<HashMap<PathBuf, (u64, std::sync::Arc<String>)>>;

/// The snapshot as JSON text, rebuilt only if the files changed since the last one.
fn cached_snapshot(target: &Target, cache: &Cache) -> (u64, std::sync::Arc<String>) {
    // held while building, so polls that arrive meanwhile wait for this snapshot
    // instead of building their own
    let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
    let fp = fingerprint(target);
    if let Some((f, body)) = c.get(target.path()) {
        if *f == fp {
            return (fp, body.clone());
        }
    }
    let body = std::sync::Arc::new(snapshot(target).to_string());
    c.insert(target.path().to_path_buf(), (fp, body.clone()));
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

fn respond_json(s: TcpStream, j: &Json) {
    respond(s, "200 OK", "application/json", j.to_string().as_bytes())
}

/// What the viewer serves: the target, its repository, the agents it runs.
struct App {
    target: Target,
    cache: Cache,
    agents: agents::Agents,
    repo: Option<activity::Repo>,
    port: u16,
}

impl App {
    /// The target as another worktree of the repository has it (`wt`), or the viewer's own.
    fn target_in(&self, wt: Option<&String>) -> Result<Target, String> {
        let (Some(wt), Some(repo)) = (wt, &self.repo) else { return Ok(self.target.clone()) };
        let wt = std::fs::canonicalize(wt).map_err(|e| format!("{wt}: {e}"))?;
        if wt == repo.top {
            return Ok(self.target.clone());
        }
        if !repo.worktrees().iter().any(|(p, _, _)| *p == wt) {
            return Err(format!("{} is not a worktree of this repository", wt.display()));
        }
        Ok(self.target.at(repo.in_worktree(&wt)))
    }

    fn repo(&self) -> Result<&activity::Repo, String> {
        self.repo.as_ref().ok_or_else(|| format!("{} is not in a git repository", self.target.path().display()))
    }

    /// The working directories whose saved agent sessions belong here: the
    /// repository's worktrees (this one first), else the target's folder.
    fn session_dirs(&self) -> Vec<PathBuf> {
        match &self.repo {
            Some(r) => std::iter::once(r.top.clone()).chain(r.worktrees().into_iter().map(|w| w.0).filter(|w| *w != r.top)).collect(),
            None => vec![self.target.path().to_path_buf()],
        }
    }

    /// The folder an agent runs in: the repository (or one of its worktrees,
    /// or a new one), else the target's folder.
    fn agent_dir(&self, place: &Json) -> Result<PathBuf, String> {
        // a saved session goes on where it was: one of the folders listed
        if let Some(cwd) = place["resume_cwd"].as_str() {
            let cwd = std::fs::canonicalize(cwd).map_err(|e| format!("{cwd}: {e}"))?;
            return if self.session_dirs().contains(&cwd) { Ok(cwd) } else { Err(format!("{} is not this repository or one of its worktrees", cwd.display())) };
        }
        let Ok(repo) = self.repo() else {
            let p = self.target.path();
            return Ok(if p.is_dir() { p.to_path_buf() } else { p.parent().unwrap_or(Path::new(".")).to_path_buf() });
        };
        if let Some(name) = place["new"].as_str() {
            let ok = !name.is_empty() && !name.starts_with('-') && !name.contains("..") && name.chars().all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c));
            if !ok {
                return Err(format!("`{name}` can't be a branch name: letters, digits, - _ . / only"));
            }
            let dir = repo.top.join(".claude").join("worktrees").join(name);
            let d = dir.display().to_string();
            let base = repo.default_branch();
            let exists = crate::git::ok(Some(&repo.top), &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{name}")]);
            if exists {
                repo.git(&["worktree", "add", &d, name])?;
            } else {
                repo.git(&["worktree", "add", "-b", name, &d, &base])?;
            }
            return Ok(std::fs::canonicalize(&dir).unwrap_or(dir));
        }
        match place["worktree"].as_str() {
            None | Some("") => Ok(repo.top.clone()),
            Some(w) => {
                let w = std::fs::canonicalize(w).map_err(|e| format!("{w}: {e}"))?;
                if repo.worktrees().iter().any(|(p, _, _)| *p == w) {
                    Ok(w)
                } else {
                    Err(format!("{} is not a worktree of this repository", w.display()))
                }
            }
        }
    }
}

/// A request, as far as the viewer reads it.
struct Request {
    method: String,
    path: String,
    query: HashMap<String, String>,
    if_none_match: Option<String>,
    host: Option<String>,
    origin: Option<String>,
    /// `X-Strata: 1`: sent by the page with every POST. A web page elsewhere
    /// can't send it without the viewer's permission, which it never gives.
    marked: bool,
    body: Json,
}

fn read_request(stream: &TcpStream) -> Option<Request> {
    use std::io::Read;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let (mut if_none_match, mut host, mut origin, mut marked, mut len) = (None, None, None, false, 0usize);
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            let (k, v) = (k.trim().to_ascii_lowercase(), v.trim().to_string());
            match k.as_str() {
                "if-none-match" => if_none_match = Some(v.trim_matches('"').to_string()),
                "host" => host = Some(v),
                "origin" => origin = Some(v),
                "x-strata" => marked = v == "1",
                "content-length" => len = v.parse().unwrap_or(0),
                _ => {}
            }
        }
    }
    let mut body = vec![0u8; len.min(8 << 20)];
    if reader.read_exact(&mut body).is_err() {
        body.clear();
    }
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    Some(Request {
        method,
        path: path.to_string(),
        query: parse_query(query),
        if_none_match,
        host,
        origin,
        marked,
        body: serde_json::from_slice(&body).unwrap_or(Json::Null),
    })
}

fn handle(stream: TcpStream, app: &App) {
    let Some(req) = read_request(&stream) else { return };
    // only this machine's pages: a name that resolves here (DNS rebinding) is refused
    let hosts = [format!("127.0.0.1:{}", app.port), format!("localhost:{}", app.port)];
    if !req.host.as_ref().is_some_and(|h| hosts.contains(h)) || req.origin.as_ref().is_some_and(|o| !hosts.iter().any(|h| *o == format!("http://{h}"))) {
        return respond(stream, "403 Forbidden", "text/plain", b"the viewer answers only its own page");
    }
    if req.method == "POST" {
        if !req.marked {
            return respond(stream, "403 Forbidden", "text/plain", b"the viewer answers only its own page");
        }
        return respond_json(stream, &post(app, &req));
    }
    let seg: Vec<&str> = req.path.trim_matches('/').split('/').collect();
    let q = |k: &str| req.query.get(k).map(|s| s.as_str()).unwrap_or("");
    match seg.as_slice() {
        [""] | ["index.html"] => respond(stream, "200 OK", "text/html; charset=utf-8", PAGE.as_bytes()),
        ["api", "state"] => {
            let target = match app.target_in(req.query.get("wt")) {
                Ok(t) => t,
                Err(e) => return respond_json(stream, &json!({"fatal": e})),
            };
            let (etag, body) = cached_snapshot(&target, &app.cache);
            if req.if_none_match.as_deref() == Some(format!("{etag:x}").as_str()) {
                let mut s = stream;
                let _ = s.write_all(format!("HTTP/1.1 304 Not Modified\r\nETag: \"{etag:x}\"\r\nConnection: close\r\n\r\n").as_bytes());
            } else {
                respond_etag(stream, etag, body.as_bytes())
            }
        }
        ["api", "config"] => {
            let dir = app.repo.as_ref().map(|r| r.path.clone()).unwrap_or(app.target.path().to_path_buf());
            respond_json(stream, &json!({"agent": agents::agent_command(&dir), "auto_allow": agents::auto_allow_default(&dir), "repo": app.repo.as_ref().map(|r| &r.top), "rel": app.repo.as_ref().map(|r| &r.rel)}))
        }
        ["api", "activity"] => respond_json(stream, &app.repo().map(activity::activity).unwrap_or_else(|e| json!({"error": e}))),
        ["api", "diff"] => respond_json(stream, &app.repo().map(|r| activity::diff(r, q("base"), q("head"))).unwrap_or_else(|e| json!({"error": e}))),
        ["api", "agents"] => respond_json(stream, &app.agents.list()),
        ["api", "agents", id] => match id.parse().ok().and_then(|id| app.agents.get(id)) {
            Some(s) => respond_json(stream, &s.events_since(q("since").parse().unwrap_or(0))),
            None => respond_json(stream, &json!({"error": "no such session"})),
        },
        ["api", "history"] => respond_json(stream, &transcripts::list(&app.session_dirs())),
        ["api", "history", id] => respond_json(stream, &transcripts::transcript(&app.session_dirs(), id)),
        ["api", "prs"] => respond_json(stream, &app.repo().map(|r| prs::list(r, q("state"))).unwrap_or_else(|e| json!({"error": e}))),
        ["api", "prs", n] => respond_json(stream, &app.repo().map(|r| prs::view(r, n)).unwrap_or_else(|e| json!({"error": e}))),
        ["api", "prs", n, "diff"] => respond_json(stream, &app.repo().map(|r| prs::diff(r, n)).unwrap_or_else(|e| json!({"error": e}))),
        _ => respond(stream, "404 Not Found", "text/plain", b"not found"),
    }
}

/// The page's actions: agents started and prompted, permissions answered, PRs reviewed and merged.
fn post(app: &App, req: &Request) -> Json {
    let seg: Vec<&str> = req.path.trim_matches('/').split('/').collect();
    let b = &req.body;
    let text = |k: &str| b[k].as_str().unwrap_or("").to_string();
    let session = |id: &str| id.parse().ok().and_then(|id| app.agents.get(id)).ok_or_else(|| json!({"error": "no such session"}));
    match seg.as_slice() {
        ["api", "agents"] => {
            let dir = match app.agent_dir(b) {
                Ok(d) => d,
                Err(e) => return json!({"error": e}),
            };
            let conf = app.repo.as_ref().map(|r| r.path.clone()).unwrap_or(app.target.path().to_path_buf());
            let command = agents::agent_command(&conf);
            let auto = b["auto_allow"].as_bool().unwrap_or_else(|| agents::auto_allow_default(&conf));
            let title = if text("title").trim().is_empty() { dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default() } else { text("title") };
            let load = b["resume"].as_str().filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')).map(String::from);
            match app.agents.start(dir, title, command, auto, load) {
                Ok(s) => {
                    if !text("prompt").trim().is_empty() {
                        s.prompt(text("prompt"));
                    }
                    s.summary()
                }
                Err(e) => json!({"error": e}),
            }
        }
        ["api", "agents", id, action] => {
            let s = match session(id) {
                Ok(s) => s,
                Err(e) => return e,
            };
            match *action {
                "prompt" if !text("text").trim().is_empty() => s.prompt(text("text")),
                "prompt" => return json!({"error": "nothing to send"}),
                "answer" => {
                    if let Err(e) = s.answer(&b["rpc"], b["option"].as_str()) {
                        return json!({"error": e});
                    }
                }
                "cancel" => s.cancel(),
                "stop" => s.stop(),
                "auto" => s.set_auto_allow(b["on"].as_bool().unwrap_or(false)),
                _ => return json!({"error": format!("no action `{action}`")}),
            }
            s.summary()
        }
        ["api", "prs", n, action] => {
            let repo = match app.repo() {
                Ok(r) => r,
                Err(e) => return json!({"error": e}),
            };
            match *action {
                "comment" => prs::comment(repo, n, &text("body")),
                "review" => prs::review(repo, n, &text("event"), &text("body")),
                "merge" => prs::merge(repo, n, b["stack"].as_bool().unwrap_or(false)),
                _ => json!({"error": format!("no action `{action}`")}),
            }
        }
        _ => json!({"error": format!("no action at {}", req.path)}),
    }
}

pub fn serve(target: Target, port: u16) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("cannot listen on port {port}: {e}"))?;
    println!("Strata viewer on http://127.0.0.1:{port}  (Ctrl-C to stop)");
    let repo = activity::Repo::of(target.path());
    let app = std::sync::Arc::new(App { target, cache: Cache::default(), agents: agents::Agents::default(), repo, port });
    for stream in listener.incoming().flatten() {
        // one thread per connection: a snapshot being rebuilt doesn't hold up the page or the agents
        let app = app.clone();
        let spawned = std::thread::Builder::new().stack_size(64 << 20).spawn(move || handle(stream, &app));
        if let Err(e) = spawned {
            eprintln!("viewer: cannot start a thread: {e}");
        }
    }
    app.agents.stop_all();
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

// ---- the codebase for Strata programs (`std.code`) ----

/// The codebase a `Code` capability reads: the current folder, unless the
/// viewer set it to the one it shows.
static CODE_ROOT: std::sync::OnceLock<String> = std::sync::OnceLock::new();

pub fn code_root() -> String {
    CODE_ROOT.get().cloned().unwrap_or_else(|| ".".into())
}

/// Make `Code` capabilities read `root` (once, before any is handed out).
pub fn set_code_root(root: &str) {
    let _ = CODE_ROOT.set(root.to_string());
}

/// A package's folder is read as the package (with its log and cached
/// verdicts); anything else as source files.
fn target_of(root: &std::path::Path) -> Target {
    if root.is_dir() && root.join(crate::package::FILE).is_file() {
        Target::Db(root.to_path_buf())
    } else {
        Target::File(root.to_path_buf())
    }
}

/// What changes whenever the code under `root` does: compare it to skip
/// reading a snapshot that hasn't changed.
pub fn code_version(root: &std::path::Path) -> String {
    format!("{:016x}", fingerprint(&target_of(root)))
}

/// The snapshot of the code under `root`, as the JSON of a `std.code.Snapshot`;
/// kept, per folder, until its files change.
pub fn code_snapshot(root: &std::path::Path) -> Result<Json, String> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<HashMap<PathBuf, (u64, std::sync::Arc<Json>)>>> = std::sync::OnceLock::new();
    let target = target_of(root);
    let fp = fingerprint(&target);
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some((f, j)) = cache.get(root) {
        if *f == fp {
            return Ok((**j).clone());
        }
    }
    let s = snapshot(&target);
    if let Some(e) = s["fatal"].as_str() {
        return Err(e.to_string());
    }
    let j = for_strata(&s, &format!("{fp:016x}"));
    cache.insert(root.to_path_buf(), (fp, std::sync::Arc::new(j.clone())));
    Ok(j)
}

/// The viewer's snapshot reshaped as a `std.code.Snapshot`.
fn for_strata(s: &Json, version: &str) -> Json {
    // an `Option` as `from_json` reads it
    let opt = |j: Json| if j.is_null() { json!("None") } else { json!({ "Some": j }) };
    let strs = |j: &Json| -> Vec<String> { j.as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect() };
    let text = |j: &Json| j.as_str().unwrap_or("").to_string();
    let definitions: Vec<Json> = s["decls"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| {
            json!({
                "name": text(&d["name"]),
                "module": text(&d["module"]),
                "label": text(&d["label"]),
                "kind": text(&d["kind"]),
                "public": d["public"].as_bool().unwrap_or(false),
                "doc": text(&d["doc"]),
                "hash": text(&d["hash"]),
                "interface": text(&d["text"]["1"]),
                "source": text(&d["text"]["2"]),
                "deps": strs(&d["deps"]),
                "refs": strs(&d["refs"]),
                "impls": strs(&d["impls"]),
                "holes": d["holes"].as_array().into_iter().flatten().map(|h| text(&h["name"])).collect::<Vec<_>>(),
                "errors": strs(&d["errors"]),
                "verdict": opt(d["verdict"]["status"].clone()),
                "last_change": opt(d["last_change"].clone()),
            })
        })
        .collect();
    let errors: Vec<String> = s["errors"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| match e["decl"].as_str().filter(|d| !d.is_empty()) {
            Some(d) => format!("{d}: {}", text(&e["msg"])),
            None => text(&e["msg"]),
        })
        .collect();
    let log: Vec<Json> = s["log"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| {
            let changes: Vec<Json> = e["changes"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|c| {
                    let what = match (c["old"].is_null(), c["new"].is_null()) {
                        (true, _) => "added",
                        (_, true) => "removed",
                        _ if c["interface_changed"] == true => "interface",
                        _ => "body",
                    };
                    json!({"name": text(&c["name"]), "what": what})
                })
                .collect();
            json!({"message": text(&e["message"]), "time": opt(e["time"].clone()), "changes": changes})
        })
        .collect();
    json!({"version": version, "label": text(&s["label"]), "definitions": definitions, "errors": errors, "log": log})
}
