//! Rendering the tree back to text. The same printer produces the canonical
//! form (used for hashing) and every level-of-detail view.

use crate::ast::*;

/// A string literal's contents, written so the lexer reads them back the
/// same: `\n`, `\t` and `"` are escaped, and so is a backslash that would
/// otherwise start one of those escapes (others, like regex `\d`, stay as is).
fn escape(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '"' => out.push_str("\\\""),
            '\\' if it.peek().is_none_or(|n| matches!(n, 'n' | 't' | '"' | '\\')) => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out
}

pub fn ty(t: &TypeExpr) -> String {
    match t {
        TypeExpr::Unit => "()".into(),
        TypeExpr::Named(n, args) if n == "Tuple" => format!("({})", args.iter().map(ty).collect::<Vec<_>>().join(", ")),
        TypeExpr::Named(n, args) if args.is_empty() => n.to_string(),
        TypeExpr::Named(n, args) => format!("{n}[{}]", args.iter().map(ty).collect::<Vec<_>>().join(", ")),
        TypeExpr::Fn(ps, r) => format!("({}) -> {}", ps.iter().map(ty).collect::<Vec<_>>().join(", "), ty(r)),
        TypeExpr::Refined(b, p) => format!("{} where {}", ty(b), expr(p, 0)),
        // a refined or function member needs parentheses, or `where` / `->` would take in the rest
        TypeExpr::Union(ms) => ms
            .iter()
            .map(|m| match m {
                TypeExpr::Refined(..) | TypeExpr::Fn(..) | TypeExpr::Union(_) => format!("({})", ty(m)),
                _ => ty(m),
            })
            .collect::<Vec<_>>()
            .join(" | "),
    }
}

fn has_comments(cs: &[String]) -> bool {
    !cs.is_empty()
}

/// `//` lines at depth `d`, each ending in a newline ("" is a blank line).
pub fn comments(cs: &[String], d: usize) -> String {
    if !has_comments(cs) {
        return String::new();
    }
    cs.iter()
        .map(|c| match c.as_str() {
            "" => "\n".to_string(),
            t => format!("{}{t}\n", ind(d)),
        })
        .collect()
}

/// Whether a field has comments or docs (so its record is written one field per line).
fn annotated(f: &Field) -> bool {
    has_comments(&f.comments) || has_comments(&f.doc)
}

/// Fields one per line, each under its comments and docs.
fn fields_broken(fs: &[Field], d: usize) -> String {
    let mut s = "{\n".to_string();
    for f in fs {
        s += &comments(&f.comments, d + 1);
        if has_comments(&f.doc) {
            s += &f.doc.iter().map(|l| format!("{}/// {l}\n", ind(d + 1))).collect::<String>();
        }
        s += &format!("{}{},\n", ind(d + 1), field(f));
    }
    s + &ind(d) + "}"
}

fn fields(fs: &[Field]) -> String {
    fs.iter().map(field).collect::<Vec<_>>().join(", ")
}

/// `name: T`, `times: Int = 1`
fn field(f: &Field) -> String {
    match &f.default {
        Some(x) => format!("{}: {} = {}", f.name, ty(&f.ty), expr(x, 1)),
        None => format!("{}: {}", f.name, ty(&f.ty)),
    }
}

fn ind(n: usize) -> String {
    "  ".repeat(n)
}

pub fn pat(p: &Pattern) -> String {
    match p {
        Pattern::Wild => "_".into(),
        // the arm's body goes between: see `arm`
        Pattern::For(p, _, _) => pat(p),
        Pattern::Bind(n) => n.to_string(),
        Pattern::Int(n) => n.to_string(),
        Pattern::Tuple(ps) => format!("({})", ps.iter().map(pat).collect::<Vec<_>>().join(", ")),
        Pattern::Str(s) => format!("{s:?}"),
        Pattern::Bool(b) => b.to_string(),
        Pattern::Cond(c) => format!("where {}", expr(c, 0)),
        Pattern::Or(ps) => ps.iter().map(pat).collect::<Vec<_>>().join(" | "),
        Pattern::Ctor(c, CtorPat::None) => c.to_string(),
        Pattern::Ctor(c, CtorPat::Positional(ps)) => {
            format!("{c}({})", ps.iter().map(pat).collect::<Vec<_>>().join(", "))
        }
        Pattern::Ctor(c, CtorPat::Type(ps)) => {
            format!("{c}[{}]", ps.iter().map(pat).collect::<Vec<_>>().join(", "))
        }
        Pattern::Ctor(c, CtorPat::Record(fs, open)) => {
            let mut parts: Vec<String> = fs
                .iter()
                .map(|(f, p)| match p {
                    Pattern::Bind(b) if b == f => f.to_string(),
                    _ => format!("{f}: {}", pat(p)),
                })
                .collect();
            if *open {
                parts.push("..".into());
            }
            format!("{c} {{ {} }}", parts.join(", "))
        }
        Pattern::List(items, rest) => {
            let mut parts: Vec<String> = items.iter().map(pat).collect();
            if let Some(r) = rest {
                parts.push(if r == "_" { "..".into() } else { format!("..{r}") });
            }
            format!("[{}]", parts.join(", "))
        }
    }
}

fn prec(e: &Expr) -> u8 {
    match e {
        Expr::Bin(op, ..) => match op {
            BinOp::Implies => 0,
            BinOp::Or => 1,
            BinOp::And => 2,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => 3,
            BinOp::Add | BinOp::Sub => 5,
            BinOp::Mul | BinOp::Div | BinOp::Mod => 6,
        },
        Expr::Is(..) => 3,
        Expr::Lambda(..) | Expr::Return(..) | Expr::Range(..) => 0,
        Expr::Except(..) | Expr::Or(..) if shorthand(e) => 4,
        Expr::Except(..) | Expr::Or(..) => 7,
        Expr::Un(..) | Expr::Mut(..) => 7,
        _ => 8,
    }
}

/// `x or { v }` is written `x ?? v`, and `x except { v }` is `x !! v`, when
/// the fallback is a plain value on one line: no error name, no block, no
/// `return`.
fn shorthand(e: &Expr) -> bool {
    let fb = match e {
        Expr::Except(_, None, fb) | Expr::Or(_, fb) => fb,
        _ => return false,
    };
    !matches!(**fb, Expr::Block(..) | Expr::If(..) | Expr::Match(..) | Expr::Return(..) | Expr::Lambda(..)) && !expr(fb, 0).contains('\n')
}

fn wrap(e: &Expr, min: u8, d: usize) -> String {
    let s = expr(e, d);
    if prec(e) < min { format!("({s})") } else { s }
}

fn args(es: &[Expr], d: usize) -> String {
    es.iter()
        .map(|e| match e {
            // `f((mut q.pop()), x)`: the call changes `q`; `f(mut q)` would pass `q` to a `mut` parameter
            Expr::Mut(x) if matches!(**x, Expr::Method(..)) => format!("({})", expr(e, d)),
            _ => expr(e, d),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn float(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 { format!("{f:.1}") } else { format!("{f}") }
}

/// Target line width for rendered code.
pub const WIDTH: usize = 80;

/// Would this single-line text overflow the width at indent depth `d`?
/// (12 columns are kept for whatever precedes it on the line, e.g. `x := `.)
fn too_long(s: &str, d: usize) -> bool {
    let mut lines = s.lines();
    lines.next().map_or(0, |l| l.chars().count()) + 2 * d + 12 > WIDTH || lines.any(|l| l.chars().count() > WIDTH)
}

/// Items one per line between `open` and `close`, with trailing commas.
fn broken(open: &str, items: Vec<String>, close: &str, d: usize) -> String {
    let mut s = format!("{open}\n");
    for it in items {
        s += &format!("{}{it},\n", ind(d + 1));
    }
    s + &ind(d) + close
}

/// Render an expression, breaking lines so the result stays within ~80 columns.
pub fn expr(e: &Expr, d: usize) -> String {
    let inline = expr_inline(e, d);
    // a chain whose parts had to break is laid out one item per line
    let chain = matches!(e, Expr::Method(..) | Expr::Bin(BinOp::And | BinOp::Or | BinOp::Add, ..));
    if !too_long(&inline, d) && !(chain && inline.contains('\n')) {
        return inline;
    }
    match e {
        // a.b().c().d()  ->  one call per line
        Expr::Method(..) => {
            let mut chain = Vec::new();
            let mut cur = e;
            while let Expr::Method(r, m, es) = cur {
                chain.push((m, es));
                cur = r;
            }
            if chain.len() < 2 {
                let (m, es) = chain[0];
                return format!("{}.{m}{}", wrap(cur, 8, d), broken("(", es.iter().map(|a| expr(a, d + 1)).collect(), ")", d));
            }
            let mut s = wrap(cur, 8, d);
            for (m, es) in chain.into_iter().rev() {
                s += &format!("\n{}.{m}({})", ind(d + 1), args(es, d + 1));
            }
            s
        }
        Expr::Call(f, es) if !es.is_empty() => format!("{}{}", wrap(f, 8, d), broken("(", es.iter().map(|a| expr(a, d + 1)).collect(), ")", d)),
        Expr::List(es) if !es.is_empty() => broken("[", es.iter().map(|a| expr(a, d + 1)).collect(), "]", d),
        Expr::Tuple(es) => broken("(", es.iter().map(|a| expr(a, d + 1)).collect(), ")", d),
        Expr::Ctor(c, CtorArgs::Record(fs)) => broken(
            &format!("{c} {{"),
            fs.iter()
                .map(|(f, x)| match x {
                    Expr::Var(v) if v == f => f.to_string(),
                    _ => format!("{f}: {}", expr(x, d + 1)),
                })
                .collect(),
            "}",
            d,
        ),
        Expr::Ctor(c, CtorArgs::Positional(es)) => format!("{c}{}", broken("(", es.iter().map(|a| expr(a, d + 1)).collect(), ")", d)),
        // x -> long body  ->  body on its own line (a block body breaks by itself)
        Expr::Lambda(ps, b) if !matches!(**b, Expr::Block(..)) => format!("{} ->\n{}{}", lambda_params(ps), ind(d + 1), expr(b, d + 1)),
        // a && b && c, "x" + y + "z"  ->  one operand per line
        Expr::Bin(op @ (BinOp::And | BinOp::Or | BinOp::Add), _, _) => {
            let mut parts = Vec::new();
            let mut cur = e;
            while let Expr::Bin(o, l, r) = cur {
                if o != op {
                    break;
                }
                parts.push(r);
                cur = l;
            }
            let mut s = wrap(cur, prec(e), d + 1);
            for p in parts.into_iter().rev() {
                s += &format!("\n{}{} {}", ind(d + 1), op.symbol(), wrap(p, prec(e) + 1, d + 1));
            }
            s
        }
        _ => inline,
    }
}

/// `x`, `(a, b)`, `(s: String)`, `()`
fn lambda_params(ps: &[LamParam]) -> String {
    match ps {
        [p] if p.ty.is_none() => p.name.to_string(),
        _ => {
            let parts: Vec<String> = ps
                .iter()
                .map(|p| match &p.ty {
                    Some(t) => format!("{}: {}", p.name, ty(t)),
                    None => p.name.to_string(),
                })
                .collect();
            format!("({})", parts.join(", "))
        }
    }
}

fn expr_inline(e: &Expr, d: usize) -> String {
    match e {
        Expr::Int(n) => n.to_string(),
        Expr::BigInt(digits) => digits.clone(),
        Expr::Float(f) => float(*f),
        Expr::Tuple(es) => format!("({})", args(es, d)),
        Expr::Str(s) => format!("\"{}\"", escape(s)),
        Expr::Bool(b) => b.to_string(),
        Expr::Unit => "()".into(),
        Expr::Var(v) => v.to_string(),
        Expr::Hole(h) => format!("?{h}"),
        Expr::List(es) => format!("[{}]", args(es, d)),
        Expr::Ctor(c, CtorArgs::None) => c.to_string(),
        Expr::Ctor(c, CtorArgs::Positional(es)) => format!("{c}({})", args(es, d)),
        Expr::Ctor(c, CtorArgs::Record(fs)) if fs.is_empty() => format!("{c} {{}}"),
        Expr::Ctor(c, CtorArgs::Record(fs)) => {
            let parts: Vec<String> = fs
                .iter()
                .map(|(f, e)| match e {
                    Expr::Var(v) if v == f => f.to_string(),
                    _ => format!("{f}: {}", expr(e, d)),
                })
                .collect();
            format!("{c} {{ {} }}", parts.join(", "))
        }
        Expr::Ctor(c, CtorArgs::Update(base, fs)) => {
            let mut parts = vec![format!("..{}", expr(base, d))];
            parts.extend(fs.iter().map(|(f, e)| match e {
                Expr::Var(v) if v == f => f.to_string(),
                _ => format!("{f}: {}", expr(e, d)),
            }));
            format!("{c} {{ {} }}", parts.join(", "))
        }
        Expr::Call(f, es) => format!("{}({})", wrap(f, 8, d), args(es, d)),
        Expr::Method(r, m, es) => format!("{}.{m}({})", wrap(r, 8, d), args(es, d)),
        Expr::Field(r, f) => format!("{}.{f}", wrap(r, 8, d)),
        Expr::Lambda(ps, b) => format!("{} -> {}", lambda_params(ps), expr(b, d)),
        Expr::Bin(op, l, r) => {
            let p = prec(e);
            if *op == BinOp::Implies {
                format!("{} ==> {}", wrap(l, p + 1, d), wrap(r, p, d))
            } else {
                format!("{} {} {}", wrap(l, p, d), op.symbol(), wrap(r, p + 1, d))
            }
        }
        Expr::Un(UnOp::Neg, x) => format!("-{}", wrap(x, 7, d)),
        Expr::Un(UnOp::Not, x) => format!("!{}", wrap(x, 7, d)),
        Expr::Is(x, c) => format!("{} is {c}", wrap(x, 4, d)),
        Expr::If(c, t, f) => {
            let mut s = format!("if {} {}", expr(c, d), block(t, d));
            match &**f {
                Expr::Unit => {}
                Expr::If(..) => s += &format!(" else {}", expr(f, d)),
                _ => s += &format!(" else {}", block(f, d)),
            }
            s
        }
        Expr::Match(s, name, arms) => {
            let as_name = name.as_ref().map(|n| format!(" as {n}")).unwrap_or_default();
            let mut out = format!("match {}{as_name} {{\n", expr(s, d));
            for a in arms {
                out += &comments(&a.comments, d + 1);
                let each = match &a.pat {
                    Pattern::For(_, x, xs) => format!(" for {} in {}", pat(x), expr(xs, d + 1)),
                    _ => String::new(),
                };
                out += &format!("{}{} => {}{each},\n", ind(d + 1), pat(&a.pat), expr(&a.body, d + 1));
            }
            out + &ind(d) + "}"
        }
        Expr::Block(..) => block(e, d),
        Expr::Mock(t) => format!("mock {}", ty(t)),
        Expr::Mut(x) => format!("mut {}", expr(x, d)),
        Expr::Comptime(x) => format!("comptime {}", expr(x, d)),
        Expr::Describe(t) => format!("${}", ty(t)),
        Expr::Interp(parts) => {
            let mut s = "f\"".to_string();
            for p in parts {
                match p {
                    StrPart::Lit(l) => s += &escape(l).replace('{', "{{").replace('}', "}}"),
                    StrPart::Expr(e, spec) => s += &format!("{{{}{}}}", expr_inline(e, 0), spec.as_ref().map(|f| format!(":{f}")).unwrap_or_default()),
                }
            }
            s + "\""
        }
        Expr::Range(lo, hi, inclusive) => {
            let end = |x: &Option<Box<Expr>>| x.as_ref().map(|x| wrap(x, 1, d)).unwrap_or_default();
            format!("{}{}{}", end(lo), if *inclusive { "..=" } else { ".." }, end(hi))
        }
        Expr::Index(x, r) => format!("{}[{}]", wrap(x, 8, d), expr(r, d)),
        Expr::Comp(kind, elem, gens) => {
            let (open, close) = match kind {
                CompKind::List => ("[".to_string(), "]"),
                CompKind::Set => ("Set { ".to_string(), " }"),
                CompKind::Dict => ("Dict { ".to_string(), " }"),
            };
            let mut s = open
                + &match (kind, &**elem) {
                    (CompKind::Dict, Expr::Tuple(kv)) if kv.len() == 2 => format!("{}: {}", expr(&kv[0], d), expr(&kv[1], d)),
                    _ => expr(elem, d),
                };
            for g in gens {
                s += &format!(" for {} in {}", pat(&g.pat), expr(&g.iter, d));
                if let Some(c) = &g.cond {
                    s += &format!(" where {}", expr(c, d));
                }
            }
            s + close
        }
        Expr::Dict(tys, kvs) => {
            let entries: Vec<String> = kvs.iter().map(|(k, v)| format!("{}: {}", expr(k, d), expr(v, d))).collect();
            collection("Dict", tys, &entries)
        }
        Expr::Set(tys, xs) => collection("Set", tys, &xs.iter().map(|x| expr(x, d)).collect::<Vec<_>>()),
        Expr::Return(x) if **x == Expr::Unit => "return".into(),
        Expr::Return(x) => format!("return {}", expr(x, d)),
        Expr::Try(x, k) => format!("{}{}", wrap(x, 8, d), if *k == TryKind::Result { "!" } else { "?" }),
        Expr::Except(x, _, fb) if shorthand(e) => format!("{} !! {}", wrap(x, 5, d), wrap(fb, 4, d)),
        Expr::Or(x, fb) if shorthand(e) => format!("{} ?? {}", wrap(x, 5, d), wrap(fb, 4, d)),
        Expr::Except(_, _, fb) | Expr::Or(_, fb) => {
            let (x, keyword) = match e {
                Expr::Except(x, Some(n), _) => (x, format!("except {n}")),
                Expr::Except(x, None, _) => (x, "except".to_string()),
                Expr::Or(x, _) => (x, "or".to_string()),
                _ => unreachable!(),
            };
            let head = format!("{} {keyword}", wrap(x, 8, d));
            // a short fallback stays on the line: `load(p) except { default() }`
            let short = match &**fb {
                Expr::Block(..) => None,
                e => Some(expr(e, d + 1)).filter(|s| !s.contains('\n') && !too_long(&format!("{head} {{ {s} }}"), d)),
            };
            match short {
                Some(s) => format!("{head} {{ {s} }}"),
                None => format!("{head} {}", block(fb, d)),
            }
        }
    }
}

/// `Dict { "a": 1 }`, `Set[Int] {}`
fn collection(name: &str, tys: &[TypeExpr], entries: &[String]) -> String {
    let targs = if tys.is_empty() { String::new() } else { format!("[{}]", tys.iter().map(ty).collect::<Vec<_>>().join(", ")) };
    if entries.is_empty() {
        format!("{name}{targs} {{}}")
    } else {
        format!("{name}{targs} {{ {} }}", entries.join(", "))
    }
}

/// Render an expression as a `{ ... }` block.
pub fn block(e: &Expr, d: usize) -> String {
    let (stmts, last): (&[Stmt], &Expr) = match e {
        Expr::Block(s, l) => (s, l),
        other => (&[], other),
    };
    let mut out = "{\n".to_string();
    for s in stmts {
        if let Stmt::Comment(cs) = s {
            // "" is a blank line; several in a row print as one
            let mut cs = cs.clone();
            cs.dedup_by(|a, b| a.is_empty() && b.is_empty());
            out += &comments(&cs, d + 1);
            continue;
        }
        out += &ind(d + 1);
        out += &match s {
            Stmt::Let(p, t, e) => match t {
                Some(t) => format!("let {}: {} = {}", pat(p), ty(t), expr(e, d + 1)),
                None => format!("{} := {}", pat(p), expr(e, d + 1)),
            },
            Stmt::Var(n, t, e) => match t {
                Some(t) => format!("mut {n}: {} = {}", ty(t), expr(e, d + 1)),
                None => format!("mut {n} := {}", expr(e, d + 1)),
            },
            Stmt::Assign(p, e) => match e {
                // `x = x + e` is written `x += e`
                Expr::Bin(op @ (BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod), l, r) if **l == *p => {
                    format!("{} {}= {}", expr(p, d + 1), op.symbol(), expr(r, d + 1))
                }
                _ => format!("{} = {}", expr(p, d + 1), expr(e, d + 1)),
            },
            Stmt::While(c, b) => format!("while {} {}", expr(c, d + 1), block(b, d + 1)),
            Stmt::Loop(b) => format!("loop {}", block(b, d + 1)),
            Stmt::Break => "break".into(),
            Stmt::Continue => "continue".into(),
            Stmt::For(p, it, b) => format!("for {} in {} {}", pat(p), expr(it, d + 1), block(b, d + 1)),
            Stmt::ComptimeFor(p, it, b) => format!("comptime for {} in {} {}", pat(p), expr(it, d + 1), block(b, d + 1)),
            Stmt::Expr(e) => expr(e, d + 1),
            Stmt::Assert(e) => format!("assert {}", expr(e, d + 1)),
            Stmt::Comment(_) => unreachable!(),
        };
        out += "\n";
    }
    // a trailing `()` is implied after `let`/`assert`, but not after an expression statement
    let last_stmt = stmts.iter().rev().find(|s| !matches!(s, Stmt::Comment(_)));
    if *last != Expr::Unit || last_stmt.is_none() || matches!(last_stmt, Some(Stmt::Expr(_))) {
        out += &format!("{}{}\n", ind(d + 1), expr(last, d + 1));
    }
    out + &ind(d) + "}"
}

pub fn type_decl(t: &TypeDecl) -> String {
    let tp = if t.params.is_empty() { String::new() } else { format!("[{}]", crate::symbol::join(&t.params, ", ")) };
    let mut s = format!("{}type {}{} =", if t.distinct { "distinct " } else { "" }, t.name, tp);
    if !matches!(t.body, TypeBody::Sum(_)) {
        s.push(' ');
    }
    match &t.body {
        TypeBody::Alias(a) => s += &ty(a),
        TypeBody::Record(fs) => {
            let one = format!("{{ {} }}", fields(fs));
            if s.chars().count() + one.chars().count() > WIDTH || fs.iter().any(annotated) {
                s += &fields_broken(fs, 0);
            } else {
                s += &one;
            }
        }
        TypeBody::Sum(vs) => {
            for v in vs {
                s += "\n";
                s += comments(&v.comments, 1).trim_end_matches('\n');
                if has_comments(&v.comments) {
                    s += "\n";
                }
                s += "  | ";
                s += &v.name;
                match &v.payload {
                    Payload::None => {}
                    Payload::Positional(ts) => {
                        s += &format!("({})", ts.iter().map(ty).collect::<Vec<_>>().join(", "))
                    }
                    Payload::Record(fs) if fs.iter().any(annotated) =>s += &format!(" {}", fields_broken(fs, 1)),
                    Payload::Record(fs) => s += &format!(" {{ {} }}", fields(fs)),
                }
            }
        }
    }
    if let Some(r) = &t.refinement {
        s += &format!("\n  where {}", expr(r, 1));
    }
    s
}

fn bounded(names: &[Symbol], bounds: &[Bound]) -> String {
    if names.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = names
        .iter()
        .map(|n| match bounds.iter().find(|b| &b.param == n) {
            Some(b) => format!("{n}: {}", b.traits.iter().map(ty).collect::<Vec<_>>().join(" + ")),
            None => n.to_string(),
        })
        .collect();
    format!("[{}]", parts.join(", "))
}

fn param_list(ps: &[Param]) -> String {
    ps.iter()
        .map(|p| {
            let m = match (p.comptime, p.mutable) {
                (true, true) => "comptime mut ",
                (true, false) => "comptime ",
                (false, true) => "mut ",
                (false, false) => "",
            };
            if p.name == "self" && matches!(&p.ty, TypeExpr::Named(n, a) if n == "Self" && a.is_empty()) { format!("{m}self") } else { format!("{m}{}: {}", p.name, ty(&p.ty)) }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn trait_decl(t: &TraitDecl) -> String {
    trait_text(t, true)
}

/// A trait with default method bodies left out (the interface level).
pub fn trait_interface(t: &TraitDecl) -> String {
    trait_text(t, false)
}

fn trait_text(t: &TraitDecl, bodies: bool) -> String {
    let tp = if t.params.is_empty() { String::new() } else { format!("[{}]", crate::symbol::join(&t.params, ", ")) };
    let sup = if t.supers.is_empty() { String::new() } else { format!(": {}", t.supers.iter().map(ty).collect::<Vec<_>>().join(" + ")) };
    let mut s = match &t.derivable {
        // `derivable(Record) trait CommandLine\n  requires ... {`
        Some(d) => {
            let mut h = format!("derivable({}) trait {}{tp}{sup}", ty(&d.kind), t.name);
            for r in &d.requires {
                h += &format!("\n  requires {}", expr(r, 1));
            }
            h + " {\n"
        }
        None => format!("trait {}{tp}{sup} {{\n", t.name),
    };
    for m in &t.methods {
        s += &comments(&m.comments, 1);
        for d in &m.doc {
            s += &format!("  /// {d}\n");
        }
        let sig = fn_signature(&m.decl).replace('\n', "\n  ");
        if m.default && bodies {
            s += &format!("  {} {}\n", sig, block(&m.decl.body, 1));
        } else {
            s += &format!("  {sig}\n");
        }
    }
    for l in &t.laws {
        s += &comments(&l.comments, 1);
        for d in &l.doc {
            s += &format!("  /// {d}\n");
        }
        let head = format!("  law {}({}):", l.name, param_list(&l.params));
        let body = expr(&l.body, 2);
        if head.chars().count() + 1 + body.lines().next().map_or(0, |x| x.chars().count()) > WIDTH {
            s += &format!("{head}\n    {body}\n");
        } else {
            s += &format!("{head} {body}\n");
        }
    }
    close(s)
}

/// Close a trait or impl body: `{}` when it is empty (`impl CommandLine for Options {}`).
fn close(s: String) -> String {
    match s.strip_suffix(" {\n") {
        Some(head) => format!("{head} {{}}"),
        None => s + "}",
    }
}

fn impl_head(i: &ImplDecl) -> String {
    if i.is_inherent() {
        return format!("impl{} {}", bounded(&i.type_params, &i.bounds), ty(&i.for_type));
    }
    let tr = ty(&TypeExpr::Named(i.trait_name.clone(), i.trait_args.clone()));
    let kw = if i.derived { "derive" } else { "impl" };
    format!("{kw}{} {tr} for {}", bounded(&i.type_params, &i.bounds), ty(&i.for_type))
}

pub fn impl_decl(i: &ImplDecl) -> String {
    // `derive CommandLine for Options`: no block when nothing is written by hand
    if i.derived && i.methods.is_empty() {
        return impl_head(i);
    }
    let mut s = impl_head(i) + " {\n";
    for (k, m) in i.methods.iter().enumerate() {
        if k > 0 {
            s += "\n";
        }
        s += &method_notes(m);
        let body = fn_decl(m).lines().map(|l| format!("  {l}\n")).collect::<String>();
        s += &body;
    }
    close(s)
}

/// An implementation with method bodies left out (the interface level).
fn method_notes(m: &FnDecl) -> String {
    comments(&m.comments, 1) + &m.doc.iter().map(|d| format!("  /// {d}\n")).collect::<String>()
}

pub fn impl_interface(i: &ImplDecl) -> String {
    if i.derived && i.methods.is_empty() {
        return impl_head(i);
    }
    let mut s = impl_head(i) + " {\n";
    for m in &i.methods {
        s += &method_notes(m);
        s += &fn_signature(m).lines().map(|l| format!("  {l}\n")).collect::<String>();
    }
    close(s)
}

pub fn fn_signature(f: &FnDecl) -> String {
    let tp = bounded(&f.type_params, &f.bounds);
    let kw = match (f.comptime, f.derived) {
        (true, _) => "comptime fn",
        (false, true) => "derived fn",
        _ => "fn",
    };
    // a function returning `()` leaves out `-> ()`
    let ret = if f.ret == TypeExpr::Unit { String::new() } else { format!(" -> {}", ty(&f.ret)) };
    let mut s = format!("{kw} {}{}({}){ret}", f.name, tp, param_list(&f.params));
    // keep 2 columns for an enclosing `module { }` block
    if s.chars().count() > WIDTH - 2 && !f.params.is_empty() {
        let ps: Vec<String> = param_list(&f.params).split(", ").map(|p| p.to_string()).collect();
        // split(", ") is safe only when no parameter type itself contains ", "
        let ps = if ps.len() == f.params.len() { ps } else { f.params.iter().map(|p| param_list(std::slice::from_ref(p))).collect() };
        s = format!("{kw} {}{}{}{ret}", f.name, tp, broken("(", ps, ")", 0));
    }
    for r in &f.requires {
        s += &format!("\n  requires {}", expr(r, 1));
    }
    for e in &f.ensures {
        s += &format!("\n  ensures {}", expr(e, 1));
    }
    if f.terminates {
        s += "\n  terminates";
    }
    s
}

pub fn fn_decl(f: &FnDecl) -> String {
    let sig = fn_signature(f);
    match &f.body {
        Expr::Hole(_) => format!("{sig}\n  = {}", expr(&f.body, 0)),
        b => format!("{sig} {}", block(b, 0)),
    }
}

pub fn doc(d: &Decl) -> String {
    // long doc lines are re-wrapped at word boundaries to fit the width
    let mut out = String::new();
    for l in &d.doc {
        let mut line = String::new();
        for w in l.split(' ') {
            if !line.is_empty() && line.chars().count() + 1 + w.chars().count() > WIDTH - 6 {
                out += &format!("/// {line}\n");
                line.clear();
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(w);
        }
        out += &format!("/// {line}\n");
    }
    out
}

/// Full (L2) rendering of a declaration.
pub fn decl(d: &Decl) -> String {
    comments(&d.comments, 0) + &doc(d) + &decl_body(d)
}

/// A declaration without its comments and doc: `pub` and the definition itself.
pub fn decl_body(d: &Decl) -> String {
    let body = match &d.kind {
        DeclKind::Type(t) => type_decl(t),
        DeclKind::Fn(f) => fn_decl(f),
        DeclKind::Test(t) => format!("test {:?} {}", t.name, block(&t.body, 0)),
        DeclKind::Trait(t) => trait_decl(t),
        DeclKind::Impl(i) => impl_decl(i),
        DeclKind::Use(u) => match &u.items {
            None => format!("use {}.*", u.path),
            Some(items) if items.len() == 1 => format!("use {}.{}", u.path, items[0]),
            Some(items) => format!("use {}.{{{}}}", u.path, crate::symbol::join(items, ", ")),
        },
        DeclKind::Extern(e) => match &e.item {
            ExternItem::Fn(f) => format!("extern {} {}", e.lang.name(), fn_signature(f)),
            ExternItem::Type(t) => format!("extern {} type {t}", e.lang.name()),
        },
    };
    (if d.public { "pub " } else { "" }).to_string() + &body
}

/// One module's declarations, as the file that holds it.
pub fn module_file(p: &Program, m: &str) -> String {
    p.decls.iter().filter(|d| d.module == m).map(decl).collect::<Vec<_>>().join("\n\n") + "\n"
}
