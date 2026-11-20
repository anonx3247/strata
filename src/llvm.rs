//! Native compilation: the elaborated program as LLVM IR (text), linked with
//! a small C runtime (`llvm/runtime.c`) by `clang` into an executable, or run
//! in memory by the JIT (`llvm/jit.rs`).
//!
//! The compiler works from the types the checker inferred for every
//! expression (`types::elaborate_typed`). Generic functions are compiled
//! once per set of type arguments (monomorphized).
//!
//! Values: `Int` is an `i64`, `Float` a `double`, `Bool` an `i1`, `()` has no
//! value; a `String` points to its length and bytes. A record, a variant or a
//! tuple points to a heap block of 8-byte slots: a tag (the constructor, a
//! number unique in the program), then its fields in declaration order, each
//! converted to 64 bits. A `distinct` type is its underlying value. Values
//! never change once built, so they are shared freely.
//!
//! Contracts are checked at run time as the interpreter does, with its fault
//! messages: refinements of parameters, results, annotated `let`s, declared
//! variables and constructed values, `requires`, `ensures`, `assert`.
//!
//! Not yet: big integers (`Int` overflow is a fault), lists and the other
//! collections, lambdas, traits, `mut` parameters, capabilities, format
//! specs, unions of anything but records and variants, and a garbage
//! collector (memory is never freed). A program that uses them is refused,
//! with the function and the construct named.

use crate::ast::*;
use crate::types::{Env, Ty};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write;

#[cfg(feature = "jit")]
pub mod jit;

/// The runtime's C source, compiled with the program.
pub const RUNTIME: &str = include_str!("llvm/runtime.c");

/// The type of an expression that never produces a value (`return`, a fault).
const NEVER: Ty = Ty::Var(usize::MAX);

fn is_never(t: &Ty) -> bool {
    *t == NEVER
}

/// A compiled value: its type and its LLVM operand ("" for `()` and nothing).
type V = (Ty, String);

fn never() -> V {
    (NEVER, String::new())
}

fn has_value(t: &Ty) -> bool {
    !matches!(t, Ty::Unit) && !is_never(t)
}

fn bool_v(b: &str) -> V {
    (Ty::Bool, b.to_string())
}

/// Fault kinds, as numbered in the runtime (`KINDS` in runtime.c).
#[derive(Clone, Copy)]
enum Fault {
    Refinement = 0,
    Requires = 1,
    Ensures = 2,
    Assert = 3,
    Runtime = 4,
    Hole = 5,
}

type Res<X> = Result<X, String>;

/// A type as written in symbol names: `Int`, `Option[String]`, `(Int, Bool)`.
fn key(t: &Ty) -> String {
    let list = |ts: &[Ty]| ts.iter().map(key).collect::<Vec<_>>().join(", ");
    match t {
        Ty::Int => "Int".into(),
        Ty::Float => "Float".into(),
        Ty::Bool => "Bool".into(),
        Ty::Str => "String".into(),
        Ty::Unit => "()".into(),
        Ty::Con(n, a) if n == "Tuple" => format!("({})", list(a)),
        Ty::Con(n, a) if a.is_empty() => n.clone(),
        Ty::Con(n, a) => format!("{n}[{}]", list(a)),
        Ty::Fn(ps, r) => format!("({}) -> {}", list(ps), key(r)),
        Ty::Union(ms) => ms.iter().map(key).collect::<Vec<_>>().join(" | "),
        Ty::Rigid(n) => n.clone(),
        Ty::Var(_) => "?".into(),
    }
}

/// The LLVM IR of a program whose entry point is `entry` (`main`): it takes
/// no parameters and returns `()`, an exit code (`Int`), or a `Result` of either.
pub fn compile(prog: &Program, entry: &str) -> Res<String> {
    let (resolved, types) = crate::types::elaborate_typed(prog);
    let (env, _) = Env::build(&resolved.program);
    let main = env.fns.get(entry).ok_or(format!("no `{entry}` function to compile"))?.clone();
    if !main.params.is_empty() {
        return Err(format!("`{entry}` takes parameters; compiled programs can't ask for capabilities or options yet"));
    }
    // every constructor gets a tag of its own, so a value's tag says what it is
    let mut ctors: Vec<&String> = env.ctors.keys().collect();
    ctors.sort();
    let tags = ctors.iter().enumerate().map(|(i, c)| ((*c).clone(), i as i64 + 1)).collect();
    // bodies as the checker saw them (its types are by node); signatures from `env`
    let mut bodies: HashMap<String, &FnDecl> = HashMap::new();
    for d in &resolved.program.decls {
        match &d.kind {
            DeclKind::Fn(f) => {
                bodies.insert(f.name.clone(), f);
            }
            DeclKind::Impl(i) if i.is_inherent() => {
                for mth in &i.methods {
                    bodies.insert(crate::types::static_fn(i, mth).name, mth);
                }
            }
            _ => {}
        }
    }
    let refinements = resolved.program.decls.iter().filter_map(|d| match &d.kind {
        DeclKind::Type(t) => t.refinement.as_ref().map(|r| (t.name.clone(), r)),
        _ => None,
    }).collect();
    let mut m = Module { env: &env, types: &types, bodies, refinements, tags, globals: String::new(), strings: HashMap::new(), body: String::new(), queue: VecDeque::new(), seen: HashSet::new(), helpers: HashSet::new() };
    let ret = m.lower(&main.ret, &HashMap::new())?;
    m.want(entry, vec![]);
    while let Some((name, targs)) = m.queue.pop_front() {
        let f = env.fns[&name].clone();
        let shown = if targs.is_empty() { name.clone() } else { format!("{name}[{}]", targs.iter().map(key).collect::<Vec<_>>().join(", ")) };
        m.function(&name, &targs, &f).map_err(|e| format!("can't compile `{shown}`: {e}"))?;
    }
    m.entry(entry, &ret)?;
    let mut out = String::from("; Strata, compiled by `strata build`\n\n");
    out += &m.globals;
    out += "\n";
    out += &m.body;
    out += DECLARATIONS;
    Ok(out)
}

const DECLARATIONS: &str = "\
declare ptr @strata_alloc(i64)
declare ptr @strata_concat(ptr, ptr)
declare i1 @strata_str_eq(ptr, ptr)
declare i64 @strata_str_cmp(ptr, ptr)
declare i64 @strata_str_len(ptr)
declare void @strata_fault(i32, ptr) noreturn
declare void @strata_overflow() noreturn cold
declare void @strata_main_error(ptr)
declare i64 @strata_div(i64, i64)
declare i64 @strata_mod(i64, i64)
declare double @strata_fmod(double, double)
declare double @strata_finite(double, double, double, ptr)
declare i64 @strata_float_to_int(double, ptr)
declare ptr @strata_show_int(i64)
declare ptr @strata_show_float(double)
declare ptr @strata_show_bool(i1)
declare ptr @strata_json_str(ptr)
declare void @strata_print(ptr)
declare { i64, i1 } @llvm.sadd.with.overflow.i64(i64, i64)
declare { i64, i1 } @llvm.ssub.with.overflow.i64(i64, i64)
declare { i64, i1 } @llvm.smul.with.overflow.i64(i64, i64)
declare i64 @llvm.abs.i64(i64, i1)
declare double @llvm.fabs.f64(double)
declare double @llvm.sqrt.f64(double)
declare double @llvm.floor.f64(double)
declare double @llvm.ceil.f64(double)
declare double @llvm.round.f64(double)
declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)
";

/// The LLVM name of a Strata function (prefixed, so it can't clash with C's),
/// one per set of type arguments.
fn sym(name: &str, targs: &[Ty]) -> String {
    if targs.is_empty() {
        format!("@\"strata:{name}\"")
    } else {
        format!("@\"strata:{name}[{}]\"", targs.iter().map(key).collect::<Vec<_>>().join(", "))
    }
}

/// A constructor's fields: name (positional ones have none), written type,
/// and type, with the type's parameters filled in.
struct FieldInfo {
    name: Option<String>,
    texpr: TypeExpr,
    ty: Ty,
}

struct Module<'a> {
    env: &'a Env,
    /// The checker's type of each expression, by node address.
    types: &'a HashMap<usize, Ty>,
    /// Each function's own declaration in the program (its body, `requires`, `ensures`).
    bodies: HashMap<String, &'a FnDecl>,
    /// Each type's refinement as written in the program (the prelude's from `env`).
    refinements: HashMap<String, &'a Expr>,
    /// Constructor -> tag.
    tags: HashMap<String, i64>,
    /// String constants.
    globals: String,
    strings: HashMap<String, String>,
    /// Compiled functions.
    body: String,
    /// Functions still to compile (with their type arguments): those the compiled ones call.
    queue: VecDeque<(String, Vec<Ty>)>,
    seen: HashSet<String>,
    /// Generated helpers (equality, JSON) already emitted.
    helpers: HashSet<String>,
}

impl<'a> Module<'a> {
    fn want(&mut self, name: &str, targs: Vec<Ty>) {
        if self.seen.insert(sym(name, &targs)) {
            self.queue.push_back((name.to_string(), targs));
        }
    }

    /// A string constant, laid out as the runtime's `Str`: its length, then its bytes.
    fn string(&mut self, s: &str) -> String {
        if let Some(g) = self.strings.get(s) {
            return g.clone();
        }
        let g = format!("@.str.{}", self.strings.len());
        let n = s.len();
        let mut bytes = String::new();
        for b in s.bytes() {
            if b.is_ascii_graphic() && b != b'"' && b != b'\\' || b == b' ' {
                bytes.push(b as char);
            } else {
                write!(bytes, "\\{b:02X}").unwrap();
            }
        }
        writeln!(self.globals, "{g} = private unnamed_addr constant {{ i64, [{n} x i8] }} {{ i64 {n}, [{n} x i8] c\"{bytes}\" }}, align 8").unwrap();
        self.strings.insert(s.to_string(), g.clone());
        g
    }

    /// A value of a constructor without fields: one shared block holding its tag.
    fn nullary(&mut self, c: &str) -> String {
        let g = format!("@\"ctor:{c}\"");
        if self.helpers.insert(g.clone()) {
            writeln!(self.globals, "{g} = private unnamed_addr constant i64 {}, align 8", self.tags[c]).unwrap();
        }
        g
    }

    /// The refinement of the type named `n`, as the checker saw it.
    fn refinement(&self, n: &str) -> Option<&'a Expr> {
        self.refinements.get(n).copied().or_else(|| self.env.types.get(n)?.refinement.as_ref())
    }

    fn is_distinct(&self, n: &str) -> bool {
        self.env.types.get(n).is_some_and(|t| t.distinct)
    }

    /// The type a value is represented as: a `distinct` type is its underlying one.
    fn repr(&self, t: &Ty) -> Res<Ty> {
        Ok(match t {
            Ty::Con(n, args) if self.is_distinct(n) => {
                let td = &self.env.types[n];
                let TypeBody::Alias(a) = &td.body else { unreachable!("distinct types are aliases") };
                let sub = td.params.iter().cloned().zip(args.iter().cloned()).collect();
                let base = self.lower(a, &sub)?;
                self.repr(&base)?
            }
            t => t.clone(),
        })
    }

    fn ll(&self, t: &Ty) -> Res<&'static str> {
        Ok(match self.repr(t)? {
            Ty::Int => "i64",
            Ty::Float => "double",
            Ty::Bool => "i1",
            Ty::Unit => "void",
            _ if is_never(t) => "void",
            Ty::Str | Ty::Con(..) | Ty::Fn(..) => "ptr",
            Ty::Union(ms) if ms.iter().all(|m| self.nominal(m)) => "ptr",
            other => return Err(format!("the type `{}` is not compiled yet", key(&other))),
        })
    }

    /// A record, a sum, one of its variants, or a tuple: a block with a tag.
    fn nominal(&self, t: &Ty) -> bool {
        match t {
            Ty::Con(n, _) if n == "Tuple" => true,
            Ty::Con(n, _) => self.env.ctors.contains_key(n) || matches!(self.env.types.get(n), Some(td) if matches!(td.body, TypeBody::Sum(_) | TypeBody::Record(_))),
            Ty::Union(ms) => ms.iter().all(|m| self.nominal(m)),
            _ => false,
        }
    }

    /// A written type as a type, with type parameters from `sub`.
    fn lower(&self, t: &TypeExpr, sub: &HashMap<String, Ty>) -> Res<Ty> {
        Ok(match t {
            TypeExpr::Unit => Ty::Unit,
            TypeExpr::Refined(b, _) => self.lower(b, sub)?,
            TypeExpr::Fn(ps, r) => Ty::Fn(ps.iter().map(|p| self.lower(p, sub)).collect::<Res<_>>()?, Box::new(self.lower(r, sub)?)),
            TypeExpr::Union(ms) => Ty::Union(ms.iter().map(|m| self.lower(m, sub)).collect::<Res<_>>()?),
            TypeExpr::Named(n, args) => {
                if let Some(t) = sub.get(n) {
                    return Ok(t.clone());
                }
                let args: Vec<Ty> = args.iter().map(|a| self.lower(a, sub)).collect::<Res<_>>()?;
                match n.as_str() {
                    "Int" => Ty::Int,
                    "Float" => Ty::Float,
                    "Bool" => Ty::Bool,
                    "String" => Ty::Str,
                    "Unit" => Ty::Unit,
                    _ => match self.env.types.get(n) {
                        Some(td) if !td.distinct && matches!(td.body, TypeBody::Alias(_)) => {
                            let TypeBody::Alias(a) = &td.body else { unreachable!() };
                            let inner = td.params.iter().cloned().zip(args).collect();
                            self.lower(a, &inner)?
                        }
                        _ => Ty::Con(n.clone(), args),
                    },
                }
            }
        })
    }

    /// The constructors a value of type `t` can have.
    fn ctors_of(&self, t: &Ty) -> Vec<String> {
        match t {
            Ty::Con(n, _) if n == "Tuple" => vec![],
            Ty::Con(n, _) => match self.env.types.get(n).map(|td| &td.body) {
                Some(TypeBody::Sum(vs)) => vs.iter().map(|v| v.name.clone()).collect(),
                Some(TypeBody::Record(_)) => vec![n.clone()],
                _ if self.env.ctors.contains_key(n) => vec![n.clone()],
                _ => vec![],
            },
            Ty::Union(ms) => ms.iter().flat_map(|m| self.ctors_of(m)).collect(),
            _ => vec![],
        }
    }

    /// The type arguments of the sum or record `t` is (or is a variant of).
    fn args_of(t: &Ty) -> Vec<Ty> {
        match t {
            Ty::Con(_, a) => a.clone(),
            _ => vec![],
        }
    }

    /// The fields of constructor `c` (of a type with arguments `args`).
    fn fields(&self, c: &str, args: &[Ty]) -> Res<Vec<FieldInfo>> {
        let tname = self.env.ctors.get(c).ok_or(format!("unknown constructor `{c}`"))?;
        let td = &self.env.types[tname];
        let sub: HashMap<String, Ty> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
        let mk = |name: Option<String>, te: &TypeExpr| -> Res<FieldInfo> { Ok(FieldInfo { name, texpr: te.clone(), ty: self.lower(te, &sub)? }) };
        match &td.body {
            TypeBody::Record(fs) => fs.iter().map(|f| mk(Some(f.name.clone()), &f.ty)).collect(),
            TypeBody::Sum(vs) => {
                let v = vs.iter().find(|v| v.name == c).unwrap();
                match &v.payload {
                    Payload::None => Ok(vec![]),
                    Payload::Positional(ts) => ts.iter().map(|t| mk(None, t)).collect(),
                    Payload::Record(fs) => fs.iter().map(|f| mk(Some(f.name.clone()), &f.ty)).collect(),
                }
            }
            TypeBody::Alias(a) => Ok(vec![mk(None, a)?]),
        }
    }

    fn function(&mut self, name: &str, targs: &[Ty], f: &FnDecl) -> Res<()> {
        let own = *self.bodies.get(name).ok_or("trait methods are not compiled yet")?;
        let sub: HashMap<String, Ty> = f.type_params.iter().cloned().zip(targs.iter().cloned()).collect();
        if f.params.iter().any(|p| matches!(&p.ty, TypeExpr::Named(n, _) if n == crate::types::TYPE_ARG)) {
            return Err("trait methods without `self` are not compiled yet".into());
        }
        let mut params = Vec::new();
        for p in &f.params {
            if p.mutable {
                return Err(format!("`mut` parameters (`{}`) are not compiled yet", p.name));
            }
            params.push(self.lower(&p.ty, &sub)?);
        }
        let ret = self.lower(&f.ret, &sub)?;
        let mut c = Func::new(self, sub);
        let mut sig = vec![];
        for (p, t) in f.params.iter().zip(&params) {
            if has_value(t) {
                sig.push(format!("{} %p.{}", c.m.ll(t)?, p.name));
            }
        }
        c.block("entry.body");
        // parameters live in slots, like every variable
        for (p, t) in f.params.iter().zip(&params) {
            let slot = c.local(&p.name, t, None)?;
            c.store(&(t.clone(), format!("%p.{}", p.name)), &slot)?;
        }
        // the written types of the program's own declaration (inline refinements are its nodes)
        for p in &own.params {
            let v = c.load(&p.name)?;
            c.check(&p.ty, &v, &format!("argument `{}` of `{name}`", p.name), false)?;
        }
        for r in &own.requires {
            let ok = c.expr(r)?;
            let (pass, fail) = (c.fresh_label("requires.ok"), c.fresh_label("requires.fail"));
            c.cond_br(&ok, &pass, &fail);
            c.block(&fail);
            let mut parts = vec![Part::Lit(format!("precondition of `{name}` violated: requires {} (with ", crate::printer::expr(r, 0)))];
            for (i, p) in f.params.iter().enumerate() {
                let v = c.load(&p.name)?;
                let shown = c.json(&v)?;
                parts.push(Part::Lit(format!("{}{} = ", if i > 0 { ", " } else { "" }, p.name)));
                parts.push(Part::Val(shown));
            }
            parts.push(Part::Lit(")".into()));
            c.fault(Fault::Requires, parts)?;
            c.block(&pass);
        }
        if has_value(&ret) {
            let slot = c.fresh("ret");
            writeln!(c.allocas, "  {slot} = alloca {}", c.m.ll(&ret)?).unwrap();
            c.ret_slot = Some((ret.clone(), slot));
        } else {
            c.ret_slot = Some((ret.clone(), String::new()));
        }
        let v = c.expr(&own.body)?;
        c.return_value(&v)?;
        // every way out: the result's refinement, then the postconditions
        c.block("exit");
        let result = if has_value(&ret) {
            let slot = c.ret_slot.clone().unwrap().1;
            let r = c.fresh("result");
            c.emit(format!("{r} = load {}, ptr {slot}", c.m.ll(&ret)?));
            (ret.clone(), r)
        } else {
            (ret.clone(), String::new())
        };
        c.check(&own.ret, &result, &format!("result of `{name}`"), true)?;
        if !own.ensures.is_empty() {
            c.scopes.push(HashMap::new());
            let slot = c.local("it", &ret, None)?;
            c.store(&result, &slot)?;
            for en in &own.ensures {
                let ok = c.expr(en)?;
                let (pass, fail) = (c.fresh_label("ensures.ok"), c.fresh_label("ensures.fail"));
                c.cond_br(&ok, &pass, &fail);
                c.block(&fail);
                let shown = c.json(&result)?;
                c.fault(
                    Fault::Ensures,
                    vec![Part::Lit(format!("postcondition of `{name}` violated: ensures {} (it = ", crate::printer::expr(en, 0))), Part::Val(shown), Part::Lit(")".into())],
                )?;
                c.block(&pass);
            }
            c.scopes.pop();
        }
        if has_value(&ret) {
            c.emit(format!("ret {} {}", c.m.ll(&ret)?, result.1));
        } else {
            c.emit("ret void".into());
        }
        let text = c.finish();
        writeln!(self.body, "define {} {}({}) {{\n{text}}}\n", self.ll(&ret)?, sym(name, targs), sig.join(", ")).unwrap();
        Ok(())
    }

    /// C's `main`: call the program's, and turn its result into an exit code.
    fn entry(&mut self, entry: &str, ret: &Ty) -> Res<()> {
        let f = sym(entry, &[]);
        let mut c = Func::new(self, HashMap::new());
        c.block("start");
        match ret {
            Ty::Unit => {
                c.emit(format!("call void {f}()"));
                c.emit("ret i32 0".into());
            }
            Ty::Int => {
                c.emit(format!("%r = call i64 {f}()"));
                c.emit("%c = trunc i64 %r to i32".into());
                c.emit("ret i32 %c".into());
            }
            Ty::Con(n, args) if n == "Result" && matches!(args[0], Ty::Unit | Ty::Int) => {
                c.emit(format!("%r = call ptr {f}()"));
                let r: V = (ret.clone(), "%r".into());
                let tag = c.tag(&r);
                let is_ok = c.fresh("ok");
                c.emit(format!("{is_ok} = icmp eq i64 {tag}, {}", c.m.tags["Ok"]));
                c.emit(format!("br i1 {is_ok}, label %ok, label %err"));
                c.block("ok");
                if args[0] == Ty::Int {
                    let v = c.field(&r, 0, &Ty::Int)?;
                    let code = c.fresh("code");
                    c.emit(format!("{code} = trunc i64 {} to i32", v.1));
                    c.emit(format!("ret i32 {code}"));
                } else {
                    c.emit("ret i32 0".into());
                }
                c.block("err");
                let e = c.field(&r, 0, &args[1])?;
                let shown = c.show(&e)?;
                c.emit(format!("call void @strata_main_error(ptr {shown})"));
                c.emit("ret i32 1".into());
            }
            other => return Err(format!("`{entry}` must return `()`, an exit code (`Int`) or a `Result` of either, not `{}`", key(other))),
        }
        let text = c.finish();
        writeln!(self.body, "define i32 @main() {{\n{text}}}\n").unwrap();
        Ok(())
    }
}

/// A piece of a message built at run time.
enum Part {
    Lit(String),
    /// An operand holding a string.
    Val(String),
}

/// A local variable: its slot, and the declared type every assignment must fit.
#[derive(Clone)]
struct Local {
    t: Ty,
    slot: String,
    declared: Option<TypeExpr>,
}

struct Func<'m, 'a> {
    m: &'m mut Module<'a>,
    /// The type arguments of the instance being compiled.
    sub: HashMap<String, Ty>,
    /// Slots of the variables, in the entry block (so LLVM keeps them in registers).
    allocas: String,
    code: String,
    tmp: usize,
    label: usize,
    scopes: Vec<HashMap<String, Local>>,
    /// The enclosing loops: where `continue` and `break` go.
    loops: Vec<(String, String)>,
    /// The result's type and slot.
    ret_slot: Option<(Ty, String)>,
    /// The current block, and whether it has ended (with a jump or `ret`).
    cur: String,
    terminated: bool,
}

impl<'m, 'a> Func<'m, 'a> {
    fn new(m: &'m mut Module<'a>, sub: HashMap<String, Ty>) -> Self {
        Func { m, sub, allocas: String::new(), code: String::new(), tmp: 0, label: 0, scopes: vec![HashMap::new()], loops: vec![], ret_slot: None, cur: String::new(), terminated: true }
    }

    /// The function's text: the slots, then the code.
    fn finish(&mut self) -> String {
        if !self.terminated {
            self.emit("unreachable".into());
        }
        let first = self.code.lines().next().unwrap_or("").trim_end_matches(':').to_string();
        format!("entry:\n{}  br label %{first}\n{}", self.allocas, self.code)
    }

    fn emit(&mut self, line: String) {
        self.terminated = line.starts_with("br ") || line.starts_with("ret") || line == "unreachable" || line.starts_with("switch ");
        self.code += "  ";
        self.code += &line;
        self.code += "\n";
    }

    /// Start a block. Every intended way into it is an explicit jump, so a
    /// block still open here can't be reached (it follows a `return` or a fault).
    fn block(&mut self, label: &str) {
        if !self.terminated {
            self.emit("unreachable".into());
        }
        self.terminated = false;
        writeln!(self.code, "{label}:").unwrap();
        self.cur = label.to_string();
    }

    fn fresh(&mut self, hint: &str) -> String {
        self.tmp += 1;
        format!("%{hint}.{}", self.tmp)
    }

    fn fresh_label(&mut self, hint: &str) -> String {
        self.label += 1;
        format!("{hint}.{}", self.label)
    }

    fn br(&mut self, to: &str) {
        self.emit(format!("br label %{to}"));
    }

    fn cond_br(&mut self, c: &V, then: &str, els: &str) {
        self.emit(format!("br i1 {}, label %{then}, label %{els}", c.1));
    }

    /// After a jump: whatever follows is unreachable, but needs a block.
    fn dead(&mut self) {
        let l = self.fresh_label("dead");
        self.block(&l);
    }

    /// The type the checker inferred for `e`, in this instance.
    fn ty_of(&self, e: &Expr) -> Res<Ty> {
        let t = self.m.types.get(&(e as *const Expr as usize)).ok_or_else(|| format!("no type for `{}`", crate::printer::expr(e, 0)))?;
        Ok(self.concrete(t))
    }

    /// A type with the instance's type arguments filled in; what the checker
    /// left open (`[]` never used, `None` printed) is `()`.
    fn concrete(&self, t: &Ty) -> Ty {
        match t {
            Ty::Rigid(n) => self.sub.get(n).cloned().unwrap_or(Ty::Unit),
            Ty::Var(_) => Ty::Unit,
            Ty::Con(n, a) => Ty::Con(n.clone(), a.iter().map(|x| self.concrete(x)).collect()),
            Ty::Fn(ps, r) => Ty::Fn(ps.iter().map(|x| self.concrete(x)).collect(), Box::new(self.concrete(r))),
            Ty::Union(ms) => Ty::Union(ms.iter().map(|x| self.concrete(x)).collect()),
            t => t.clone(),
        }
    }

    fn lower(&self, t: &TypeExpr) -> Res<Ty> {
        self.m.lower(t, &self.sub)
    }

    fn local(&mut self, name: &str, t: &Ty, declared: Option<TypeExpr>) -> Res<String> {
        let slot = self.fresh(&format!("v.{}", name.replace(|c: char| !c.is_ascii_alphanumeric() && c != '_', "_")));
        if has_value(t) {
            writeln!(self.allocas, "  {slot} = alloca {}", self.m.ll(t)?).unwrap();
        }
        self.scopes.last_mut().unwrap().insert(name.to_string(), Local { t: t.clone(), slot: slot.clone(), declared });
        Ok(slot)
    }

    fn lookup(&self, name: &str) -> Option<Local> {
        self.scopes.iter().rev().find_map(|s| s.get(name).cloned())
    }

    fn store(&mut self, v: &V, slot: &str) -> Res<()> {
        if has_value(&v.0) {
            let ll = self.m.ll(&v.0)?;
            self.emit(format!("store {ll} {}, ptr {slot}", v.1));
        }
        Ok(())
    }

    fn load(&mut self, name: &str) -> Res<V> {
        let l = self.lookup(name).ok_or(format!("`{name}` is not a local variable (function values are not compiled yet)"))?;
        if !has_value(&l.t) {
            return Ok((l.t, String::new()));
        }
        let r = self.fresh("x");
        let ll = self.m.ll(&l.t)?;
        self.emit(format!("{r} = load {ll}, ptr {}", l.slot));
        Ok((l.t, r))
    }

    fn call(&mut self, ret: &Ty, f: &str, args: &[V]) -> Res<V> {
        let mut xs = vec![];
        for a in args.iter().filter(|a| has_value(&a.0)) {
            xs.push(format!("{} {}", self.m.ll(&a.0)?, a.1));
        }
        if has_value(ret) {
            let r = self.fresh("c");
            let ll = self.m.ll(ret)?;
            self.emit(format!("{r} = call {ll} {f}({})", xs.join(", ")));
            Ok((ret.clone(), r))
        } else {
            self.emit(format!("call void {f}({})", xs.join(", ")));
            Ok((ret.clone(), String::new()))
        }
    }

    fn string(&mut self, s: &str) -> V {
        (Ty::Str, self.m.string(s))
    }

    /// A string built from pieces.
    fn concat(&mut self, parts: Vec<Part>) -> Res<V> {
        let mut acc: Option<V> = None;
        for p in parts {
            let v = match p {
                Part::Lit(s) if s.is_empty() => continue,
                Part::Lit(s) => self.string(&s),
                Part::Val(op) => (Ty::Str, op),
            };
            acc = Some(match acc {
                None => v,
                Some(a) => self.call(&Ty::Str, "@strata_concat", &[a, v])?,
            });
        }
        Ok(acc.unwrap_or_else(|| self.string("")))
    }

    /// Stop with a fault; the code after it is unreachable.
    fn fault(&mut self, kind: Fault, msg: Vec<Part>) -> Res<()> {
        let m = self.concat(msg)?;
        self.emit(format!("call void @strata_fault(i32 {}, ptr {})", kind as i32, m.1));
        self.emit("unreachable".into());
        self.dead();
        Ok(())
    }

    // ---- values in blocks ----

    /// A value as a 64-bit slot.
    fn to_slot(&mut self, v: &V) -> Res<String> {
        let r = self.fresh("slot");
        match self.m.repr(&v.0)? {
            Ty::Int => return Ok(v.1.clone()),
            Ty::Unit => return Ok("0".into()),
            _ if is_never(&v.0) => return Ok("0".into()),
            Ty::Float => self.emit(format!("{r} = bitcast double {} to i64", v.1)),
            Ty::Bool => self.emit(format!("{r} = zext i1 {} to i64", v.1)),
            _ => self.emit(format!("{r} = ptrtoint ptr {} to i64", v.1)),
        }
        Ok(r)
    }

    /// A 64-bit slot as a value of type `t`.
    fn from_slot(&mut self, s: &str, t: &Ty) -> Res<V> {
        let r = self.fresh("val");
        match self.m.repr(t)? {
            Ty::Int => return Ok((t.clone(), s.to_string())),
            Ty::Unit => return Ok((Ty::Unit, String::new())),
            Ty::Float => self.emit(format!("{r} = bitcast i64 {s} to double")),
            Ty::Bool => self.emit(format!("{r} = trunc i64 {s} to i1")),
            _ => self.emit(format!("{r} = inttoptr i64 {s} to ptr")),
        }
        Ok((t.clone(), r))
    }

    /// A new block: a tag, then slots.
    fn block_of(&mut self, tag: i64, fields: &[V]) -> Res<String> {
        let p = self.fresh("obj");
        self.emit(format!("{p} = call ptr @strata_alloc(i64 {})", 8 * (fields.len() + 1)));
        self.emit(format!("store i64 {tag}, ptr {p}"));
        for (i, f) in fields.iter().enumerate() {
            let s = self.to_slot(f)?;
            let at = self.fresh("at");
            self.emit(format!("{at} = getelementptr i64, ptr {p}, i64 {}", i + 1));
            self.emit(format!("store i64 {s}, ptr {at}"));
        }
        Ok(p)
    }

    fn tag(&mut self, v: &V) -> String {
        let t = self.fresh("tag");
        self.emit(format!("{t} = load i64, ptr {}", v.1));
        t
    }

    /// Field `i` of a block, as a value of type `t`.
    fn field(&mut self, v: &V, i: usize, t: &Ty) -> Res<V> {
        let (at, s) = (self.fresh("at"), self.fresh("f"));
        self.emit(format!("{at} = getelementptr i64, ptr {}, i64 {}", v.1, i + 1));
        self.emit(format!("{s} = load i64, ptr {at}"));
        self.from_slot(&s, t)
    }

    /// A copy of a block of `n` fields.
    fn copy_block(&mut self, v: &V, n: usize) -> String {
        let p = self.fresh("copy");
        let size = 8 * (n + 1);
        self.emit(format!("{p} = call ptr @strata_alloc(i64 {size})"));
        self.emit(format!("call void @llvm.memcpy.p0.p0.i64(ptr {p}, ptr {}, i64 {size}, i1 false)", v.1));
        p
    }

    fn set_field(&mut self, p: &str, i: usize, v: &V) -> Res<()> {
        let s = self.to_slot(v)?;
        let at = self.fresh("at");
        self.emit(format!("{at} = getelementptr i64, ptr {p}, i64 {}", i + 1));
        self.emit(format!("store i64 {s}, ptr {at}"));
        Ok(())
    }

    /// Whether `v`'s tag is one of `cs` (always, when it can't be anything else).
    fn tag_in(&mut self, v: &V, cs: &[String]) -> Res<V> {
        let all = self.m.ctors_of(&self.m.repr(&v.0)?);
        if !all.is_empty() && all.iter().all(|c| cs.contains(c)) {
            return Ok(bool_v("true"));
        }
        let tag = self.tag(v);
        let mut acc = bool_v("false");
        for c in cs {
            let Some(&n) = self.m.tags.get(c) else { continue };
            let (e, o) = (self.fresh("is"), self.fresh("or"));
            self.emit(format!("{e} = icmp eq i64 {tag}, {n}"));
            self.emit(format!("{o} = or i1 {}, {e}", acc.1));
            acc = bool_v(&o);
        }
        Ok(acc)
    }

    /// `switch` on the constructor of `v`: one block per possible constructor,
    /// each given to `each`, which must end its block; then `end` continues.
    fn on_ctor(&mut self, v: &V, end: &str, mut each: impl FnMut(&mut Self, &str) -> Res<()>) -> Res<()> {
        let cs = self.m.ctors_of(&self.m.repr(&v.0)?);
        if cs.len() == 1 {
            return each(self, &cs[0]);
        }
        let tag = self.tag(v);
        let labels: Vec<String> = cs.iter().map(|c| self.fresh_label(&format!("case.{}", crate::names::short(c)))).collect();
        let cases: Vec<String> = cs.iter().zip(&labels).map(|(c, l)| format!("i64 {}, label %{l}", self.m.tags[c])).collect();
        let other = self.fresh_label("case.none");
        self.emit(format!("switch i64 {tag}, label %{other} [ {} ]", cases.join(" ")));
        for (c, l) in cs.iter().zip(&labels) {
            self.block(l);
            each(self, c)?;
        }
        self.block(&other);
        self.emit("unreachable".into());
        let _ = end;
        Ok(())
    }

    // ---- what values look like ----

    /// A value as `to_string` shows it: a string as itself, a variant without
    /// fields as its name, everything else as JSON.
    fn show(&mut self, v: &V) -> Res<String> {
        let t = self.m.repr(&v.0)?;
        match t {
            Ty::Str => Ok(v.1.clone()),
            t if self.m.nominal(&t) && self.m.ctors_of(&t).iter().any(|c| self.m.fields(c, &[]).map(|f| f.is_empty()).unwrap_or(false)) => {
                let f = self.helper("show", &t)?;
                Ok(self.call(&Ty::Str, &f, &[(t.clone(), v.1.clone())])?.1)
            }
            _ => self.json(v),
        }
    }

    /// A value as JSON, for messages and nested in other values.
    fn json(&mut self, v: &V) -> Res<String> {
        let t = self.m.repr(&v.0)?;
        Ok(match &t {
            Ty::Int => self.call(&Ty::Str, "@strata_show_int", &[v.clone()])?.1,
            Ty::Float => self.call(&Ty::Str, "@strata_show_float", &[v.clone()])?.1,
            Ty::Bool => self.call(&Ty::Str, "@strata_show_bool", &[v.clone()])?.1,
            Ty::Str => self.call(&Ty::Str, "@strata_json_str", &[v.clone()])?.1,
            Ty::Unit => self.m.string("null"),
            _ if is_never(&t) => self.m.string("null"),
            t if self.m.nominal(t) => {
                let f = self.helper("json", t)?;
                self.call(&Ty::Str, &f, &[(t.clone(), v.1.clone())])?.1
            }
            t => return Err(format!("showing a `{}` is not compiled yet", key(t))),
        })
    }

    /// Whether two values are equal (structurally).
    fn equal(&mut self, a: &V, b: &V) -> Res<V> {
        let t = self.m.repr(&a.0)?;
        Ok(match &t {
            Ty::Unit => bool_v("true"),
            Ty::Int | Ty::Bool => {
                let r = self.fresh("eq");
                self.emit(format!("{r} = icmp eq {} {}, {}", self.m.ll(&t)?, a.1, b.1));
                bool_v(&r)
            }
            Ty::Float => {
                let r = self.fresh("eq");
                self.emit(format!("{r} = fcmp oeq double {}, {}", a.1, b.1));
                bool_v(&r)
            }
            Ty::Str => self.call(&Ty::Bool, "@strata_str_eq", &[a.clone(), b.clone()])?,
            t if self.m.nominal(t) => {
                let f = self.helper("eq", t)?;
                self.call(&Ty::Bool, &f, &[(t.clone(), a.1.clone()), (t.clone(), b.1.clone())])?
            }
            t => return Err(format!("comparing `{}` values is not compiled yet", key(t))),
        })
    }

    /// A generated function on values of type `t` (`eq`, `json`, `show`).
    fn helper(&mut self, what: &str, t: &Ty) -> Res<String> {
        let name = format!("@\"{what}:{}\"", key(t));
        if !self.m.helpers.insert(name.clone()) {
            return Ok(name);
        }
        let mut h = Func::new(self.m, HashMap::new());
        h.block("start");
        let a: V = (t.clone(), "%a".into());
        let b: V = (t.clone(), "%b".into());
        let tuple = matches!(t, Ty::Con(n, _) if n == "Tuple");
        let args = Module::args_of(t);
        let sig = match what {
            "eq" => {
                // the same block, or the same constructor and equal fields
                let same = h.fresh("same");
                h.emit(format!("{same} = icmp eq ptr %a, %b"));
                h.emit(format!("br i1 {same}, label %yes, label %compare"));
                h.block("compare");
                let (ta, tb) = (h.tag(&a), h.tag(&b));
                let st = h.fresh("sametag");
                h.emit(format!("{st} = icmp eq i64 {ta}, {tb}"));
                h.emit(format!("br i1 {st}, label %fields, label %no"));
                h.block("fields");
                let field_lists: Vec<(Option<String>, Vec<Ty>)> = if tuple {
                    vec![(None, args.clone())]
                } else {
                    h.m.ctors_of(t).into_iter().map(|c| Ok((Some(c.clone()), h.m.fields(&c, &h.m.ctor_args(&c, t))?.into_iter().map(|f| f.ty).collect()))).collect::<Res<_>>()?
                };
                let compare = |h: &mut Func, tys: &[Ty]| -> Res<()> {
                    for (i, ft) in tys.iter().enumerate() {
                        let (x, y) = (h.field(&a, i, ft)?, h.field(&b, i, ft)?);
                        let e = h.equal(&x, &y)?;
                        let next = h.fresh_label("next");
                        h.emit(format!("br i1 {}, label %{next}, label %no", e.1));
                        h.block(&next);
                    }
                    h.br("yes");
                    Ok(())
                };
                if tuple {
                    compare(&mut h, &args)?;
                } else {
                    h.on_ctor(&a, "yes", |h, c| {
                        let tys = field_lists.iter().find(|(n, _)| n.as_deref() == Some(c)).map(|x| x.1.clone()).unwrap_or_default();
                        compare(h, &tys)
                    })?;
                }
                h.block("yes");
                h.emit("ret i1 true".into());
                h.block("no");
                h.emit("ret i1 false".into());
                format!("define i1 {name}(ptr %a, ptr %b)")
            }
            "show" => {
                h.on_ctor(&a, "", |h, c| {
                    if h.m.fields(c, &[])?.is_empty() {
                        let s = h.m.string(crate::names::short(c));
                        h.emit(format!("ret ptr {s}"));
                    } else {
                        let j = h.json(&a)?;
                        h.emit(format!("ret ptr {j}"));
                    }
                    Ok(())
                })?;
                format!("define ptr {name}(ptr %a)")
            }
            _ => {
                // serde_json's compact form, fields in declaration order
                let fields_json = |h: &mut Func, fs: &[FieldInfo], parts: &mut Vec<Part>, record: bool| -> Res<()> {
                    parts.push(Part::Lit(if record { "{" } else { "[" }.into()));
                    for (i, f) in fs.iter().enumerate() {
                        if i > 0 {
                            parts.push(Part::Lit(",".into()));
                        }
                        if let (true, Some(n)) = (record, &f.name) {
                            parts.push(Part::Lit(format!("{}:", serde_json::to_string(n).unwrap())));
                        }
                        let x = h.field(&a, i, &f.ty)?;
                        let s = h.json(&x)?;
                        parts.push(Part::Val(s));
                    }
                    parts.push(Part::Lit(if record { "}" } else { "]" }.into()));
                    Ok(())
                };
                if tuple {
                    let fs: Vec<FieldInfo> = args.iter().map(|t| FieldInfo { name: None, texpr: TypeExpr::Unit, ty: t.clone() }).collect();
                    let mut parts = vec![];
                    fields_json(&mut h, &fs, &mut parts, false)?;
                    let s = h.concat(parts)?;
                    h.emit(format!("ret ptr {}", s.1));
                } else {
                    let tc = t.clone();
                    h.on_ctor(&a, "", |h, c| {
                        let fs = h.m.fields(c, &h.m.ctor_args(c, &tc))?;
                        let is_record = matches!(h.m.env.types.get(c).map(|td| &td.body), Some(TypeBody::Record(_)));
                        let named = |c: &str| serde_json::to_string(crate::names::short(c)).unwrap();
                        let mut parts = vec![];
                        if is_record {
                            fields_json(h, &fs, &mut parts, true)?;
                        } else if fs.is_empty() {
                            parts.push(Part::Lit(named(c)));
                        } else if fs.len() == 1 && fs[0].name.is_none() {
                            parts.push(Part::Lit(format!("{{{}:", named(c))));
                            let x = h.field(&a, 0, &fs[0].ty)?;
                            let s = h.json(&x)?;
                            parts.push(Part::Val(s));
                            parts.push(Part::Lit("}".into()));
                        } else {
                            parts.push(Part::Lit(format!("{{{}:", named(c))));
                            fields_json(h, &fs, &mut parts, fs[0].name.is_some())?;
                            parts.push(Part::Lit("}".into()));
                        }
                        let s = h.concat(parts)?;
                        h.emit(format!("ret ptr {}", s.1));
                        Ok(())
                    })?;
                }
                format!("define ptr {name}(ptr %a)")
            }
        };
        let text = h.finish();
        writeln!(self.m.body, "{sig} {{\n{text}}}\n").unwrap();
        Ok(name)
    }

    // ---- contracts ----

    /// Fault unless `v` is a value of type `t`: the refinements reachable
    /// through it (`Interp::check_type_in`). `outer` says whether the
    /// enclosing scope is visible to an inline refinement (a function's
    /// parameters, for its result type). A record or variant was checked
    /// when it was built, so only fields of a type argument's type are.
    fn check(&mut self, t: &TypeExpr, v: &V, ctx: &str, outer: bool) -> Res<()> {
        if !has_value(&v.0) || !self.needs_check(t) {
            return Ok(());
        }
        match t {
            TypeExpr::Refined(b, pred) => {
                self.check(b, v, ctx, false)?;
                let ok = self.with_it(v, outer, |c| c.expr(pred))?;
                let msg = format!("does not satisfy `{}`", crate::printer::ty(t));
                self.fail_unless(&ok, ctx, v, msg)?;
            }
            TypeExpr::Named(n, args) => {
                let n = match self.m.env.variant_parent(n) {
                    Some(p) => p,
                    None => n.clone(),
                };
                let Some(td) = self.m.env.types.get(&n).cloned() else { return Ok(()) };
                let sub: HashMap<String, TypeExpr> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
                match &td.body {
                    TypeBody::Alias(a) => self.check(&crate::types::subst_texpr(a, &sub), v, ctx, false)?,
                    _ => {
                        // fields whose type is a type argument (`Option[Positive]`)
                        let vt = v.0.clone();
                        let end = self.fresh_label("checked");
                        self.on_ctor(v, &end, |c, ctor| {
                            let fs = c.m.fields(ctor, &c.m.ctor_args(ctor, &vt))?;
                            for (i, f) in fs.iter().enumerate() {
                                let ft = crate::types::subst_texpr(&f.texpr, &sub);
                                if f.texpr != ft && c.needs_check(&ft) {
                                    let x = c.field(v, i, &f.ty)?;
                                    let fname = f.name.clone().unwrap_or(i.to_string());
                                    c.check(&ft, &x, &format!("{ctx}: {}.{fname}", crate::names::short(ctor)), false)?;
                                }
                            }
                            c.br(&end);
                            Ok(())
                        })?;
                        self.block(&end);
                        return Ok(());
                    }
                }
                if let (Some(r), false) = (self.m.refinement(&n), td.distinct) {
                    let ok = self.with_it(v, false, |c| c.expr(r))?;
                    let msg = format!("does not satisfy `{n}` (where {})", crate::printer::expr(r, 0));
                    self.fail_unless(&ok, ctx, v, msg)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Whether a value of this type can fail `check`.
    fn needs_check(&self, t: &TypeExpr) -> bool {
        match t {
            TypeExpr::Refined(..) => true,
            TypeExpr::Named(n, args) => {
                if self.sub.contains_key(n) {
                    return false;
                }
                let n = self.m.env.variant_parent(n).unwrap_or(n.clone());
                match self.m.env.types.get(&n) {
                    Some(td) if td.distinct => false,
                    Some(td) => match &td.body {
                        TypeBody::Alias(a) => td.refinement.is_some() || self.needs_check(a) || args.iter().any(|x| self.needs_check(x)),
                        _ => args.iter().any(|x| self.needs_check(x)),
                    },
                    None => false,
                }
            }
            _ => false,
        }
    }

    /// The refinements of constructor `c`'s fields and of its type, on a new value.
    fn check_new(&mut self, c: &str, fields: &[V], fs: &[FieldInfo]) -> Res<()> {
        let ctx = format!("constructing `{c}`");
        for (i, (f, x)) in fs.iter().zip(fields).enumerate() {
            let fname = f.name.clone().unwrap_or(i.to_string());
            self.check(&f.texpr, x, &format!("{ctx}: {}.{fname}", crate::names::short(c)), false)?;
        }
        Ok(())
    }

    /// The refinement of the type itself (`type Range = { lo: Int, hi: Int } where it.lo <= it.hi`).
    fn check_type_refinement(&mut self, c: &str, v: &V) -> Res<()> {
        let tname = self.m.env.ctors[c].clone();
        if let Some(r) = self.m.refinement(&tname) {
            let ok = self.with_it(v, false, |s| s.expr(r))?;
            self.fail_unless(&ok, &format!("constructing `{c}`"), v, format!("does not satisfy `{tname}` (where {})", crate::printer::expr(&r, 0)))?;
        }
        Ok(())
    }

    fn fail_unless(&mut self, ok: &V, ctx: &str, v: &V, msg: String) -> Res<()> {
        let (pass, fail) = (self.fresh_label("check.ok"), self.fresh_label("check.fail"));
        self.cond_br(ok, &pass, &fail);
        self.block(&fail);
        let shown = self.json(v)?;
        self.fault(Fault::Refinement, vec![Part::Lit(format!("{ctx}: ")), Part::Val(shown), Part::Lit(format!(" {msg}"))])?;
        self.block(&pass);
        Ok(())
    }

    /// Whether `v` is a value of the type named `name` (a type's name as a
    /// pattern, or `is`): its constructor, and every refinement.
    fn holds(&mut self, name: &str, v: &V) -> Res<V> {
        let mut conds = vec![];
        if self.m.nominal(&self.m.repr(&v.0)?) {
            let target = Ty::Con(name.to_string(), Module::args_of(&v.0));
            let cs = self.m.ctors_of(&target);
            conds.push(self.tag_in(v, &cs)?);
        }
        let mut t = TypeExpr::Named(name.to_string(), vec![]);
        loop {
            match t {
                TypeExpr::Refined(b, pred) => {
                    conds.push(self.with_it(v, false, |c| c.expr(&pred))?);
                    t = *b;
                }
                TypeExpr::Named(n, _) => {
                    let Some(td) = self.m.env.types.get(&n).cloned() else { break };
                    if let Some(r) = self.m.refinement(&n) {
                        conds.push(self.with_it(v, false, |c| c.expr(r))?);
                    }
                    let TypeBody::Alias(a) = td.body else { break };
                    t = a;
                }
                _ => break,
            }
        }
        // all of them, though each is evaluated (refinements are pure)
        let mut acc = bool_v("true");
        for c in conds {
            let r = self.fresh("and");
            self.emit(format!("{r} = and i1 {}, {}", acc.1, c.1));
            acc = bool_v(&r);
        }
        Ok(acc)
    }

    /// Evaluate `f` with `it` bound to `v` (and only that, unless `outer`).
    fn with_it(&mut self, v: &V, outer: bool, f: impl FnOnce(&mut Self) -> Res<V>) -> Res<V> {
        let saved = if outer { None } else { Some(std::mem::replace(&mut self.scopes, vec![HashMap::new()])) };
        self.scopes.push(HashMap::new());
        let slot = self.local("it", &v.0, None);
        let r = slot.and_then(|slot| self.store(v, &slot)).and_then(|_| f(self));
        self.scopes.pop();
        if let Some(s) = saved {
            self.scopes = s;
        }
        let r = r?;
        if r.0 != Ty::Bool {
            return Err("a refinement must be a `Bool`".into());
        }
        Ok(r)
    }

    fn return_value(&mut self, v: &V) -> Res<()> {
        if is_never(&v.0) {
            return Ok(());
        }
        let (t, slot) = self.ret_slot.clone().ok_or("`return` outside a function")?;
        if has_value(&t) {
            let ll = self.m.ll(&t)?;
            self.emit(format!("store {ll} {}, ptr {slot}", v.1));
        }
        self.br("exit");
        Ok(())
    }

    fn unsupported<X>(&self, what: &str) -> Res<X> {
        Err(format!("{what} is not compiled yet"))
    }

    // ---- expressions ----

    fn expr(&mut self, e: &Expr) -> Res<V> {
        Ok(match e {
            Expr::Int(n) => (Ty::Int, n.to_string()),
            Expr::BigInt(n) => return self.unsupported(&format!("the big integer {n}")),
            Expr::Float(x) => (Ty::Float, format!("0x{:016X}", x.to_bits())),
            Expr::Bool(b) => bool_v(&b.to_string()),
            Expr::Str(s) => self.string(s),
            Expr::Unit => (Ty::Unit, String::new()),
            Expr::Var(x) => self.load(x)?,
            Expr::Hole(h) => {
                self.fault(Fault::Hole, vec![Part::Lit(format!("reached unfilled hole `?{h}`"))])?;
                never()
            }
            Expr::Call(f, args) => match &**f {
                Expr::Var(name) => self.call_named(e, name, args.iter().collect())?,
                _ => return self.unsupported("calling a function value"),
            },
            // method-call sugar: `e.f(args)` is `f(e, args)`
            Expr::Method(recv, m, args) => {
                if m.contains("::") {
                    return self.unsupported(&format!("the trait method `{m}`"));
                }
                self.call_named(e, m, std::iter::once(&**recv).chain(args.iter()).collect())?
            }
            Expr::Bin(op, l, r) => self.binary(*op, l, r)?,
            Expr::Un(UnOp::Not, x) => {
                let v = self.expr(x)?;
                if is_never(&v.0) {
                    return Ok(v);
                }
                let r = self.fresh("not");
                self.emit(format!("{r} = xor i1 {}, true", v.1));
                bool_v(&r)
            }
            Expr::Un(UnOp::Neg, x) => {
                let v = self.expr(x)?;
                match self.m.repr(&v.0)? {
                    Ty::Int => self.int_op("ssub", &(Ty::Int, "0".into()), &v),
                    Ty::Float => {
                        let r = self.fresh("neg");
                        self.emit(format!("{r} = fneg double {}", v.1));
                        (Ty::Float, r)
                    }
                    _ if is_never(&v.0) => v,
                    t => return Err(format!("can't negate a `{}`", key(&t))),
                }
            }
            Expr::If(c, t, f) => {
                let want = self.ty_of(e)?;
                let cv = self.expr(c)?;
                let (lt, lf, end) = (self.fresh_label("then"), self.fresh_label("else"), self.fresh_label("endif"));
                self.cond_br(&cv, &lt, &lf);
                self.block(&lt);
                let tv = self.expr(t)?;
                let from_t = self.cur.clone();
                self.br(&end);
                self.block(&lf);
                let fv = self.expr(f)?;
                let from_f = self.cur.clone();
                self.br(&end);
                self.block(&end);
                self.join(&want, vec![(tv, from_t), (fv, from_f)])?
            }
            Expr::Block(stmts, last) => {
                self.scopes.push(HashMap::new());
                for s in stmts {
                    self.stmt(s)?;
                }
                let v = self.expr(last)?;
                self.scopes.pop();
                v
            }
            Expr::Return(x) => {
                let v = self.expr(x)?;
                self.return_value(&v)?;
                self.dead();
                never()
            }
            Expr::Interp(parts) => {
                let mut ps = vec![];
                for p in parts {
                    match p {
                        StrPart::Lit(s) => ps.push(Part::Lit(s.clone())),
                        StrPart::Expr(x, None) => {
                            let v = self.expr(x)?;
                            let s = self.show(&v)?;
                            ps.push(Part::Val(s));
                        }
                        StrPart::Expr(_, Some(_)) => return self.unsupported("a format spec in an f-string"),
                    }
                }
                self.concat(ps)?
            }
            Expr::Match(scrut, as_name, arms) => self.matching(e, scrut, as_name.as_deref(), arms)?,
            Expr::Tuple(xs) => {
                let t = self.ty_of(e)?;
                let mut vs = vec![];
                for x in xs {
                    vs.push(self.expr(x)?);
                }
                if vs.iter().any(|v| is_never(&v.0)) {
                    return Ok(never());
                }
                (t, self.block_of(0, &vs)?)
            }
            Expr::Ctor(c, args) => self.construct(e, c, args)?,
            Expr::Field(r, f) => self.get_field(r, f)?,
            Expr::Is(x, name) => {
                let v = self.expr(x)?;
                if self.m.env.ctors.contains_key(name) && !self.m.is_distinct(name) {
                    self.tag_in(&v, &[name.clone()])?
                } else if self.m.env.is_type_name(name) {
                    self.holds(name, &v)?
                } else {
                    return self.unsupported(&format!("`is {name}`"));
                }
            }
            Expr::Try(x, kind) => {
                let v = self.expr(x)?;
                let (good, bad) = match kind {
                    TryKind::Result => ("Ok", "Err"),
                    TryKind::Option => ("Some", "None"),
                };
                let t = self.ty_of(e)?;
                let is_good = self.tag_in(&v, &[good.into()])?;
                let (ok, fail) = (self.fresh_label("try.ok"), self.fresh_label("try.fail"));
                self.cond_br(&is_good, &ok, &fail);
                self.block(&fail);
                // the `Err` or `None` itself is what the function returns
                let _ = bad;
                self.return_value(&v)?;
                self.block(&ok);
                self.field(&v, 0, &t)?
            }
            Expr::Except(x, name, fallback) => {
                let want = self.ty_of(e)?;
                let v = self.expr(x)?;
                let vt = self.m.repr(&v.0)?;
                let good = match &vt {
                    Ty::Con(n, _) if n == "Option" => "Some",
                    _ => "Ok",
                };
                let is_good = self.tag_in(&v, &[good.into()])?;
                let (ok, fail, end) = (self.fresh_label("except.ok"), self.fresh_label("except.else"), self.fresh_label("except.end"));
                self.cond_br(&is_good, &ok, &fail);
                self.block(&ok);
                let gv = self.field(&v, 0, &want)?;
                let from_ok = self.cur.clone();
                self.br(&end);
                self.block(&fail);
                self.scopes.push(HashMap::new());
                if let Some(n) = name {
                    let et = match &vt {
                        Ty::Con(n, a) if n == "Result" => a[1].clone(),
                        _ => Ty::Unit,
                    };
                    let err = self.field(&v, 0, &et)?;
                    let slot = self.local(n, &et, None)?;
                    self.store(&err, &slot)?;
                }
                let fv = self.expr(fallback)?;
                self.scopes.pop();
                let from_f = self.cur.clone();
                self.br(&end);
                self.block(&end);
                self.join(&want, vec![(gv, from_ok), (fv, from_f)])?
            }
            Expr::List(_) => return self.unsupported("a list"),
            Expr::Lambda(..) => return self.unsupported("a lambda"),
            Expr::Mut(_) => return self.unsupported("a `mut` argument"),
            Expr::Mock(_) => return self.unsupported("`mock`"),
        })
    }

    /// The value of a branching expression, from the branches that produce one.
    fn join(&mut self, want: &Ty, incoming: Vec<(V, String)>) -> Res<V> {
        if incoming.iter().all(|(v, _)| is_never(&v.0)) {
            self.emit("unreachable".into());
            self.dead();
            return Ok(never());
        }
        if !has_value(want) {
            return Ok((want.clone(), String::new()));
        }
        let ll = self.m.ll(want)?;
        for (v, _) in &incoming {
            if !is_never(&v.0) && self.m.ll(&v.0)? != ll {
                return Err(format!("branches of types `{}` and `{}` (unions of these) are not compiled yet", key(&v.0), key(want)));
            }
        }
        let r = self.fresh("phi");
        // a branch that never gets here still jumps here from its (unreachable) end
        let ins: Vec<String> = incoming.iter().map(|((u, op), from)| format!("[ {}, %{from} ]", if is_never(u) { "poison" } else { op })).collect();
        self.emit(format!("{r} = phi {ll} {}", ins.join(", ")));
        Ok((want.clone(), r))
    }

    /// `+`, `-`, `*` on `Int`: a fault on overflow.
    fn int_op(&mut self, op: &str, a: &V, b: &V) -> V {
        let (pair, r, ov) = (self.fresh("pair"), self.fresh("int"), self.fresh("ov"));
        self.emit(format!("{pair} = call {{ i64, i1 }} @llvm.{op}.with.overflow.i64(i64 {}, i64 {})", a.1, b.1));
        self.emit(format!("{r} = extractvalue {{ i64, i1 }} {pair}, 0"));
        self.emit(format!("{ov} = extractvalue {{ i64, i1 }} {pair}, 1"));
        let (bad, ok) = (self.fresh_label("overflow"), self.fresh_label("no_overflow"));
        self.emit(format!("br i1 {ov}, label %{bad}, label %{ok}"));
        self.block(&bad);
        self.emit("call void @strata_overflow()".into());
        self.emit("unreachable".into());
        self.block(&ok);
        (Ty::Int, r)
    }

    fn binary(&mut self, op: BinOp, l: &Expr, r: &Expr) -> Res<V> {
        use BinOp::*;
        if matches!(op, And | Or | Implies) {
            // short-circuit: the right side runs only when it decides
            let a = self.expr(l)?;
            if is_never(&a.0) {
                return Ok(a);
            }
            let from = self.cur.clone();
            let (rhs, end) = (self.fresh_label("rhs"), self.fresh_label("logic"));
            let short = match op {
                And => {
                    self.cond_br(&a, &rhs, &end);
                    "false"
                }
                Or => {
                    self.cond_br(&a, &end, &rhs);
                    "true"
                }
                _ => {
                    self.cond_br(&a, &rhs, &end);
                    "true"
                }
            };
            self.block(&rhs);
            let b = self.expr(r)?;
            let from_b = self.cur.clone();
            self.br(&end);
            self.block(&end);
            return self.join(&Ty::Bool, vec![(bool_v(short), from), (b, from_b)]);
        }
        let a = self.expr(l)?;
        let b = self.expr(r)?;
        self.apply(op, &a, &b)
    }

    fn apply(&mut self, op: BinOp, a: &V, b: &V) -> Res<V> {
        use BinOp::*;
        if is_never(&a.0) || is_never(&b.0) {
            return Ok(never());
        }
        let t = self.m.repr(&a.0)?;
        if matches!(op, Eq | Ne) {
            let e = self.equal(a, b)?;
            if op == Eq {
                return Ok(e);
            }
            let r = self.fresh("ne");
            self.emit(format!("{r} = xor i1 {}, true", e.1));
            return Ok(bool_v(&r));
        }
        let cmp = |s: &mut Self, ins: &str| -> Res<V> {
            let r = s.fresh("cmp");
            let ll = s.m.ll(&t)?;
            s.emit(format!("{r} = {ins} {ll} {}, {}", a.1, b.1));
            Ok(bool_v(&r))
        };
        Ok(match (op, &t) {
            (Add, Ty::Str) => self.call(&Ty::Str, "@strata_concat", &[a.clone(), b.clone()])?,
            (Lt | Le | Gt | Ge, Ty::Str) => {
                let c = self.call(&Ty::Int, "@strata_str_cmp", &[a.clone(), b.clone()])?;
                let r = self.fresh("cmp");
                let p = match op {
                    Lt => "slt",
                    Le => "sle",
                    Gt => "sgt",
                    _ => "sge",
                };
                self.emit(format!("{r} = icmp {p} i64 {}, 0", c.1));
                bool_v(&r)
            }
            (Add, Ty::Int) => self.int_op("sadd", a, b),
            (Sub, Ty::Int) => self.int_op("ssub", a, b),
            (Mul, Ty::Int) => self.int_op("smul", a, b),
            (Div, Ty::Int) => self.call(&Ty::Int, "@strata_div", &[a.clone(), b.clone()])?,
            (Mod, Ty::Int) => self.call(&Ty::Int, "@strata_mod", &[a.clone(), b.clone()])?,
            (Add | Sub | Mul | Div | Mod, Ty::Float) => {
                let r = match op {
                    Mod => self.call(&Ty::Float, "@strata_fmod", &[a.clone(), b.clone()])?,
                    _ => {
                        let ins = match op {
                            Add => "fadd",
                            Sub => "fsub",
                            Mul => "fmul",
                            _ => "fdiv",
                        };
                        let r = self.fresh("f");
                        self.emit(format!("{r} = {ins} double {}, {}", a.1, b.1));
                        (Ty::Float, r)
                    }
                };
                // no NaN or infinity
                let sym = self.string(op.symbol());
                self.call(&Ty::Float, "@strata_finite", &[r, a.clone(), b.clone(), sym])?
            }
            (Lt, Ty::Int) => cmp(self, "icmp slt")?,
            (Le, Ty::Int) => cmp(self, "icmp sle")?,
            (Gt, Ty::Int) => cmp(self, "icmp sgt")?,
            (Ge, Ty::Int) => cmp(self, "icmp sge")?,
            (Lt, Ty::Float) => cmp(self, "fcmp olt")?,
            (Le, Ty::Float) => cmp(self, "fcmp ole")?,
            (Gt, Ty::Float) => cmp(self, "fcmp ogt")?,
            (Ge, Ty::Float) => cmp(self, "fcmp oge")?,
            (op, t) => return Err(format!("`{}` on a `{}`", op.symbol(), key(t))),
        })
    }

    fn call_named(&mut self, call: &Expr, name: &str, args: Vec<&Expr>) -> Res<V> {
        let env = self.m.env;
        if self.lookup(name).is_some() {
            return self.unsupported("calling a function value");
        }
        if let Some(f) = env.fns.get(name) {
            if f.params.len() != args.len() {
                return Err(format!("`{name}` takes {} arguments", f.params.len()));
            }
            let mut vs = vec![];
            for a in &args {
                vs.push(self.expr(a)?);
            }
            if vs.iter().any(|v| is_never(&v.0)) {
                return Ok(never());
            }
            // the type arguments: the declared types against the actual ones
            let mut targs = HashMap::new();
            for (p, v) in f.params.iter().zip(&vs) {
                self.bind_targs(&p.ty, &v.0, &f.type_params, &mut targs);
            }
            if let Ok(rt) = self.ty_of(call) {
                self.bind_targs(&f.ret, &rt, &f.type_params, &mut targs);
            }
            let targs: Vec<Ty> = f.type_params.iter().map(|p| targs.get(p).cloned().unwrap_or(Ty::Unit)).collect();
            let sub: HashMap<String, Ty> = f.type_params.iter().cloned().zip(targs.iter().cloned()).collect();
            let ret = self.m.lower(&f.ret, &sub)?;
            // an argument's own type may differ from the parameter's (a variant for its sum): same representation
            let vs: Vec<V> = f.params.iter().zip(vs).map(|(p, v)| Ok((self.m.lower(&p.ty, &sub)?, v.1))).collect::<Res<_>>()?;
            self.m.want(name, targs.clone());
            return self.call(&ret, &sym(name, &targs), &vs);
        }
        if crate::stdlib::is_host(name) || env.is_method(name) {
            return self.unsupported(&format!("`{name}`"));
        }
        let mut vs = vec![];
        for a in &args {
            vs.push(self.expr(a)?);
        }
        if vs.iter().any(|v| is_never(&v.0)) {
            return Ok(never());
        }
        let types: Vec<Ty> = vs.iter().map(|v| self.m.repr(&v.0)).collect::<Res<_>>()?;
        Ok(match (name, types.as_slice()) {
            ("print", [_]) => {
                let s = self.show(&vs[0])?;
                self.call(&Ty::Unit, "@strata_print", &[(Ty::Str, s)])?
            }
            ("to_string", [_]) => {
                let s = self.show(&vs[0])?;
                (Ty::Str, s)
            }
            ("len", [Ty::Str]) => self.call(&Ty::Int, "@strata_str_len", &vs)?,
            ("abs", [Ty::Int]) => {
                // |i64::MIN| doesn't fit
                let is_min = self.apply(BinOp::Eq, &vs[0], &(Ty::Int, i64::MIN.to_string()))?;
                let (bad, ok) = (self.fresh_label("overflow"), self.fresh_label("abs"));
                self.cond_br(&is_min, &bad, &ok);
                self.block(&bad);
                self.emit("call void @strata_overflow()".into());
                self.emit("unreachable".into());
                self.block(&ok);
                self.call(&Ty::Int, "@llvm.abs.i64", &[vs[0].clone(), bool_v("false")])?
            }
            ("abs", [Ty::Float]) => self.call(&Ty::Float, "@llvm.fabs.f64", &vs)?,
            ("min" | "max", [a, b]) if a == b && matches!(a, Ty::Int | Ty::Float) => {
                let lt = self.apply(BinOp::Lt, &vs[0], &vs[1])?;
                let (x, y) = if name == "min" { (&vs[0], &vs[1]) } else { (&vs[1], &vs[0]) };
                let r = self.fresh("sel");
                let ll = self.m.ll(a)?;
                self.emit(format!("{r} = select i1 {}, {ll} {}, {ll} {}", lt.1, x.1, y.1));
                (a.clone(), r)
            }
            ("float", [Ty::Int]) => {
                let r = self.fresh("fl");
                self.emit(format!("{r} = sitofp i64 {} to double", vs[0].1));
                (Ty::Float, r)
            }
            ("sqrt", [Ty::Float]) => {
                // no NaN: the square root of a negative number is a fault
                let r = self.call(&Ty::Float, "@llvm.sqrt.f64", &vs)?;
                let s = self.string("sqrt");
                self.call(&Ty::Float, "@strata_finite", &[r, vs[0].clone(), vs[0].clone(), s])?
            }
            ("floor" | "ceil" | "round", [Ty::Float]) => {
                let r = self.call(&Ty::Float, &format!("@llvm.{name}.f64"), &vs)?;
                let s = self.string(name);
                self.call(&Ty::Int, "@strata_float_to_int", &[r, s])?
            }
            _ => {
                let ts: Vec<String> = types.iter().map(key).collect();
                return self.unsupported(&format!("the built-in `{name}` on ({})", ts.join(", ")));
            }
        })
    }

    /// Bind type parameters by matching a written type against an actual one.
    fn bind_targs(&self, te: &TypeExpr, t: &Ty, params: &[String], out: &mut HashMap<String, Ty>) {
        match (te, t) {
            (TypeExpr::Refined(b, _), t) => self.bind_targs(b, t, params, out),
            (TypeExpr::Named(n, args), t) if args.is_empty() && params.contains(n) => {
                out.entry(n.clone()).or_insert_with(|| t.clone());
            }
            (TypeExpr::Named(n, args), t) => {
                if let Some(td) = self.m.env.types.get(n) {
                    if let (TypeBody::Alias(a), false) = (&td.body, td.distinct) {
                        let sub: HashMap<String, TypeExpr> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
                        return self.bind_targs(&crate::types::subst_texpr(a, &sub), t, params, out);
                    }
                }
                if let Ty::Con(_, targs) = t {
                    for (a, x) in args.iter().zip(targs) {
                        self.bind_targs(a, x, params, out);
                    }
                }
            }
            (TypeExpr::Fn(ps, r), Ty::Fn(qs, s)) => {
                for (p, q) in ps.iter().zip(qs) {
                    self.bind_targs(p, q, params, out);
                }
                self.bind_targs(r, s, params, out);
            }
            _ => {}
        }
    }

    /// `Some(x)`, `None`, `Line { sku: s, qty: 1 }`, `Entry { ..e, done: true }`, `UserId(s)`.
    fn construct(&mut self, e: &Expr, c: &str, args: &CtorArgs) -> Res<V> {
        let t = self.ty_of(e)?;
        // a distinct type is its value, checked
        if self.m.is_distinct(c) {
            let CtorArgs::Positional(xs) = args else { return Err(format!("`{c}` takes one value")) };
            let v = self.expr(&xs[0])?;
            self.check(&TypeExpr::Named(c.to_string(), vec![]), &v, &format!("constructing `{c}`"), false)?;
            if let Some(r) = self.m.refinement(c) {
                let ok = self.with_it(&v, false, |s| s.expr(r))?;
                self.fail_unless(&ok, &format!("constructing `{c}`"), &v, format!("does not satisfy `{c}` (where {})", crate::printer::expr(r, 0)))?;
            }
            return Ok((t, v.1));
        }
        let fs = self.m.fields(c, &self.m.ctor_args(c, &t))?;
        let vals: Vec<V> = match args {
            CtorArgs::None => {
                let g = self.m.nullary(c);
                return Ok((t, g));
            }
            CtorArgs::Positional(xs) => {
                let mut vs = vec![];
                for x in xs {
                    vs.push(self.expr(x)?);
                }
                vs
            }
            CtorArgs::Record(given) => {
                // in declaration order
                let mut vs = vec![];
                for f in &fs {
                    let x = given.iter().find(|(n, _)| Some(n) == f.name.as_ref()).ok_or(format!("`{c}` needs its field `{}`", f.name.clone().unwrap_or_default()))?;
                    vs.push(self.expr(&x.1)?);
                }
                vs
            }
            CtorArgs::Update(base, changed) => {
                let b = self.expr(base)?;
                if is_never(&b.0) {
                    return Ok(b);
                }
                let mut vs = vec![];
                for (i, f) in fs.iter().enumerate() {
                    match changed.iter().find(|(n, _)| Some(n) == f.name.as_ref()) {
                        Some((_, x)) => vs.push(self.expr(x)?),
                        None => vs.push(self.field(&b, i, &f.ty)?),
                    }
                }
                vs
            }
        };
        if vals.iter().any(|v| is_never(&v.0)) {
            return Ok(never());
        }
        self.check_new(c, &vals, &fs)?;
        let p = self.block_of(self.m.tags[c], &vals)?;
        let v = (t, p);
        self.check_type_refinement(c, &v)?;
        Ok(v)
    }

    fn get_field(&mut self, r: &Expr, f: &str) -> Res<V> {
        let v = self.expr(r)?;
        let raw = self.ty_of(r).unwrap_or_else(|_| v.0.clone());
        if is_never(&v.0) {
            return Ok(v);
        }
        if let Ty::Con(n, _) = &raw {
            if self.m.is_distinct(n) && f == "value" {
                return Ok((self.m.repr(&raw)?, v.1));
            }
        }
        let t = self.m.repr(&raw)?;
        if let Ty::Con(n, args) = &t {
            if n == "Tuple" {
                let i: usize = f.parse().map_err(|_| format!("tuples have no field `{f}`"))?;
                return self.field(&v, i, &args[i]);
            }
        }
        // the field in each constructor the value can have
        let cs = self.m.ctors_of(&t);
        let mut at = vec![];
        for c in &cs {
            let fs = self.m.fields(c, &self.m.ctor_args(c, &t))?;
            if let Some(i) = fs.iter().position(|x| x.name.as_deref() == Some(f)) {
                at.push((c.clone(), i, fs[i].ty.clone()));
            }
        }
        let Some((_, i, ft)) = at.first().cloned() else { return Err(format!("`{}` has no field `{f}`", key(&t))) };
        if at.len() == cs.len() && at.iter().all(|x| x.1 == i) {
            return self.field(&v, i, &ft);
        }
        self.unsupported(&format!("the field `{f}` at different places in `{}`'s variants", key(&t)))
    }

    fn matching(&mut self, e: &Expr, scrut: &Expr, as_name: Option<&str>, arms: &[Arm]) -> Res<V> {
        let want = self.ty_of(e)?;
        let v = self.expr(scrut)?;
        if is_never(&v.0) {
            return Ok(v);
        }
        self.scopes.push(HashMap::new());
        if let Some(n) = as_name {
            let slot = self.local(n, &v.0, None)?;
            self.store(&v, &slot)?;
        }
        let end = self.fresh_label("endmatch");
        let mut incoming = vec![];
        for arm in arms {
            let next = self.fresh_label("next_arm");
            self.scopes.push(HashMap::new());
            self.pattern(&arm.pat, &v, &next)?;
            let r = self.expr(&arm.body)?;
            incoming.push((r, self.cur.clone()));
            self.br(&end);
            self.scopes.pop();
            self.block(&next);
        }
        let shown = self.json(&v)?;
        self.fault(Fault::Runtime, vec![Part::Lit("no match arm for ".into()), Part::Val(shown)])?;
        self.block(&end);
        self.scopes.pop();
        self.join(&want, incoming)
    }

    /// Match `v` against a pattern: jump to `fail` if it doesn't match, else
    /// go on with its names bound in the current scope.
    fn pattern(&mut self, p: &Pattern, v: &V, fail: &str) -> Res<()> {
        let test = |s: &mut Self, ok: V| {
            let next = s.fresh_label("matched");
            s.cond_br(&ok, &next, fail);
            s.block(&next);
        };
        match p {
            Pattern::Wild => {}
            Pattern::Bind(x) => self.bind(x, v)?,
            Pattern::Int(n) => {
                let ok = self.apply(BinOp::Eq, v, &(Ty::Int, n.to_string()))?;
                test(self, ok);
            }
            Pattern::Bool(b) => {
                let ok = self.apply(BinOp::Eq, v, &bool_v(&b.to_string()))?;
                test(self, ok);
            }
            Pattern::Str(s) => {
                let l = self.string(s);
                let ok = self.apply(BinOp::Eq, v, &l)?;
                test(self, ok);
            }
            Pattern::Cond(c) => {
                // the condition sees the variables in scope, and `it`
                self.scopes.push(HashMap::new());
                let slot = self.local("it", &v.0, None)?;
                self.store(v, &slot)?;
                let ok = self.expr(c)?;
                self.scopes.pop();
                test(self, ok);
            }
            Pattern::Or(ps) => {
                // each alternative binds the same names, into the same slots
                let matched = self.fresh_label("alt.matched");
                for (i, alt) in ps.iter().enumerate() {
                    let next = if i + 1 == ps.len() { fail.to_string() } else { self.fresh_label("alt.next") };
                    self.pattern(alt, v, &next)?;
                    self.br(&matched);
                    if i + 1 < ps.len() {
                        self.block(&next);
                    }
                }
                self.block(&matched);
            }
            Pattern::Ctor(c, CtorPat::None) if !self.m.env.ctors.contains_key(c) && self.m.env.is_type_name(c) => {
                let ok = self.holds(c, v)?;
                test(self, ok);
            }
            Pattern::Ctor(c, cp) => {
                if self.m.is_distinct(c) {
                    if let CtorPat::Positional(ps) = cp {
                        let inner = (self.m.repr(&v.0)?, v.1.clone());
                        return self.pattern(&ps[0], &inner, fail);
                    }
                    return Ok(());
                }
                let ok = self.tag_in(v, &[c.clone()])?;
                test(self, ok);
                let fs = self.m.fields(c, &self.m.ctor_args(c, &v.0))?;
                match cp {
                    CtorPat::None => {}
                    CtorPat::Positional(ps) => {
                        for (i, (sp, f)) in ps.iter().zip(&fs).enumerate() {
                            let x = self.field(v, i, &f.ty)?;
                            self.pattern(sp, &x, fail)?;
                        }
                    }
                    CtorPat::Record(fps, _) => {
                        for (name, sp) in fps {
                            let i = fs.iter().position(|f| f.name.as_ref() == Some(name)).ok_or(format!("`{c}` has no field `{name}`"))?;
                            let x = self.field(v, i, &fs[i].ty)?;
                            self.pattern(sp, &x, fail)?;
                        }
                    }
                }
            }
            Pattern::Tuple(ps) => {
                let Ty::Con(_, ts) = self.m.repr(&v.0)? else { return Err("a tuple pattern on something else".into()) };
                for (i, (sp, t)) in ps.iter().zip(&ts).enumerate() {
                    let x = self.field(v, i, t)?;
                    self.pattern(sp, &x, fail)?;
                }
            }
            Pattern::List(..) => return self.unsupported("a list pattern"),
        }
        Ok(())
    }

    /// Bind a name in the current scope (again, in the same slot, for another
    /// alternative of an or-pattern).
    fn bind(&mut self, x: &str, v: &V) -> Res<()> {
        let slot = match self.scopes.last().unwrap().get(x) {
            Some(l) => l.slot.clone(),
            None => self.local(x, &v.0, None)?,
        };
        self.store(v, &slot)
    }

    fn stmt(&mut self, s: &Stmt) -> Res<()> {
        match s {
            Stmt::Let(_, ty, e) | Stmt::Var(_, ty, e) => {
                let v = self.expr(e)?;
                if is_never(&v.0) {
                    return Ok(());
                }
                let v = match ty {
                    Some(t) => {
                        self.check(t, &v, "let binding", false)?;
                        (self.lower(t)?, v.1)
                    }
                    None => v,
                };
                match s {
                    Stmt::Var(n, ty, _) => {
                        let slot = self.local(n, &v.0, ty.clone())?;
                        self.store(&v, &slot)?;
                    }
                    Stmt::Let(Pattern::Bind(n), _, _) => {
                        let slot = self.local(n, &v.0, None)?;
                        self.store(&v, &slot)?;
                    }
                    Stmt::Let(p, _, _) => {
                        let (ok, fail) = (self.fresh_label("let.ok"), self.fresh_label("let.fail"));
                        self.pattern(p, &v, &fail)?;
                        self.br(&ok);
                        self.block(&fail);
                        let shown = self.json(&v)?;
                        self.fault(Fault::Runtime, vec![Part::Lit("let pattern did not match ".into()), Part::Val(shown)])?;
                        self.block(&ok);
                    }
                    _ => unreachable!(),
                }
            }
            Stmt::Assign(place, e) => {
                // `x.a.b = e`: a copy of `x` with the field changed
                let mut path = vec![];
                let mut root = place;
                while let Expr::Field(r, f) = root {
                    path.push(f.clone());
                    root = r;
                }
                path.reverse();
                let Expr::Var(x) = root else { return self.unsupported("assigning to anything but a variable or its fields") };
                let l = self.lookup(x).ok_or(format!("`{x}` is not a local variable"))?;
                let v = self.expr(e)?;
                if is_never(&v.0) {
                    return Ok(());
                }
                let shown = crate::printer::expr(place, 0);
                let cur = self.load(x)?;
                let new = self.set_path(&cur, &path, v, &shown)?;
                if let Some(t) = &l.declared {
                    self.check(t, &new, &format!("assigning `{shown}`"), false)?;
                }
                self.store(&(l.t.clone(), new.1), &l.slot)?;
            }
            Stmt::While(c, body) => {
                let (head, inner, end) = (self.fresh_label("while"), self.fresh_label("do"), self.fresh_label("endwhile"));
                self.br(&head);
                self.block(&head);
                let cv = self.expr(c)?;
                self.cond_br(&cv, &inner, &end);
                self.block(&inner);
                self.loop_body(body, &head, &end)?;
                self.block(&end);
            }
            Stmt::Loop(body) => {
                let (head, end) = (self.fresh_label("loop"), self.fresh_label("endloop"));
                self.br(&head);
                self.block(&head);
                self.loop_body(body, &head, &end)?;
                self.block(&end);
            }
            Stmt::For(p, xs, body) => {
                let Expr::Call(f, args) = xs else { return self.unsupported("`for` over anything but `range(a, b)`") };
                if !matches!(&**f, Expr::Var(n) if n == "range") || args.len() != 2 || self.m.env.fns.contains_key("range") {
                    return self.unsupported("`for` over anything but `range(a, b)`");
                }
                let (lo, hi) = (self.expr(&args[0])?, self.expr(&args[1])?);
                let counter = self.fresh("i");
                writeln!(self.allocas, "  {counter} = alloca i64").unwrap();
                self.store(&lo, &counter)?;
                let (head, inner, step, end) = (self.fresh_label("for"), self.fresh_label("for.body"), self.fresh_label("for.next"), self.fresh_label("endfor"));
                self.br(&head);
                self.block(&head);
                let i = self.fresh("i");
                self.emit(format!("{i} = load i64, ptr {counter}"));
                let more = self.apply(BinOp::Lt, &(Ty::Int, i.clone()), &hi)?;
                self.cond_br(&more, &inner, &end);
                self.block(&inner);
                self.scopes.push(HashMap::new());
                match p {
                    Pattern::Bind(x) => self.bind(x, &(Ty::Int, i.clone()))?,
                    Pattern::Wild => {}
                    _ => return self.unsupported("this `for` pattern"),
                }
                self.loop_body(body, &step, &end)?;
                self.scopes.pop();
                self.block(&step);
                let next = self.fresh("i");
                self.emit(format!("{next} = add i64 {i}, 1"));
                self.emit(format!("store i64 {next}, ptr {counter}"));
                self.br(&head);
                self.block(&end);
            }
            Stmt::Break | Stmt::Continue => {
                let (cont, brk) = self.loops.last().cloned().ok_or("`break` or `continue` outside a loop")?;
                self.br(if matches!(s, Stmt::Break) { &brk } else { &cont });
                self.dead();
            }
            Stmt::Expr(e) => {
                self.expr(e)?;
            }
            Stmt::Assert(x) => {
                // a comparison shows both sides when it fails
                let (ok, sides) = match x {
                    Expr::Bin(op, l, r) if !matches!(op, BinOp::And | BinOp::Or | BinOp::Implies) => {
                        let (a, b) = (self.expr(l)?, self.expr(r)?);
                        (self.apply(*op, &a, &b)?, Some((*op, a, b)))
                    }
                    _ => (self.expr(x)?, None),
                };
                if is_never(&ok.0) {
                    return Ok(());
                }
                let (pass, fail) = (self.fresh_label("assert.ok"), self.fresh_label("assert.fail"));
                self.cond_br(&ok, &pass, &fail);
                self.block(&fail);
                let mut parts = vec![Part::Lit(format!("assertion failed: {}", crate::printer::expr(x, 0)))];
                if let Some((op, a, b)) = sides {
                    let (sa, sb) = (self.json(&a)?, self.json(&b)?);
                    parts.extend([Part::Lit(format!(" (left of {} = ", op.symbol())), Part::Val(sa), Part::Lit(", right = ".into()), Part::Val(sb), Part::Lit(")".into())]);
                }
                self.fault(Fault::Assert, parts)?;
                self.block(&pass);
            }
            Stmt::Comment(_) => {}
        }
        Ok(())
    }

    /// `cur` with the field at `path` replaced by `v` (a new block per level),
    /// each changed record checked again as the interpreter does.
    fn set_path(&mut self, cur: &V, path: &[String], v: V, shown: &str) -> Res<V> {
        let Some((f, rest)) = path.split_first() else { return Ok(v) };
        let t = self.m.repr(&cur.0)?;
        let cs = self.m.ctors_of(&t);
        if cs.len() != 1 {
            return self.unsupported(&format!("assigning to a field of a `{}`", key(&t)));
        }
        let c = cs[0].clone();
        let fs = self.m.fields(&c, &self.m.ctor_args(&c, &t))?;
        let i = fs.iter().position(|x| x.name.as_deref() == Some(f)).ok_or(format!("`{}` has no field `{f}`", key(&t)))?;
        let inner = self.field(cur, i, &fs[i].ty)?;
        let new_inner = self.set_path(&inner, rest, v, shown)?;
        let p = self.copy_block(cur, fs.len());
        self.set_field(&p, i, &new_inner)?;
        let new = (cur.0.clone(), p);
        // the changed field's refinement, and the type's own
        let ctx = format!("assigning `{shown}`");
        self.check(&fs[i].texpr, &new_inner, &format!("{ctx}: {}.{f}", crate::names::short(&c)), false)?;
        let tname = self.m.env.ctors[&c].clone();
        if let Some(r) = self.m.refinement(&tname) {
            let ok = self.with_it(&new, false, |s| s.expr(r))?;
            self.fail_unless(&ok, &ctx, &new, format!("does not satisfy `{tname}` (where {})", crate::printer::expr(&r, 0)))?;
        }
        Ok(new)
    }

    /// A loop's body, then back to `cont`; `break` goes to `end`.
    fn loop_body(&mut self, body: &Expr, cont: &str, end: &str) -> Res<()> {
        self.loops.push((cont.to_string(), end.to_string()));
        self.expr(body)?;
        self.loops.pop();
        self.br(cont);
        Ok(())
    }
}

impl Module<'_> {
    /// The type arguments of constructor `c`'s type, from a value's type `t`
    /// (the sum, the variant, or a union containing it).
    fn ctor_args(&self, c: &str, t: &Ty) -> Vec<Ty> {
        let parent = self.env.ctors.get(c).cloned().unwrap_or_default();
        match t {
            Ty::Con(n, a) if *n == parent || n == c => a.clone(),
            Ty::Union(ms) => ms.iter().find_map(|m| match m {
                Ty::Con(n, a) if *n == parent || n == c => Some(a.clone()),
                _ => None,
            }).unwrap_or_default(),
            _ => vec![],
        }
    }
}
