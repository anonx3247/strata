//! The syntax tree as a `std.ast` value, written in Strata syntax: what
//! `debug` prints for the same tree decoded by `std.ast.binary`. Names follow
//! `std/ast.strata` (`IntLit`, `pre` / `post`, …), which mirrors `ast.rs`
//! node for node. `strata header FILE --tree` prints it; the tests compare it
//! with what the Strata decoder reads from every module file.

use crate::ast::*;

/// A definition as a `std.ast.Decl` value.
pub fn decl(d: &Decl) -> String {
    let kind = match &d.kind {
        DeclKind::Type(t) => tag("TypeDef", type_decl(t)),
        DeclKind::Fn(f) => tag("FnDef", fn_decl(f)),
        DeclKind::Test(t) => tag("TestDef", rec("TestDecl", &[("name", s(&t.name)), ("params", list(&t.params, param)), ("body", expr(&t.body))])),
        DeclKind::Trait(t) => tag("TraitDef", trait_decl(t)),
        DeclKind::Impl(i) => tag("ImplDef", impl_decl(i)),
        DeclKind::Use(u) => tag("UseDef", rec("UseDecl", &[("path", s(&u.path)), ("items", opt(&u.items, |xs| strs(xs)))])),
        DeclKind::Extern(e) => {
            let item = match &e.item {
                ExternItem::Fn(f) => tag("ExternFn", fn_decl(f)),
                ExternItem::Type(t) => tag("ExternType", s(t)),
            };
            let lang = match e.lang {
                Lang::C => "C".to_string(),
            };
            tag("ExternDef", rec("ExternDecl", &[("lang", lang), ("item", item)]))
        }
        DeclKind::Capability(c) => tag("CapabilityDef", rec("CapabilityDecl", &[("name", s(&c.name)), ("methods", list(&c.methods, fn_decl))])),
    };
    rec("Decl", &[("comments", strs(&d.comments)), ("doc", strs(&d.doc)), ("module", s(&d.module)), ("public", d.public.to_string()), ("kind", kind)])
}

// ── how `debug` writes values ───────────────────────────────────────────────

/// `Name { a: x, b: y }`.
fn rec(name: &str, fields: &[(&str, String)]) -> String {
    let fs: Vec<String> = fields.iter().map(|(n, v)| format!("{n}: {v}")).collect();
    format!("{name} {{ {} }}", fs.join(", "))
}

/// `Name(x)`.
fn tag(name: &str, v: String) -> String {
    format!("{name}({v})")
}

fn list<T>(xs: &[T], f: impl Fn(&T) -> String) -> String {
    format!("[{}]", xs.iter().map(f).collect::<Vec<_>>().join(", "))
}

fn opt<T>(x: &Option<T>, f: impl Fn(&T) -> String) -> String {
    match x {
        Some(v) => format!("Some({})", f(v)),
        None => "None".into(),
    }
}

/// A string literal, escaped as `Debug` escapes (the printer's escapes).
fn s(text: &str) -> String {
    format!("\"{}\"", crate::printer::escape(text))
}

fn strs<S: AsRef<str>>(xs: &[S]) -> String {
    list(xs, |x| s(x.as_ref()))
}

fn pair(a: String, b: String) -> String {
    format!("({a}, {b})")
}

/// A float as `Debug` writes it (the runtime's `strata_show_float`): the
/// shortest digits that read back the same, `1.0`, `0.1`, `1e+21`, `1.5e-7`.
fn float(x: f64) -> String {
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    let e = format!("{:e}", x.abs());
    let (mantissa, exp) = e.split_once('e').unwrap();
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    // the value is 0.DIGITS × 10^kk
    let kk: i32 = exp.parse::<i32>().unwrap() + 1;
    let len = digits.len() as i32;
    let sign = if x < 0.0 { "-" } else { "" };
    let body = if kk >= len && kk <= 16 {
        format!("{digits}{}.0", "0".repeat((kk - len) as usize))
    } else if kk > 0 && kk <= 16 {
        format!("{}.{}", &digits[..kk as usize], &digits[kk as usize..])
    } else if kk > -5 && kk <= 0 {
        format!("0.{}{digits}", "0".repeat(-kk as usize))
    } else if len == 1 {
        format!("{digits}e{:+}", kk - 1)
    } else {
        format!("{}.{}e{:+}", &digits[..1], &digits[1..], kk - 1)
    };
    format!("{sign}{body}")
}

// ── the tree ────────────────────────────────────────────────────────────────

fn type_decl(t: &TypeDecl) -> String {
    let body = match &t.body {
        TypeBody::Alias(a) => tag("Alias", ty(a)),
        TypeBody::Record(fs) => tag("RecordBody", list(fs, field)),
        TypeBody::Sum(vs) => tag("SumBody", list(vs, variant)),
    };
    rec(
        "TypeDecl",
        &[
            ("distinct", t.distinct.to_string()),
            ("name", s(&t.name)),
            ("params", strs(&t.params)),
            ("body", body),
            ("refinement", opt(&t.refinement, expr)),
            ("mocked", t.mock.to_string()),
        ],
    )
}

fn field(f: &Field) -> String {
    rec("Field", &[("comments", strs(&f.comments)), ("doc", strs(&f.doc)), ("name", s(&f.name)), ("ty", ty(&f.ty)), ("default", opt(&f.default, expr))])
}

fn variant(v: &Variant) -> String {
    let payload = match &v.payload {
        Payload::None => "NoPayload".into(),
        Payload::Positional(ts) => tag("Positional", list(ts, ty)),
        Payload::Record(fs) => tag("RecordPayload", list(fs, field)),
        Payload::Type(t) => tag("MemberType", ty(t)),
    };
    rec("Variant", &[("comments", strs(&v.comments)), ("name", s(&v.name)), ("payload", payload)])
}

fn fn_decl(f: &FnDecl) -> String {
    rec(
        "FnDecl",
        &[
            ("doc", strs(&f.doc)),
            ("comments", strs(&f.comments)),
            ("name", s(&f.name)),
            ("type_params", strs(&f.type_params)),
            ("bounds", list(&f.bounds, bound)),
            ("params", list(&f.params, param)),
            ("ret", ty(&f.ret)),
            ("pre", list(&f.requires, expr)),
            ("post", list(&f.ensures, expr)),
            ("terminating", f.terminates.to_string()),
            ("body", expr(&f.body)),
            ("compile_time", f.comptime.to_string()),
            ("derived", f.derived.to_string()),
        ],
    )
}

fn param(p: &Param) -> String {
    rec("Param", &[("name", s(&p.name)), ("ty", ty(&p.ty)), ("mutable", p.mutable.to_string()), ("compile_time", p.comptime.to_string())])
}

fn bound(b: &Bound) -> String {
    rec("Bound", &[("param", s(&b.param)), ("traits", list(&b.traits, ty))])
}

fn trait_decl(t: &TraitDecl) -> String {
    let method = |m: &TraitMethod| rec("TraitMethod", &[("comments", strs(&m.comments)), ("doc", strs(&m.doc)), ("decl", fn_decl(&m.decl)), ("default", m.default.to_string())]);
    let law = |l: &Law| rec("Law", &[("comments", strs(&l.comments)), ("doc", strs(&l.doc)), ("name", s(&l.name)), ("params", list(&l.params, param)), ("body", expr(&l.body))]);
    let derivable = |d: &Derivable| rec("Derivable", &[("kind", ty(&d.kind)), ("conditions", list(&d.requires, expr))]);
    rec(
        "TraitDecl",
        &[
            ("name", s(&t.name)),
            ("params", strs(&t.params)),
            ("supers", list(&t.supers, ty)),
            ("methods", list(&t.methods, method)),
            ("laws", list(&t.laws, law)),
            ("derivable", opt(&t.derivable, derivable)),
            ("capability", t.capability.to_string()),
        ],
    )
}

fn impl_decl(i: &ImplDecl) -> String {
    rec(
        "ImplDecl",
        &[
            ("type_params", strs(&i.type_params)),
            ("bounds", list(&i.bounds, bound)),
            ("trait_name", s(&i.trait_name)),
            ("trait_args", list(&i.trait_args, ty)),
            ("for_type", ty(&i.for_type)),
            ("methods", list(&i.methods, fn_decl)),
            ("derived", i.derived.to_string()),
        ],
    )
}

fn ty(t: &TypeExpr) -> String {
    match t {
        TypeExpr::Named(n, args) => rec("Named", &[("name", s(n)), ("args", list(args, ty))]),
        TypeExpr::Fn(ps, r) => rec("FnType", &[("params", list(ps, ty)), ("ret", ty(r))]),
        TypeExpr::Unit => "UnitType".into(),
        TypeExpr::Refined(b, p) => rec("Refined", &[("base", ty(b)), ("pred", expr(p))]),
        TypeExpr::Union(ts) => tag("UnionType", list(ts, ty)),
    }
}

fn pattern(p: &Pattern) -> String {
    match p {
        Pattern::Wild => "Wild".into(),
        Pattern::Bind(n) => tag("Bind", s(n)),
        Pattern::Int(i) => tag("IntPat", i.to_string()),
        Pattern::Str(x) => tag("StrPat", s(x)),
        Pattern::Bool(b) => tag("BoolPat", b.to_string()),
        Pattern::Ctor(n, args) => {
            let args = match args {
                CtorPat::None => "NoPats".into(),
                CtorPat::Positional(ps) => tag("PositionalPats", list(ps, pattern)),
                CtorPat::Record(fs, rest) => rec("RecordPats", &[("fields", list(fs, |(n, p)| pair(s(n), pattern(p)))), ("rest", rest.to_string())]),
                CtorPat::Type(ps) => tag("TypePats", list(ps, pattern)),
            };
            rec("CtorPattern", &[("name", s(n)), ("args", args)])
        }
        Pattern::List(ps, rest) => rec("ListPat", &[("items", list(ps, pattern)), ("rest", opt(rest, |r| s(r)))]),
        Pattern::Tuple(ps) => tag("TuplePat", list(ps, pattern)),
        Pattern::Cond(e) => tag("CondPat", expr(e)),
        Pattern::Or(ps) => tag("OrPat", list(ps, pattern)),
        Pattern::For(arm, each, xs) => rec("ForPat", &[("arm", pattern(arm)), ("each", pattern(each)), ("list", expr(xs))]),
    }
}

fn stmt(st: &Stmt) -> String {
    let three = |name: &str, p: &Pattern, xs: &Expr, b: &Expr| rec(name, &[("pat", pattern(p)), ("iter", expr(xs)), ("body", expr(b))]);
    match st {
        Stmt::Let(p, t, e) => rec("Let", &[("pat", pattern(p)), ("ty", opt(t, ty)), ("value", expr(e))]),
        Stmt::Var(n, t, e) => rec("VarDecl", &[("name", s(n)), ("ty", opt(t, ty)), ("value", expr(e))]),
        Stmt::Assign(a, b) => rec("Assign", &[("place", expr(a)), ("value", expr(b))]),
        Stmt::While(c, b) => rec("While", &[("cond", expr(c)), ("body", expr(b))]),
        Stmt::For(p, xs, b) => three("For", p, xs, b),
        Stmt::Loop(b) => tag("Loop", expr(b)),
        Stmt::Break => "Break".into(),
        Stmt::Continue => "Continue".into(),
        Stmt::Expr(e) => tag("ExprStmt", expr(e)),
        Stmt::Assert(e) => tag("Assert", expr(e)),
        Stmt::Comment(ls) => tag("Comment", strs(ls)),
        Stmt::ComptimeFor(p, xs, b) => three("ComptimeFor", p, xs, b),
    }
}

fn bin_op(op: BinOp) -> &'static str {
    use BinOp::*;
    match op {
        Add => "Add",
        Sub => "Sub",
        Mul => "Mul",
        Div => "Div",
        Mod => "Mod",
        Eq => "Equal",
        Ne => "NotEqual",
        Lt => "Less",
        Le => "LessEq",
        Gt => "Greater",
        Ge => "GreaterEq",
        And => "And",
        Or => "Or",
        Implies => "Implies",
    }
}

fn exprs(xs: &[Expr]) -> String {
    list(xs, expr)
}

fn named(fs: &[(Symbol, Expr)]) -> String {
    list(fs, |(n, e)| pair(s(n), expr(e)))
}

fn spec(f: &FmtSpec) -> String {
    rec(
        "FmtSpec",
        &[
            ("fill", s(&f.fill.to_string())),
            ("align", opt(&f.align, |c| s(&c.to_string()))),
            ("plus", f.plus.to_string()),
            ("zero", f.zero.to_string()),
            ("width", f.width.to_string()),
            ("precision", opt(&f.precision, |p| p.to_string())),
        ],
    )
}

fn expr(e: &Expr) -> String {
    match e {
        Expr::Int(i) => tag("IntLit", i.to_string()),
        Expr::BigInt(d) => tag("BigIntLit", s(d)),
        Expr::Float(x) => tag("FloatLit", float(*x)),
        Expr::Tuple(xs) => tag("TupleExpr", exprs(xs)),
        Expr::Str(x) => tag("StrLit", s(x)),
        Expr::Bool(b) => tag("BoolLit", b.to_string()),
        Expr::Unit => "UnitExpr".into(),
        Expr::Var(n) => tag("Var", s(n)),
        Expr::Hole(n) => tag("Hole", s(n)),
        Expr::List(xs) => tag("ListExpr", exprs(xs)),
        Expr::Ctor(n, args) => {
            let args = match args {
                CtorArgs::None => "NoArgs".into(),
                CtorArgs::Positional(xs) => tag("PositionalArgs", exprs(xs)),
                CtorArgs::Record(fs) => tag("RecordArgs", named(fs)),
                CtorArgs::Update(b, fs) => rec("UpdateArgs", &[("base", expr(b)), ("fields", named(fs))]),
            };
            rec("CtorExpr", &[("name", s(n)), ("args", args)])
        }
        Expr::Call(f, xs) => rec("Call", &[("callee", expr(f)), ("args", exprs(xs))]),
        Expr::Method(r, m, xs) => rec("MethodCall", &[("recv", expr(r)), ("name", s(m)), ("args", exprs(xs))]),
        Expr::Field(b, n) => rec("FieldOf", &[("base", expr(b)), ("name", s(n))]),
        Expr::Lambda(ps, b) => {
            let lp = |p: &LamParam| rec("LamParam", &[("name", s(&p.name)), ("ty", opt(&p.ty, ty))]);
            rec("Lambda", &[("params", list(ps, lp)), ("body", expr(b))])
        }
        Expr::Bin(op, a, b) => rec("Binary", &[("op", bin_op(*op).into()), ("left", expr(a)), ("right", expr(b))]),
        Expr::Un(op, x) => {
            let op = match op {
                UnOp::Neg => "Neg",
                UnOp::Not => "Not",
            };
            rec("Unary", &[("op", op.into()), ("operand", expr(x))])
        }
        Expr::Is(x, v) => rec("IsExpr", &[("value", expr(x)), ("variant", s(v))]),
        Expr::If(c, t, o) => rec("IfExpr", &[("cond", expr(c)), ("then", expr(t)), ("otherwise", expr(o))]),
        Expr::Match(x, name, arms) => {
            let arm = |a: &Arm| rec("Arm", &[("comments", strs(&a.comments)), ("pat", pattern(&a.pat)), ("body", expr(&a.body))]);
            rec("MatchExpr", &[("scrutinee", expr(x)), ("binding", opt(name, |n| s(n))), ("arms", list(arms, arm))])
        }
        Expr::Mut(x) => tag("MutExpr", expr(x)),
        Expr::Block(ss, last) => rec("Block", &[("stmts", list(ss, stmt)), ("last", expr(last))]),
        Expr::Mock(t) => tag("MockExpr", ty(t)),
        Expr::Return(x) => tag("Return", expr(x)),
        Expr::Try(x, k) => {
            let k = match k {
                TryKind::Result => "TryResult",
                TryKind::Option => "TryOption",
            };
            rec("Try", &[("value", expr(x)), ("kind", k.into())])
        }
        Expr::Except(x, err, fb) => rec("Except", &[("value", expr(x)), ("err", opt(err, |n| s(n))), ("fallback", expr(fb))]),
        Expr::Interp(parts) => {
            let part = |p: &StrPart| match p {
                StrPart::Lit(t) => tag("TextPart", s(t)),
                StrPart::Expr(e, f) => rec("ValuePart", &[("value", expr(e)), ("spec", opt(f, spec))]),
            };
            tag("Interp", list(parts, part))
        }
        Expr::Range(lo, hi, inc) => rec("Range", &[("lo", opt(lo, |x| expr(x))), ("hi", opt(hi, |x| expr(x))), ("inclusive", inc.to_string())]),
        Expr::Index(b, i) => rec("Index", &[("base", expr(b)), ("index", expr(i))]),
        Expr::Comp(kind, elem, fors) => {
            let kind = match kind {
                CompKind::List => "ListComp",
                CompKind::Set => "SetComp",
                CompKind::Dict => "DictComp",
            };
            let cf = |c: &CompFor| rec("CompFor", &[("pat", pattern(&c.pat)), ("iter", expr(&c.iter)), ("cond", opt(&c.cond, expr))]);
            rec("Comp", &[("kind", kind.into()), ("elem", expr(elem)), ("fors", list(fors, cf))])
        }
        Expr::Dict(tys, kvs) => rec("DictExpr", &[("types", list(tys, ty)), ("entries", list(kvs, |(k, v)| pair(expr(k), expr(v))))]),
        Expr::Set(tys, xs) => rec("SetExpr", &[("types", list(tys, ty)), ("elems", exprs(xs))]),
        Expr::Or(x, fb) => rec("Fallback", &[("value", expr(x)), ("fallback", expr(fb))]),
        Expr::Comptime(x) => tag("Comptime", expr(x)),
    }
}

#[cfg(test)]
mod tests {
    use super::float;

    #[test]
    fn floats_as_debug_writes_them() {
        for (x, want) in [(1.0, "1.0"), (0.1, "0.1"), (-2.5, "-2.5"), (1e21, "1e+21"), (1.5e-7, "1.5e-7"), (123456.0, "123456.0"), (0.00012, "0.00012"), (-0.0, "-0.0"), (1e16, "1e+16"), (1e15, "1000000000000000.0")] {
            assert_eq!(float(x), want, "{x}");
        }
    }
}
