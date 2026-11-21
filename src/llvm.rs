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
//! Lists point to a header over a shared buffer of 8-byte slots; a `Set` is
//! a list kept sorted in the interpreter's order, a `Dict` a list of
//! `(key, value)` pairs sorted by key.
//!
//! Not yet: big integers (`Int` overflow is a fault), regexes, options from
//! the command line, unions of anything but records and variants, and a
//! garbage collector (memory is never freed). A program that uses them is refused, with the function and
//! the construct named.

use crate::ast::*;
use crate::types::{Env, Ty};
use std::collections::VecDeque;
use crate::fx::{HashMap, HashSet};
use std::fmt::Write;

#[cfg(feature = "jit")]
pub mod jit;

/// The list built-ins written in Strata, compiled with every program.
const PRELUDE: &[u8] = include_bytes!("llvm/prelude.strata");
const PRELUDE_MODULE: &str = "strata.compiled";

/// A function of the compiled prelude.
fn prelude(name: &str) -> String {
    format!("{PRELUDE_MODULE}.{name}")
}

/// The place an argument names (`x`, `o.status`): its variable and the path of fields.
fn place_of(e: &Expr) -> Option<(String, Vec<String>)> {
    match e {
        Expr::Mut(x) => place_of(x),
        Expr::Var(x) => Some((x.to_string(), vec![])),
        Expr::Field(r, f) => {
            let (root, mut path) = place_of(r)?;
            path.push(f.to_string());
            Some((root, path))
        }
        _ => None,
    }
}

/// The variables an expression uses (what a lambda captures).
fn vars_in(e: &Expr) -> Vec<String> {
    struct Vars(Vec<String>);
    impl Visit for Vars {
        fn expr(&mut self, e: &Expr) {
            if let Expr::Var(x) = e {
                self.0.push(x.to_string());
            }
            walk_expr(self, e)
        }
    }
    let mut v = Vars(vec![]);
    v.expr(e);
    v.0
}

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
        Ty::Con(n, a) if a.is_empty() => n.to_string(),
        Ty::Con(n, a) => format!("{n}[{}]", list(a)),
        Ty::Fn(ps, r) => format!("({}) -> {}", list(ps), key(r)),
        Ty::Union(ms) => ms.iter().map(key).collect::<Vec<_>>().join(" | "),
        Ty::Rigid(n) => n.to_string(),
        Ty::Var(_) => "?".into(),
    }
}

/// The LLVM IR of a program whose entry point is `entry` (`main`): it takes
/// no parameters and returns `()`, an exit code (`Int`), or a `Result` of either.
pub fn compile(prog: &Program, entry: &str) -> Res<String> {
    // the list built-ins in Strata, compiled with the program
    let mut with = prog.clone();
    let prelude = crate::binary::decode_module(PRELUDE).map_err(|e| format!("llvm/prelude.strata: {e}"))?;
    with.decls.extend(prelude);
    let (resolved, types) = crate::types::elaborate_typed(&with);
    let (env, _) = Env::build(&resolved.program);
    let main = env.fns.get(&Symbol::new(entry)).ok_or(format!("no `{entry}` function to compile"))?.clone();
    // every constructor gets a tag of its own, so a value's tag says what it is
    let mut ctors: Vec<&Symbol> = env.ctors.keys().collect();
    ctors.sort();
    let tags = ctors.iter().enumerate().map(|(i, c)| (c.to_string(), i as i64 + 1)).collect();
    // bodies as the checker saw them (its types are by node); signatures from `env`
    let mut bodies: HashMap<String, &FnDecl> = HashMap::default();
    for d in &resolved.program.decls {
        match &d.kind {
            DeclKind::Fn(f) => {
                bodies.insert(f.name.to_string(), f);
            }
            DeclKind::Impl(i) if i.is_inherent() => {
                for mth in &i.methods {
                    bodies.insert((crate::types::static_fn(i, mth).name).to_string(), mth);
                }
            }
            _ => {}
        }
    }
    let traits = resolved.program.decls.iter().filter_map(|d| match &d.kind {
        DeclKind::Trait(t) => Some((t.name.clone(), t)),
        _ => None,
    }).collect();
    let impls = resolved.program.decls.iter().filter_map(|d| match &d.kind {
        DeclKind::Impl(i) if !i.is_inherent() => Some(i),
        _ => None,
    }).collect();
    let refinements = resolved.program.decls.iter().filter_map(|d| match &d.kind {
        DeclKind::Type(t) => t.refinement.as_ref().map(|r| (t.name.to_string(), r)),
        _ => None,
    }).collect();
    let mut m = Module { env: &env, types: &types, bodies, traits, impls, refinements, tags, globals: String::new(), strings: HashMap::default(), body: String::new(), queue: VecDeque::new(), seen: HashSet::default(), helpers: HashSet::default(), lambdas: 0 };
    let ret = m.lower(&main.ret, &HashMap::default())?;
    // C's `main` first: it asks for the program's `main`, with its capabilities
    m.entry(entry, &main, &ret)?;
    while let Some(job) = m.queue.pop_front() {
        match job {
            Job::Fn(name, targs, impls) => {
                let f = env.fns[&Symbol::new(&name)].clone();
                let shown = if targs.is_empty() { name.clone() } else { format!("{name}[{}]", targs.iter().map(key).collect::<Vec<_>>().join(", ")) };
                m.function(&name, &targs, &impls, &f).map_err(|e| format!("can't compile `{shown}`: {e}"))?;
            }
            Job::Method(mi) => {
                let shown = format!("{}.{} for {}", mi.tr, mi.method, key(&mi.recv));
                m.method(&mi).map_err(|e| format!("can't compile `{shown}`: {e}"))?;
            }
        }
    }
    let mut out = String::from("; Strata, compiled by `strata build`\n\n");
    out += &m.globals;
    out += "\n";
    out += &m.body;
    out += DECLARATIONS;
    Ok(out)
}

const DECLARATIONS: &str = "\
declare ptr @strata_alloc(i64)
declare void @strata_init(i32, ptr)
declare i64 @strata_pid()
declare void @strata_host_write(i64, ptr)
declare ptr @strata_host_read_line()
declare ptr @strata_host_read_file(ptr, ptr, ptr)
declare i1 @strata_host_write_file(ptr, ptr, i1, ptr, ptr)
declare i1 @strata_host_exists(ptr)
declare ptr @strata_host_list_dir(ptr, ptr, ptr)
declare i1 @strata_host_make_dir(ptr, ptr, ptr)
declare i1 @strata_host_remove(ptr, ptr, ptr)
declare ptr @strata_host_args()
declare ptr @strata_host_env_var(ptr)
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
declare i64 @strata_float_to_int(double, double)
declare double @strata_sqrt(double)
declare ptr @strata_list_new(i64)
declare ptr @strata_list_push(ptr, i64)
declare ptr @strata_list_slice(ptr, i64, i64)
declare ptr @strata_list_concat(ptr, ptr)
declare ptr @strata_range(i64, i64)
declare i64 @llvm.smax.i64(i64, i64)
declare i64 @llvm.smin.i64(i64, i64)
declare ptr @strata_show_int(i64)
declare ptr @strata_show_float(double)
declare ptr @strata_show_bool(i1)
declare ptr @strata_json_str(ptr)
declare void @strata_print(ptr)
declare ptr @strata_str_split(ptr, ptr)
declare ptr @strata_str_trim(ptr)
declare i1 @strata_str_contains(ptr, ptr)
declare i1 @strata_str_starts_with(ptr, ptr)
declare i1 @strata_str_ends_with(ptr, ptr)
declare i64 @strata_str_index_of(ptr, ptr)
declare ptr @strata_str_reverse(ptr)
declare ptr @strata_str_chars(ptr)
declare ptr @strata_str_at(ptr, i64)
declare ptr @strata_str_at_or_fault(ptr, i64)
declare ptr @strata_str_slice(ptr, i64, i64)
declare ptr @strata_str_replace(ptr, ptr, ptr)
declare ptr @strata_str_repeat(ptr, i64)
declare ptr @strata_str_case(ptr, ptr, i64, ptr, i64, i1)
declare i1 @strata_str_is(ptr, i64, ptr, i64)
declare i64 @strata_str_ord(ptr)
declare ptr @strata_str_chr(i64)
declare i1 @strata_parse_int(ptr, ptr)
declare i1 @strata_parse_float(ptr, ptr)
declare ptr @strata_show_float_fixed(double, i64)
declare ptr @strata_format(ptr, i1, i32, i32, i1, i1, i64)
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
/// `sym`, for an instance taking these implementations for its trait-typed parameters.
fn sym_impls(name: &str, targs: &[Ty], impls: &[Option<Ty>]) -> String {
    let base = sym(name, targs);
    if impls.iter().all(|i| i.is_none()) {
        return base;
    }
    let shown: Vec<String> = impls.iter().map(|i| i.as_ref().map(key).unwrap_or("_".into())).collect();
    format!("{}{{{}}}\"", base.trim_end_matches('"'), shown.join(", "))
}

fn sym(name: &str, targs: &[Ty]) -> String {
    if targs.is_empty() {
        format!("@\"strata:{name}\"")
    } else {
        format!("@\"strata:{name}[{}]\"", targs.iter().map(key).collect::<Vec<_>>().join(", "))
    }
}

/// What to compile: a function, or a trait method, for some types.
enum Job {
    /// A function, its type arguments, and the implementation passed to each
    /// parameter whose type is a trait (`console: Console` is `console: C`
    /// for some `C: Console`).
    Fn(String, Vec<Ty>, Vec<Option<Ty>>),
    Method(MethodInst),
}

/// A trait method for one receiver type (and type arguments of its own).
#[derive(Clone)]
struct MethodInst {
    tr: String,
    method: String,
    /// The type it is called on, and the name of its implementation's type
    /// (a variant with an impl of its own, or the type).
    recv: Ty,
    head: String,
    targs: Vec<Ty>,
}

impl MethodInst {
    fn symbol(&self) -> String {
        let own = if self.targs.is_empty() { String::new() } else { format!("[{}]", self.targs.iter().map(key).collect::<Vec<_>>().join(", ")) };
        format!("@\"strata:{}::{}@{}{own}\"", self.tr, self.method, key(&self.recv))
    }
}

/// One instance of a function to compile.
struct Instance<'a> {
    /// Its name in messages (`total`, `Item.price`).
    label: String,
    symbol: String,
    /// The implementation each trait-typed parameter takes (`None`: not one).
    impls: Vec<Option<Ty>>,
    /// The signature (written types, lowered with `sub`).
    params: Vec<Param>,
    ret: TypeExpr,
    /// The declaration's own nodes, which the checker typed.
    own_params: &'a [Param],
    own_ret: &'a TypeExpr,
    requires: Vec<&'a Expr>,
    ensures: Vec<&'a Expr>,
    body: &'a Expr,
    /// Type parameters (and `Self`) -> types.
    sub: HashMap<Symbol, Ty>,
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
    /// Traits and their implementations, as written (their bodies are what the checker typed).
    traits: HashMap<Symbol, &'a TraitDecl>,
    impls: Vec<&'a ImplDecl>,
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
    queue: VecDeque<Job>,
    seen: HashSet<String>,
    /// Generated helpers (equality, JSON) already emitted.
    helpers: HashSet<String>,
    /// Lambdas compiled so far (each is a function of its own).
    lambdas: usize,
}

impl<'a> Module<'a> {
    fn want_impls(&mut self, name: &str, targs: Vec<Ty>, impls: Vec<Option<Ty>>) -> String {
        let s = sym_impls(name, &targs, &impls);
        if self.seen.insert(s.clone()) {
            self.queue.push_back(Job::Fn(name.to_string(), targs, impls));
        }
        s
    }

    /// Whether a written type is a trait (a parameter that takes any implementation).
    fn is_trait(&self, t: &TypeExpr) -> bool {
        matches!(t, TypeExpr::Named(n, a) if a.is_empty() && self.traits.contains_key(n))
    }

    fn want_method(&mut self, mi: MethodInst) -> String {
        let s = mi.symbol();
        if self.seen.insert(s.clone()) {
            self.queue.push_back(Job::Method(mi));
        }
        s
    }

    /// The type named `head` as it runs (a value's head, for method dispatch).
    fn head(t: &Ty) -> Option<String> {
        Some(match t {
            Ty::Int => "Int".into(),
            Ty::Float => "Float".into(),
            Ty::Bool => "Bool".into(),
            Ty::Str => "String".into(),
            Ty::Unit => "Unit".into(),
            Ty::Con(n, _) => n.to_string(),
            _ => return None,
        })
    }

    /// The trait and method a call of `name` on a value whose type is `head` means.
    fn resolve_method(&self, name: &str, head: &str) -> Option<(String, String)> {
        match crate::types::split_trait_call(name) {
            Some((tr, m)) => Some((tr.to_string(), m.to_string())),
            None => match self.env.traits_with(name, head).as_slice() {
                [tr] => Some((tr.to_string(), name.to_string())),
                _ => None,
            },
        }
    }

    /// The implementation of trait `tr` for type `head`, as written.
    fn impl_of(&self, tr: &str, head: &str) -> Option<&'a ImplDecl> {
        self.impls.iter().copied().find(|i| i.trait_name == tr && crate::types::texpr_head(&i.for_type) == head)
    }

    /// The trait method `m` of `tr`, as written.
    fn trait_method(&self, tr: &str, m: &str) -> Option<&'a TraitMethod> {
        self.traits.get(&Symbol::new(tr))?.methods.iter().find(|x| x.decl.name == m)
    }

    /// Compile one instance of a trait method: the impl's own, or the trait's default.
    /// A method instance's types (`Self`, the impl's and trait's
    /// parameters, its own), and its declaration (the impl's, or the default).
    fn method_sub(&self, mi: &MethodInst) -> Res<(HashMap<Symbol, Ty>, &'a FnDecl, &'a TraitMethod)> {
        let tm = self.trait_method(&mi.tr, &mi.method).ok_or(format!("trait `{}` has no method `{}`", mi.tr, mi.method))?;
        let imp = self.impl_of(&mi.tr, &mi.head).ok_or(format!("`{}` does not implement `{}`", mi.head, mi.tr))?;
        let own = imp.methods.iter().find(|m| m.name == mi.method).unwrap_or(&tm.decl);
        let mut sub = HashMap::default();
        sub.insert(Symbol::new("Self"), mi.recv.clone());
        self.bind_targs(&imp.for_type, &mi.recv, &imp.type_params, &mut sub);
        let tr = self.traits[&Symbol::new(&mi.tr)];
        for (p, a) in tr.params.iter().zip(&imp.trait_args) {
            let t = self.lower(a, &sub)?;
            sub.insert(p.clone(), t);
        }
        for (p, t) in own.type_params.iter().zip(&mi.targs) {
            sub.insert(p.clone(), t.clone());
        }
        Ok((sub, own, tm))
    }

    fn method(&mut self, mi: &MethodInst) -> Res<()> {
        let (sub, own, tm) = self.method_sub(mi)?;
        // the trait's contract binds every implementation
        let mut requires: Vec<&Expr> = tm.decl.requires.iter().collect();
        let mut ensures: Vec<&Expr> = tm.decl.ensures.iter().collect();
        if !std::ptr::eq(own, &tm.decl) {
            requires.extend(&own.requires);
            ensures.extend(&own.ensures);
        }
        let label = format!("{}.{}", crate::names::short(&mi.head), mi.method);
        let inst = Instance { label, symbol: mi.symbol(), impls: vec![], params: own.params.clone(), ret: own.ret.clone(), own_params: &own.params, own_ret: &own.ret, requires, ensures, body: &own.body, sub };
        self.instance(&inst)
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
        self.refinements.get(n).copied().or_else(|| self.env.types.get(&Symbol::new(n))?.refinement.as_ref())
    }

    fn is_distinct(&self, n: &str) -> bool {
        self.env.types.get(&Symbol::new(n)).is_some_and(|t| t.distinct)
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
    fn lower(&self, t: &TypeExpr, sub: &HashMap<Symbol, Ty>) -> Res<Ty> {
        Ok(match t {
            TypeExpr::Unit => Ty::Unit,
            TypeExpr::Refined(b, _) => self.lower(b, sub)?,
            TypeExpr::Fn(ps, r) => Ty::Fn(ps.iter().map(|p| self.lower(p, sub)).collect::<Res<_>>()?, Box::new(self.lower(r, sub)?)),
            TypeExpr::Union(ms) => Ty::Union(ms.iter().map(|m| self.lower(m, sub)).collect::<Res<_>>()?),
            TypeExpr::Named(n, args) => {
                if let Some(t) = sub.get(&n.sym()) {
                    return Ok(t.clone());
                }
                let args: Vec<Ty> = args.iter().map(|a| self.lower(a, sub)).collect::<Res<_>>()?;
                match n.as_str() {
                    crate::types::TYPE_ARG => Ty::Unit,
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
                Some(TypeBody::Sum(vs)) => vs.iter().map(|v| v.name.to_string()).collect(),
                Some(TypeBody::Record(_)) => vec![n.to_string()],
                _ if self.env.ctors.contains_key(n) => vec![n.to_string()],
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
        let tname = self.env.ctors.get(&Symbol::new(c)).ok_or(format!("unknown constructor `{c}`"))?;
        let td = &self.env.types[tname];
        let sub: HashMap<Symbol, Ty> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
        let mk = |name: Option<String>, te: &TypeExpr| -> Res<FieldInfo> { Ok(FieldInfo { name, texpr: te.clone(), ty: self.lower(te, &sub)? }) };
        match &td.body {
            TypeBody::Record(fs) => fs.iter().map(|f| mk(Some(f.name.to_string()), &f.ty)).collect(),
            TypeBody::Sum(vs) => {
                let v = vs.iter().find(|v| v.name == c).unwrap();
                match &v.payload {
                    Payload::None => Ok(vec![]),
                    Payload::Positional(ts) => ts.iter().map(|t| mk(None, t)).collect(),
                    Payload::Record(fs) => fs.iter().map(|f| mk(Some(f.name.to_string()), &f.ty)).collect(),
                }
            }
            TypeBody::Alias(a) => Ok(vec![mk(None, a)?]),
        }
    }

    fn function(&mut self, name: &str, targs: &[Ty], impls: &[Option<Ty>], f: &FnDecl) -> Res<()> {
        let own = *self.bodies.get(name).ok_or(format!("no declaration of `{name}`"))?;
        let sub: HashMap<Symbol, Ty> = f.type_params.iter().cloned().zip(targs.iter().cloned()).collect();
        let inst = Instance {
            label: name.to_string(),
            symbol: sym_impls(name, targs, impls),
            impls: impls.to_vec(),
            params: f.params.clone(),
            ret: f.ret.clone(),
            own_params: &own.params,
            own_ret: &own.ret,
            requires: own.requires.iter().collect(),
            ensures: own.ensures.iter().collect(),
            body: &own.body,
            sub,
        };
        self.instance(&inst)
    }

    /// Compile a function's instance: its parameters' and result's
    /// refinements, its contract, its body.
    fn instance(&mut self, inst: &Instance<'a>) -> Res<()> {
        let (name, sub, own) = (&inst.label, inst.sub.clone(), inst);
        let f = inst;
        let mut params = Vec::new();
        for (i, p) in f.params.iter().enumerate() {
            match f.impls.get(i).cloned().flatten() {
                Some(t) => params.push(t),
                None if self.is_trait(&p.ty) => return Err(format!("`{}` takes any `{}`: compiled for the implementation a caller passes, not on its own", p.name, crate::printer::ty(&p.ty))),
                None => params.push(self.lower(&p.ty, &sub)?),
            }
        }
        let ret = self.lower(&f.ret, &sub)?;
        if self.is_trait(&f.ret) {
            return Err(format!("returning a value of the trait type `{}` is not compiled yet", crate::printer::ty(&f.ret)));
        }
        let mut c = Func::new(self, sub);
        let mut sig = vec![];
        for (p, t) in f.params.iter().zip(&params) {
            if p.mutable {
                // the caller's slot: changes go straight to its variable
                sig.push(format!("ptr %p.{}", p.name));
            } else if has_value(t) {
                sig.push(format!("{} %p.{}", c.m.ll(t)?, p.name));
            }
        }
        c.block("entry.body");
        // parameters live in slots, like every variable
        for (i, (p, t)) in f.params.iter().zip(&params).enumerate() {
            if p.mutable {
                let declared = own.own_params.get(i).map(|q| q.ty.clone());
                c.scopes.last_mut().unwrap().insert(p.name.to_string(), Local { t: t.clone(), slot: format!("%p.{}", p.name), declared });
                continue;
            }
            let slot = c.local(&p.name, t, None)?;
            c.store(&(t.clone(), format!("%p.{}", p.name)), &slot)?;
        }
        // the written types of the program's own declaration (inline refinements are its nodes)
        for p in own.own_params {
            let v = c.load(&p.name)?;
            c.check(&p.ty, &v, &format!("argument `{}` of `{name}`", p.name), false)?;
        }
        for r in own.requires.clone() {
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
        let v = c.expr(own.body)?;
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
        c.check(own.own_ret, &result, &format!("result of `{name}`"), true)?;
        if !own.ensures.is_empty() {
            c.scopes.push(HashMap::default());
            let slot = c.local("it", &ret, None)?;
            c.store(&result, &slot)?;
            for en in own.ensures.clone() {
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
        writeln!(self.body, "define {} {}({}) {{\n{text}}}\n", self.ll(&ret)?, inst.symbol, sig.join(", ")).unwrap();
        Ok(())
    }

    /// C's `main`: make the capabilities the program's `main` asks for (as
    /// `strata run` does: the console, the disk from `.`, the process), call
    /// it, and turn its result into an exit code.
    fn entry(&mut self, entry: &str, main: &FnDecl, ret: &Ty) -> Res<()> {
        let mut c = Func::new(self, HashMap::default());
        c.block("start");
        c.emit("call void @strata_init(i32 %argc, ptr %argv)".into());
        let (mut args, mut impls) = (vec![], vec![]);
        for p in &main.params {
            let wanted = crate::printer::ty(&p.ty);
            let (imp, fields): (&str, Vec<(&str, V)>) = match wanted.as_str() {
                "std.io.Console" => ("std.io.Terminal", vec![("stdout", (Ty::Int, "1".into())), ("stderr", (Ty::Int, "2".into()))]),
                "std.fs.Files" => ("std.fs.Disk", vec![("root", c.string("."))]),
                "std.process.Environment" => ("std.process.Process", vec![("pid", c.call(&Ty::Int, "@strata_pid", &[])?)]),
                _ => return Err(format!("`{entry}` asks for `{}: {wanted}`: compiled programs get `Console`, `Files` and `Environment` (options from the command line are not compiled yet)", p.name)),
            };
            let fs = c.m.fields(imp, &[])?;
            let vals: Vec<V> = fs.iter().map(|f| fields.iter().find(|(n, _)| Some(*n) == f.name.as_deref()).map(|x| x.1.clone()).ok_or(format!("`{imp}` has a field `{}`", f.name.clone().unwrap_or_default()))).collect::<Res<_>>()?;
            let t = Ty::Con(Symbol::new(imp), vec![]);
            let b = c.block_of(c.m.tags[imp], &vals)?;
            if p.mutable {
                let slot = c.fresh("cap");
                writeln!(c.allocas, "  {slot} = alloca ptr").unwrap();
                c.emit(format!("store ptr {b}, ptr {slot}"));
                args.push((Ty::Str, slot));
            } else {
                args.push((t.clone(), b));
            }
            impls.push(Some(t));
        }
        let f = c.m.want_impls(entry, vec![], impls);
        match ret {
            Ty::Unit => {
                c.call(&Ty::Unit, &f, &args)?;
                c.emit("ret i32 0".into());
            }
            Ty::Int => {
                let r = c.call(&Ty::Int, &f, &args)?;
                c.emit(format!("%r = add i64 {}, 0", r.1));
                c.emit("%c = trunc i64 %r to i32".into());
                c.emit("ret i32 %c".into());
            }
            Ty::Con(n, rargs) if n == "Result" && matches!(rargs[0], Ty::Unit | Ty::Int) => {
                let args0 = c.call(ret, &f, &args)?;
                c.emit(format!("%r = getelementptr i8, ptr {}, i64 0", args0.1));
                let args = rargs;
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
        writeln!(self.body, "define i32 @main(i32 %argc, ptr %argv) {{\n{text}}}\n").unwrap();
        Ok(())
    }
}

/// A piece of a message built at run time.
#[derive(Clone)]
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
    sub: HashMap<Symbol, Ty>,
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
    /// The places of the arguments of the call being compiled (for `mut` parameters).
    places: Vec<Option<(String, Vec<String>)>>,
}

impl<'m, 'a> Func<'m, 'a> {
    fn new(m: &'m mut Module<'a>, sub: HashMap<Symbol, Ty>) -> Self {
        Func { m, sub, allocas: String::new(), code: String::new(), tmp: 0, label: 0, scopes: vec![HashMap::default()], loops: vec![], ret_slot: None, cur: String::new(), terminated: true, places: vec![] }
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
            Ty::Rigid(n) => self.sub.get(&n.sym()).cloned().unwrap_or(Ty::Unit),
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
        if self.is_dyn(&t) {
            return self.dyn_entry_call(v, 1);
        }
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
        if self.is_dyn(&t) {
            return self.dyn_entry_call(v, 0);
        }
        Ok(match &t {
            Ty::Int => self.call(&Ty::Str, "@strata_show_int", &[v.clone()])?.1,
            Ty::Float => self.call(&Ty::Str, "@strata_show_float", &[v.clone()])?.1,
            Ty::Bool => self.call(&Ty::Str, "@strata_show_bool", &[v.clone()])?.1,
            Ty::Str => self.call(&Ty::Str, "@strata_json_str", &[v.clone()])?.1,
            Ty::Unit => self.m.string("null"),
            _ if is_never(&t) => self.m.string("null"),
            Ty::Con(n, _) if n == "Set" => {
                let l = self.as_list(v)?;
                self.json(&l)?
            }
            t if self.m.nominal(t) || matches!(t, Ty::Con(l, _) if l == "List" || l == "Dict") => {
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
            t if collection(t).is_some() => {
                let (x, y) = (self.as_list(a)?, self.as_list(b)?);
                self.equal(&x, &y)?
            }
            t if self.m.nominal(t) || matches!(t, Ty::Con(l, _) if l == "List") => {
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
        let mut h = Func::new(self.m, HashMap::default());
        h.block("start");
        let a: V = (t.clone(), "%a".into());
        let b: V = (t.clone(), "%b".into());
        let tuple = matches!(t, Ty::Con(n, _) if n == "Tuple");
        let args = Module::args_of(t);
        let list = matches!(t, Ty::Con(l, _) if l == "List");
        let sig = match what {
            "cmp" => {
                // the interpreter's order (`interp::compare`): -1, 0 or 1
                let ll = h.m.ll(t)?;
                let three = |h: &mut Func, lt: &str, gt: &str| {
                    let (x, y, r) = (h.fresh("lt"), h.fresh("gt"), h.fresh("c"));
                    h.emit(format!("{x} = zext i1 {lt} to i64"));
                    h.emit(format!("{y} = zext i1 {gt} to i64"));
                    h.emit(format!("{r} = sub i64 {y}, {x}"));
                    h.emit(format!("ret i64 {r}"));
                };
                let distinct = matches!(t, Ty::Con(n, _) if h.m.is_distinct(n));
                match h.m.repr(t)? {
                    _ if distinct => {
                        // a `distinct` value is not a number or a string to `compare`: by its JSON
                        let (x, y) = (h.json(&a)?, h.json(&b)?);
                        let c = h.call(&Ty::Int, "@strata_str_cmp", &[(Ty::Str, x), (Ty::Str, y)])?;
                        h.emit(format!("ret i64 {}", c.1));
                    }
                    Ty::Int | Ty::Bool => {
                        let (lt, gt) = (h.fresh("l"), h.fresh("g"));
                        let (pl, pg) = if matches!(h.m.repr(t)?, Ty::Int) { ("slt", "sgt") } else { ("ult", "ugt") };
                        h.emit(format!("{lt} = icmp {pl} {ll} %a, %b"));
                        h.emit(format!("{gt} = icmp {pg} {ll} %a, %b"));
                        three(&mut h, &lt, &gt);
                    }
                    Ty::Float => {
                        // `f64::total_cmp`: the bits, negative numbers' flipped, as signed integers (-0.0 < 0.0)
                        let mut keyed = vec![];
                        for x in ["%a", "%b"] {
                            let (i, sign, mask, k) = (h.fresh("bits"), h.fresh("sign"), h.fresh("mask"), h.fresh("key"));
                            h.emit(format!("{i} = bitcast double {x} to i64"));
                            h.emit(format!("{sign} = ashr i64 {i}, 63"));
                            h.emit(format!("{mask} = lshr i64 {sign}, 1"));
                            h.emit(format!("{k} = xor i64 {i}, {mask}"));
                            keyed.push(k);
                        }
                        let (lt, gt) = (h.fresh("l"), h.fresh("g"));
                        h.emit(format!("{lt} = icmp slt i64 {}, {}", keyed[0], keyed[1]));
                        h.emit(format!("{gt} = icmp sgt i64 {}, {}", keyed[0], keyed[1]));
                        three(&mut h, &lt, &gt);
                    }
                    Ty::Str => {
                        let c = h.call(&Ty::Int, "@strata_str_cmp", &[a.clone(), b.clone()])?;
                        h.emit(format!("ret i64 {}", c.1));
                    }
                    Ty::Unit => h.emit("ret i64 0".into()),
                    _ if tuple => {
                        for (i, ft) in args.iter().enumerate() {
                            let (x, y) = (h.field(&a, i, ft)?, h.field(&b, i, ft)?);
                            let f = h.helper("cmp", ft)?;
                            let c = h.call(&Ty::Int, &f, &[x, y])?;
                            let (z, next, out) = (h.fresh("z"), h.fresh_label("next"), h.fresh_label("differ"));
                            h.emit(format!("{z} = icmp eq i64 {}, 0", c.1));
                            h.emit(format!("br i1 {z}, label %{next}, label %{out}"));
                            h.block(&out);
                            h.emit(format!("ret i64 {}", c.1));
                            h.block(&next);
                        }
                        h.emit("ret i64 0".into());
                    }
                    _ if list => {
                        let et = args[0].clone();
                        let f = h.helper("cmp", &et)?;
                        let (na, nb) = (h.list_len(&a), h.list_len(&b));
                        h.each(&a, &et, |h, i, x| {
                            let inb = h.fresh("inb");
                            h.emit(format!("{inb} = icmp slt i64 {i}, {}", nb.1));
                            let (go, past) = (h.fresh_label("both"), h.fresh_label("past"));
                            h.cond_br(&bool_v(&inb), &go, &past);
                            h.block(&past);
                            h.emit("ret i64 1".into());
                            h.block(&go);
                            let y = h.list_at(&b, i, &et)?;
                            let c = h.call(&Ty::Int, &f, &[x.clone(), y])?;
                            let (z, next, out) = (h.fresh("z"), h.fresh_label("next"), h.fresh_label("differ"));
                            h.emit(format!("{z} = icmp eq i64 {}, 0", c.1));
                            h.emit(format!("br i1 {z}, label %{next}, label %{out}"));
                            h.block(&out);
                            h.emit(format!("ret i64 {}", c.1));
                            h.block(&next);
                            Ok(())
                        })?;
                        let (lt, gt) = (h.fresh("l"), h.fresh("g"));
                        h.emit(format!("{lt} = icmp slt i64 {}, {}", na.1, nb.1));
                        h.emit(format!("{gt} = icmp sgt i64 {}, {}", na.1, nb.1));
                        three(&mut h, &lt, &gt);
                    }
                    _ => {
                        // anything else: by its JSON
                        let (x, y) = (h.json(&a)?, h.json(&b)?);
                        let c = h.call(&Ty::Int, "@strata_str_cmp", &[(Ty::Str, x), (Ty::Str, y)])?;
                        h.emit(format!("ret i64 {}", c.1));
                    }
                }
                format!("define i64 {name}({ll} %a, {ll} %b)")
            }
            "eq" if list => {
                let et = args[0].clone();
                let (na, nb) = (h.list_len(&a), h.list_len(&b));
                let same = h.fresh("samelen");
                h.emit(format!("{same} = icmp eq i64 {}, {}", na.1, nb.1));
                h.emit(format!("br i1 {same}, label %elements, label %no"));
                h.block("elements");
                h.each(&a, &et, |h, i, x| {
                    let y = h.list_at(&b, i, &et)?;
                    let e = h.equal(x, &y)?;
                    let next = h.fresh_label("next");
                    h.emit(format!("br i1 {}, label %{next}, label %no", e.1));
                    h.block(&next);
                    Ok(())
                })?;
                h.emit("ret i1 true".into());
                h.block("no");
                h.emit("ret i1 false".into());
                format!("define i1 {name}(ptr %a, ptr %b)")
            }
            "json" if matches!(t, Ty::Con(d, _) if d == "Dict") => {
                // an object when the keys are strings (or there are none), else an array of pairs
                let (kt, vt) = (args[0].clone(), args[1].clone());
                let object = kt == Ty::Str;
                let pair = Ty::Con("Tuple".into(), args.clone());
                let n = h.list_len(&a);
                let empty = h.fresh("empty");
                h.emit(format!("{empty} = icmp eq i64 {}, 0", n.1));
                h.emit(format!("br i1 {empty}, label %none, label %some"));
                h.block("none");
                let braces = h.m.string("{}");
                h.emit(format!("ret ptr {braces}"));
                h.block("some");
                let acc = h.fresh("acc");
                writeln!(h.allocas, "  {acc} = alloca ptr").unwrap();
                let open = h.m.string(if object { "{" } else { "[" });
                h.emit(format!("store ptr {open}, ptr {acc}"));
                let (empty_s, comma) = (h.m.string(""), h.m.string(","));
                h.each(&a, &pair, |h, i, p| {
                    let first = h.fresh("first");
                    h.emit(format!("{first} = icmp eq i64 {i}, 0"));
                    let sep = h.fresh("sep");
                    h.emit(format!("{sep} = select i1 {first}, ptr {empty_s}, ptr {comma}"));
                    let (k, v) = (h.field(p, 0, &kt)?, h.field(p, 1, &vt)?);
                    let (kj, vj) = (h.json(&k)?, h.json(&v)?);
                    let cur = h.fresh("cur");
                    h.emit(format!("{cur} = load ptr, ptr {acc}"));
                    let parts = if object {
                        vec![Part::Val(cur), Part::Val(sep), Part::Val(kj), Part::Lit(":".into()), Part::Val(vj)]
                    } else {
                        vec![Part::Val(cur), Part::Val(sep), Part::Lit("[".into()), Part::Val(kj), Part::Lit(",".into()), Part::Val(vj), Part::Lit("]".into())]
                    };
                    let s = h.concat(parts)?;
                    h.emit(format!("store ptr {}, ptr {acc}", s.1));
                    Ok(())
                })?;
                let cur = h.fresh("cur");
                h.emit(format!("{cur} = load ptr, ptr {acc}"));
                let s = h.concat(vec![Part::Val(cur), Part::Lit(if object { "}" } else { "]" }.into())])?;
                h.emit(format!("ret ptr {}", s.1));
                format!("define ptr {name}(ptr %a)")
            }
            "json" if list => {
                let et = args[0].clone();
                let acc = h.fresh("acc");
                writeln!(h.allocas, "  {acc} = alloca ptr").unwrap();
                let open = h.m.string("[");
                h.emit(format!("store ptr {open}, ptr {acc}"));
                let (empty, comma) = (h.m.string(""), h.m.string(","));
                h.each(&a, &et, |h, i, x| {
                    let first = h.fresh("first");
                    h.emit(format!("{first} = icmp eq i64 {i}, 0"));
                    let sep = h.fresh("sep");
                    h.emit(format!("{sep} = select i1 {first}, ptr {empty}, ptr {comma}"));
                    let j = h.json(x)?;
                    let cur = h.fresh("cur");
                    h.emit(format!("{cur} = load ptr, ptr {acc}"));
                    let s = h.concat(vec![Part::Val(cur), Part::Val(sep), Part::Val(j)])?;
                    h.emit(format!("store ptr {}, ptr {acc}", s.1));
                    Ok(())
                })?;
                let cur = h.fresh("cur");
                h.emit(format!("{cur} = load ptr, ptr {acc}"));
                let s = h.concat(vec![Part::Val(cur), Part::Lit("]".into())])?;
                h.emit(format!("ret ptr {}", s.1));
                format!("define ptr {name}(ptr %a)")
            }
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
                        let is_record = matches!(h.m.env.types.get(&Symbol::new(c)).map(|td| &td.body), Some(TypeBody::Record(_)));
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
        self.check_in(t, v, &[Part::Lit(ctx.to_string())], outer)
    }

    /// `check`, with a context built at run time (a list element's index).
    fn check_in(&mut self, t: &TypeExpr, v: &V, ctx: &[Part], outer: bool) -> Res<()> {
        if !has_value(&v.0) || !self.needs_check(t) {
            return Ok(());
        }
        let within = |ctx: &[Part], more: Vec<Part>| -> Vec<Part> { ctx.iter().cloned().chain(more).collect() };
        match t {
            TypeExpr::Refined(b, pred) => {
                self.check_in(b, v, ctx, false)?;
                let ok = self.with_it(v, outer, |c| c.expr(pred))?;
                let msg = format!("does not satisfy `{}`", crate::printer::ty(t));
                self.fail_unless_in(&ok, ctx.to_vec(), v, msg)?;
            }
            TypeExpr::Named(n, args) if n == "List" && args.len() == 1 => {
                // every element: `[2]: -1 does not satisfy …`
                let et = self.m.repr(&self.lower(&args[0])?)?;
                let et_written = args[0].clone();
                let ctx = ctx.to_vec();
                self.each(v, &et, |c, i, x| {
                    let shown = c.json(&(Ty::Int, i.to_string()))?;
                    let inner = within(&ctx, vec![Part::Lit(": [".into()), Part::Val(shown), Part::Lit("]".into())]);
                    c.check_in(&et_written, x, &inner, false)
                })?;
            }
            TypeExpr::Named(n, args) if (n == "Set" && args.len() == 1) || (n == "Dict" && args.len() == 2) => {
                // every element (`element 3: …`), every key and value (`key "a": …`, `["a"]: …`)
                let lowered: Vec<Ty> = args.iter().map(|a| self.lower(a)).collect::<Res<_>>()?;
                let list = self.as_list(&(Ty::Con(n.clone(), lowered.clone()), v.1.clone()))?;
                let et = self.elem_ty(&list.0)?;
                let (written, ctx) = (args.clone(), ctx.to_vec());
                self.each(&list, &et, |c, _, x| {
                    if written.len() == 1 {
                        let shown = c.json(x)?;
                        return c.check_in(&written[0], x, &within(&ctx, vec![Part::Lit(": element ".into()), Part::Val(shown)]), false);
                    }
                    let (k, val) = (c.field(x, 0, &lowered[0])?, c.field(x, 1, &lowered[1])?);
                    let shown = c.json(&k)?;
                    c.check_in(&written[0], &k, &within(&ctx, vec![Part::Lit(": key ".into()), Part::Val(shown.clone())]), false)?;
                    c.check_in(&written[1], &val, &within(&ctx, vec![Part::Lit(": [".into()), Part::Val(shown), Part::Lit("]".into())]), false)
                })?;
            }
            TypeExpr::Named(n, args) => {
                let n = match self.m.env.variant_parent(n.sym()) {
                    Some(p) => p,
                    None => n.clone(),
                };
                let Some(td) = self.m.env.types.get(&n).cloned() else { return Ok(()) };
                let sub: HashMap<Symbol, TypeExpr> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
                match &td.body {
                    TypeBody::Alias(a) => self.check_in(&crate::types::subst_texpr(a, &sub), v, ctx, false)?,
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
                                    c.check_in(&ft, &x, &within(ctx, vec![Part::Lit(format!(": {}.{fname}", crate::names::short(ctor)))]), false)?;
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
                    self.fail_unless_in(&ok, ctx.to_vec(), v, msg)?;
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
                if self.sub.contains_key(&n.sym()) {
                    return false;
                }
                if n == "List" || n == "Set" || n == "Dict" {
                    return args.iter().any(|x| self.needs_check(x));
                }
                let n = self.m.env.variant_parent(n.sym()).unwrap_or(n.clone());
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
        let tname = self.m.env.ctors[&Symbol::new(c)].clone();
        if let Some(r) = self.m.refinement(&tname) {
            let ok = self.with_it(v, false, |s| s.expr(r))?;
            self.fail_unless(&ok, &format!("constructing `{c}`"), v, format!("does not satisfy `{tname}` (where {})", crate::printer::expr(&r, 0)))?;
        }
        Ok(())
    }

    fn fail_unless(&mut self, ok: &V, ctx: &str, v: &V, msg: String) -> Res<()> {
        self.fail_unless_in(ok, vec![Part::Lit(ctx.to_string())], v, msg)
    }

    fn fail_unless_in(&mut self, ok: &V, mut ctx: Vec<Part>, v: &V, msg: String) -> Res<()> {
        let (pass, fail) = (self.fresh_label("check.ok"), self.fresh_label("check.fail"));
        self.cond_br(ok, &pass, &fail);
        self.block(&fail);
        let shown = self.json(v)?;
        ctx.extend([Part::Lit(": ".into()), Part::Val(shown), Part::Lit(format!(" {msg}"))]);
        self.fault(Fault::Refinement, ctx)?;
        self.block(&pass);
        Ok(())
    }

    /// Whether `v` is a value of the type named `name` (a type's name as a
    /// pattern, or `is`): its constructor, and every refinement.
    fn holds(&mut self, name: &str, v: &V) -> Res<V> {
        let mut conds = vec![];
        if self.m.nominal(&self.m.repr(&v.0)?) {
            let target = Ty::Con(Symbol::from(Symbol::from(name)), Module::args_of(&v.0));
            let cs = self.m.ctors_of(&target);
            conds.push(self.tag_in(v, &cs)?);
        }
        let mut t = TypeExpr::Named(Symbol::from(Symbol::from(name)), vec![]);
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
        let saved = if outer { None } else { Some(std::mem::replace(&mut self.scopes, vec![HashMap::default()])) };
        self.scopes.push(HashMap::default());
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
            let v = self.coerce(v.clone(), &t)?;
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
            Expr::Var(x) if self.lookup(x).is_none() => self.function_value(e, x)?,
            Expr::Var(x) => self.load(x)?,
            Expr::Hole(h) => {
                self.fault(Fault::Hole, vec![Part::Lit(format!("reached unfilled hole `?{h}`"))])?;
                never()
            }
            Expr::Call(f, args) => match &**f {
                Expr::Var(name) => self.call_named(e, name, args.iter().collect())?,
                other => {
                    let fv = self.expr(other)?;
                    let mut vs = vec![];
                    for a in args {
                        vs.push(self.expr(a)?);
                    }
                    self.call_closure(&fv, vs)?
                }
            },
            // method-call sugar: `e.f(args)` is `f(e, args)`
            Expr::Method(recv, m, args) => self.call_named(e, m, std::iter::once(&**recv).chain(args.iter()).collect())?,
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
                let tv = self.coerce(tv, &want)?;
                let from_t = self.cur.clone();
                self.br(&end);
                self.block(&lf);
                let fv = self.expr(f)?;
                let fv = self.coerce(fv, &want)?;
                let from_f = self.cur.clone();
                self.br(&end);
                self.block(&end);
                self.join(&want, vec![(tv, from_t), (fv, from_f)])?
            }
            Expr::Block(stmts, last) => {
                self.scopes.push(HashMap::default());
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
                        StrPart::Expr(x, Some(spec)) => {
                            let v = self.expr(x)?;
                            let s = self.format_spec(&v, spec)?;
                            ps.push(Part::Val(s));
                        }
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
                    self.tag_in(&v, &[name.to_string()])?
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
            // `x or { … }` / `x ?? v` on an Option, `x except { … }` / `x !! v` on a Result
            Expr::Except(..) | Expr::Or(..) => {
                let (x, name, fallback) = match e {
                    Expr::Except(x, name, fb) => (x, name, fb),
                    Expr::Or(x, fb) => (x, &None, fb),
                    _ => unreachable!(),
                };
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
                self.scopes.push(HashMap::default());
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
                let fv = self.coerce(fv, &want)?;
                self.scopes.pop();
                let from_f = self.cur.clone();
                self.br(&end);
                self.block(&end);
                self.join(&want, vec![(gv, from_ok), (fv, from_f)])?
            }
            Expr::List(xs) => {
                let t = self.ty_of(e)?;
                let mut vs = vec![];
                for x in xs {
                    vs.push(self.expr(x)?);
                }
                if vs.iter().any(|v| is_never(&v.0)) {
                    return Ok(never());
                }
                let et = self.elem_ty(&t)?;
                let vs: Vec<V> = vs.into_iter().map(|v| self.coerce(v, &et)).collect::<Res<_>>()?;
                let mut l = self.call(&t, "@strata_list_new", &[(Ty::Int, vs.len().to_string())])?;
                for v in &vs {
                    let s = self.to_slot(v)?;
                    l = self.call(&t, "@strata_list_push", &[l, (Ty::Int, s)])?;
                }
                l
            }
            Expr::Lambda(ps, body) => self.lambda(e, ps, body)?,
            // the mark of a `mut` argument or receiver: the call writes back to its place
            Expr::Mut(x) => self.expr(x)?,
            // evaluated at build time once deriving lands; until then, as written
            Expr::Comptime(x) => self.expr(x)?,
            Expr::Mock(_) => return self.unsupported("`mock`"),
            // rewritten into builtin calls when the program is resolved
            Expr::Range(..) | Expr::Index(..) | Expr::Comp(..) | Expr::Dict(..) | Expr::Set(..) => return self.unsupported("a range, slice, comprehension or collection literal not desugared"),
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
            (Add, Ty::Con(l, _)) if l == "List" => self.call(&a.0, "@strata_list_concat", &[a.clone(), b.clone()])?,
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
        if self.lookup(name).is_some() {
            // a local closure
            let f = self.load(name)?;
            let mut vs = vec![];
            for a in &args {
                vs.push(self.expr(a)?);
            }
            return self.call_closure(&f, vs);
        }
        // `Trait::m(type, ..)`: a trait method without `self`, for a type known here
        if let Some((tr, m)) = crate::types::split_trait_call(name) {
            let takes_self = self.m.trait_method(tr, m).map(|tm| crate::resolve::takes_self(&tm.decl)).unwrap_or(true);
            if !takes_self {
                let hint = self.ty_of(call).ok();
                let recv = match args.first() {
                    Some(Expr::Str(h)) => match (h.as_str(), &hint) {
                        ("Int", _) => Ty::Int,
                        ("Float", _) => Ty::Float,
                        ("Bool", _) => Ty::Bool,
                        ("String", _) => Ty::Str,
                        (_, Some(t @ Ty::Con(n, _))) if n == h => t.clone(),
                        _ => Ty::Con(Symbol::new(h), vec![]),
                    },
                    Some(Expr::Var(x)) if x.starts_with("type:") => self.sub.get(&Symbol::new(&x["type:".len()..])).cloned().ok_or(format!("the type `{x}` is not known here"))?,
                    _ => return Err(format!("`{name}` needs the type it is called on")),
                };
                let mut vs = vec![];
                for a in &args[1..] {
                    vs.push(self.expr(a)?);
                }
                return self.call_method(name, recv, vs, hint, &[]);
            }
        }
        if crate::stdlib::is_host(name) {
            let mut vs = vec![];
            for a in &args {
                vs.push(self.expr(a)?);
            }
            let hint = self.ty_of(call).ok();
            return self.host(name, vs, hint);
        }
        let mut vs = vec![];
        for a in &args {
            vs.push(self.expr(a)?);
        }
        if vs.iter().any(|v| is_never(&v.0)) {
            return Ok(never());
        }
        let hint = self.ty_of(call).ok();
        // where a `mut` parameter's changes go: the argument's place
        self.places = args.iter().map(|a| place_of(a)).collect();
        self.call_values(name, vs, hint)
    }

    /// The slot a `mut` parameter changes: the variable's own, or for a field
    /// (`mut o.status`) a slot of its own, written back after the call.
    fn mut_slot(&mut self, place: &Option<(String, Vec<String>)>, v: &V, param: &str) -> Res<String> {
        let Some((root, path)) = place else { return Err(format!("the argument for the `mut` parameter `{param}` must be a variable or its field")) };
        let l = self.lookup(root).ok_or(format!("`{root}` is not a local variable"))?;
        if path.is_empty() {
            return Ok(l.slot);
        }
        let tmp = self.fresh("mutarg");
        writeln!(self.allocas, "  {tmp} = alloca {}", self.m.ll(&v.0)?).unwrap();
        self.store(v, &tmp)?;
        Ok(tmp)
    }

    /// After a call: a field passed `mut` goes back into its variable; the
    /// variable's declared type holds (`assigning \`x\``).
    fn write_back(&mut self, place: &Option<(String, Vec<String>)>, slot: &str, t: &Ty, param: &TypeExpr) -> Res<()> {
        let Some((root, path)) = place else { return Ok(()) };
        let l = self.lookup(root).ok_or(format!("`{root}` is not a local variable"))?;
        let shown = std::iter::once(root.clone()).chain(path.iter().cloned()).collect::<Vec<_>>().join(".");
        if !path.is_empty() {
            let r = self.fresh("back");
            let ll = self.m.ll(t)?;
            self.emit(format!("{r} = load {ll}, ptr {slot}"));
            let cur = self.load(root)?;
            let new = self.set_path(&cur, path, (t.clone(), r), &shown)?;
            self.store(&(l.t.clone(), new.1), &l.slot)?;
        }
        // the parameter's type held on every assignment in the callee: when it
        // fits in the variable's (the checker proved it), so does the result
        if let Some(d) = &l.declared {
            if !(path.is_empty() && crate::types::refinement_within(self.m.env, param, d) == Some(true)) {
                let v = self.load(root)?;
                self.check(d, &v, &format!("assigning `{shown}`"), false)?;
            }
        }
        Ok(())
    }

    /// Arguments for a function's parameters: `mut` ones as the slot of
    /// their place, the others as values (of the parameter's type; a
    /// trait-typed one keeps its own). Then what to write back.
    fn bind_args(&mut self, params: &[Param], vs: Vec<V>, sub: &HashMap<Symbol, Ty>, places: &[Option<(String, Vec<String>)>]) -> Res<(Vec<V>, Vec<(usize, String, Ty, TypeExpr)>)> {
        let mut out = vec![];
        let mut backs = vec![];
        for (i, (p, v)) in params.iter().zip(vs).enumerate() {
            let t = if self.m.is_trait(&p.ty) { v.0.clone() } else { self.m.lower(&p.ty, sub)? };
            if p.mutable {
                let place = places.get(i).cloned().flatten();
                let slot = self.mut_slot(&place, &(t.clone(), v.1), &p.name)?;
                out.push((Ty::Str, slot.clone()));
                backs.push((i, slot, t, p.ty.clone()));
            } else if self.m.is_trait(&p.ty) {
                out.push((t, v.1));
            } else {
                out.push(self.coerce(v, &t)?);
            }
        }
        Ok((out, backs))
    }

    fn write_backs(&mut self, backs: Vec<(usize, String, Ty, TypeExpr)>, places: &[Option<(String, Vec<String>)>]) -> Res<()> {
        for (i, slot, t, param) in backs {
            let place = places.get(i).cloned().flatten();
            self.write_back(&place, &slot, &t, &param)?;
        }
        Ok(())
    }

    /// Call the function or built-in `name` on values; `hint` is the call's
    /// type, when known (a type argument only the result mentions).
    fn call_values(&mut self, name: &str, vs: Vec<V>, hint: Option<Ty>) -> Res<V> {
        let places = std::mem::take(&mut self.places);
        if self.m.env.fns.contains_key(&Symbol::new(name)) {
            return self.call_fn_at(name, vs, hint, &places);
        }
        if self.m.env.is_method(Symbol::new(name)) {
            if let Some(recv) = vs.first().map(|v| v.0.clone()) {
                if self.is_dyn(&recv) || Module::head(&recv).is_some_and(|h| self.m.resolve_method(name, &h).is_some() || self.m.ctors_of(&recv).iter().any(|c| self.m.resolve_method(name, c).is_some())) {
                    return self.call_method(name, recv, vs, hint, &places);
                }
            }
        }
        if let Some(v) = self.string_builtin(name, &vs)? {
            return Ok(v);
        }
        if let Some(v) = self.collection_builtin(name, &vs, &hint)? {
            return Ok(v);
        }
        // the list built-ins written in Strata (llvm/prelude.strata)
        if let Some(Ty::Con(l, et)) = vs.first().map(|v| self.m.repr(&v.0)).transpose()? {
            if l == "List" {
                let own = match (name, et[0].clone()) {
                    ("sum", Ty::Float) => Some("sum_float".to_string()),
                    ("sum", _) => Some("sum_int".to_string()),
                    ("sort", a) => {
                        let less = self.less(&a)?;
                        return self.call_fn(&prelude("sort_with"), vec![vs[0].clone(), less], hint);
                    }
                    ("sort_by", _) => {
                        let Ty::Fn(_, b) = self.m.repr(&vs[1].0)? else { return Err("`sort_by` needs a function".into()) };
                        let less = self.less(&b)?;
                        return self.call_fn(&prelude("sort_by_with"), vec![vs[0].clone(), vs[1].clone(), less], hint);
                    }
                    (n, _) if self.m.env.fns.contains_key(&Symbol::new(&(prelude(n)))) => Some(n.to_string()),
                    _ => None,
                };
                if let Some(own) = own {
                    return self.call_fn(&prelude(&own), vs, hint);
                }
            }
        }
        self.builtin(name, vs)
    }

    /// Call trait method `name` on a value of type `recv` (`vs` includes it,
    /// unless the method has no `self`). The implementation is chosen here,
    /// from the type; a variant with an implementation of its own is chosen
    /// by its tag.
    fn call_method(&mut self, name: &str, recv: Ty, vs: Vec<V>, hint: Option<Ty>, places: &[Option<(String, Vec<String>)>]) -> Res<V> {
        if vs.iter().any(|v| is_never(&v.0)) {
            return Ok(never());
        }
        let head = Module::head(&recv).ok_or(format!("`{name}` on a `{}`", key(&recv)))?;
        if self.m.traits.contains_key(&Symbol::new(&head)) {
            return self.dyn_call(name, recv, vs, places);
        }
        let args = Module::args_of(&recv);
        let own: Vec<String> = self.m.ctors_of(&recv).into_iter().filter(|c| *c != head && self.m.resolve_method(name, c).is_some_and(|(tr, _)| self.m.impl_of(&tr, c).is_some())).collect();
        if own.is_empty() {
            return self.method_target(name, recv, head, vs, hint, places);
        }
        // by constructor: its own implementation, or the type's
        let ret = hint.clone().ok_or(format!("the result type of `{name}`"))?;
        let slot = self.fresh("mres");
        if has_value(&ret) {
            writeln!(self.allocas, "  {slot} = alloca {}", self.m.ll(&ret)?).unwrap();
        }
        let end = self.fresh_label("dispatched");
        let v0 = vs[0].clone();
        self.on_ctor(&v0, &end, |c, ctor| {
            let (r, h) = if own.iter().any(|o| o == ctor) { (Ty::Con(Symbol::new(ctor), args.clone()), ctor.to_string()) } else { (recv.clone(), head.clone()) };
            let out = c.method_target(name, r, h, vs.clone(), hint.clone(), places)?;
            if has_value(&ret) && !is_never(&out.0) {
                let ll = c.m.ll(&ret)?;
                c.emit(format!("store {ll} {}, ptr {slot}", out.1));
            }
            c.br(&end);
            Ok(())
        })?;
        self.block(&end);
        if !has_value(&ret) {
            return Ok((ret, String::new()));
        }
        let r = self.fresh("mr");
        let ll = self.m.ll(&ret)?;
        self.emit(format!("{r} = load {ll}, ptr {slot}"));
        Ok((ret, r))
    }

    /// Call the implementation of `name` for type `head` (on a `recv`).
    fn method_target(&mut self, name: &str, recv: Ty, head: String, vs: Vec<V>, hint: Option<Ty>, places: &[Option<(String, Vec<String>)>]) -> Res<V> {
        let (tr, m) = self.m.resolve_method(name, &head).ok_or(format!("`{head}` has no method `{name}`"))?;
        let tm = self.m.trait_method(&tr, &m).ok_or(format!("trait `{tr}` has no method `{m}`"))?;
        let imp = self.m.impl_of(&tr, &head).ok_or(format!("`{head}` does not implement `{tr}`"))?;
        let own = imp.methods.iter().find(|x| x.name == m).unwrap_or(&tm.decl);
        // the method's own type parameters, from the arguments and the result
        let mut targs = HashMap::default();
        for (p, v) in own.params.iter().zip(&vs) {
            self.m.bind_targs(&p.ty, &v.0, &own.type_params, &mut targs);
        }
        if let Some(rt) = &hint {
            self.m.bind_targs(&own.ret, rt, &own.type_params, &mut targs);
        }
        let targs: Vec<Ty> = own.type_params.iter().map(|p| targs.get(p).cloned().unwrap_or(Ty::Unit)).collect();
        let mi = MethodInst { tr, method: m, recv, head, targs };
        let sub = self.m.method_sub(&mi)?.0;
        let symbol = self.m.want_method(mi);
        let own_params = own.params.clone();
        let ret = self.m.lower(&own.ret, &sub)?;
        let (args, backs) = self.bind_args(&own_params, vs, &sub, places)?;
        let r = self.call(&ret, &symbol, &args)?;
        self.write_backs(backs, places)?;
        Ok(r)
    }

    /// Call a function of the program (one instance of it, if generic).
    fn call_fn(&mut self, name: &str, vs: Vec<V>, hint: Option<Ty>) -> Res<V> {
        self.call_fn_at(name, vs, hint, &[])
    }

    /// `call_fn`, with the places of the arguments (for `mut` parameters).
    fn call_fn_at(&mut self, name: &str, vs: Vec<V>, hint: Option<Ty>, places: &[Option<(String, Vec<String>)>]) -> Res<V> {
        let f = &self.m.env.fns[&Symbol::new(name)];
        if f.params.len() != vs.len() {
            return Err(format!("`{name}` takes {} arguments", f.params.len()));
        }
        // the type arguments: the declared types against the actual ones
        let mut targs = HashMap::default();
        for (p, v) in f.params.iter().zip(&vs) {
            self.m.bind_targs(&p.ty, &v.0, &f.type_params, &mut targs);
        }
        if let Some(rt) = hint {
            self.m.bind_targs(&f.ret, &rt, &f.type_params, &mut targs);
        }
        let targs: Vec<Ty> = f.type_params.iter().map(|p| targs.get(p).cloned().unwrap_or(Ty::Unit)).collect();
        let sub: HashMap<Symbol, Ty> = f.type_params.iter().cloned().zip(targs.iter().cloned()).collect();
        let ret = self.m.lower(&f.ret, &sub)?;
        // a trait-typed parameter takes the implementation passed: an instance per implementation
        let impls: Vec<Option<Ty>> = f.params.iter().zip(&vs).map(|(p, v)| self.m.is_trait(&p.ty).then(|| v.0.clone())).collect();
        let params = f.params.clone();
        // an argument's own type may differ from the parameter's (a variant for its sum): same representation
        let (args, backs) = self.bind_args(&params, vs, &sub, places)?;
        let symbol = self.m.want_impls(name, targs.clone(), impls);
        let r = self.call(&ret, &symbol, &args)?;
        self.write_backs(backs, places)?;
        Ok(r)
    }

    /// A native operation of std (`host_*`): the runtime's, its result made
    /// an `Option` or a `Result` here (only the generated code knows their tags).
    fn host(&mut self, name: &str, vs: Vec<V>, hint: Option<Ty>) -> Res<V> {
        let t = hint.clone().unwrap_or(Ty::Unit);
        let s_ty = Ty::Str;
        let lst = Ty::Con(Symbol::new("List"), vec![Ty::Str]);
        Ok(match name {
            "host_write" => self.call(&Ty::Unit, "@strata_host_write", &vs)?,
            "host_exists" => self.call(&Ty::Bool, "@strata_host_exists", &vs)?,
            "host_args" => self.call(&lst, "@strata_host_args", &[])?,
            "host_read_line" | "host_env_var" => {
                let r = self.call(&s_ty, &format!("@strata_{name}"), &vs)?;
                self.optional(&r, &t)?
            }
            "host_read_file" | "host_list_dir" => {
                // a value, or NULL and the failure
                let (k, m) = (self.fresh("kind"), self.fresh("msg"));
                writeln!(self.allocas, "  {k} = alloca ptr\n  {m} = alloca ptr").unwrap();
                let mut args = vs.clone();
                args.extend([(Ty::Str, k.clone()), (Ty::Str, m.clone())]);
                let r = self.call(&s_ty, &format!("@strata_{name}"), &args)?;
                let ok = self.fresh("ok");
                self.emit(format!("{ok} = icmp ne ptr {}, null", r.1));
                let okv = (if name == "host_list_dir" { lst } else { Ty::Str }, r.1);
                self.outcome(&ok, Some(okv), &k, &m, &t)?
            }
            "host_write_file" | "host_make_dir" | "host_remove" => {
                let (k, m) = (self.fresh("kind"), self.fresh("msg"));
                writeln!(self.allocas, "  {k} = alloca ptr\n  {m} = alloca ptr").unwrap();
                let mut args = vs.clone();
                args.extend([(Ty::Str, k.clone()), (Ty::Str, m.clone())]);
                let r = self.call(&Ty::Bool, &format!("@strata_{name}"), &args)?;
                self.outcome(&r.1, None, &k, &m, &t)?
            }
            _ => return self.unsupported(&format!("`{name}`")),
        })
    }

    /// `Some(v)` when the pointer `v` is not NULL, else `None`.
    fn optional(&mut self, v: &V, t: &Ty) -> Res<V> {
        let (some, none, end) = (self.fresh_label("some"), self.fresh_label("none"), self.fresh_label("opt"));
        let isnull = self.fresh("null");
        self.emit(format!("{isnull} = icmp eq ptr {}, null", v.1));
        self.cond_br(&bool_v(&isnull), &none, &some);
        self.block(&some);
        let b = self.block_of(self.m.tags["Some"], &[v.clone()])?;
        let from = self.cur.clone();
        self.br(&end);
        self.block(&none);
        let n = self.m.nullary("None");
        self.br(&end);
        self.block(&end);
        let r = self.fresh("o");
        self.emit(format!("{r} = phi ptr [ {b}, %{from} ], [ {n}, %{none} ]"));
        Ok((t.clone(), r))
    }

    /// `Ok(v)` (or `Ok(())`) when `ok`, else `Err((kind, message))` from the slots.
    fn outcome(&mut self, ok: &str, v: Option<V>, kslot: &str, mslot: &str, t: &Ty) -> Res<V> {
        let (good, bad, end) = (self.fresh_label("ok"), self.fresh_label("err"), self.fresh_label("res"));
        self.cond_br(&bool_v(ok), &good, &bad);
        self.block(&good);
        let okb = self.block_of(self.m.tags["Ok"], &[v.unwrap_or((Ty::Unit, String::new()))])?;
        let from_ok = self.cur.clone();
        self.br(&end);
        self.block(&bad);
        let (k, m) = (self.fresh("k"), self.fresh("m"));
        self.emit(format!("{k} = load ptr, ptr {kslot}"));
        self.emit(format!("{m} = load ptr, ptr {mslot}"));
        let pair = self.block_of(0, &[(Ty::Str, k), (Ty::Str, m)])?;
        let errb = self.block_of(self.m.tags["Err"], &[(Ty::Str, pair)])?;
        let from_err = self.cur.clone();
        self.br(&end);
        self.block(&end);
        let r = self.fresh("r");
        self.emit(format!("{r} = phi ptr [ {okb}, %{from_ok} ], [ {errb}, %{from_err} ]"));
        Ok((t.clone(), r))
    }

    /// A native built-in.
    fn builtin(&mut self, name: &str, vs: Vec<V>) -> Res<V> {
        let types: Vec<Ty> = vs.iter().map(|v| self.m.repr(&v.0)).collect::<Res<_>>()?;
        let list_elem = |t: &Ty| match t {
            Ty::Con(l, a) if l == "List" => Some(a[0].clone()),
            _ => None,
        };
        Ok(match (name, types.as_slice()) {
            ("len", [t]) if list_elem(t).is_some() => self.list_len(&vs[0]),
            ("push", [t, _]) if list_elem(t).is_some() => {
                let x = self.coerce(vs[1].clone(), &list_elem(t).unwrap())?;
                let s = self.to_slot(&x)?;
                let r = self.call(&types[0], "@strata_list_push", &[vs[0].clone(), (Ty::Int, s)])?;
                (vs[0].0.clone(), r.1)
            }
            ("get", [t, Ty::Int]) if list_elem(t).is_some() => {
                let et = list_elem(t).unwrap();
                let opt = Ty::Con("Option".into(), vec![et.clone()]);
                let n = self.list_len(&vs[0]);
                let (lo, hi) = (self.fresh("lo"), self.fresh("hi"));
                self.emit(format!("{lo} = icmp sge i64 {}, 0", vs[1].1));
                self.emit(format!("{hi} = icmp slt i64 {}, {}", vs[1].1, n.1));
                let inside = self.fresh("in");
                self.emit(format!("{inside} = and i1 {lo}, {hi}"));
                let (some, none, end) = (self.fresh_label("get.some"), self.fresh_label("get.none"), self.fresh_label("get.end"));
                self.cond_br(&bool_v(&inside), &some, &none);
                self.block(&some);
                let x = self.list_at(&vs[0], &vs[1].1, &et)?;
                let b = self.block_of(self.m.tags["Some"], &[x])?;
                let from_some = self.cur.clone();
                self.br(&end);
                self.block(&none);
                let nothing = self.m.nullary("None");
                self.br(&end);
                self.block(&end);
                let r = self.fresh("opt");
                self.emit(format!("{r} = phi ptr [ {b}, %{from_some} ], [ {nothing}, %{none} ]"));
                (opt, r)
            }
            ("take" | "drop", [t, Ty::Int]) if list_elem(t).is_some() => {
                // the count, within 0..len
                let n = self.list_len(&vs[0]);
                let (k0, k) = (self.fresh("k"), self.fresh("k"));
                self.emit(format!("{k0} = call i64 @llvm.smax.i64(i64 {}, i64 0)", vs[1].1));
                self.emit(format!("{k} = call i64 @llvm.smin.i64(i64 {k0}, i64 {})", n.1));
                let (from, to) = if name == "take" { ("0".to_string(), k) } else { (k, n.1.clone()) };
                let r = self.call(&types[0], "@strata_list_slice", &[vs[0].clone(), (Ty::Int, from), (Ty::Int, to)])?;
                (vs[0].0.clone(), r.1)
            }
            ("at", [t, Ty::Int]) if list_elem(t).is_some() => {
                let et = list_elem(t).unwrap();
                let n = self.list_len(&vs[0]);
                let (lo, hi, inside) = (self.fresh("lo"), self.fresh("hi"), self.fresh("in"));
                self.emit(format!("{lo} = icmp sge i64 {}, 0", vs[1].1));
                self.emit(format!("{hi} = icmp slt i64 {}, {}", vs[1].1, n.1));
                self.emit(format!("{inside} = and i1 {lo}, {hi}"));
                let (ok, bad) = (self.fresh_label("at.ok"), self.fresh_label("at.out"));
                self.cond_br(&bool_v(&inside), &ok, &bad);
                self.block(&bad);
                let (si, sn) = (self.json(&vs[1])?, self.json(&n)?);
                self.fault(Fault::Runtime, vec![Part::Lit("index ".into()), Part::Val(si), Part::Lit(" out of range (length ".into()), Part::Val(sn), Part::Lit(")".into())])?;
                self.block(&ok);
                self.list_at(&vs[0], &vs[1].1, &et)?
            }
            ("slice", [t, Ty::Int, Ty::Int]) if list_elem(t).is_some() => {
                // clamped to 0..len, empty when the end is before the start (as in Python)
                let n = self.list_len(&vs[0]);
                let mut ends = vec![];
                for x in [&vs[1], &vs[2]] {
                    let (k0, k) = (self.fresh("k"), self.fresh("k"));
                    self.emit(format!("{k0} = call i64 @llvm.smax.i64(i64 {}, i64 0)", x.1));
                    self.emit(format!("{k} = call i64 @llvm.smin.i64(i64 {k0}, i64 {})", n.1));
                    ends.push(k);
                }
                let to = self.fresh("to");
                self.emit(format!("{to} = call i64 @llvm.smax.i64(i64 {}, i64 {})", ends[0], ends[1]));
                let r = self.call(&types[0], "@strata_list_slice", &[vs[0].clone(), (Ty::Int, ends[0].clone()), (Ty::Int, to)])?;
                (vs[0].0.clone(), r.1)
            }
            ("to_json", [_]) => {
                let s = self.json(&vs[0])?;
                (Ty::Str, s)
            }
            ("range", [Ty::Int, Ty::Int]) => self.call(&Ty::Con("List".into(), vec![Ty::Int]), "@strata_range", &vs)?,
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
                self.call(&Ty::Float, "@strata_sqrt", &vs)?
            }
            ("floor" | "ceil" | "round", [Ty::Float]) => {
                let r = self.call(&Ty::Float, &format!("@llvm.{name}.f64"), &vs)?;
                self.call(&Ty::Int, "@strata_float_to_int", &[r, vs[0].clone()])?
            }
            _ => {
                let ts: Vec<String> = types.iter().map(key).collect();
                return self.unsupported(&format!("the built-in `{name}` on ({})", ts.join(", ")));
            }
        })
    }

    // ---- lists ----
    //
    // A list points to a header { i64 len, ptr buf, i64 off }; the buffer is
    // { i64 cap, i64 used, i64 slots[] } (runtime.c). Element i is
    // slots[off + i].

    fn list_len(&mut self, v: &V) -> V {
        let r = self.fresh("len");
        self.emit(format!("{r} = load i64, ptr {}", v.1));
        (Ty::Int, r)
    }

    /// Element `i` (known to be within the list).
    fn list_at(&mut self, v: &V, i: &str, et: &Ty) -> Res<V> {
        let (bp, buf, op, off, k, at, s) = (self.fresh("bufp"), self.fresh("buf"), self.fresh("offp"), self.fresh("off"), self.fresh("k"), self.fresh("at"), self.fresh("el"));
        self.emit(format!("{bp} = getelementptr i8, ptr {}, i64 8", v.1));
        self.emit(format!("{buf} = load ptr, ptr {bp}"));
        self.emit(format!("{op} = getelementptr i8, ptr {}, i64 16", v.1));
        self.emit(format!("{off} = load i64, ptr {op}"));
        self.emit(format!("{k} = add i64 {off}, {i}"));
        self.emit(format!("{at} = getelementptr i64, ptr {buf}, i64 2"));
        let el = self.fresh("elp");
        self.emit(format!("{el} = getelementptr i64, ptr {at}, i64 {k}"));
        self.emit(format!("{s} = load i64, ptr {el}"));
        self.from_slot(&s, et)
    }

    fn elem_ty(&self, t: &Ty) -> Res<Ty> {
        match self.m.repr(t)? {
            Ty::Con(l, a) if l == "List" => Ok(a[0].clone()),
            t if collection(&t).is_some() => self.elem_ty(&collection(&t).unwrap().1),
            other => Err(format!("`{}` is not a list", key(&other))),
        }
    }

    /// Run `f` on each element of a list (with its index), in a loop.
    fn each(&mut self, v: &V, et: &Ty, mut f: impl FnMut(&mut Self, &str, &V) -> Res<()>) -> Res<()> {
        let counter = self.fresh("idx");
        writeln!(self.allocas, "  {counter} = alloca i64").unwrap();
        self.emit(format!("store i64 0, ptr {counter}"));
        let (head, body, end) = (self.fresh_label("each"), self.fresh_label("each.body"), self.fresh_label("each.end"));
        self.br(&head);
        self.block(&head);
        let i = self.fresh("i");
        self.emit(format!("{i} = load i64, ptr {counter}"));
        let n = self.list_len(v);
        let more = self.fresh("more");
        self.emit(format!("{more} = icmp slt i64 {i}, {}", n.1));
        self.cond_br(&bool_v(&more), &body, &end);
        self.block(&body);
        let x = self.list_at(v, &i, et)?;
        f(self, &i, &x)?;
        let next = self.fresh("i");
        self.emit(format!("{next} = add i64 {i}, 1"));
        self.emit(format!("store i64 {next}, ptr {counter}"));
        self.br(&head);
        self.block(&end);
        Ok(())
    }

    // ---- values of a trait type ----
    //
    // A `Console` that could be any implementation (a field `console:
    // Console`, an element of `List[Shape]`) points to two slots: its
    // implementation's table, then the value. The table is [show as JSON,
    // show, then each method of the trait and its supertraits]; each entry
    // takes the value's slot (a `mut self` method: a pointer to it). A
    // parameter of a trait type doesn't need this: it is compiled per
    // implementation.

    fn is_dyn(&self, t: &Ty) -> bool {
        matches!(t, Ty::Con(n, _) if self.m.traits.contains_key(n))
    }

    /// `v` where a value of type `want` is expected: a concrete value going
    /// where a trait is wanted gets its table (in a list, each element).
    fn coerce(&mut self, v: V, want: &Ty) -> Res<V> {
        if is_never(&v.0) || is_never(want) {
            return Ok(v);
        }
        let want_r = self.m.repr(want)?;
        let have = self.m.repr(&v.0)?;
        if self.is_dyn(&want_r) && !self.is_dyn(&have) {
            let vt = self.vtable(&want_r, &v.0)?;
            let s = self.to_slot(&v)?;
            let (p, a) = (self.fresh("dyn"), self.fresh("vt"));
            self.emit(format!("{p} = call ptr @strata_alloc(i64 16)"));
            self.emit(format!("{a} = ptrtoint ptr {vt} to i64"));
            self.emit(format!("store i64 {a}, ptr {p}"));
            self.set_field(&p, 0, &(Ty::Int, s))?;
            return Ok((want.clone(), p));
        }
        if let (Ty::Con(l, a), Ty::Con(l2, b)) = (&want_r, &have) {
            if l == "List" && l2 == "List" && self.has_dyn(&a[0]) && a[0] != b[0] {
                // each element, into a new list
                let (et, from) = (a[0].clone(), b[0].clone());
                let acc = self.fresh("conv");
                writeln!(self.allocas, "  {acc} = alloca ptr").unwrap();
                let n = self.list_len(&v);
                let l0 = self.call(&want_r, "@strata_list_new", &[n])?;
                self.emit(format!("store ptr {}, ptr {acc}", l0.1));
                self.each(&v, &from, |c, _, x| {
                    let y = c.coerce(x.clone(), &et)?;
                    let s = c.to_slot(&y)?;
                    let cur = c.fresh("cur");
                    c.emit(format!("{cur} = load ptr, ptr {acc}"));
                    let next = c.call(&Ty::Str, "@strata_list_push", &[(Ty::Str, cur), (Ty::Int, s)])?;
                    c.emit(format!("store ptr {}, ptr {acc}", next.1));
                    Ok(())
                })?;
                let r = self.fresh("conv");
                self.emit(format!("{r} = load ptr, ptr {acc}"));
                return Ok((want.clone(), r));
            }
        }
        Ok((want.clone(), v.1))
    }

    fn has_dyn(&self, t: &Ty) -> bool {
        self.is_dyn(t) || matches!(t, Ty::Con(_, a) if a.iter().any(|x| self.has_dyn(x)))
    }

    /// The methods a value of trait `tr` can be called with: its own, then
    /// its supertraits', each once (as (trait, method)).
    fn dyn_methods(&self, tr: &str) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = vec![];
        let mut todo = vec![tr.to_string()];
        while let Some(t) = todo.pop() {
            let Some(td) = self.m.traits.get(&Symbol::new(&t)) else { continue };
            for m in &td.methods {
                if !out.iter().any(|(_, x)| *x == m.decl.name) {
                    out.push((t.clone(), m.decl.name.to_string()));
                }
            }
            for sup in td.supers.iter().rev() {
                todo.push(crate::types::texpr_head(sup).to_string());
            }
        }
        out
    }

    /// Call entry `i` of a trait value's table that shows it (0: JSON, 1: as `to_string`).
    fn dyn_entry_call(&mut self, v: &V, i: usize) -> Res<String> {
        let fp = self.dyn_entry(v, i);
        let s = self.field(v, 0, &Ty::Int)?;
        let r = self.fresh("shown");
        self.emit(format!("{r} = call ptr {fp}(i64 {})", s.1));
        Ok(r)
    }

    /// Entry `i` of a trait value's table.
    fn dyn_entry(&mut self, v: &V, i: usize) -> String {
        let (raw, vt, at, fp) = (self.fresh("vtr"), self.fresh("vt"), self.fresh("ent"), self.fresh("fn"));
        self.emit(format!("{raw} = load i64, ptr {}", v.1));
        self.emit(format!("{vt} = inttoptr i64 {raw} to ptr"));
        self.emit(format!("{at} = getelementptr ptr, ptr {vt}, i64 {i}"));
        self.emit(format!("{fp} = load ptr, ptr {at}"));
        fp
    }

    /// A method's signature for a call through a trait value: (method's
    /// trait, declaration, its types with the trait's parameters filled in).
    fn dyn_signature(&self, recv: &Ty, trm: &str, m: &str) -> Res<(&'a TraitMethod, HashMap<Symbol, Ty>)> {
        let tm = self.m.trait_method(trm, m).ok_or(format!("trait `{trm}` has no method `{m}`"))?;
        let Ty::Con(tr, args) = recv else { unreachable!() };
        if tr.as_str() != trm && !self.m.traits[&Symbol::new(trm)].params.is_empty() {
            return self.unsupported(&format!("calling `{m}` of the generic supertrait `{trm}` through a `{tr}`"));
        }
        let mentions = |t: &TypeExpr| crate::types::mentions_self(&FnDecl { params: vec![], ret: t.clone(), ..tm.decl.clone() });
        if mentions(&tm.decl.ret) || tm.decl.params.iter().skip(1).any(|p| mentions(&p.ty) || p.mutable) || !tm.decl.type_params.is_empty() {
            return self.unsupported(&format!("calling `{m}` (it mentions `Self`, has a `mut` parameter or type parameters of its own) on a value of the trait type `{tr}`"));
        }
        let td = self.m.traits[tr];
        let sub: HashMap<Symbol, Ty> = if tr.as_str() == trm { td.params.iter().cloned().zip(args.iter().cloned()).collect() } else { HashMap::default() };
        Ok((tm, sub))
    }

    /// Call `name` on a value of a trait type, through its table.
    fn dyn_call(&mut self, name: &str, recv: Ty, vs: Vec<V>, places: &[Option<(String, Vec<String>)>]) -> Res<V> {
        let Ty::Con(tr, _) = &recv else { unreachable!() };
        let m = crate::types::split_trait_call(name).map_or(name, |(_, m)| m);
        let methods = self.dyn_methods(tr);
        let i = methods.iter().position(|(_, x)| x == m).ok_or(format!("`{tr}` has no method `{m}`"))?;
        let trm = methods[i].0.clone();
        let (tm, sub) = self.dyn_signature(&recv, &trm, m)?;
        let ret = self.m.lower(&tm.decl.ret, &sub)?;
        let fp = self.dyn_entry(&vs[0], i + 2);
        let mut args = vec![];
        let mut back = None;
        if tm.decl.params[0].mutable {
            // a copy of the pair, changed by the method, then stored in the place
            let place = places.first().cloned().flatten();
            let slot = self.mut_slot(&place, &vs[0], "self")?;
            let cur = self.fresh("cur");
            self.emit(format!("{cur} = load ptr, ptr {slot}"));
            let copy = self.copy_block(&(recv.clone(), cur), 1);
            self.emit(format!("store ptr {copy}, ptr {slot}"));
            let at = self.fresh("selfp");
            self.emit(format!("{at} = getelementptr i64, ptr {copy}, i64 1"));
            args.push((Ty::Str, at));
            back = Some((place, slot));
        } else {
            let s = self.field(&vs[0], 0, &Ty::Int)?;
            args.push(s);
        }
        for (p, v) in tm.decl.params.iter().skip(1).zip(vs.into_iter().skip(1)) {
            let t = self.m.lower(&p.ty, &sub)?;
            args.push(self.coerce(v, &t)?);
        }
        let r = self.call(&ret, &fp, &args)?;
        if let Some((place, slot)) = back {
            self.write_back(&place, &slot, &recv, &tm.decl.params[0].ty)?;
        }
        Ok(r)
    }

    /// The table of `imp`'s implementation of trait type `tr_ty` (made once).
    fn vtable(&mut self, tr_ty: &Ty, imp: &Ty) -> Res<String> {
        let Ty::Con(tr, _) = tr_ty else { unreachable!() };
        let name = format!("@\"vt:{}@{}\"", key(tr_ty), key(imp));
        if !self.m.helpers.insert(name.clone()) {
            return Ok(name);
        }
        let head = Module::head(imp).ok_or(format!("a `{}` can't be a `{tr}`", key(imp)))?;
        let ll = self.m.ll(imp)?;
        let mut entries = vec![];
        // showing it: as JSON, and as `to_string`
        for (i, what) in ["json", "show"].iter().enumerate() {
            let f = format!("@\"dyn{what}:{}\"", key(imp));
            entries.push(f.clone());
            if !self.m.helpers.insert(f.clone()) {
                continue;
            }
            let mut h = Func::new(self.m, HashMap::default());
            h.block("start");
            let v = h.from_slot("%v", imp)?;
            let s = if i == 0 { h.json(&v)? } else { h.show(&v)? };
            h.emit(format!("ret ptr {s}"));
            let text = h.finish();
            writeln!(self.m.body, "define ptr {f}(i64 %v) {{\n{text}}}\n").unwrap();
        }
        for (trm, m) in self.dyn_methods(tr) {
            let f = format!("@\"dyn:{trm}::{m}@{}\"", key(imp));
            entries.push(f.clone());
            let Ok((tm, sub)) = self.dyn_signature(tr_ty, &trm, &m) else {
                // not callable through the table: an entry that is never called
                writeln!(self.m.body, "define void {f}() {{\nentry:\n  unreachable\n}}\n").unwrap();
                continue;
            };
            let (_, imp_m) = self.m.resolve_method(&crate::types::trait_call(&trm, &m), &head).ok_or(format!("`{head}` has no method `{m}`"))?;
            let mi = MethodInst { tr: trm.clone(), method: imp_m, recv: imp.clone(), head: head.clone(), targs: vec![] };
            let symbol = self.m.want_method(mi);
            let ret = self.m.lower(&tm.decl.ret, &sub)?;
            let mut h = Func::new(self.m, HashMap::default());
            h.block("start");
            let mutable = tm.decl.params[0].mutable;
            let mut sig = vec![if mutable { "ptr %self".to_string() } else { "i64 %self".to_string() }];
            let mut args = vec![];
            let tmp = h.fresh("self");
            if mutable {
                writeln!(h.allocas, "  {tmp} = alloca {ll}").unwrap();
                let s = h.fresh("s");
                h.emit(format!("{s} = load i64, ptr %self"));
                let v = h.from_slot(&s, imp)?;
                h.store(&v, &tmp)?;
                args.push((Ty::Str, tmp.clone()));
            } else {
                args.push(h.from_slot("%self", imp)?);
            }
            for (k, p) in tm.decl.params.iter().skip(1).enumerate() {
                let t = h.m.lower(&p.ty, &sub)?;
                if has_value(&t) {
                    sig.push(format!("{} %a{k}", h.m.ll(&t)?));
                }
                args.push((t, format!("%a{k}")));
            }
            let r = h.call(&ret, &symbol, &args)?;
            if mutable {
                let v = h.fresh("v");
                h.emit(format!("{v} = load {ll}, ptr {tmp}"));
                let s = h.to_slot(&(imp.clone(), v))?;
                h.emit(format!("store i64 {s}, ptr %self"));
            }
            if has_value(&ret) {
                let rl = h.m.ll(&ret)?;
                h.emit(format!("ret {rl} {}", r.1));
            } else {
                h.emit("ret void".into());
            }
            let text = h.finish();
            let rl = self.m.ll(&ret)?;
            writeln!(self.m.body, "define {rl} {f}({}) {{\n{text}}}\n", sig.join(", ")).unwrap();
        }
        let list: Vec<String> = entries.iter().map(|e| format!("ptr {e}")).collect();
        writeln!(self.m.globals, "{name} = private constant [{} x ptr] [{}], align 8", entries.len(), list.join(", ")).unwrap();
        Ok(name)
    }

    // ---- closures ----
    //
    // A function value points to a block: the function's address, then the
    // values it captured. The function takes that block first, then its
    // parameters.

    fn call_closure(&mut self, f: &V, args: Vec<V>) -> Res<V> {
        if is_never(&f.0) || args.iter().any(|a| is_never(&a.0)) {
            return Ok(never());
        }
        let Ty::Fn(pts, rt) = self.m.repr(&f.0)? else { return Err(format!("`{}` is not a function", key(&f.0))) };
        let (raw, fp) = (self.fresh("fnp"), self.fresh("fn"));
        self.emit(format!("{raw} = load i64, ptr {}", f.1));
        self.emit(format!("{fp} = inttoptr i64 {raw} to ptr"));
        let mut all = vec![(Ty::Str, f.1.clone())];
        for (t, a) in pts.iter().zip(args) {
            all.push((t.clone(), a.1));
        }
        self.call(&rt, &fp, &all)
    }

    /// A closure block: the function, then the captured values.
    fn closure(&mut self, fsym: &str, captured: &[V]) -> Res<String> {
        let p = self.fresh("clo");
        self.emit(format!("{p} = call ptr @strata_alloc(i64 {})", 8 * (captured.len() + 1)));
        let a = self.fresh("fa");
        self.emit(format!("{a} = ptrtoint ptr {fsym} to i64"));
        self.emit(format!("store i64 {a}, ptr {p}"));
        for (i, c) in captured.iter().enumerate() {
            self.set_field(&p, i, c)?;
        }
        Ok(p)
    }

    /// `x -> body`: a function of its own, closing over the variables it uses.
    fn lambda(&mut self, e: &Expr, ps: &[LamParam], body: &Expr) -> Res<V> {
        let t = self.ty_of(e)?;
        let Ty::Fn(pts, rt) = t.clone() else { return Err("a lambda without a function type".into()) };
        let names: Vec<Symbol> = ps.iter().map(|p| p.name.clone()).collect();
        let mut free: Vec<String> = vars_in(body).into_iter().filter(|x| !names.iter().any(|n| n == x.as_str()) && self.lookup(x).is_some()).collect();
        free.sort();
        free.dedup();
        let mut captured = vec![];
        for x in &free {
            captured.push(self.load(x)?);
        }
        self.m.lambdas += 1;
        let fsym = format!("@\"lambda.{}\"", self.m.lambdas);
        let mut l = Func::new(self.m, self.sub.clone());
        l.block("entry.body");
        let env: V = (Ty::Str, "%env".into());
        for (i, (x, v)) in free.iter().zip(&captured).enumerate() {
            let val = l.field(&env, i, &v.0)?;
            let slot = l.local(x, &v.0, None)?;
            l.store(&val, &slot)?;
        }
        let mut sig = vec!["ptr %env".to_string()];
        for (i, (p, pt)) in ps.iter().zip(&pts).enumerate() {
            let slot = l.local(&p.name, pt, None)?;
            if has_value(pt) {
                sig.push(format!("{} %p.{i}", l.m.ll(pt)?));
                l.store(&(pt.clone(), format!("%p.{i}")), &slot)?;
            }
            if let Some(w) = &p.ty {
                let v = l.load(&p.name)?;
                l.check(w, &v, &format!("argument `{}` of a lambda", p.name), false)?;
            }
        }
        if has_value(&rt) {
            let slot = l.fresh("ret");
            writeln!(l.allocas, "  {slot} = alloca {}", l.m.ll(&rt)?).unwrap();
            l.ret_slot = Some(((*rt).clone(), slot));
        } else {
            l.ret_slot = Some(((*rt).clone(), String::new()));
        }
        let v = l.expr(body)?;
        l.return_value(&v)?;
        l.block("exit");
        if has_value(&rt) {
            let slot = l.ret_slot.clone().unwrap().1;
            let r = l.fresh("result");
            let ll = l.m.ll(&rt)?;
            l.emit(format!("{r} = load {ll}, ptr {slot}"));
            l.emit(format!("ret {ll} {r}"));
        } else {
            l.emit("ret void".into());
        }
        let text = l.finish();
        let ll = self.m.ll(&rt)?;
        writeln!(self.m.body, "define {ll} {fsym}({}) {{\n{text}}}\n", sig.join(", ")).unwrap();
        let p = self.closure(&fsym, &captured)?;
        Ok((t, p))
    }

    /// A named function or built-in used as a value (`filter(xs, is_prime)`):
    /// a closure that calls it.
    fn function_value(&mut self, e: &Expr, name: &str) -> Res<V> {
        let t = self.ty_of(e)?;
        let Ty::Fn(pts, rt) = t.clone() else { return Err(format!("`{name}` is not a function")) };
        self.m.lambdas += 1;
        let fsym = format!("@\"fn.{}.{}\"", name, self.m.lambdas);
        let mut l = Func::new(self.m, self.sub.clone());
        l.block("entry.body");
        let mut sig = vec!["ptr %env".to_string()];
        let mut args = vec![];
        for (i, pt) in pts.iter().enumerate() {
            if has_value(pt) {
                sig.push(format!("{} %p.{i}", l.m.ll(pt)?));
            }
            args.push((pt.clone(), format!("%p.{i}")));
        }
        let r = l.call_values(name, args, Some((*rt).clone()))?;
        if has_value(&rt) {
            let ll = l.m.ll(&rt)?;
            l.emit(format!("ret {ll} {}", r.1));
        } else {
            l.emit("ret void".into());
        }
        let text = l.finish();
        let ll = self.m.ll(&rt)?;
        writeln!(self.m.body, "define {ll} {fsym}({}) {{\n{text}}}\n", sig.join(", ")).unwrap();
        let p = self.closure(&fsym, &[])?;
        Ok((t, p))
    }

    /// The order `sort` uses, as a closure `(a, b) -> a < b` on values of type `t`.
    fn less(&mut self, t: &Ty) -> Res<V> {
        let cmp = self.helper("cmp", t)?;
        let fsym = format!("@\"less:{}\"", key(t));
        if self.m.helpers.insert(fsym.clone()) {
            let ll = self.m.ll(t)?;
            writeln!(self.m.body, "define i1 {fsym}(ptr %env, {ll} %a, {ll} %b) {{\nentry:\n  %c = call i64 {cmp}({ll} %a, {ll} %b)\n  %r = icmp slt i64 %c, 0\n  ret i1 %r\n}}\n").unwrap();
        }
        let p = self.closure(&fsym, &[])?;
        Ok((Ty::Fn(vec![t.clone(), t.clone()], Box::new(Ty::Bool)), p))
    }

    /// `Some(x)`, `None`, `Line { sku: s, qty: 1 }`, `Entry { ..e, done: true }`, `UserId(s)`.
    fn construct(&mut self, e: &Expr, c: &str, args: &CtorArgs) -> Res<V> {
        let t = self.ty_of(e)?;
        // a distinct type is its value, checked
        if self.m.is_distinct(c) {
            let CtorArgs::Positional(xs) = args else { return Err(format!("`{c}` takes one value")) };
            let v = self.expr(&xs[0])?;
            self.check(&TypeExpr::Named(Symbol::from(Symbol::from(c)), vec![]), &v, &format!("constructing `{c}`"), false)?;
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
                    let x = given.iter().find(|(n, _)| f.name.as_ref().map(|s| s.as_str()) == Some(n.as_str())).ok_or(format!("`{c}` needs its field `{}`", f.name.clone().unwrap_or_default()))?;
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
                    match changed.iter().find(|(n, _)| f.name.as_ref().map(|s| s.as_str()) == Some(n.as_str())) {
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
        let vals: Vec<V> = vals.into_iter().zip(&fs).map(|(v, f)| self.coerce(v, &f.ty)).collect::<Res<_>>()?;
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
        self.scopes.push(HashMap::default());
        if let Some(n) = as_name {
            let slot = self.local(n, &v.0, None)?;
            self.store(&v, &slot)?;
        }
        let end = self.fresh_label("endmatch");
        let mut incoming = vec![];
        for arm in arms {
            let next = self.fresh_label("next_arm");
            self.scopes.push(HashMap::default());
            self.pattern(&arm.pat, &v, &next)?;
            let r = self.expr(&arm.body)?;
            let r = self.coerce(r, &want)?;
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
                self.scopes.push(HashMap::default());
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
                let ok = self.tag_in(v, &[c.to_string()])?;
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
                            let i = fs.iter().position(|f| f.name.as_ref() == Some(&name.to_string())).ok_or(format!("`{c}` has no field `{name}`"))?;
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
            Pattern::List(items, rest) => {
                let et = self.elem_ty(&v.0)?;
                let n = self.list_len(v);
                let ok = self.fresh("lenok");
                let cmp = if rest.is_some() { "sge" } else { "eq" };
                self.emit(format!("{ok} = icmp {cmp} i64 {}, {}", n.1, items.len()));
                test(self, bool_v(&ok));
                for (i, ip) in items.iter().enumerate() {
                    let x = self.list_at(v, &i.to_string(), &et)?;
                    self.pattern(ip, &x, fail)?;
                }
                if let Some(r) = rest.as_ref().filter(|r| *r != "_") {
                    let tail = self.call(&v.0, "@strata_list_slice", &[v.clone(), (Ty::Int, items.len().to_string()), n])?;
                    self.bind(r, &tail)?;
                }
            }
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
                        let w = self.lower(t)?;
                        self.coerce(v, &w)?
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
                let new = self.set_path(&cur, &path.iter().map(|s| s.to_string()).collect::<Vec<_>>(), v, &shown)?;
                let new = self.coerce(new, &l.t)?;
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
                let is_range = matches!(xs, Expr::Call(f, args) if matches!(&**f, Expr::Var(n) if n == "range") && args.len() == 2 && !self.m.env.fns.contains_key(&Symbol::new("range")));
                if !is_range {
                    let v = self.expr(xs)?;
                    if is_never(&v.0) {
                        return Ok(());
                    }
                    let et = self.elem_ty(&v.0)?;
                    let counter = self.fresh("idx");
                    writeln!(self.allocas, "  {counter} = alloca i64").unwrap();
                    self.emit(format!("store i64 0, ptr {counter}"));
                    let (head, inner, step, end) = (self.fresh_label("for"), self.fresh_label("for.body"), self.fresh_label("for.next"), self.fresh_label("endfor"));
                    self.br(&head);
                    self.block(&head);
                    let i = self.fresh("i");
                    self.emit(format!("{i} = load i64, ptr {counter}"));
                    let n = self.list_len(&v);
                    let more = self.apply(BinOp::Lt, &(Ty::Int, i.clone()), &n)?;
                    self.cond_br(&more, &inner, &end);
                    self.block(&inner);
                    let x = self.list_at(&v, &i, &et)?;
                    self.scopes.push(HashMap::default());
                    let fail = self.fresh_label("for.nomatch");
                    self.pattern(p, &x, &fail)?;
                    self.loop_body(body, &step, &end)?;
                    self.block(&fail);
                    let shown = self.json(&x)?;
                    self.fault(Fault::Runtime, vec![Part::Lit("for-loop pattern did not match ".into()), Part::Val(shown)])?;
                    self.scopes.pop();
                    self.block(&step);
                    let next = self.fresh("i");
                    self.emit(format!("{next} = add i64 {i}, 1"));
                    self.emit(format!("store i64 {next}, ptr {counter}"));
                    self.br(&head);
                    self.block(&end);
                    return Ok(());
                }
                let Expr::Call(_, args) = xs else { unreachable!() };
                let (lo, hi) = (self.expr(&args[0])?, self.expr(&args[1])?);
                // as the interpreter, which makes the list first
                let (big, d) = (self.fresh("big"), self.fresh("span"));
                self.emit(format!("{d} = sub i64 {}, {}", hi.1, lo.1));
                let (up, far) = (self.fresh("up"), self.fresh("far"));
                self.emit(format!("{up} = icmp sgt i64 {}, {}", hi.1, lo.1));
                self.emit(format!("{far} = icmp ugt i64 {d}, 1000000"));
                self.emit(format!("{big} = and i1 {up}, {far}"));
                let (toobig, fine) = (self.fresh_label("range.big"), self.fresh_label("range.ok"));
                self.cond_br(&bool_v(&big), &toobig, &fine);
                self.block(&toobig);
                self.fault(Fault::Runtime, vec![Part::Lit("range too large".into())])?;
                self.block(&fine);
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
                self.scopes.push(HashMap::default());
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
        let new_inner = self.coerce(new_inner, &fs[i].ty)?;
        let p = self.copy_block(cur, fs.len());
        self.set_field(&p, i, &new_inner)?;
        let new = (cur.0.clone(), p);
        // the changed field's refinement, and the type's own
        let ctx = format!("assigning `{shown}`");
        self.check(&fs[i].texpr, &new_inner, &format!("{ctx}: {}.{f}", crate::names::short(&c)), false)?;
        let tname = self.m.env.ctors[&Symbol::new(&c)].clone();
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
    /// Bind type parameters by matching a written type against an actual one.
    fn bind_targs(&self, te: &TypeExpr, t: &Ty, params: &[Symbol], out: &mut HashMap<Symbol, Ty>) {
        match (te, t) {
            (TypeExpr::Refined(b, _), t) => self.bind_targs(b, t, params, out),
            (TypeExpr::Named(n, args), t) if args.is_empty() && params.contains(n) => {
                out.entry(n.clone()).or_insert_with(|| t.clone());
            }
            (TypeExpr::Named(n, args), t) => {
                if let Some(td) = self.env.types.get(n) {
                    if let (TypeBody::Alias(a), false) = (&td.body, td.distinct) {
                        let sub: HashMap<Symbol, TypeExpr> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
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

    /// The type arguments of constructor `c`'s type, from a value's type `t`
    /// (the sum, the variant, or a union containing it).
    fn ctor_args(&self, c: &str, t: &Ty) -> Vec<Ty> {
        let parent = self.env.ctors.get(&Symbol::new(c)).cloned().unwrap_or_default();
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

// ---- Unicode tables ----
//
// What the string built-ins need from Unicode, made from Rust's own tables
// (which the interpreter uses), so a compiled program agrees with it.

/// Rows of a character and the (up to three) characters `to_upper` or
/// `to_lower` makes of it, for those it changes; sorted.
fn case_table(upper: bool) -> &'static [[u32; 4]] {
    static UPPER: std::sync::OnceLock<Vec<[u32; 4]>> = std::sync::OnceLock::new();
    static LOWER: std::sync::OnceLock<Vec<[u32; 4]>> = std::sync::OnceLock::new();
    (if upper { &UPPER } else { &LOWER }).get_or_init(|| {
        let mut rows = vec![];
        for c in (0..=0x10FFFFu32).filter_map(char::from_u32) {
            let to: Vec<char> = if upper { c.to_uppercase().collect() } else { c.to_lowercase().collect() };
            if to != [c] {
                let mut row = [c as u32, 0, 0, 0];
                for (i, x) in to.iter().enumerate() {
                    row[i + 1] = *x as u32;
                }
                rows.push(row);
            }
        }
        rows
    })
}

/// The characters with a property, as ranges (first, last): `alpha` is
/// Unicode's Alphabetic (`char::is_alphabetic`), `cased` those with a case.
fn char_ranges(which: &str) -> &'static [[u32; 2]] {
    static ALPHA: std::sync::OnceLock<Vec<[u32; 2]>> = std::sync::OnceLock::new();
    static CASED: std::sync::OnceLock<Vec<[u32; 2]>> = std::sync::OnceLock::new();
    let alpha = which == "alpha";
    (if alpha { &ALPHA } else { &CASED }).get_or_init(|| {
        let has = |c: char| {
            if alpha {
                c.is_alphabetic()
            } else {
                c.is_lowercase() || c.is_uppercase() || c.to_lowercase().ne([c]) || c.to_uppercase().ne([c])
            }
        };
        let mut out: Vec<[u32; 2]> = vec![];
        for c in (0..=0x10FFFFu32).filter_map(char::from_u32).filter(|c| has(*c)) {
            match out.last_mut() {
                Some(r) if r[1] + 1 == c as u32 => r[1] = c as u32,
                _ => out.push([c as u32, c as u32]),
            }
        }
        out
    })
}

impl Module<'_> {
    /// A Unicode table as a global of `i32`s, and its number of rows.
    fn unicode_table(&mut self, which: &str) -> (String, usize) {
        let (cells, rows): (Vec<u32>, usize) = match which {
            "upper" | "lower" => {
                let t = case_table(which == "upper");
                (t.iter().flatten().copied().collect(), t.len())
            }
            _ => {
                let t = char_ranges(which);
                (t.iter().flatten().copied().collect(), t.len())
            }
        };
        let g = format!("@\"unicode:{which}\"");
        if self.helpers.insert(g.clone()) {
            let xs: Vec<String> = cells.iter().map(|c| format!("i32 {c}")).collect();
            writeln!(self.globals, "{g} = private unnamed_addr constant [{} x i32] [{}], align 4", cells.len(), xs.join(", ")).unwrap();
        }
        (g, rows)
    }
}

// ---- strings ----
//
// The string built-ins: native (runtime.c), but `lines`, which is Strata
// (llvm/prelude.strata) over `split`.

impl Func<'_, '_> {
    /// `Some(v)` when `ok`, else `None` (an `Option` of type `t`).
    fn option_if(&mut self, ok: &str, v: &V, t: &Ty) -> Res<V> {
        let (some, none, end) = (self.fresh_label("some"), self.fresh_label("none"), self.fresh_label("opt"));
        self.cond_br(&bool_v(ok), &some, &none);
        self.block(&some);
        let b = self.block_of(self.m.tags["Some"], &[v.clone()])?;
        let from = self.cur.clone();
        self.br(&end);
        self.block(&none);
        let n = self.m.nullary("None");
        self.br(&end);
        self.block(&end);
        let r = self.fresh("o");
        self.emit(format!("{r} = phi ptr [ {b}, %{from} ], [ {n}, %{none} ]"));
        Ok((t.clone(), r))
    }

    /// A string built-in on `vs` (the first a `String`), if `name` is one.
    fn string_builtin(&mut self, name: &str, vs: &[V]) -> Res<Option<V>> {
        let types: Vec<Ty> = vs.iter().map(|v| self.m.repr(&v.0)).collect::<Res<_>>()?;
        if types.first() != Some(&Ty::Str) && !(name == "chr" && types == [Ty::Int]) {
            return Ok(None);
        }
        let s = Ty::Str;
        let strings = Ty::Con("List".into(), vec![Ty::Str]);
        let opt = |t: Ty| Ty::Con("Option".into(), vec![t]);
        let args: Vec<V> = vs.iter().zip(&types).map(|(v, t)| (t.clone(), v.1.clone())).collect();
        Ok(Some(match (name, types.as_slice()) {
            ("split", [_, Ty::Str]) => self.call(&strings, "@strata_str_split", &args)?,
            ("trim", [_]) => self.call(&s, "@strata_str_trim", &args)?,
            ("contains", [_, Ty::Str]) => self.call(&Ty::Bool, "@strata_str_contains", &args)?,
            ("starts_with", [_, Ty::Str]) => self.call(&Ty::Bool, "@strata_str_starts_with", &args)?,
            ("ends_with", [_, Ty::Str]) => self.call(&Ty::Bool, "@strata_str_ends_with", &args)?,
            ("reverse", [_]) => self.call(&s, "@strata_str_reverse", &args)?,
            ("chars", [_]) => self.call(&strings, "@strata_str_chars", &args)?,
            ("lines", [_]) => self.call_fn(&prelude("lines"), args, Some(strings))?,
            ("get", [_, Ty::Int]) => {
                let c = self.call(&s, "@strata_str_at", &args)?;
                let ok = self.fresh("found");
                self.emit(format!("{ok} = icmp ne ptr {}, null", c.1));
                self.option_if(&ok, &c, &opt(Ty::Str))?
            }
            ("at", [_, Ty::Int]) => self.call(&s, "@strata_str_at_or_fault", &args)?,
            ("slice", [_, Ty::Int, Ty::Int]) => self.call(&s, "@strata_str_slice", &args)?,
            ("index_of", [_, Ty::Str]) => {
                let i = self.call(&Ty::Int, "@strata_str_index_of", &args)?;
                let ok = self.fresh("found");
                self.emit(format!("{ok} = icmp sge i64 {}, 0", i.1));
                self.option_if(&ok, &i, &opt(Ty::Int))?
            }
            ("replace", [_, Ty::Str, Ty::Str]) => self.call(&s, "@strata_str_replace", &args)?,
            ("repeat", [_, Ty::Int]) => self.call(&s, "@strata_str_repeat", &args)?,
            ("to_upper" | "to_lower", [_]) => {
                let (table, n) = self.m.unicode_table(if name == "to_upper" { "upper" } else { "lower" });
                let (cased, m) = self.m.unicode_table("cased");
                let lower = bool_v(if name == "to_lower" { "true" } else { "false" });
                let a = [args[0].clone(), (Ty::Str, table), (Ty::Int, n.to_string()), (Ty::Str, cased), (Ty::Int, m.to_string()), lower];
                self.call(&s, "@strata_str_case", &a)?
            }
            ("is_digit" | "is_alpha" | "is_space", [_]) => {
                let (ranges, n) = self.m.unicode_table("alpha");
                let class = match name {
                    "is_digit" => "0",
                    "is_alpha" => "1",
                    _ => "2",
                };
                self.call(&Ty::Bool, "@strata_str_is", &[args[0].clone(), (Ty::Int, class.into()), (Ty::Str, ranges), (Ty::Int, n.to_string())])?
            }
            ("ord", [_]) => self.call(&Ty::Int, "@strata_str_ord", &args)?,
            ("chr", [Ty::Int]) => self.call(&s, "@strata_str_chr", &args)?,
            ("parse_int" | "parse_float", [_]) => {
                let t = if name == "parse_int" { Ty::Int } else { Ty::Float };
                let ll = self.m.ll(&t)?;
                let slot = self.fresh("parsed");
                writeln!(self.allocas, "  {slot} = alloca {ll}").unwrap();
                self.emit(format!("store {ll} {}, ptr {slot}", if name == "parse_int" { "0" } else { "0.0" }));
                let ok = self.call(&Ty::Bool, &format!("@strata_{name}"), &[args[0].clone(), (Ty::Str, slot.clone())])?;
                let x = self.fresh("x");
                self.emit(format!("{x} = load {ll}, ptr {slot}"));
                self.option_if(&ok.1, &(t.clone(), x), &opt(t))?
            }
            _ => return Ok(None),
        }))
    }

    /// An f-string part with a format spec (`{x:.2}`, `{n:>5}`, `{n:05}`),
    /// as `interp::format_spec`.
    fn format_spec(&mut self, v: &V, f: &FmtSpec) -> Res<String> {
        // what the value is at run time: a distinct type's is not a number or a string
        let numeric = matches!(v.0, Ty::Int | Ty::Float);
        if (f.plus || f.zero) && !numeric {
            let shown = self.json(v)?;
            self.fault(Fault::Runtime, vec![Part::Lit(format!("format spec `{f}`: `+` and `0` apply to numbers, not ")), Part::Val(shown)])?;
            return Ok(self.m.string(""));
        }
        let body = match (&v.0, f.precision) {
            (Ty::Float, Some(p)) => self.call(&Ty::Str, "@strata_show_float_fixed", &[v.clone(), (Ty::Int, p.to_string())])?.1,
            (Ty::Str, Some(p)) => self.call(&Ty::Str, "@strata_str_slice", &[v.clone(), (Ty::Int, "0".into()), (Ty::Int, p.to_string())])?.1,
            (_, Some(_)) => {
                let shown = self.json(v)?;
                self.fault(Fault::Runtime, vec![Part::Lit(format!("format spec `{f}`: `.N` applies to a Float or a String, not ")), Part::Val(shown)])?;
                return Ok(self.m.string(""));
            }
            _ => self.show(v)?,
        };
        let align = f.align.map(|c| c as u32).unwrap_or(0);
        let r = self.fresh("fmt");
        self.emit(format!(
            "{r} = call ptr @strata_format(ptr {body}, i1 {numeric}, i32 {fill}, i32 {align}, i1 {plus}, i1 {zero}, i64 {width})",
            fill = f.fill as u32,
            plus = f.plus,
            zero = f.zero,
            width = f.width
        ));
        Ok(r)
    }
}

// ---- dictionaries and sets ----
//
// A `Set[A]` is a `List[A]` kept sorted in the interpreter's order
// (`interp::compare`, the generated `cmp`) without duplicates; a
// `Dict[K, V]` is a `List[(K, V)]` sorted by key, one pair per key. So equal
// collections are equal lists, `for`, `len` and `items` are the list's, and
// the built-ins are Strata over lists (llvm/prelude.strata), given the order.

/// `Dict` or `Set`, and the list type it is kept as.
fn collection(t: &Ty) -> Option<(&'static str, Ty)> {
    let list = |t: Ty| Ty::Con("List".into(), vec![t]);
    match t {
        Ty::Con(n, a) if n == "Set" && a.len() == 1 => Some(("Set", list(a[0].clone()))),
        Ty::Con(n, a) if n == "Dict" && a.len() == 2 => Some(("Dict", list(Ty::Con("Tuple".into(), a.clone())))),
        _ => None,
    }
}

impl Func<'_, '_> {
    /// A dictionary or set as the list it is kept as.
    fn as_list(&self, v: &V) -> Res<V> {
        let t = self.m.repr(&v.0)?;
        Ok((collection(&t).map(|c| c.1).unwrap_or(t), v.1.clone()))
    }

    /// The order on values of type `t`, as a closure `(a, b) -> Int`: -1, 0 or 1.
    fn ordering(&mut self, t: &Ty) -> Res<V> {
        let cmp = self.helper("cmp", t)?;
        let fsym = format!("@\"order:{}\"", key(t));
        if self.m.helpers.insert(fsym.clone()) {
            let ll = self.m.ll(t)?;
            writeln!(self.m.body, "define i64 {fsym}(ptr %env, {ll} %a, {ll} %b) {{\nentry:\n  %c = call i64 {cmp}({ll} %a, {ll} %b)\n  ret i64 %c\n}}\n").unwrap();
        }
        let p = self.closure(&fsym, &[])?;
        Ok((Ty::Fn(vec![t.clone(), t.clone()], Box::new(Ty::Int)), p))
    }

    /// A built-in on dictionaries or sets (or making one), if `name` is one.
    fn collection_builtin(&mut self, name: &str, vs: &[V], hint: &Option<Ty>) -> Res<Option<V>> {
        let Some(first) = vs.first() else { return Ok(None) };
        let ft = self.m.repr(&first.0)?;
        // the type of the result, as written: the call's, or `t` when it has none
        let typed = |v: V, t: Ty| -> V { (hint.clone().filter(|h| !matches!(h, Ty::Unit)).unwrap_or(t), v.1) };
        // making one from a list
        if let (Ty::Con(l, et), "set" | "dict") = (&ft, name) {
            if l != "List" {
                return Ok(None);
            }
            let et = et[0].clone();
            let made = if name == "set" {
                let less = self.less(&et)?;
                let r = self.call_fn(&prelude("set_from"), vec![first.clone(), less], None)?;
                typed(r, Ty::Con("Set".into(), vec![et]))
            } else {
                let Ty::Con(_, kv) = self.m.repr(&et)? else { return Err("`dict` needs a list of pairs".into()) };
                let less = self.less(&kv[0])?;
                let r = self.call_fn(&prelude("dict_from"), vec![first.clone(), less], None)?;
                typed(r, Ty::Con("Dict".into(), kv))
            };
            return Ok(Some(made));
        }
        let Some((kind, _)) = collection(&ft) else { return Ok(None) };
        let Ty::Con(_, targs) = ft.clone() else { unreachable!() };
        let c = self.as_list(first)?;
        let rest: Vec<V> = vs[1..].to_vec();
        let same = |v: V| (ft.clone(), v.1);
        let with = |s: &mut Self, f: &str, mut more: Vec<V>, order: bool, ret: Option<Ty>| -> Res<V> {
            let mut args = vec![c.clone()];
            args.append(&mut more);
            if order {
                let o = s.ordering(&targs[0])?;
                args.push(o);
            }
            s.call_fn(&prelude(f), args, ret)
        };
        Ok(Some(match (kind, name, rest.len()) {
            (_, "len", 0) => self.list_len(&c),
            ("Set", "to_list", 0) | ("Dict", "items", 0) => c.clone(),
            ("Set", "contains", 1) => with(self, "set_contains", rest, true, None)?,
            ("Set", "insert", 1) => same(with(self, "set_insert", rest, true, None)?),
            ("Set", "remove", 1) => same(with(self, "set_remove", rest, false, None)?),
            ("Set", "union" | "intersection" | "difference", 1) => {
                let other = self.as_list(&rest[0])?;
                same(with(self, &format!("set_{name}"), vec![other], true, None)?)
            }
            ("Dict", "contains", 1) => with(self, "dict_contains", rest, true, None)?,
            ("Dict", "get", 1) => with(self, "dict_get", rest, true, hint.clone())?,
            ("Dict", "insert", 2) => same(with(self, "dict_insert", rest, true, None)?),
            ("Dict", "remove", 1) => same(with(self, "dict_remove", rest, false, None)?),
            ("Dict", "keys" | "values", 0) => with(self, &format!("dict_{name}"), vec![], false, None)?,
            ("Dict", "map_values", 1) => {
                let r = with(self, "dict_map_values", rest, false, None)?;
                let Ty::Con(_, kv) = self.elem_ty(&r.0).and_then(|t| self.m.repr(&t))? else { unreachable!() };
                typed(r, Ty::Con("Dict".into(), kv))
            }
            _ => return Ok(None),
        }))
    }

}
