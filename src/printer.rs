//! Rendering the tree back to text. The same printer produces the canonical
//! form (used for hashing) and every level-of-detail view.

use crate::ast::*;

pub fn ty(t: &TypeExpr) -> String {
    match t {
        TypeExpr::Unit => "()".into(),
        TypeExpr::Named(n, args) if args.is_empty() => n.clone(),
        TypeExpr::Named(n, args) => format!("{n}[{}]", args.iter().map(ty).collect::<Vec<_>>().join(", ")),
        TypeExpr::Fn(ps, r) => format!("({}) -> {}", ps.iter().map(ty).collect::<Vec<_>>().join(", "), ty(r)),
    }
}

fn fields(fs: &[Field]) -> String {
    fs.iter().map(|f| format!("{}: {}", f.name, ty(&f.ty))).collect::<Vec<_>>().join(", ")
}

fn params(ps: &[Param]) -> String {
    ps.iter().map(|p| format!("{}: {}", p.name, ty(&p.ty))).collect::<Vec<_>>().join(", ")
}

fn ind(n: usize) -> String {
    "  ".repeat(n)
}

pub fn pat(p: &Pattern) -> String {
    match p {
        Pattern::Wild => "_".into(),
        Pattern::Bind(n) => n.clone(),
        Pattern::Int(n) => n.to_string(),
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

pub fn expr(e: &Expr, d: usize) -> String {
    match e {
        Expr::Int(n) => n.to_string(),
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
        Expr::Call(f, es) => format!("{}({})", wrap(f, 8, d), args(es, d)),
        Expr::Perform(eff, op, es) => format!("{eff}.{op}({})", args(es, d)),
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
        Expr::Handle(body, hs) => {
            let mut out = format!("handle {}", expr(body, d));
            for h in hs {
                match &h.kind {
                    HandlerKind::MockAuto => out += &format!(" with mock {}", h.effect),
                    HandlerKind::Clauses(cs) => {
                        out += &format!(" with {} {{\n", h.effect);
                        for c in cs {
                            out += &format!(
                                "{}{}({}) => {},\n",
                                ind(d + 1),
                                c.op,
                                c.params.join(", "),
                                expr(&c.body, d + 1)
                            );
                        }
                        out += &format!("{}}}", ind(d));
                    }
                }
            }
            out
        }
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
            Stmt::Expr(e) => expr(e, d + 1),
            Stmt::Assert(e) => format!("assert {}", expr(e, d + 1)),
        };
        out += "\n";
    }
    if *last != Expr::Unit || stmts.is_empty() {
        out += &format!("{}{}\n", ind(d + 1), expr(last, d + 1));
    }
    out + &ind(d) + "}"
}

pub fn type_decl(t: &TypeDecl) -> String {
    let tp = if t.params.is_empty() { String::new() } else { format!("[{}]", t.params.join(", ")) };
    let mut s = format!("type {}{} =", t.name, tp);
    if !matches!(t.body, TypeBody::Sum(_)) {
        s.push(' ');
    }
    match &t.body {
        TypeBody::Alias(a) => s += &ty(a),
        TypeBody::Record(fs) => s += &format!("{{ {} }}", fields(fs)),
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

pub fn effect_decl(e: &EffectDecl) -> String {
    let mut s = format!("effect {} {{\n", e.name);
    for op in &e.ops {
        s += &format!("  fn {}({}) -> {}\n", op.name, params(&op.params), ty(&op.ret));
    }
    s + "}"
}

pub fn fn_signature(f: &FnDecl) -> String {
    let tp = if f.type_params.is_empty() { String::new() } else { format!("[{}]", f.type_params.join(", ")) };
    let mut s = format!("fn {}{}({}) -> {}", f.name, tp, params(&f.params), ty(&f.ret));
    if !f.effects.is_empty() {
        s += &format!(" / {{{}}}", f.effects.join(", "));
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
        b @ (Expr::Block(..) | Expr::Match(..) | Expr::If(..) | Expr::Handle(..)) => {
            format!("{sig} {}", block(b, 0))
        }
        b => format!("{sig} {}", block(b, 0)),
    }
}

pub fn doc(d: &Decl) -> String {
    d.doc.iter().map(|l| format!("/// {l}\n")).collect()
}

/// Full (L3) rendering of a declaration.
pub fn decl(d: &Decl) -> String {
    let body = match &d.kind {
        DeclKind::Type(t) => type_decl(t),
        DeclKind::Effect(e) => effect_decl(e),
        DeclKind::Fn(f) => fn_decl(f),
        DeclKind::Test(t) => format!("test {:?} {}", t.name, block(&t.body, 0)),
    };
    doc(d) + &body
}

pub fn program(p: &Program) -> String {
    p.decls.iter().map(decl).collect::<Vec<_>>().join("\n\n") + "\n"
}
