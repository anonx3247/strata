//! Native compilation: the elaborated program as LLVM IR (text), linked with
//! a small C runtime (`llvm/runtime.c`) by `clang` into an executable.
//!
//! This is the first step: functions over `Int`, `Float`, `Bool`, `String`
//! and `()`, with `let`, `let mut`, assignment, `if`, `match` on literals,
//! conditions and refinement types, `while`, `loop`, `for` over `range`,
//! `return`, `assert`, f-strings without format specs, and the built-ins
//! `print`, `to_string`, `len`, `abs`, `min`, `max`. Contracts are checked at
//! run time as the interpreter does: refinements of parameters, results,
//! annotated `let`s and assignments to declared variables, `requires`,
//! `ensures`, with the interpreter's fault messages.
//!
//! Not yet: big integers (`Int` is 64 bits here, and overflow is a fault),
//! records, variants, lists and the other collections, lambdas, traits,
//! `mut` parameters, capabilities, format specs, and a garbage collector
//! (memory is never freed). A program that uses them is refused, with the
//! function and the construct named.

use crate::ast::*;
use crate::types::Env;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write;

/// The runtime's C source, compiled with the program.
pub const RUNTIME: &str = include_str!("llvm/runtime.c");

/// A value's representation.
#[derive(Clone, Copy, PartialEq, Debug)]
enum T {
    Int,
    Float,
    Bool,
    Str,
    Unit,
    /// The expression never produces a value (`return`, `break`, a fault).
    Never,
}

impl T {
    fn ll(self) -> &'static str {
        match self {
            T::Int => "i64",
            T::Float => "double",
            T::Bool => "i1",
            T::Str => "ptr",
            T::Unit | T::Never => "void",
        }
    }
    fn has_value(self) -> bool {
        !matches!(self, T::Unit | T::Never)
    }
    fn name(self) -> &'static str {
        match self {
            T::Int => "Int",
            T::Float => "Float",
            T::Bool => "Bool",
            T::Str => "String",
            T::Unit => "()",
            T::Never => "nothing",
        }
    }
}

/// A compiled value: its representation and its LLVM operand ("" for `()`).
type V = (T, String);

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

/// The LLVM IR of a program whose entry point is `entry` (`main`): it takes
/// no parameters and returns `()` or an exit code (`Int`).
pub fn compile(prog: &Program, entry: &str) -> Res<String> {
    let elaborated = crate::types::elaborate(prog).program;
    let (env, _) = Env::build(&elaborated);
    let main = env.fns.get(entry).ok_or(format!("no `{entry}` function to compile"))?;
    if !main.params.is_empty() {
        return Err(format!("`{entry}` takes parameters; compiled programs can't ask for capabilities or options yet"));
    }
    let mut m = Module { env: &env, globals: String::new(), strings: HashMap::new(), body: String::new(), queue: VecDeque::new(), seen: HashSet::new() };
    let main_ret = m.lower(&main.ret).map_err(|e| format!("`{entry}`: {e}"))?;
    if !matches!(main_ret, T::Unit | T::Int) {
        return Err(format!("`{entry}` must return `()` or an exit code (`Int`), not `{}`", crate::printer::ty(&main.ret)));
    }
    m.want(entry);
    while let Some(name) = m.queue.pop_front() {
        let f = env.fns[&name].clone();
        m.function(&name, &f).map_err(|e| format!("can't compile `{name}`: {e}"))?;
    }
    let mut out = String::from("; Strata, compiled by `strata build`\n\n");
    out += &m.globals;
    out += "\n";
    out += &m.body;
    out += &format!("define i32 @main() {{\n");
    if main_ret == T::Int {
        out += &format!("  %r = call i64 {}()\n  %c = trunc i64 %r to i32\n  ret i32 %c\n}}\n\n", sym(entry));
    } else {
        out += &format!("  call void {}()\n  ret i32 0\n}}\n\n", sym(entry));
    }
    out += DECLARATIONS;
    Ok(out)
}

const DECLARATIONS: &str = "\
declare ptr @strata_concat(ptr, ptr)
declare i1 @strata_str_eq(ptr, ptr)
declare i64 @strata_str_len(ptr)
declare void @strata_fault(i32, ptr) noreturn
declare void @strata_overflow() noreturn cold
declare i64 @strata_div(i64, i64)
declare i64 @strata_mod(i64, i64)
declare double @strata_fmod(double, double)
declare double @strata_finite(double, double, double, ptr)
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
";

/// The LLVM name of a Strata function (prefixed, so it can't clash with C's).
fn sym(name: &str) -> String {
    format!("@\"strata:{name}\"")
}

struct Module<'a> {
    env: &'a Env,
    /// String constants.
    globals: String,
    strings: HashMap<String, String>,
    /// Compiled functions.
    body: String,
    /// Functions still to compile: those the compiled ones call.
    queue: VecDeque<String>,
    seen: HashSet<String>,
}

impl<'a> Module<'a> {
    fn want(&mut self, name: &str) {
        if self.seen.insert(name.to_string()) {
            self.queue.push_back(name.to_string());
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

    /// How values of a type are represented.
    fn lower(&self, t: &TypeExpr) -> Res<T> {
        match t {
            TypeExpr::Unit => Ok(T::Unit),
            TypeExpr::Refined(b, _) => self.lower(b),
            TypeExpr::Named(n, args) if args.is_empty() => match n.as_str() {
                "Int" => Ok(T::Int),
                "Float" => Ok(T::Float),
                "Bool" => Ok(T::Bool),
                "String" => Ok(T::Str),
                _ => match self.env.types.get(n) {
                    Some(TypeDecl { distinct: false, params, body: TypeBody::Alias(a), .. }) if params.is_empty() => self.lower(a),
                    _ => Err(format!("type `{n}` is not compiled yet")),
                },
            },
            other => Err(format!("type `{}` is not compiled yet", crate::printer::ty(other))),
        }
    }

    fn function(&mut self, name: &str, f: &FnDecl) -> Res<()> {
        if !f.type_params.is_empty() {
            return Err("generic functions are not compiled yet".into());
        }
        let mut params = Vec::new();
        for p in &f.params {
            if p.mutable {
                return Err(format!("`mut` parameters (`{}`) are not compiled yet", p.name));
            }
            params.push(self.lower(&p.ty)?);
        }
        let ret = self.lower(&f.ret)?;
        let mut c = Func { m: self, allocas: String::new(), code: String::new(), tmp: 0, label: 0, scopes: vec![HashMap::new()], loops: vec![], exit: "exit".into(), ret_slot: None, cur: String::new(), terminated: true };
        let sig: Vec<String> = f.params.iter().zip(&params).filter(|(_, t)| t.has_value()).map(|(p, t)| format!("{} %p.{}", t.ll(), p.name)).collect();
        c.block("entry.body");
        // parameters live in slots, like every variable
        for (p, &t) in f.params.iter().zip(&params) {
            let v = (t, format!("%p.{}", p.name));
            let slot = c.local(&p.name, t, None)?;
            c.store(&v, &slot);
        }
        for (p, &t) in f.params.iter().zip(&params) {
            let v = c.load(&p.name)?;
            debug_assert_eq!(v.0, t);
            c.check(&p.ty, &v, &format!("argument `{}` of `{name}`", p.name), false)?;
        }
        for r in &f.requires {
            let ok = c.expr(r)?;
            let (pass, fail) = (c.fresh_label("requires.ok"), c.fresh_label("requires.fail"));
            c.cond_br(&ok, &pass, &fail);
            c.block(&fail);
            let mut parts = vec![Part::Lit(format!("precondition of `{name}` violated: requires {} (with ", crate::printer::expr(r, 0)))];
            for (i, p) in f.params.iter().enumerate() {
                let v = c.load(&p.name)?;
                let shown = c.json(&v);
                parts.push(Part::Lit(format!("{}{} = ", if i > 0 { ", " } else { "" }, p.name)));
                parts.push(Part::Val(shown));
            }
            parts.push(Part::Lit(")".into()));
            c.fault(Fault::Requires, parts);
            c.block(&pass);
        }
        if ret.has_value() {
            let slot = c.fresh("ret");
            writeln!(c.allocas, "  {slot} = alloca {}", ret.ll()).unwrap();
            c.ret_slot = Some((ret, slot));
        }
        let v = c.expr(&f.body)?;
        c.return_value(&v)?;
        // every way out: the result's refinement, then the postconditions
        c.block("exit");
        let result = match &c.ret_slot {
            Some((t, slot)) => {
                let (t, slot) = (*t, slot.clone());
                let r = c.fresh("result");
                c.emit(format!("{r} = load {}, ptr {slot}", t.ll()));
                (t, r)
            }
            None => (T::Unit, String::new()),
        };
        c.check(&f.ret, &result, &format!("result of `{name}`"), true)?;
        if !f.ensures.is_empty() {
            c.scopes.push(HashMap::new());
            let slot = c.local("it", result.0, None)?;
            c.store(&result, &slot);
            for en in &f.ensures {
                let ok = c.expr(en)?;
                let (pass, fail) = (c.fresh_label("ensures.ok"), c.fresh_label("ensures.fail"));
                c.cond_br(&ok, &pass, &fail);
                c.block(&fail);
                let shown = c.json(&result);
                c.fault(
                    Fault::Ensures,
                    vec![Part::Lit(format!("postcondition of `{name}` violated: ensures {} (it = ", crate::printer::expr(en, 0))), Part::Val(shown), Part::Lit(")".into())],
                );
                c.block(&pass);
            }
            c.scopes.pop();
        }
        match result.0 {
            t if t.has_value() => c.emit(format!("ret {} {}", t.ll(), result.1)),
            _ => c.emit("ret void".into()),
        }
        let (allocas, code) = (std::mem::take(&mut c.allocas), std::mem::take(&mut c.code));
        writeln!(self.body, "define {} {}({}) {{\nentry:\n{allocas}  br label %entry.body\n{code}}}\n", ret.ll(), sym(name), sig.join(", ")).unwrap();
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
    t: T,
    slot: String,
    declared: Option<TypeExpr>,
}

struct Func<'m, 'a> {
    m: &'m mut Module<'a>,
    /// Slots of the variables, in the entry block (so LLVM keeps them in registers).
    allocas: String,
    code: String,
    tmp: usize,
    label: usize,
    scopes: Vec<HashMap<String, Local>>,
    /// The enclosing loops: where `continue` and `break` go.
    loops: Vec<(String, String)>,
    exit: String,
    ret_slot: Option<(T, String)>,
    /// The current block, and whether it has ended (with a jump or `ret`).
    cur: String,
    terminated: bool,
}

impl Func<'_, '_> {
    fn emit(&mut self, line: String) {
        self.terminated = line.starts_with("br ") || line.starts_with("ret") || line == "unreachable";
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

    fn local(&mut self, name: &str, t: T, declared: Option<TypeExpr>) -> Res<String> {
        let slot = self.fresh(&format!("v.{}", name.replace(|c: char| !c.is_ascii_alphanumeric() && c != '_', "_")));
        if t.has_value() {
            writeln!(self.allocas, "  {slot} = alloca {}", t.ll()).unwrap();
        }
        self.scopes.last_mut().unwrap().insert(name.to_string(), Local { t, slot: slot.clone(), declared });
        Ok(slot)
    }

    fn lookup(&self, name: &str) -> Option<Local> {
        self.scopes.iter().rev().find_map(|s| s.get(name).cloned())
    }

    fn store(&mut self, v: &V, slot: &str) {
        if v.0.has_value() {
            self.emit(format!("store {} {}, ptr {slot}", v.0.ll(), v.1));
        }
    }

    fn load(&mut self, name: &str) -> Res<V> {
        let l = self.lookup(name).ok_or(format!("`{name}` is not a local variable (function values are not compiled yet)"))?;
        if !l.t.has_value() {
            return Ok((l.t, String::new()));
        }
        let r = self.fresh("x");
        self.emit(format!("{r} = load {}, ptr {}", l.t.ll(), l.slot));
        Ok((l.t, r))
    }

    fn call(&mut self, ret: T, f: &str, args: &[V]) -> V {
        let args: Vec<String> = args.iter().filter(|a| a.0.has_value()).map(|a| format!("{} {}", a.0.ll(), a.1)).collect();
        if ret.has_value() {
            let r = self.fresh("c");
            self.emit(format!("{r} = call {} {f}({})", ret.ll(), args.join(", ")));
            (ret, r)
        } else {
            self.emit(format!("call void {f}({})", args.join(", ")));
            (ret, String::new())
        }
    }

    fn string(&mut self, s: &str) -> V {
        (T::Str, self.m.string(s))
    }

    /// A string built from pieces.
    fn concat(&mut self, parts: Vec<Part>) -> V {
        let mut acc: Option<V> = None;
        for p in parts {
            let v = match p {
                Part::Lit(s) if s.is_empty() => continue,
                Part::Lit(s) => self.string(&s),
                Part::Val(op) => (T::Str, op),
            };
            acc = Some(match acc {
                None => v,
                Some(a) => self.call(T::Str, "@strata_concat", &[a, v]),
            });
        }
        acc.unwrap_or_else(|| self.string(""))
    }

    /// Stop with a fault; the code after it is unreachable.
    fn fault(&mut self, kind: Fault, msg: Vec<Part>) {
        let m = self.concat(msg);
        self.emit(format!("call void @strata_fault(i32 {}, ptr {})", kind as i32, m.1));
        self.emit("unreachable".into());
        self.dead();
    }

    /// A value as `to_string` shows it (a string as itself).
    fn show(&mut self, v: &V) -> String {
        match v.0 {
            T::Str => v.1.clone(),
            _ => self.json(v),
        }
    }

    /// A value as JSON, for messages.
    fn json(&mut self, v: &V) -> String {
        match v.0 {
            T::Int => self.call(T::Str, "@strata_show_int", &[v.clone()]).1,
            T::Float => self.call(T::Str, "@strata_show_float", &[v.clone()]).1,
            T::Bool => self.call(T::Str, "@strata_show_bool", &[v.clone()]).1,
            T::Str => self.call(T::Str, "@strata_json_str", &[v.clone()]).1,
            T::Unit | T::Never => self.m.string("null"),
        }
    }

    /// Fault unless `v` is a value of type `t`: every refinement reachable
    /// through it (`Interp::check_type_in`). `outer` says whether the
    /// enclosing scope is visible to an inline refinement (a function's
    /// parameters, for its result type).
    fn check(&mut self, t: &TypeExpr, v: &V, ctx: &str, outer: bool) -> Res<()> {
        if !v.0.has_value() {
            return Ok(());
        }
        match t {
            TypeExpr::Refined(b, pred) => {
                self.check(b, v, ctx, false)?;
                let ok = self.with_it(v, outer, |c| c.expr(pred))?;
                let msg = format!("does not satisfy `{}`", crate::printer::ty(t));
                self.fail_unless(&ok, ctx, v, msg);
            }
            TypeExpr::Named(n, _) => {
                let Some(td) = self.m.env.types.get(n).cloned() else { return Ok(()) };
                if let TypeBody::Alias(a) = &td.body {
                    self.check(a, v, ctx, false)?;
                }
                if let Some(r) = &td.refinement {
                    let ok = self.with_it(v, false, |c| c.expr(r))?;
                    let msg = format!("does not satisfy `{n}` (where {})", crate::printer::expr(r, 0));
                    self.fail_unless(&ok, ctx, v, msg);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn fail_unless(&mut self, ok: &V, ctx: &str, v: &V, msg: String) {
        let (pass, fail) = (self.fresh_label("check.ok"), self.fresh_label("check.fail"));
        self.cond_br(ok, &pass, &fail);
        self.block(&fail);
        let shown = self.json(v);
        self.fault(Fault::Refinement, vec![Part::Lit(format!("{ctx}: ")), Part::Val(shown), Part::Lit(format!(" {msg}"))]);
        self.block(&pass);
    }

    /// Whether `v` is a value of type `t` (a type's name as a pattern).
    fn holds(&mut self, t: &TypeExpr, v: &V) -> Res<V> {
        let mut conds = vec![];
        let mut t = t.clone();
        loop {
            match t {
                TypeExpr::Refined(b, pred) => {
                    conds.push(self.with_it(v, false, |c| c.expr(&pred))?);
                    t = *b;
                }
                TypeExpr::Named(n, _) => {
                    let Some(td) = self.m.env.types.get(&n).cloned() else { break };
                    if let Some(r) = &td.refinement {
                        conds.push(self.with_it(v, false, |c| c.expr(r))?);
                    }
                    let TypeBody::Alias(a) = td.body else { break };
                    t = a;
                }
                _ => break,
            }
        }
        // all of them, though each is evaluated (refinements are pure)
        let mut acc: V = (T::Bool, "true".into());
        for c in conds {
            let r = self.fresh("and");
            self.emit(format!("{r} = and i1 {}, {}", acc.1, c.1));
            acc = (T::Bool, r);
        }
        Ok(acc)
    }

    /// Evaluate `f` with `it` bound to `v` (and only that, unless `outer`).
    fn with_it(&mut self, v: &V, outer: bool, f: impl FnOnce(&mut Self) -> Res<V>) -> Res<V> {
        let saved = if outer { None } else { Some(std::mem::replace(&mut self.scopes, vec![HashMap::new()])) };
        self.scopes.push(HashMap::new());
        let slot = self.local("it", v.0, None)?;
        self.store(v, &slot);
        let r = f(self);
        self.scopes.pop();
        if let Some(s) = saved {
            self.scopes = s;
        }
        let r = r?;
        if r.0 != T::Bool {
            return Err("a refinement must be a `Bool`".into());
        }
        Ok(r)
    }

    fn return_value(&mut self, v: &V) -> Res<()> {
        if v.0 == T::Never {
            return Ok(());
        }
        if let Some((t, slot)) = self.ret_slot.clone() {
            if t != v.0 {
                return Err(format!("returns a {} where a {} is expected", v.0.name(), t.name()));
            }
            self.store(v, &slot);
        }
        let exit = self.exit.clone();
        self.br(&exit);
        Ok(())
    }

    fn unsupported<X>(&self, what: &str) -> Res<X> {
        Err(format!("{what} is not compiled yet"))
    }

    fn expr(&mut self, e: &Expr) -> Res<V> {
        Ok(match e {
            Expr::Int(n) => (T::Int, n.to_string()),
            Expr::BigInt(n) => return self.unsupported(&format!("the big integer {n}")),
            Expr::Float(x) => (T::Float, format!("0x{:016X}", x.to_bits())),
            Expr::Bool(b) => (T::Bool, b.to_string()),
            Expr::Str(s) => self.string(s),
            Expr::Unit => (T::Unit, String::new()),
            Expr::Var(x) => self.load(x)?,
            Expr::Hole(h) => {
                self.fault(Fault::Hole, vec![Part::Lit(format!("reached unfilled hole `?{h}`"))]);
                (T::Never, String::new())
            }
            Expr::Call(f, args) => match &**f {
                Expr::Var(name) => self.call_named(name, args)?,
                _ => return self.unsupported("calling a function value"),
            },
            Expr::Bin(op, l, r) => self.binary(*op, l, r)?,
            Expr::Un(UnOp::Not, x) => {
                let v = self.expr(x)?;
                let r = self.fresh("not");
                self.emit(format!("{r} = xor i1 {}, true", v.1));
                (T::Bool, r)
            }
            Expr::Un(UnOp::Neg, x) => {
                let v = self.expr(x)?;
                match v.0 {
                    T::Int => self.int_op("ssub", &(T::Int, "0".into()), &v),
                    T::Float => {
                        let r = self.fresh("neg");
                        self.emit(format!("{r} = fneg double {}", v.1));
                        (T::Float, r)
                    }
                    t => return Err(format!("can't negate a {}", t.name())),
                }
            }
            Expr::If(c, t, f) => {
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
                self.join(vec![(tv, from_t), (fv, from_f)])?
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
                (T::Never, String::new())
            }
            Expr::Interp(parts) => {
                let mut ps = vec![];
                for p in parts {
                    match p {
                        StrPart::Lit(s) => ps.push(Part::Lit(s.clone())),
                        StrPart::Expr(x, None) => {
                            let v = self.expr(x)?;
                            let s = self.show(&v);
                            ps.push(Part::Val(s));
                        }
                        StrPart::Expr(_, Some(_)) => return self.unsupported("a format spec in an f-string"),
                    }
                }
                self.concat(ps)
            }
            Expr::Match(scrut, as_name, arms) => self.matching(scrut, as_name.as_deref(), arms)?,
            Expr::Tuple(_) => return self.unsupported("a tuple"),
            Expr::List(_) => return self.unsupported("a list"),
            Expr::Ctor(c, _) => return self.unsupported(&format!("the constructor `{c}`")),
            Expr::Method(_, m, _) => return self.unsupported(&format!("the method call `.{m}`")),
            Expr::Field(_, f) => return self.unsupported(&format!("the field access `.{f}`")),
            Expr::Lambda(..) => return self.unsupported("a lambda"),
            Expr::Is(..) => return self.unsupported("`is`"),
            Expr::Mut(_) => return self.unsupported("a `mut` argument"),
            Expr::Mock(_) => return self.unsupported("`mock`"),
            Expr::Try(..) => return self.unsupported("`!` / `?`"),
            Expr::Except(..) => return self.unsupported("`except`"),
        })
    }

    /// The value of a branching expression, from the branches that produce one.
    fn join(&mut self, incoming: Vec<(V, String)>) -> Res<V> {
        let live: Vec<&(V, String)> = incoming.iter().filter(|(v, _)| v.0 != T::Never).collect();
        let Some(((t, _), _)) = live.first() else {
            // no branch comes here
            self.emit("unreachable".into());
            self.dead();
            return Ok((T::Never, String::new()));
        };
        let t = *t;
        if live.iter().any(|((u, _), _)| *u != t) {
            return Err("branches of different types (unions) are not compiled yet".into());
        }
        if !t.has_value() {
            return Ok((t, String::new()));
        }
        let r = self.fresh("phi");
        // a branch that never gets here still jumps here from its (unreachable) end
        let ins: Vec<String> = incoming.iter().map(|((u, op), from)| format!("[ {}, %{from} ]", if *u == T::Never { "poison" } else { op })).collect();
        self.emit(format!("{r} = phi {} {}", t.ll(), ins.join(", ")));
        Ok((t, r))
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
        (T::Int, r)
    }

    fn binary(&mut self, op: BinOp, l: &Expr, r: &Expr) -> Res<V> {
        use BinOp::*;
        if matches!(op, And | Or | Implies) {
            // short-circuit: the right side runs only when it decides
            let a = self.expr(l)?;
            let from = self.cur.clone();
            let (rhs, end) = (self.fresh_label("rhs"), self.fresh_label("logic"));
            let short = match op {
                And => {
                    self.cond_br(&a, &rhs, &end);
                    "false"
                }
                _ => {
                    self.cond_br(&a, if op == Or { &end } else { &rhs }, if op == Or { &rhs } else { &end });
                    "true"
                }
            };
            self.block(&rhs);
            let b = self.expr(r)?;
            let from_b = self.cur.clone();
            self.br(&end);
            self.block(&end);
            return self.join(vec![((T::Bool, short.into()), from), (b, from_b)]);
        }
        let a = self.expr(l)?;
        let b = self.expr(r)?;
        self.apply(op, &a, &b)
    }

    fn apply(&mut self, op: BinOp, a: &V, b: &V) -> Res<V> {
        use BinOp::*;
        if a.0 == T::Never || b.0 == T::Never {
            return Ok((T::Never, String::new()));
        }
        if a.0 != b.0 {
            return Err(format!("`{}` between a {} and a {}", op.symbol(), a.0.name(), b.0.name()));
        }
        let t = a.0;
        let cmp = |s: &mut Self, ins: &str| {
            let r = s.fresh("cmp");
            s.emit(format!("{r} = {ins} {} {}, {}", t.ll(), a.1, b.1));
            (T::Bool, r)
        };
        Ok(match (op, t) {
            (Eq | Ne, T::Unit) => (T::Bool, (op == Eq).to_string()),
            (Eq, T::Str) => self.call(T::Bool, "@strata_str_eq", &[a.clone(), b.clone()]),
            (Ne, T::Str) => {
                let e = self.call(T::Bool, "@strata_str_eq", &[a.clone(), b.clone()]);
                let r = self.fresh("ne");
                self.emit(format!("{r} = xor i1 {}, true", e.1));
                (T::Bool, r)
            }
            (Add, T::Str) => self.call(T::Str, "@strata_concat", &[a.clone(), b.clone()]),
            (Add, T::Int) => self.int_op("sadd", a, b),
            (Sub, T::Int) => self.int_op("ssub", a, b),
            (Mul, T::Int) => self.int_op("smul", a, b),
            (Div, T::Int) => self.call(T::Int, "@strata_div", &[a.clone(), b.clone()]),
            (Mod, T::Int) => self.call(T::Int, "@strata_mod", &[a.clone(), b.clone()]),
            (Add | Sub | Mul | Div | Mod, T::Float) => {
                let r = match op {
                    Mod => self.call(T::Float, "@strata_fmod", &[a.clone(), b.clone()]),
                    _ => {
                        let ins = match op {
                            Add => "fadd",
                            Sub => "fsub",
                            Mul => "fmul",
                            _ => "fdiv",
                        };
                        let r = self.fresh("f");
                        self.emit(format!("{r} = {ins} double {}, {}", a.1, b.1));
                        (T::Float, r)
                    }
                };
                // no NaN or infinity
                let sym = self.string(op.symbol());
                self.call(T::Float, "@strata_finite", &[r, a.clone(), b.clone(), sym])
            }
            (Eq, T::Int | T::Bool) => cmp(self, "icmp eq"),
            (Ne, T::Int | T::Bool) => cmp(self, "icmp ne"),
            (Lt, T::Int) => cmp(self, "icmp slt"),
            (Le, T::Int) => cmp(self, "icmp sle"),
            (Gt, T::Int) => cmp(self, "icmp sgt"),
            (Ge, T::Int) => cmp(self, "icmp sge"),
            (Eq, T::Float) => cmp(self, "fcmp oeq"),
            (Ne, T::Float) => cmp(self, "fcmp une"),
            (Lt, T::Float) => cmp(self, "fcmp olt"),
            (Le, T::Float) => cmp(self, "fcmp ole"),
            (Gt, T::Float) => cmp(self, "fcmp ogt"),
            (Ge, T::Float) => cmp(self, "fcmp oge"),
            (op, t) => return Err(format!("`{}` on a {}", op.symbol(), t.name())),
        })
    }

    fn call_named(&mut self, name: &str, args: &[Expr]) -> Res<V> {
        let env = self.m.env;
        if let Some(f) = env.fns.get(name) {
            if f.params.len() != args.len() {
                return Err(format!("`{name}` takes {} arguments", f.params.len()));
            }
            let mut vs = vec![];
            for (p, a) in f.params.iter().zip(args) {
                let v = self.expr(a)?;
                let want = self.m.lower(&p.ty)?;
                if v.0 != want && v.0 != T::Never {
                    return Err(format!("argument `{}` of `{name}` is a {}", p.name, v.0.name()));
                }
                vs.push(v);
            }
            if vs.iter().any(|v| v.0 == T::Never) {
                return Ok((T::Never, String::new()));
            }
            let ret = self.m.lower(&f.ret)?;
            self.m.want(name);
            return Ok(self.call(ret, &sym(name), &vs));
        }
        if crate::stdlib::is_host(name) || env.is_method(name) {
            return self.unsupported(&format!("`{name}`"));
        }
        let mut vs = vec![];
        for a in args {
            vs.push(self.expr(a)?);
        }
        if vs.iter().any(|v| v.0 == T::Never) {
            return Ok((T::Never, String::new()));
        }
        let types: Vec<T> = vs.iter().map(|v| v.0).collect();
        Ok(match (name, types.as_slice()) {
            ("print", [_]) => {
                let s = self.show(&vs[0]);
                self.call(T::Unit, "@strata_print", &[(T::Str, s)])
            }
            ("to_string", [_]) => {
                let s = self.show(&vs[0]);
                (T::Str, s)
            }
            ("len", [T::Str]) => self.call(T::Int, "@strata_str_len", &vs),
            ("abs", [T::Int]) => {
                // |i64::MIN| doesn't fit
                let min = (T::Int, i64::MIN.to_string());
                let is_min = self.apply(BinOp::Eq, &vs[0], &min)?;
                let (bad, ok) = (self.fresh_label("overflow"), self.fresh_label("abs"));
                self.cond_br(&is_min, &bad, &ok);
                self.block(&bad);
                self.emit("call void @strata_overflow()".into());
                self.emit("unreachable".into());
                self.block(&ok);
                self.call(T::Int, "@llvm.abs.i64", &[vs[0].clone(), (T::Bool, "false".into())])
            }
            ("abs", [T::Float]) => self.call(T::Float, "@llvm.fabs.f64", &vs),
            ("min" | "max", [a, b]) if a == b && matches!(a, T::Int | T::Float) => {
                let lt = self.apply(BinOp::Lt, &vs[0], &vs[1])?;
                let (x, y) = if name == "min" { (&vs[0], &vs[1]) } else { (&vs[1], &vs[0]) };
                let r = self.fresh("sel");
                self.emit(format!("{r} = select i1 {}, {} {}, {} {}", lt.1, a.ll(), x.1, a.ll(), y.1));
                (*a, r)
            }
            _ => {
                let ts: Vec<&str> = types.iter().map(|t| t.name()).collect();
                return self.unsupported(&format!("the built-in `{name}` on ({})", ts.join(", ")));
            }
        })
    }

    fn matching(&mut self, scrut: &Expr, as_name: Option<&str>, arms: &[Arm]) -> Res<V> {
        let v = self.expr(scrut)?;
        if v.0 == T::Never {
            return Ok(v);
        }
        self.scopes.push(HashMap::new());
        if let Some(n) = as_name {
            let slot = self.local(n, v.0, None)?;
            self.store(&v, &slot);
        }
        let end = self.fresh_label("endmatch");
        let mut incoming = vec![];
        for arm in arms {
            let (body, next) = (self.fresh_label("arm"), self.fresh_label("next_arm"));
            self.scopes.push(HashMap::new());
            let ok = self.pattern(&arm.pat, &v)?;
            self.cond_br(&ok, &body, &next);
            self.block(&body);
            self.bind(&arm.pat, &v)?;
            let r = self.expr(&arm.body)?;
            incoming.push((r, self.cur.clone()));
            self.br(&end);
            self.scopes.pop();
            self.block(&next);
        }
        let shown = self.json(&v);
        self.fault(Fault::Runtime, vec![Part::Lit("no match arm for ".into()), Part::Val(shown)]);
        self.block(&end);
        self.scopes.pop();
        self.join(incoming)
    }

    /// Whether `v` matches a pattern (bindings come after, in `bind`).
    fn pattern(&mut self, p: &Pattern, v: &V) -> Res<V> {
        let lit = |s: &mut Self, l: V| s.apply(BinOp::Eq, v, &l);
        Ok(match p {
            Pattern::Wild | Pattern::Bind(_) => (T::Bool, "true".into()),
            Pattern::Int(n) => lit(self, (T::Int, n.to_string()))?,
            Pattern::Bool(b) => lit(self, (T::Bool, b.to_string()))?,
            Pattern::Str(s) => {
                let l = self.string(s);
                lit(self, l)?
            }
            Pattern::Cond(c) => {
                // the condition sees the variables in scope, and `it`
                self.scopes.push(HashMap::new());
                let slot = self.local("it", v.0, None)?;
                self.store(v, &slot);
                let r = self.expr(c)?;
                self.scopes.pop();
                r
            }
            Pattern::Or(ps) => {
                if ps.iter().any(|p| !binds(p).is_empty()) {
                    return self.unsupported("an or-pattern that binds names");
                }
                let mut acc: V = (T::Bool, "false".into());
                for p in ps {
                    let c = self.pattern(p, v)?;
                    let r = self.fresh("or");
                    self.emit(format!("{r} = or i1 {}, {}", acc.1, c.1));
                    acc = (T::Bool, r);
                }
                acc
            }
            Pattern::Ctor(c, CtorPat::None) if !self.m.env.ctors.contains_key(c) && self.m.env.is_type_name(c) => {
                self.holds(&TypeExpr::Named(c.clone(), vec![]), v)?
            }
            Pattern::Ctor(c, _) => return self.unsupported(&format!("the pattern `{c}`")),
            Pattern::List(..) => return self.unsupported("a list pattern"),
            Pattern::Tuple(_) => return self.unsupported("a tuple pattern"),
        })
    }

    fn bind(&mut self, p: &Pattern, v: &V) -> Res<()> {
        if let Pattern::Bind(x) = p {
            let slot = self.local(x, v.0, None)?;
            self.store(v, &slot);
        }
        Ok(())
    }

    fn stmt(&mut self, s: &Stmt) -> Res<()> {
        match s {
            Stmt::Let(Pattern::Bind(_) | Pattern::Wild, ty, e) | Stmt::Var(_, ty, e) => {
                let v = self.expr(e)?;
                if let Some(t) = ty {
                    let want = self.m.lower(t)?;
                    if want != v.0 && v.0 != T::Never {
                        return Err(format!("a {} where a {} is expected", v.0.name(), want.name()));
                    }
                    self.check(t, &v, "let binding", false)?;
                }
                let (name, declared) = match s {
                    Stmt::Var(n, ty, _) => (n.clone(), ty.clone()),
                    Stmt::Let(Pattern::Bind(n), _, _) => (n.clone(), None),
                    _ => return Ok(()),
                };
                let t = match ty {
                    Some(t) => self.m.lower(t)?,
                    None => v.0,
                };
                let slot = self.local(&name, t, declared)?;
                self.store(&v, &slot);
            }
            Stmt::Let(..) => return self.unsupported("a destructuring `let`"),
            Stmt::Assign(place, e) => {
                let Expr::Var(x) = place else { return self.unsupported("assigning to a field or element") };
                let l = self.lookup(x).ok_or(format!("`{x}` is not a local variable"))?;
                let v = self.expr(e)?;
                if v.0 != l.t && v.0 != T::Never {
                    return Err(format!("assigning a {} to `{x}`, a {}", v.0.name(), l.t.name()));
                }
                if let Some(t) = &l.declared {
                    self.check(t, &v, &format!("assigning `{x}`"), false)?;
                }
                self.store(&v, &l.slot);
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
                self.store(&lo, &counter);
                let (head, inner, step, end) = (self.fresh_label("for"), self.fresh_label("for.body"), self.fresh_label("for.next"), self.fresh_label("endfor"));
                self.br(&head);
                self.block(&head);
                let i = self.fresh("i");
                self.emit(format!("{i} = load i64, ptr {counter}"));
                let more = self.apply(BinOp::Lt, &(T::Int, i.clone()), &hi)?;
                self.cond_br(&more, &inner, &end);
                self.block(&inner);
                self.scopes.push(HashMap::new());
                match p {
                    Pattern::Bind(_) | Pattern::Wild => self.bind(p, &(T::Int, i.clone()))?,
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
                if ok.0 == T::Never {
                    return Ok(());
                }
                let (pass, fail) = (self.fresh_label("assert.ok"), self.fresh_label("assert.fail"));
                self.cond_br(&ok, &pass, &fail);
                self.block(&fail);
                let mut parts = vec![Part::Lit(format!("assertion failed: {}", crate::printer::expr(x, 0)))];
                if let Some((op, a, b)) = sides {
                    let (sa, sb) = (self.json(&a), self.json(&b));
                    parts.extend([Part::Lit(format!(" (left of {} = ", op.symbol())), Part::Val(sa), Part::Lit(", right = ".into()), Part::Val(sb), Part::Lit(")".into())]);
                }
                self.fault(Fault::Assert, parts);
                self.block(&pass);
            }
            Stmt::Comment(_) => {}
        }
        Ok(())
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

/// The names a pattern binds.
fn binds(p: &Pattern) -> Vec<String> {
    struct B(Vec<String>);
    impl Visit for B {
        fn pattern(&mut self, p: &Pattern) {
            if let Pattern::Bind(x) = p {
                self.0.push(x.clone());
            }
            walk_pattern(self, p)
        }
    }
    let mut b = B(vec![]);
    b.pattern(p);
    b.0
}
