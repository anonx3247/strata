//! Level-of-detail projections of the codebase.
//!
//! L0  map: one line per definition, with dependencies and status
//! L1  interfaces: types, traits, signatures and contracts (bodies elided)
//! L2  full source
//!
//! Every level is valid Strata text for the parts it shows; L2 round-trips.

use crate::ast::*;
use crate::printer;
use crate::store;
use std::collections::BTreeSet;
use crate::fx::HashMap;

pub struct Status {
    /// decl name -> hole names
    pub holes: HashMap<String, Vec<String>>,
    /// decl name -> verdict tag
    pub verdicts: HashMap<String, String>,
}

fn glob(pat: &str, name: &str) -> bool {
    match pat.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => pat == name,
    }
}

/// Resolve a selector to the set of definition names it covers.
/// `a,b` (union), `prefix*`, `slice:Name` (Name, what it uses, and what uses it).
/// `main.name` is `name` in the root module.
pub fn select(p: &Program, selector: Option<&str>) -> BTreeSet<String> {
    let all: Vec<String> = p.decls.iter().map(|d| d.qname()).collect();
    let Some(sel) = selector else { return all.into_iter().collect() };
    let g = store::graph(p);
    let mut out = BTreeSet::new();
    for term in sel.split(',').map(str::trim) {
        if let Some(m) = term.strip_prefix("module:") {
            out.extend(p.decls.iter().filter(|d| d.module == m || d.module.starts_with(&format!("{m}."))).map(|d| d.qname()));
        } else if let Some(n) = term.strip_prefix("slice:") {
            for (name, deps) in &g {
                if glob(n, name) {
                    out.insert(name.clone());
                    out.extend(deps.iter().cloned());
                }
                if deps.iter().any(|d| glob(n, d)) {
                    out.insert(name.clone());
                }
            }
        } else {
            out.extend(all.iter().filter(|a| glob(term, a)).cloned());
            // `main.total`: a definition of the root module, whose names have no prefix
            if let Some(rest) = term.strip_prefix("main.") {
                out.extend(p.decls.iter().filter(|d| d.module.is_empty() && glob(rest, &d.qname())).map(|d| d.qname()));
            }
        }
    }
    out
}

fn status_note(d: &Decl, st: &Status) -> String {
    let mut parts = Vec::new();
    if let Some(hs) = st.holes.get(&d.qname()) {
        parts.push(format!("holes: {}", hs.iter().map(|h| format!("?{h}")).collect::<Vec<_>>().join(" ")));
    }
    if let Some(v) = st.verdicts.get(&d.qname()) {
        parts.push(v.clone());
    }
    if parts.is_empty() { String::new() } else { format!("  // [{}]", parts.join(", ")) }
}

/// One definition at `level`: `shown` is its written form
/// ([`crate::resolve::written_form`]), `stored` as stored; `g` is the
/// program's [`store::graph`] (level 0 lists the dependencies).
pub fn render_decl(shown: &Decl, stored: &Decl, level: u8, st: &Status, g: &std::collections::BTreeMap<String, BTreeSet<String>>) -> String {
    let (st_d, d) = (stored, shown);
    let note = status_note(st_d, st);
    match level {
        0 => {
            let deps = g.get(&st_d.qname()).cloned().unwrap_or_default();
            let deps = if deps.is_empty() { String::new() } else { format!("  → {}", deps.into_iter().collect::<Vec<_>>().join(", ")) };
            let name = match &d.kind {
                DeclKind::Test(t) => format!("{:?}", t.name),
                _ => st_d.qname(),
            };
            format!("{:<6} {name}{deps}{note}", d.kind_str())
        }
        _ => {
            let body = match &d.kind {
                DeclKind::Fn(f) if level == 1 => format!("{}{note}", printer::fn_signature(f)),
                DeclKind::Impl(i) if level < 2 => format!("{}{note}", printer::impl_interface(i)),
                DeclKind::Trait(t) if level < 2 => format!("{}{note}", printer::trait_interface(t)),
                DeclKind::Capability(c) if level < 2 => format!("{}{note}", printer::capability_interface(c)),
                DeclKind::Test(t) if level < 2 && t.params.is_empty() => format!("test {:?} {{ … }}", t.name),
                DeclKind::Test(t) if level < 2 => format!("test {:?} ({}) {{ … }}", t.name, t.params.iter().map(|p| format!("{}{}: {}", if p.mutable { "mut " } else { "" }, p.name, printer::ty(&p.ty))).collect::<Vec<_>>().join(", ")),
                _ => format!("{}{note}", printer::decl_body(d)),
            };
            let doc = printer::comments(&d.comments, 0) + &printer::doc(d);
            let pubk = if d.public && !body.starts_with("pub ") { "pub " } else { "" };
            format!("{doc}{pubk}{body}")
        }
    }
}

/// The definitions of `p` (as stored) that `names` lists, shown as written.
pub fn render(stored: &Program, names: &BTreeSet<String>, level: u8, st: &Status) -> String {
    // only the map (level 0) shows dependencies: don't resolve the program for the others
    let g = if level == 0 { store::graph(stored) } else { Default::default() };
    let shown = crate::resolve::written_form(stored);
    let mut out: Vec<(String, String)> = Vec::new();
    for (d, s) in shown.decls.iter().zip(&stored.decls).filter(|(_, s)| names.contains(&s.qname())) {
        out.push((d.module.to_string(), render_decl(d, s, level, st, &g)));
    }
    if level == 0 {
        return out.into_iter().map(|(_, t)| t).collect::<Vec<_>>().join("\n") + "\n";
    }
    // a module is a file: a view of one module is that file's text (so it can go
    // back through `edit --module m`); a view across modules names each one
    let mut order: Vec<String> = Vec::new();
    for (m, _) in &out {
        if !order.contains(m) {
            order.push(m.clone());
        }
    }
    let mut parts = Vec::new();
    for m in &order {
        let body: Vec<String> = out.iter().filter(|(mm, _)| mm == m).map(|(_, t)| t.clone()).collect();
        if order.len() > 1 {
            parts.push(if m.is_empty() { "// ── root module".to_string() } else { format!("// ── module {m}") });
        }
        parts.extend(body);
    }
    parts.join("\n\n") + "\n"
}
