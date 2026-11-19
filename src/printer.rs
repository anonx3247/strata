//! Rendering the tree back to text. The same printer produces the canonical
//! form (used for hashing) and every level-of-detail view.

use crate::ast::*;

pub fn ty(t: &TypeExpr) -> String {
    match t {
        TypeExpr::Unit => "()".into(),
        TypeExpr::Named(n, args) if n == "Tuple" => format!("({})", args.iter().map(ty).collect::<Vec<_>>().join(", ")),
        TypeExpr::Named(n, args) if args.is_empty() => n.clone(),
        TypeExpr::Named(n, args) => format!("{n}[{}]", args.iter().map(ty).collect::<Vec<_>>().join(", ")),
        TypeExpr::Fn(ps, r) => format!("({}) -> {}", ps.iter().map(ty).collect::<Vec<_>>().join(", "), ty(r)),
        TypeExpr::Refined(b, p) => format!("{} where {}", ty(b), expr(p, 0)),
    }
}

fn fields(fs: &[Field]) -> String {
    fs.iter().map(|f| format!("{}: {}", f.name, ty(&f.ty))).collect::<Vec<_>>().join(", ")
}

fn ind(n: usize) -> String {
    "  ".repeat(n)
}

pub fn pat(p: &Pattern) -> String {
    match p {
        Pattern::Wild => "_".into(),
        Pattern::Bind(n) => n.clone(),
        Pattern::Int(n) => n.to_string(),
        Pattern::Tuple(ps) => format!("({})", ps.iter().map(pat).collect::<Vec<_>>().join(", ")),
        Pattern::Str(s) => format!("{s:?}"),
        Pattern::Bool(b) => b.to_string(),
        Pattern::Ctor(c, CtorPat::None) => c.clone(),
        Pattern::Ctor(c, CtorPat::Positional(ps)) => {
            format!("{c}({})", ps.iter().map(pat).collect::<Vec<_>>().join(", "))
        }
        Pattern::Ctor(c, CtorPat::Record(fs, open)) => {
            let mut parts: Vec<String> = fs
                .iter()
                .map(|(f, p)| match p {
                    Pattern::Bind(b) if b == f => f.clone(),
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
            BinOp::Concat => 4,
            BinOp::Add | BinOp::Sub => 5,
            BinOp::Mul | BinOp::Div | BinOp::Mod => 6,
        },
        Expr::Is(..) => 3,
        Expr::Lambda(..) => 0,
        Expr::Un(..) => 7,
        _ => 8,
    }
}

fn wrap(e: &Expr, min: u8, d: usize) -> String {
    let s = expr(e, d);
    if prec(e) < min { format!("({s})") } else { s }
}

fn args(es: &[Expr], d: usize) -> String {
    es.iter().map(|e| expr(e, d)).collect::<Vec<_>>().join(", ")
}

pub fn float(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 { format!("{f:.1}") } else { format!("{f}") }
}

/// Target line width for rendered code.
pub const WIDTH: usize = 80;

/// Would this single-line text overflow the width at indent depth `d`?
/// (12 columns are kept for whatever precedes it on the line, e.g. `let x = `.)
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
    let chain = matches!(e, Expr::Method(..) | Expr::Bin(BinOp::And | BinOp::Or | BinOp::Concat, ..));
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
                    Expr::Var(v) if v == f => f.clone(),
                    _ => format!("{f}: {}", expr(x, d + 1)),
                })
                .collect(),
            "}",
            d,
        ),
        Expr::Ctor(c, CtorArgs::Positional(es)) => format!("{c}{}", broken("(", es.iter().map(|a| expr(a, d + 1)).collect(), ")", d)),
        // |x| long body  ->  body on its own line
        Expr::Lambda(ps, b) => {
            let head = if ps.is_empty() { "||".to_string() } else { format!("|{}|", ps.join(", ")) };
            format!("{head}\n{}{}", ind(d + 1), expr(b, d + 1))
        }
        // a && b && c  ->  one operand per line
        Expr::Bin(op @ (BinOp::And | BinOp::Or | BinOp::Concat), _, _) => {
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

fn expr_inline(e: &Expr, d: usize) -> String {
    match e {
        Expr::Int(n) => n.to_string(),
        Expr::Float(f) => float(*f),
        Expr::Tuple(es) => format!("({})", args(es, d)),
        Expr::Str(s) => format!("\"{}\"", s.replace('"', "\\\"").replace('\n', "\\n")),
        Expr::Bool(b) => b.to_string(),
        Expr::Unit => "()".into(),
        Expr::Var(v) => v.clone(),
        Expr::Hole(h) => format!("?{h}"),
        Expr::List(es) => format!("[{}]", args(es, d)),
        Expr::Ctor(c, CtorArgs::None) => c.clone(),
        Expr::Ctor(c, CtorArgs::Positional(es)) => format!("{c}({})", args(es, d)),
        Expr::Ctor(c, CtorArgs::Record(fs)) => {
            let parts: Vec<String> = fs
                .iter()
                .map(|(f, e)| match e {
                    Expr::Var(v) if v == f => f.clone(),
                    _ => format!("{f}: {}", expr(e, d)),
                })
                .collect();
            format!("{c} {{ {} }}", parts.join(", "))
        }
        Expr::Ctor(c, CtorArgs::Update(base, fs)) => {
            let mut parts = vec![format!("..{}", expr(base, d))];
            parts.extend(fs.iter().map(|(f, e)| match e {
                Expr::Var(v) if v == f => f.clone(),
                _ => format!("{f}: {}", expr(e, d)),
            }));
            format!("{c} {{ {} }}", parts.join(", "))
        }
        Expr::Call(f, es) => format!("{}({})", wrap(f, 8, d), args(es, d)),
        Expr::Method(r, m, es) => format!("{}.{m}({})", wrap(r, 8, d), args(es, d)),
        Expr::Field(r, f) => format!("{}.{f}", wrap(r, 8, d)),
        Expr::Lambda(ps, b) => {
            if ps.is_empty() { format!("|| {}", expr(b, d)) } else { format!("|{}| {}", ps.join(", "), expr(b, d)) }
        }
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
        Expr::Match(s, arms) => {
            let mut out = format!("match {} {{\n", expr(s, d));
            for a in arms {
                out += &format!("{}{} => {},\n", ind(d + 1), pat(&a.pat), expr(&a.body, d + 1));
            }
            out + &ind(d) + "}"
        }
        Expr::Block(..) => block(e, d),
        Expr::Mock(t) => format!("mock {}", ty(t)),
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
        out += &ind(d + 1);
        out += &match s {
            Stmt::Let(p, t, e) => match t {
                Some(t) => format!("let {}: {} = {}", pat(p), ty(t), expr(e, d + 1)),
                None => format!("let {} = {}", pat(p), expr(e, d + 1)),
            },
            Stmt::Var(n, t, e) => match t {
                Some(t) => format!("let mut {n}: {} = {}", ty(t), expr(e, d + 1)),
                None => format!("let mut {n} = {}", expr(e, d + 1)),
            },
            Stmt::Assign(p, e) => format!("{} = {}", expr(p, d + 1), expr(e, d + 1)),
            Stmt::While(c, b) => format!("while {} {}", expr(c, d + 1), block(b, d + 1)),
            Stmt::Loop(b) => format!("loop {}", block(b, d + 1)),
            Stmt::Break => "break".into(),
            Stmt::Continue => "continue".into(),
            Stmt::For(p, it, b) => format!("for {} in {} {}", pat(p), expr(it, d + 1), block(b, d + 1)),
            Stmt::Expr(e) => expr(e, d + 1),
            Stmt::Assert(e) => format!("assert {}", expr(e, d + 1)),
        };
        out += "\n";
    }
    // a trailing `()` is implied after `let`/`assert`, but not after an expression statement
    if *last != Expr::Unit || stmts.is_empty() || matches!(stmts.last(), Some(Stmt::Expr(_))) {
        out += &format!("{}{}\n", ind(d + 1), expr(last, d + 1));
    }
    out + &ind(d) + "}"
}

pub fn type_decl(t: &TypeDecl) -> String {
    let tp = if t.params.is_empty() { String::new() } else { format!("[{}]", t.params.join(", ")) };
    let mut s = format!("{}type {}{} =", if t.distinct { "distinct " } else { "" }, t.name, tp);
    if !matches!(t.body, TypeBody::Sum(_)) {
        s.push(' ');
    }
    match &t.body {
        TypeBody::Alias(a) => s += &ty(a),
        TypeBody::Record(fs) => {
            let one = format!("{{ {} }}", fields(fs));
            if s.chars().count() + one.chars().count() > WIDTH {
                s += &broken("{", fs.iter().map(|f| format!("{}: {}", f.name, ty(&f.ty))).collect(), "}", 0);
            } else {
                s += &one;
            }
        }
        TypeBody::Sum(vs) => {
            for v in vs {
                s += "\n  | ";
                s += &v.name;
                match &v.payload {
                    Payload::None => {}
                    Payload::Positional(ts) => {
                        s += &format!("({})", ts.iter().map(ty).collect::<Vec<_>>().join(", "))
                    }
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

fn bounded(names: &[String], bounds: &[Bound]) -> String {
    if names.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = names
        .iter()
        .map(|n| match bounds.iter().find(|b| &b.param == n) {
            Some(b) => format!("{n}: {}", b.traits.iter().map(ty).collect::<Vec<_>>().join(" + ")),
            None => n.clone(),
        })
        .collect();
    format!("[{}]", parts.join(", "))
}

fn param_list(ps: &[Param]) -> String {
    ps.iter()
        .map(|p| {
            let m = if p.mutable { "mut " } else { "" };
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
    let tp = if t.params.is_empty() { String::new() } else { format!("[{}]", t.params.join(", ")) };
    let sup = if t.supers.is_empty() { String::new() } else { format!(": {}", t.supers.iter().map(ty).collect::<Vec<_>>().join(" + ")) };
    let mut s = format!("trait {}{tp}{sup} {{\n", t.name);
    for m in &t.methods {
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
        let head = format!("  law {}({}):", l.name, param_list(&l.params));
        let body = expr(&l.body, 2);
        if head.chars().count() + 1 + body.lines().next().map_or(0, |x| x.chars().count()) > WIDTH {
            s += &format!("{head}\n    {body}\n");
        } else {
            s += &format!("{head} {body}\n");
        }
    }
    s + "}"
}

fn impl_head(i: &ImplDecl) -> String {
    let tr = ty(&TypeExpr::Named(i.trait_name.clone(), i.trait_args.clone()));
    format!("impl{} {tr} for {}", bounded(&i.type_params, &i.bounds), ty(&i.for_type))
}

pub fn impl_decl(i: &ImplDecl) -> String {
    let mut s = impl_head(i) + " {\n";
    for (k, m) in i.methods.iter().enumerate() {
        if k > 0 {
            s += "\n";
        }
        let body = fn_decl(m).lines().map(|l| format!("  {l}\n")).collect::<String>();
        s += &body;
    }
    s + "}"
}

/// An implementation with method bodies left out (the interface level).
pub fn impl_interface(i: &ImplDecl) -> String {
    let mut s = impl_head(i) + " {\n";
    for m in &i.methods {
        s += &fn_signature(m).lines().map(|l| format!("  {l}\n")).collect::<String>();
    }
    s + "}"
}

pub fn fn_signature(f: &FnDecl) -> String {
    let tp = bounded(&f.type_params, &f.bounds);
    let mut s = format!("fn {}{}({}) -> {}", f.name, tp, param_list(&f.params), ty(&f.ret));
    // keep 2 columns for an enclosing `module { }` block
    if s.chars().count() > WIDTH - 2 && !f.params.is_empty() {
        let ps: Vec<String> = param_list(&f.params).split(", ").map(|p| p.to_string()).collect();
        // split(", ") is safe only when no parameter type itself contains ", "
        let ps = if ps.len() == f.params.len() { ps } else { f.params.iter().map(|p| param_list(std::slice::from_ref(p))).collect() };
        s = format!("fn {}{}{} -> {}", f.name, tp, broken("(", ps, ")", 0), ty(&f.ret));
    }
    for r in &f.requires {
        s += &format!("\n  requires {}", expr(r, 1));
    }
    for e in &f.ensures {
        s += &format!("\n  ensures {}", expr(e, 1));
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

/// Full (L3) rendering of a declaration.
pub fn decl(d: &Decl) -> String {
    let body = match &d.kind {
        DeclKind::Type(t) => type_decl(t),
        DeclKind::Fn(f) => fn_decl(f),
        DeclKind::Test(t) => format!("test {:?} {}", t.name, block(&t.body, 0)),
        DeclKind::Trait(t) => trait_decl(t),
        DeclKind::Impl(i) => impl_decl(i),
        DeclKind::Use(u) => match &u.items {
            None => format!("use {}.*", u.path),
            Some(items) if items.len() == 1 => format!("use {}.{}", u.path, items[0]),
            Some(items) => format!("use {}.{{{}}}", u.path, items.join(", ")),
        },
    };
    doc(d) + if d.public { "pub " } else { "" } + &body
}

/// Render declarations, grouping each module's into one `module m { ... }` block.
pub fn program(p: &Program) -> String {
    let mut order: Vec<String> = Vec::new();
    for d in &p.decls {
        if !order.contains(&d.module) {
            order.push(d.module.clone());
        }
    }
    let mut out = Vec::new();
    for m in order {
        let body: Vec<String> = p.decls.iter().filter(|d| d.module == m).map(decl).collect();
        if m.is_empty() {
            out.extend(body);
        } else {
            out.push(module_block(&m, &body.join("\n\n")));
        }
    }
    out.join("\n\n") + "\n"
}

pub fn module_block(m: &str, body: &str) -> String {
    let inner: String = body.lines().map(|l| if l.is_empty() { "\n".to_string() } else { format!("  {l}\n") }).collect();
    format!("module {m} {{\n{inner}}}")
}

/// Render one module's declarations as a standalone file (no `module` wrapper).
pub fn module_file(p: &Program, m: &str) -> String {
    p.decls.iter().filter(|d| d.module == m).map(decl).collect::<Vec<_>>().join("\n\n") + "\n"
}
