//! Structural diffs between two versions of a codebase.
//!
//! Definitions are paired by name (and through recorded renames), and each
//! pair becomes one typed change: what happened in words (a variant added, a
//! stronger guarantee, a method added), the code with changes marked in place
//! and unchanged parts folded, and a word diff of its doc. A body that was
//! mostly rewritten falls back to a plain line diff. Definitions whose only
//! change is following a rename are folded into that rename.

use crate::names::short;
use crate::ast::*;
use crate::printer;
use crate::store;
use crate::verify::Verdict;
use serde_json::{json, Value as Json};
use crate::fx::{HashMap, HashSet};

/// The steps (patches or commits) a range is made of: step number and the
/// definitions it touched, so each change can say which steps made it.
pub type StepNames = Vec<(usize, Vec<String>)>;

/// Code as the diff shows it: comments and the definition, without its doc
/// (the doc is diffed as prose).
fn code(d: &Decl) -> String {
    printer::comments(&d.comments, 0) + &printer::decl_body(d)
}

/// A doc as prose: its lines are only how it was wrapped.
fn doc_text(d: &Decl) -> String {
    d.doc.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
}


// ---------- sequence diff ----------

#[derive(Clone, Copy, PartialEq, Debug)]
enum Op {
    Eq(usize, usize),
    Del(usize),
    Ins(usize),
}

fn lcs<T: PartialEq>(a: &[T], b: &[T]) -> Vec<Op> {
    let (n, m) = (a.len(), b.len());
    let mut t = vec![0u32; (n + 1) * (m + 1)];
    let w = m + 1;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            t[i * w + j] = if a[i] == b[j] { t[(i + 1) * w + j + 1] + 1 } else { t[(i + 1) * w + j].max(t[i * w + j + 1]) };
        }
    }
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            out.push(Op::Eq(i, j));
            i += 1;
            j += 1;
        } else if i < n && (j == m || t[(i + 1) * w + j] >= t[i * w + j + 1]) {
            out.push(Op::Del(i));
            i += 1;
        } else {
            out.push(Op::Ins(j));
            j += 1;
        }
    }
    out
}

fn tokens(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let class = |c: char| if c.is_alphanumeric() || c == '_' { 0 } else if c.is_whitespace() { 1 } else { 2 };
    let cs: Vec<(usize, char)> = s.char_indices().collect();
    for k in 0..cs.len() {
        let (i, c) = cs[k];
        let next = cs.get(k + 1);
        let end = next.map_or(s.len(), |(j, _)| *j);
        // punctuation is one token per character; words and spaces are runs
        if class(c) == 2 || next.is_none_or(|(_, d)| class(*d) != class(c)) {
            out.push(&s[start..end]);
            start = end;
        }
        let _ = i;
    }
    out
}

/// Word-level segments between two texts: `=` kept, `-` removed, `+` added.
/// When more changed than stayed between the first and last change, that
/// whole span reads as one replacement (`[-len(xs) == 0-]{+xs == []+}`).
fn segments(a: &str, b: &str) -> Vec<(char, String)> {
    let fine = fine_segments(a, b);
    let first = fine.iter().position(|(k, _)| *k != '=');
    let last = fine.iter().rposition(|(k, _)| *k != '=');
    let (Some(f), Some(l)) = (first, last) else { return fine };
    let words = |s: &str| tokens(s).iter().filter(|t| !t.trim().is_empty()).count();
    let kept: usize = fine[f..=l].iter().filter(|(k, _)| *k == '=').map(|(_, s)| words(s)).sum();
    let changed: usize = fine[f..=l].iter().filter(|(k, _)| *k != '=').map(|(_, s)| words(s)).sum();
    // (only spaces kept between the edits counts as nothing kept)
    if changed <= kept {
        return fine;
    }
    let old: String = fine[f..=l].iter().filter(|(k, _)| *k != '+').map(|(_, s)| s.as_str()).collect();
    let new: String = fine[f..=l].iter().filter(|(k, _)| *k != '-').map(|(_, s)| s.as_str()).collect();
    let mut out: Vec<(char, String)> = fine[..f].to_vec();
    out.push(('-', old));
    out.push(('+', new));
    out.extend(fine[l + 1..].iter().cloned());
    out.retain(|(_, t)| !t.is_empty());
    out
}

fn fine_segments(a: &str, b: &str) -> Vec<(char, String)> {
    let (ta, tb) = (tokens(a), tokens(b));
    let mut out: Vec<(char, String)> = Vec::new();
    for op in lcs(&ta, &tb) {
        let (k, s) = match op {
            Op::Eq(i, _) => ('=', ta[i]),
            Op::Del(i) => ('-', ta[i]),
            Op::Ins(j) => ('+', tb[j]),
        };
        match out.last_mut() {
            Some((lk, ls)) if *lk == k => ls.push_str(s),
            _ => out.push((k, s.to_string())),
        }
    }
    out
}

pub fn similarity(a: &str, b: &str) -> f64 {
    let (ta, tb): (Vec<&str>, Vec<&str>) = (tokens(a).into_iter().filter(|t| !t.trim().is_empty()).collect(), tokens(b).into_iter().filter(|t| !t.trim().is_empty()).collect());
    if ta.is_empty() && tb.is_empty() {
        return 1.0;
    }
    let same = lcs(&ta, &tb).iter().filter(|o| matches!(o, Op::Eq(..))).count();
    2.0 * same as f64 / (ta.len() + tb.len()) as f64
}

fn segs_json(s: &[(char, String)]) -> Json {
    Json::Array(s.iter().map(|(k, t)| json!([k.to_string(), t])).collect())
}

// ---------- lines ----------

#[derive(Clone, Debug)]
struct Line {
    /// ' ' unchanged, '+' added, '-' removed, '~' changed in place
    s: char,
    text: String,
    segs: Option<Vec<(char, String)>>,
    note: Option<&'static str>,
}

impl Line {
    fn new(s: char, text: &str) -> Line {
        Line { s, text: text.to_string(), segs: None, note: None }
    }
    fn json(&self) -> Json {
        let mut j = json!({"s": self.s.to_string(), "t": self.text});
        if let Some(s) = &self.segs {
            j["segs"] = segs_json(s);
        }
        if let Some(n) = self.note {
            j["note"] = json!(n);
        }
        j
    }
}

fn indent(s: &str) -> usize {
    (s.len() - s.trim_start().len()) / 2
}

/// Line diff of two texts. Similar removed/added lines are paired into one
/// line changed in place, unless most of the text changed (`rewritten`).
fn line_diff(a: &str, b: &str) -> (Vec<Line>, bool) {
    let (la, lb): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    let ops = lcs(&la, &lb);
    let changed = ops.iter().filter(|o| !matches!(o, Op::Eq(..))).count();
    let rewritten = la.len() + lb.len() > 8 && changed as f64 >= 0.6 * (la.len() + lb.len()) as f64;
    let mut out = Vec::new();
    let mut k = 0;
    while k < ops.len() {
        if let Op::Eq(_, j) = ops[k] {
            out.push(Line::new(' ', lb[j]));
            k += 1;
            continue;
        }
        // one hunk of removals and additions
        let (mut dels, mut ins) = (Vec::new(), Vec::new());
        while k < ops.len() && !matches!(ops[k], Op::Eq(..)) {
            match ops[k] {
                Op::Del(i) => dels.push(la[i]),
                Op::Ins(j) => ins.push(lb[j]),
                _ => {}
            }
            k += 1;
        }
        if rewritten {
            out.extend(dels.iter().map(|d| Line::new('-', d)));
            out.extend(ins.iter().map(|i| Line::new('+', i)));
            continue;
        }
        let mut next_ins = 0;
        // prose (comments) is paired only when it is mostly the same line;
        // a rewrapped paragraph reads better as lines removed and added
        let is_com = |l: &str| l.trim_start().starts_with("//");
        let close = |d: &str, i: &str| similarity(d, i) >= if is_com(d) && is_com(i) { 0.75 } else { 0.5 };
        for d in &dels {
            match (next_ins..ins.len()).find(|&j| close(d, ins[j])) {
                Some(j) => {
                    out.extend(ins[next_ins..j].iter().map(|i| Line::new('+', i)));
                    let mut l = Line::new('~', ins[j]);
                    l.segs = Some(segments(d, ins[j]));
                    out.push(l);
                    next_ins = j + 1;
                }
                None => out.push(Line::new('-', d)),
            }
        }
        out.extend(ins[next_ins..].iter().map(|i| Line::new('+', i)));
    }
    (out, rewritten)
}

/// What a folded run of unchanged lines holds: member names when it is a run
/// of whole members, else a line count.
fn fold_label(hidden: &[Line], kind: &str) -> String {
    if hidden.iter().all(|l| l.text.trim_start().starts_with("//") || l.text.trim().is_empty()) {
        return "comments unchanged".into();
    }
    let base = hidden.iter().filter(|l| !l.text.trim().is_empty()).map(|l| indent(&l.text)).min().unwrap_or(0);
    let mut names = Vec::new();
    for l in hidden {
        let t = l.text.trim_start();
        if indent(&l.text) != base {
            continue;
        }
        let t = t.strip_prefix("pub ").unwrap_or(t);
        let name = if let Some(r) = t.strip_prefix("fn ") {
            r.split(|c| c == '(' || c == '[').next()
        } else if let Some(r) = t.strip_prefix("| ") {
            r.split(|c: char| !(c.is_alphanumeric() || c == '_')).next()
        } else if kind == "type" && base == 1 && t.contains(": ") && !t.starts_with("//") {
            t.split(':').next()
        } else {
            None
        };
        if let Some(n) = name {
            names.push(n.to_string());
        }
    }
    let n = hidden.len();
    if names.is_empty() {
        return format!("{n} line{} unchanged", if n == 1 { "" } else { "s" });
    }
    let shown: Vec<String> = names.iter().take(4).cloned().collect();
    let more = if names.len() > 4 { format!(" and {} more", names.len() - 4) } else { String::new() };
    let all = match shown.len() {
        1 => shown[0].clone(),
        _ => format!("{} and {}", shown[..shown.len() - 1].join(", "), shown[shown.len() - 1]),
    };
    format!("{all}{more} unchanged")
}

/// Keep changed lines, the first and last line, the lines that enclose each
/// change (its member's signature, its block's header) and the line after
/// it; fold the rest.
fn fold(lines: Vec<Line>, kind: &str) -> Vec<Json> {
    let n = lines.len();
    let mut keep = vec![false; n];
    if n > 0 {
        // the definition's first line (after any comments above it) and its last
        let head = lines.iter().position(|l| !l.text.trim_start().starts_with("//") && !l.text.trim().is_empty()).unwrap_or(0);
        keep[head] = true;
        keep[n - 1] = true;
    }
    for i in 0..n {
        if lines[i].s == ' ' {
            continue;
        }
        keep[i] = true;
        // the enclosing headers, out to the definition's first line (kept already)
        let mut lvl = indent(&lines[i].text);
        for j in (0..i).rev() {
            let t = &lines[j].text;
            if !t.trim().is_empty() && indent(t) < lvl && !t.trim_start().starts_with("//") {
                lvl = indent(t);
                if lvl == 0 {
                    break;
                }
                keep[j] = true;
            }
        }
        if i + 1 < n && lines[i + 1].s == ' ' && !lines[i + 1].text.trim_start().starts_with("///") && !lines[i + 1].text.trim().is_empty() {
            keep[i + 1] = true;
        }
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        if keep[i] {
            out.push(lines[i].json());
            i += 1;
            continue;
        }
        let start = i;
        while i < n && !keep[i] {
            i += 1;
        }
        let hidden = &lines[start..i];
        if hidden.len() == 1 {
            out.push(hidden[0].json());
        } else {
            out.push(json!({"s": "fold", "t": fold_label(hidden, kind), "hidden": hidden.iter().map(Line::json).collect::<Vec<_>>()}));
        }
    }
    out
}

// ---------- what changed, in words ----------

fn fn_sig(f: &FnDecl) -> String {
    let mut g = f.clone();
    g.requires.clear();
    g.ensures.clear();
    printer::fn_signature(&g)
}
fn exprs(es: &[Expr]) -> Vec<String> {
    es.iter().map(|e| printer::expr(e, 0)).collect()
}
fn body_text(f: &FnDecl) -> String {
    printer::expr(&f.body, 0)
}

/// Contract changes between two versions of a function, as words.
fn contract_words(o: &FnDecl, n: &FnDecl, out: &mut Vec<String>) {
    let (oe, ne, or, nr) = (exprs(&o.ensures), exprs(&n.ensures), exprs(&o.requires), exprs(&n.requires));
    if ne.iter().any(|e| !oe.contains(e)) && oe.iter().all(|e| ne.contains(e)) {
        out.push("stronger guarantee".into());
    } else if oe.iter().any(|e| !ne.contains(e)) && ne.iter().all(|e| oe.contains(e)) {
        out.push("weaker guarantee".into());
    } else if oe != ne {
        out.push("guarantee changed".into());
    }
    if nr.iter().any(|e| !or.contains(e)) && or.iter().all(|e| nr.contains(e)) {
        out.push("stricter precondition".into());
    } else if or.iter().any(|e| !nr.contains(e)) && nr.iter().all(|e| or.contains(e)) {
        out.push("looser precondition".into());
    } else if or != nr {
        out.push("precondition changed".into());
    }
}

fn list(kind: &str, names: &[String]) -> String {
    let s = if names.len() > 1 { "s" } else { "" };
    format!("{kind}{s} {}", names.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", "))
}

fn type_words(o: &TypeDecl, n: &TypeDecl, out: &mut Vec<String>) {
    let fields = |b: &TypeBody| -> Vec<(String, String)> {
        match b {
            TypeBody::Record(fs) => fs.iter().map(|f| (f.name.to_string(), printer::ty(&f.ty))).collect(),
            _ => vec![],
        }
    };
    let variants = |b: &TypeBody| -> Vec<(String, String)> {
        match b {
            TypeBody::Sum(vs) => vs.iter().map(|v| (v.name.to_string(), format!("{:?}", v.payload))).collect(),
            _ => vec![],
        }
    };
    for (what, get) in [("field", &fields as &dyn Fn(&TypeBody) -> Vec<(String, String)>), ("variant", &variants)] {
        let (a, b) = (get(&o.body), get(&n.body));
        let added: Vec<String> = b.iter().filter(|(x, _)| !a.iter().any(|(y, _)| y == x)).map(|(x, _)| x.clone()).collect();
        let removed: Vec<String> = a.iter().filter(|(x, _)| !b.iter().any(|(y, _)| y == x)).map(|(x, _)| x.clone()).collect();
        let changed: Vec<String> = b.iter().filter(|(x, t)| a.iter().any(|(y, u)| y == x && u != t)).map(|(x, _)| x.clone()).collect();
        if !added.is_empty() {
            out.push(format!("{} added", list(what, &added)));
        }
        if !removed.is_empty() {
            out.push(format!("{} removed", list(what, &removed)));
        }
        if !changed.is_empty() {
            out.push(format!("{} changed", list(what, &changed)));
        }
    }
    if std::mem::discriminant(&o.body) != std::mem::discriminant(&n.body) {
        out.push("shape changed".into());
    } else if let (TypeBody::Alias(a), TypeBody::Alias(b)) = (&o.body, &n.body) {
        if a != b {
            out.push("base type changed".into());
        }
    }
    let r = |t: &TypeDecl| t.refinement.as_ref().map(|e| printer::expr(e, 0));
    match (r(o), r(n)) {
        (None, Some(_)) => out.push("refinement added".into()),
        (Some(_), None) => out.push("refinement removed".into()),
        (a, b) if a != b => out.push("refinement changed".into()),
        _ => {}
    }
}

fn methods_words(o: &[(String, Option<&FnDecl>)], n: &[(String, Option<&FnDecl>)], out: &mut Vec<String>, contracts: bool) {
    let added: Vec<String> = n.iter().filter(|(x, _)| !o.iter().any(|(y, _)| y == x)).map(|(x, _)| x.clone()).collect();
    let removed: Vec<String> = o.iter().filter(|(x, _)| !n.iter().any(|(y, _)| y == x)).map(|(x, _)| x.clone()).collect();
    if !added.is_empty() {
        out.push(format!("{} added", list("method", &added)));
    }
    if !removed.is_empty() {
        out.push(format!("{} removed", list("method", &removed)));
    }
    let mut changed = Vec::new();
    for (name, nf) in n {
        let Some((_, of)) = o.iter().find(|(y, _)| y == name) else { continue };
        if let (Some(a), Some(b)) = (of, nf) {
            if contracts {
                let mut w = Vec::new();
                contract_words(a, b, &mut w);
                out.extend(w.into_iter().map(|x| format!("`{name}`: {x}")));
            }
            if fn_sig(a) != fn_sig(b) {
                out.push(format!("`{name}`: signature changed"));
            } else if body_text(a) != body_text(b) && !matches!(b.body, Expr::Unit) {
                changed.push(name.clone());
            }
        }
    }
    if !changed.is_empty() {
        out.push(format!("{} changed", list("method", &changed)));
    }
}

/// The change in words, and whether it is only its doc or comments.
fn words(o: &Decl, n: &Decl) -> Vec<String> {
    let mut out = Vec::new();
    match (&o.kind, &n.kind) {
        (DeclKind::Fn(a), DeclKind::Fn(b)) => {
            if fn_sig(a) != fn_sig(b) {
                out.push("signature changed".into());
            }
            contract_words(a, b, &mut out);
            if body_text(a) != body_text(b) {
                out.push("body changed".into());
            }
        }
        (DeclKind::Type(a), DeclKind::Type(b)) => type_words(a, b, &mut out),
        (DeclKind::Trait(a), DeclKind::Trait(b)) => {
            let ma: Vec<(String, Option<&FnDecl>)> = a.methods.iter().map(|m| (m.decl.name.to_string(), Some(&m.decl))).collect();
            let mb: Vec<(String, Option<&FnDecl>)> = b.methods.iter().map(|m| (m.decl.name.to_string(), Some(&m.decl))).collect();
            methods_words(&ma, &mb, &mut out, true);
            let la: Vec<&Symbol> = a.laws.iter().map(|l| &l.name).collect();
            let lb: Vec<&Symbol> = b.laws.iter().map(|l| &l.name).collect();
            if la != lb {
                out.push("laws changed".into());
            }
        }
        (DeclKind::Impl(a), DeclKind::Impl(b)) => {
            let ma: Vec<(String, Option<&FnDecl>)> = a.methods.iter().map(|m| (m.name.to_string(), Some(m))).collect();
            let mb: Vec<(String, Option<&FnDecl>)> = b.methods.iter().map(|m| (m.name.to_string(), Some(m))).collect();
            methods_words(&ma, &mb, &mut out, false);
        }
        (DeclKind::Test(_), DeclKind::Test(_)) => out.push("body changed".into()),
        (DeclKind::Use(a), DeclKind::Use(b)) if a != b => out.push("imports changed".into()),
        (DeclKind::Use(_), DeclKind::Use(_)) => {}
        _ => out.push("kind changed".into()),
    }
    if o.public != n.public {
        out.push(if n.public { "made public".into() } else { "made private".into() });
    }
    if out.is_empty() && doc_text(o) != doc_text(n) {
        out.push("doc changed".into());
    }
    if out.is_empty() && o.comments != n.comments {
        out.push("comments changed".into());
    }
    if out.is_empty() {
        out.push("changed".into());
    }
    out
}

/// Notes on lines: contracts made stronger or weaker, methods added.
fn annotate(lines: &mut [Line], old_code: &str, kind: &str, renames: &[(String, String)]) {
    let old: HashSet<&str> = old_code.lines().map(|l| l.trim()).collect();
    let old_methods: HashSet<String> = old_code.lines().filter_map(method_name).collect();
    // contracts of a method that is new are part of it, not a change
    let mut in_new_method = false;
    for l in lines.iter_mut() {
        let t = l.text.trim();
        if let Some(m) = method_name(&l.text) {
            in_new_method = !old_methods.contains(&m);
        } else if indent(&l.text) <= 1 && !t.starts_with("ensures ") && !t.starts_with("requires ") {
            in_new_method = false;
        }
        if in_new_method && (t.starts_with("ensures ") || t.starts_with("requires ")) {
            continue;
        }
        match l.s {
            '+' if t.starts_with("ensures ") && !old.contains(t) => l.note = Some("stronger guarantee"),
            '+' if t.starts_with("requires ") && !old.contains(t) => l.note = Some("stricter precondition"),
            '-' if t.starts_with("ensures ") => l.note = Some("weaker guarantee"),
            '-' if t.starts_with("requires ") => l.note = Some("looser precondition"),
            '~' if t.starts_with("ensures ") || t.starts_with("requires ") => l.note = Some("contract changed"),
            '+' if (kind == "trait" || kind == "impl") && method_name(&l.text).is_some_and(|m| !old_methods.contains(&m)) => l.note = Some("new method"),
            '~' => {
                // a line that only follows a rename
                let segs = l.segs.as_ref().unwrap();
                let changes: Vec<&(char, String)> = segs.iter().filter(|(k, _)| *k != '=').collect();
                let dels: Vec<&str> = changes.iter().filter(|(k, _)| *k == '-').map(|(_, s)| s.as_str()).collect();
                let ins: Vec<&str> = changes.iter().filter(|(k, _)| *k == '+').map(|(_, s)| s.as_str()).collect();
                let renamed = !dels.is_empty() && dels.len() == ins.len() && dels.iter().zip(&ins).all(|(d, i)| renames.iter().any(|(f, t)| short(f) == *d && short(t) == *i));
                if renamed {
                    l.note = Some("from the rename");
                }
            }
            _ => {}
        }
    }
}

fn method_name(l: &str) -> Option<String> {
    if indent(l) == 0 {
        return None;
    }
    let t = l.trim_start();
    let t = t.strip_prefix("pub ").unwrap_or(t);
    t.strip_prefix("fn ").and_then(|r| r.split(|c| c == '(' || c == '[').next()).map(|s| s.to_string())
}

/// Replace whole-word identifiers: the renamed definitions' short names.
fn substitute(text: &str, renames: &[(String, String)]) -> String {
    tokens(text).into_iter().map(|t| renames.iter().find(|(f, _)| short(f) == t).map_or(t, |(_, to)| short(to))).collect()
}

fn verdict_word(v: Option<&Verdict>) -> String {
    // "terminates → may not terminate" is a change worth showing
    let t = match v.and_then(|v| v.termination()) {
        Some(crate::termination::Termination::Unknown { .. }) => ", may not terminate",
        Some(crate::termination::Termination::Assumed { .. }) => ", terminates (assumed)",
        _ => "",
    };
    let w: String = match v {
        Some(Verdict::Proven { .. }) => "proven".into(),
        Some(Verdict::Assumed { .. }) => "assumed".into(),
        Some(Verdict::Fuzzed { cases, .. }) => format!("fuzzed {cases}"),
        Some(Verdict::Counterexample { .. }) => "counterexample".into(),
        Some(Verdict::Incomplete { .. }) => "incomplete".into(),
        Some(Verdict::Skipped { .. }) => "not checkable".into(),
        None => "unverified".into(),
    };
    w + t
}

fn section(kind: &str, mk: &str, interface_changed: bool) -> &'static str {
    match kind {
        "type" => "Types",
        "trait" | "use" => "Interfaces",
        "fn" if mk == "ren" || (mk == "chg" && interface_changed) => "Interfaces",
        "test" => "Tests",
        _ => "Implementations",
    }
}

const SECTIONS: &[(&str, &str)] = &[
    ("Types", "new fields, variants and shapes first: everything else builds on them"),
    ("Interfaces", "signatures and contracts: what other code can rely on"),
    ("Implementations", "bodies"),
    ("Tests", ""),
];

/// Follow chains (`a → b`, `b → c` is `a → c`) and keep renames from `old` to `new`.
fn effective_renames(old: &Program, new: &Program, renames: &[(String, String)]) -> Vec<(String, String)> {
    let olds: HashSet<String> = old.decls.iter().map(|d| d.qname()).collect();
    let news: HashSet<String> = new.decls.iter().map(|d| d.qname()).collect();
    let mut out = Vec::new();
    for (f, _) in renames {
        if !olds.contains(f) {
            continue;
        }
        let mut t = f.clone();
        for (a, b) in renames {
            if *a == t {
                t = b.clone();
            }
        }
        if t != *f && news.contains(&t) && !news.contains(f) {
            out.push((f.clone(), t));
        }
    }
    out
}

/// The structural diff from `old` to `new`.
pub fn diff(old: &Program, new: &Program, renames: &[(String, String)], steps: &StepNames, verdicts: &HashMap<String, Verdict>) -> Json {
    // renames of a variant, trait method or field: (from, to, kind, owning definition)
    let members: Vec<(String, String, &str, String)> = renames
        .iter()
        .filter_map(|(f, t)| {
            let (kind, _) = crate::resolve::member(old, f).filter(|(k, _)| matches!(*k, "variant" | "method" | "field"))?;
            crate::resolve::member(new, t)?;
            Some((f.clone(), t.clone(), kind, crate::resolve::owner(old, f)?))
        })
        .collect();
    let owners: HashSet<&str> = members.iter().map(|m| m.3.as_str()).collect();
    let renames = effective_renames(old, new, renames);
    // what code that only follows a rename looks like: definitions' and members' short names
    let substitutions: Vec<(String, String)> = renames.iter().cloned().chain(members.iter().map(|m| (m.0.clone(), m.1.clone()))).collect();
    let om: HashMap<String, &Decl> = old.decls.iter().map(|d| (d.qname(), d)).collect();
    let nm: HashMap<String, &Decl> = new.decls.iter().map(|d| (d.qname(), d)).collect();
    let renamed_to: HashMap<&str, &str> = renames.iter().map(|(f, t)| (t.as_str(), f.as_str())).collect();
    let renamed_from: HashSet<&str> = renames.iter().map(|(f, _)| f.as_str()).collect();
    let step_of = |names: &[&str]| -> Vec<usize> { steps.iter().filter(|(_, ns)| ns.iter().any(|n| names.contains(&n.as_str()))).map(|(k, _)| *k).collect() };

    // pairs (old, new) in the new program's order, then removals
    let mut pairs: Vec<(Option<&Decl>, Option<&Decl>)> = Vec::new();
    for d in &new.decls {
        let q = d.qname();
        match (om.get(&q), renamed_to.get(q.as_str())) {
            (Some(o), _) if store::self_hash(o) != store::self_hash(d) => pairs.push((Some(o), Some(d))),
            (Some(_), _) => {}
            (None, Some(f)) => pairs.push((om.get(*f).copied(), Some(d))),
            (None, None) => pairs.push((None, Some(d))),
        }
    }
    for d in &old.decls {
        if !nm.contains_key(&d.qname()) && !renamed_from.contains(d.qname().as_str()) {
            pairs.push((Some(d), None));
        }
    }

    // definitions that only follow a rename are folded into it
    let mut refs_of: HashMap<String, Vec<String>> = HashMap::default();
    let mut items: Vec<(String, Json)> = Vec::new();
    let mut ref_only: HashSet<String> = HashSet::default();
    if !substitutions.is_empty() {
        for (o, n) in &pairs {
            let (Some(o), Some(n)) = (o, n) else { continue };
            // the type or trait that declares a renamed member is shown, not folded
            if o.qname() != n.qname() || doc_text(o) != doc_text(n) || owners.contains(o.qname().as_str()) {
                continue;
            }
            let (oc, nc) = (code(o), code(n));
            for (f, t) in &substitutions {
                let (fs, ts) = (short(f), short(t));
                if tokens(&oc).contains(&fs) && tokens(&nc).contains(&ts) {
                    refs_of.entry(t.clone()).or_default().push(n.qname());
                }
            }
            if substitute(&oc, &substitutions) == nc {
                ref_only.insert(n.qname());
            }
        }
    }

    let graph = store::graph(new);
    let users_of = |q: &str| -> Vec<String> { graph.iter().filter(|(n, ds)| n.as_str() != q && ds.contains(q)).map(|(n, _)| n.clone()).collect() };
    let (mut added, mut removed, mut renamed, mut changed, mut iface) = (0, 0, 0, 0, 0);
    for (o, n) in &pairs {
        let d = n.or(*o).unwrap();
        let q = d.qname();
        if ref_only.contains(&q) {
            continue;
        }
        let kind = d.kind_str();
        let (mk, what, lines, interface_changed, rewritten) = match (o, n) {
            (None, Some(n)) => {
                added += 1;
                ("add", vec!["added".to_string()], code(n).lines().map(|l| Line::new('+', l).json()).collect::<Vec<_>>(), true, false)
            }
            (Some(o), None) => {
                removed += 1;
                ("del", vec!["removed".to_string()], code(o).lines().map(|l| Line::new('-', l).json()).collect(), true, false)
            }
            (Some(o), Some(n)) => {
                let is_rename = o.qname() != n.qname();
                let ic = is_rename || store::interface_hash(o) != store::interface_hash(n);
                let (oc, nc) = (code(o), code(n));
                let (mut ls, rewritten) = line_diff(&oc, &nc);
                annotate(&mut ls, &oc, kind, if is_rename { &[] } else { &renames });
                // a move keeps the short name and changes the module
                let moved = is_rename && short(&o.qname()) == short(&n.qname());
                let mut w = if moved {
                    vec![format!("moved from `{}`", if o.module.is_empty() { "main" } else { o.module.as_str() })]
                } else if is_rename {
                    vec![format!("renamed from `{}`", short(&o.qname()))]
                } else {
                    vec![]
                };
                // a rename alone changes only the name
                let mut rest = if is_rename { substitute(&oc, &renames) != nc || doc_text(o) != doc_text(n) } else { true };
                // a renamed member: say so, then whatever else changed, against the old
                // definition with the rename applied
                for (f, t, k, _) in members.iter().filter(|m| m.3 == q && !is_rename) {
                    w.push(format!("{k} `{}` renamed to `{}`", short(f), short(t)));
                    let o2 = crate::resolve::rename(old, f, short(t)).ok().and_then(|p| p.decls.into_iter().find(|d| d.qname() == q));
                    match o2 {
                        Some(o2) if code(&o2) == nc && doc_text(&o2) == doc_text(n) => {}
                        Some(o2) => w.extend(words(&o2, n)),
                        None => w.extend(words(o, n)),
                    }
                    rest = false;
                }
                if rest {
                    let ws = words(o, n);
                    if !(is_rename && ws == ["signature changed"] && substitute(&oc, &renames) == nc) {
                        w.extend(ws);
                    }
                }
                if is_rename {
                    renamed += 1;
                } else {
                    changed += 1;
                }
                if ic {
                    iface += 1;
                }
                (if is_rename { "ren" } else { "chg" }, w, fold(ls, kind), ic, rewritten)
            }
            _ => unreachable!(),
        };
        // the consequences, for the reader
        // what uses it directly (and how much depends on it in all)
        let deps = users_of(&q);
        let total = store::dependents(new, &q).len();
        let affected: Vec<Json> = if interface_changed && n.is_some() && o.is_some() && kind != "impl" {
            deps.iter().map(|x| json!({"name": x, "updated": pairs.iter().any(|(_, nn)| nn.is_some_and(|nn| &nn.qname() == x)) })).collect()
        } else {
            vec![]
        };
        let mut detail = Vec::new();
        if rewritten {
            detail.push("most of it changed, so it is shown as a line diff".to_string());
        }
        let only_renamed = mk == "ren" && what.len() == 1;
        let member_only = owners.contains(q.as_str()) && what.iter().all(|w| w.contains("` renamed to `"));
        if member_only {
            let r: usize = members.iter().filter(|m| m.3 == q).map(|m| refs_of.get(&m.1).map_or(0, |r| r.len())).sum();
            detail.push(format!("only a name changed · {r} reference{} updated", if r == 1 { "" } else { "s" }));
        } else if only_renamed {
            let r = refs_of.get(&q).map_or(0, |r| r.len());
            let what_changed = if what[0].starts_with("moved") { "its module" } else { "the name" };
            detail.push(format!("only {what_changed} changed · {r} reference{} updated", if r == 1 { "" } else { "s" }));
        } else if what == ["doc changed"] || what == ["comments changed"] {
            detail.push(format!("only the {} changed · interface unchanged", if what[0].starts_with("doc") { "doc" } else { "comments" }));
        } else if mk == "chg" && kind == "impl" {
            if let DeclKind::Impl(i) = &d.kind {
                detail.push(if i.is_inherent() {
                    format!("methods of `{}` · checked against the contracts of the traits they serve", short(&crate::types::texpr_head(&i.for_type)))
                } else {
                    format!("implementation of `{}` · checked against its contracts", short(&i.trait_name))
                });
            }
        } else if mk == "chg" || mk == "ren" {
            if kind == "trait" && what.iter().any(|w| w.contains("added") || w.contains("stronger")) {
                let impls = new.decls.iter().filter(|x| matches!(&x.kind, DeclKind::Impl(i) if short(&i.trait_name) == short(&q))).count();
                detail.push(format!("every implementation must follow · {impls} in the codebase"));
            } else if interface_changed && kind != "test" {
                detail.push(if deps.is_empty() {
                    "interface changed · nothing uses it".into()
                } else {
                    format!("interface changed · used by {}{}", deps.len(), if total > deps.len() { format!(", {total} depend on it in all") } else { String::new() })
                });
            } else if kind != "test" {
                detail.push("implementation only · nothing else needs re-checking".into());
            }
        }
        if mk == "add" {
            if let Some(doc) = n.filter(|n| !n.doc.is_empty()) {
                detail.push(doc_text(doc));
            }
        }
        let doc_diff = match (o, n) {
            (Some(o), Some(n)) if doc_text(o) != doc_text(n) => Some(segs_json(&segments(&doc_text(o), &doc_text(n)))),
            _ => None,
        };
        let owned: Vec<String> = [o.map(|x| x.qname()), n.map(|x| x.qname())].into_iter().flatten().collect();
        let name_refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
        let verdict = {
            let before = o.map(|o| verdict_word(verdicts.get(&store::closure_hash(old, &o.qname()))));
            let after = n.map(|n| verdict_word(verdicts.get(&store::closure_hash(new, &n.qname()))));
            json!({"before": before, "after": after})
        };
        let sect = section(kind, mk, interface_changed);
        let label = match (o, n) {
            (Some(o), Some(n)) if o.qname() != n.qname() => format!("{} → {}", short_name(o), short_name(n)),
            _ => short_name(d),
        };
        items.push((
            sect.to_string(),
            json!({
                "id": q, "old_id": o.map(|o| o.qname()), "label": label, "kind": kind, "module": d.module,
                "mk": mk, "what": what, "detail": detail, "doc_diff": doc_diff, "doc": n.map(doc_text),
                "lines": lines, "rewritten": rewritten, "interface_changed": interface_changed,
                "affected": affected, "affected_total": if affected.is_empty() { 0 } else { total }, "refs": refs_of.get(&q).cloned().unwrap_or_default(),
                "refs_folded": refs_of.get(&q).map(|r| r.iter().filter(|x| ref_only.contains(*x)).cloned().collect::<Vec<_>>()).unwrap_or_default(),
                "steps": step_of(&name_refs), "verdict": verdict,
            }),
        ));
    }

    // verification before and after: what changed, and dependents whose result changed
    let category = |v: Option<&Verdict>| std::mem::discriminant(&v.cloned().unwrap_or(Verdict::Skipped { reason: String::new() }));
    let mut verification = Vec::new();
    let mut rechecked_same = 0;
    for d in &new.decls {
        let q = d.qname();
        let oq = renamed_to.get(q.as_str()).map(|s| s.to_string()).unwrap_or(q.clone());
        let before_hash = om.get(&oq).map(|_| store::closure_hash(old, &oq));
        let after_hash = store::closure_hash(new, &q);
        if before_hash.as_deref() == Some(after_hash.as_str()) {
            continue;
        }
        let (b, a) = (before_hash.as_ref().and_then(|h| verdicts.get(h)), verdicts.get(&after_hash));
        if b.is_none() && a.is_none() && !(before_hash.is_none() && matches!(d.kind, DeclKind::Fn(_))) {
            continue;
        }
        let changed_here = pairs.iter().any(|(_, n)| n.is_some_and(|n| n.qname() == q));
        let why = if before_hash.is_none() {
            "new definition".to_string()
        } else if changed_here {
            "changed".to_string()
        } else {
            // a dependent: only worth a row if both results are known and differ
            match (b, a) {
                (Some(_), Some(_)) if category(b) == category(a) => {
                    rechecked_same += 1;
                    continue;
                }
                (Some(_), Some(_)) => "something it depends on changed".to_string(),
                _ => continue,
            }
        };
        let after = if a.is_none() && before_hash.is_some() { "not verified yet".to_string() } else { verdict_word(a) };
        verification.push(json!({"name": q, "before": before_hash.map(|_| verdict_word(b)), "after": after, "why": why}));
    }

    let sections: Vec<Json> = SECTIONS
        .iter()
        .map(|(s, why)| json!({"name": s, "why": why, "items": items.iter().filter(|(x, _)| x == s).map(|(_, j)| j.clone()).collect::<Vec<_>>()}))
        .filter(|s| !s["items"].as_array().unwrap().is_empty())
        .collect();
    json!({
        "summary": {"added": added, "removed": removed, "renamed": renamed, "changed": changed, "interface_changes": iface, "follow_renames": ref_only.len()},
        "renames": renames.iter().map(|(f, t)| json!({"from": f, "to": t})).collect::<Vec<_>>(),
        "sections": sections,
        "verification": verification,
        "rechecked_same": rechecked_same,
    })
}

fn short_name(d: &Decl) -> String {
    match &d.kind {
        DeclKind::Test(t) => format!("test {:?}", t.name),
        _ => d.name(),
    }
}

// ---------- text, for the terminal and for agents ----------

pub fn render_text(j: &Json) -> String {
    let mut out = String::new();
    let s = &j["summary"];
    let n = |k: &str| s[k].as_u64().unwrap_or(0);
    let mut parts = Vec::new();
    for (k, w) in [("renamed", "renamed"), ("added", "added"), ("removed", "removed"), ("changed", "changed")] {
        if n(k) > 0 {
            parts.push(format!("{} {w}", n(k)));
        }
    }
    if parts.is_empty() {
        return "no changes\n".into();
    }
    out += &format!("{}  ·  {} interface change{}\n", parts.join(", "), n("interface_changes"), if n("interface_changes") == 1 { "" } else { "s" });
    for sec in j["sections"].as_array().into_iter().flatten() {
        out += &format!("\n{}\n", sec["name"].as_str().unwrap_or("").to_uppercase());
        for it in sec["items"].as_array().into_iter().flatten() {
            let mk = match it["mk"].as_str() {
                Some("add") => "+",
                Some("del") => "-",
                Some("ren") => "↪",
                _ => "~",
            };
            let what: Vec<&str> = it["what"].as_array().into_iter().flatten().filter_map(|w| w.as_str()).collect();
            out += &format!("\n{mk} {}  ({})\n", it["id"].as_str().unwrap_or(""), what.join(", "));
            for d in it["detail"].as_array().into_iter().flatten() {
                out += &format!("    {}\n", d.as_str().unwrap_or(""));
            }
            if let Some(dd) = it["doc_diff"].as_array() {
                out += &format!("    doc: {}\n", text_segs(dd));
            }
            for l in it["lines"].as_array().into_iter().flatten() {
                let t = l["t"].as_str().unwrap_or("");
                let note = l["note"].as_str().map(|n| format!("    ◀ {n}")).unwrap_or_default();
                match l["s"].as_str().unwrap_or(" ") {
                    "fold" => out += &format!("    … {t}\n"),
                    "~" => out += &format!("  ~ {}{note}\n", text_segs(l["segs"].as_array().unwrap())),
                    k => out += &format!("  {k} {t}{note}\n"),
                }
            }
            let refs: Vec<&str> = it["refs"].as_array().into_iter().flatten().filter_map(|r| r.as_str()).collect();
            if !refs.is_empty() {
                out += &format!("    references updated: {}\n", refs.join(", "));
            }
        }
    }
    let v = j["verification"].as_array().cloned().unwrap_or_default();
    let same = j["rechecked_same"].as_u64().unwrap_or(0);
    if !v.is_empty() || same > 0 {
        out += "\nVERIFICATION\n";
        for x in v {
            out += &format!("  {:<28} {} → {}   ({})\n", x["name"].as_str().unwrap_or(""), x["before"].as_str().unwrap_or("new"), x["after"].as_str().unwrap_or(""), x["why"].as_str().unwrap_or(""));
        }
        if same > 0 {
            out += &format!("  and {same} definition{} that depend on these, re-verified with the same result\n", if same == 1 { "" } else { "s" });
        }
    }
    out
}

fn text_segs(segs: &[Json]) -> String {
    segs.iter()
        .map(|s| {
            let (k, t) = (s[0].as_str().unwrap_or("="), s[1].as_str().unwrap_or(""));
            match k {
                "-" => format!("[-{t}-]"),
                "+" => format!("{{+{t}+}}"),
                _ => t.to_string(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mostly_changed_spans_are_one_replacement() {
        // only spaces in common between the edits: one replacement, not word by word
        let s = segments("// x\"). Strata values are immutable, so a stateful backend", "// x\"). Methods that change a backend say `mut self`");
        assert_eq!(s.iter().filter(|(k, _)| *k == '-').count(), 1, "{s:?}");
        assert_eq!(s.iter().filter(|(k, _)| *k == '+').count(), 1, "{s:?}");
        assert_eq!(s[0], ('=', "// x\"). ".to_string()));
        // a small edit stays small
        let s = segments("if len(xs) == 0 {", "if len(ys) == 0 {");
        assert_eq!(s, vec![('=', "if len(".into()), ('-', "xs".into()), ('+', "ys".into()), ('=', ") == 0 {".into())]);
    }

    #[test]
    fn rewrapped_comments_are_lines_removed_and_added() {
        let old = "// object changes\"). Strata values are immutable, so a stateful backend\n// returns its next state from each operation.\ntype T = Int";
        let new = "// object changes\"). Methods that change a backend say `mut self`.\ntype T = Int";
        let (lines, _) = line_diff(old, new);
        let kinds: Vec<char> = lines.iter().map(|l| l.s).collect();
        assert_eq!(kinds, vec!['-', '-', '+', ' '], "{lines:?}");
        // a light touch-up of a comment is still marked in place
        let (lines, _) = line_diff("// claim the next batch\nfn f() -> Int", "// claim the next batches\nfn f() -> Int");
        assert_eq!(lines[0].s, '~', "{lines:?}");
    }
}
