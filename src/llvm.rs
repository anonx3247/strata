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
//! never change once built, so they are shared freely. Every heap object has
//! a reference count just before it (runtime.c, "memory"); constants have a
//! count of 0 and are never freed. The generated code counts (`Scope` has
//! the conventions) and drops each value with a function made for its type
//! (`drop_fn`), so a value is freed as soon as nothing holds it; values
//! never form cycles.
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
//! the command line, and unions of anything but records and variants. A
//! program that uses them is refused, with the function and the construct
//! named.

use crate::ast::*;
use crate::types::{Env, Ty};
use std::collections::VecDeque;
use crate::fx::{HashMap, HashSet};
use std::fmt::Write;

pub mod jit;
pub mod native;

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

/// Whether a program starts tasks: a `spawn` or a `race` anywhere.
fn uses_tasks(decls: &[Decl]) -> bool {
    struct Tasks(bool);
    impl Visit for Tasks {
        fn expr(&mut self, e: &Expr) {
            if matches!(e, Expr::Spawn(_) | Expr::SupervisedSpawn(_) | Expr::Race(_)) {
                self.0 = true;
            }
            if !self.0 {
                walk_expr(self, e)
            }
        }
    }
    let mut t = Tasks(false);
    for d in decls {
        walk_decl(&mut t, &d.kind);
    }
    t.0
}

/// Whether evaluating `e` may assign a variable: an assignment in a block,
/// or a `mut` argument or receiver (a lambda's body can't change outer
/// variables, and runs later).
fn assigns(e: &Expr) -> bool {
    struct Assigns(bool);
    impl Visit for Assigns {
        fn expr(&mut self, e: &Expr) {
            match e {
                Expr::Mut(_) => self.0 = true,
                Expr::Lambda(..) | Expr::Spawn(..) | Expr::SupervisedSpawn(..) => return,
                Expr::Race(arms) => {
                    arms.iter().for_each(|a| self.expr(&a.body));
                    return;
                }
                Expr::Block(stmts, _) if stmts.iter().any(|s| matches!(s, Stmt::Assign(..))) => self.0 = true,
                _ => {}
            }
            if !self.0 {
                walk_expr(self, e)
            }
        }
    }
    let mut a = Assigns(false);
    a.expr(e);
    a.0
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

/// The entries of a trait value's table: showing the value as JSON and as
/// `to_string`, dropping and holding it, comparing two of the same type,
/// then the trait's methods.
const DYN_JSON: usize = 0;
const DYN_SHOW: usize = 1;
const DYN_DEBUG: usize = 2;
const DYN_DROP: usize = 3;
const DYN_DUP: usize = 4;
const DYN_EQ: usize = 5;
const DYN_METHODS: usize = 6;

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
    Ok(compile_for(prog, Entry::Main(entry))?.0)
}

thread_local! {
    static TRACING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// What `verify` proved, for the next compilation (`compile_proven`),
    /// and how many checks it then left out and kept.
    static PROOFS: std::cell::RefCell<(HashMap<String, Proofs>, usize, usize)> = std::cell::RefCell::new(Default::default());
}

/// What `verify` found of one function's checks, by what each is called
/// (`assert x > 0`, `ensures it >= 0`, `index in range`): those proven,
/// and those it didn't prove (refuted, unknown, stopped).
#[derive(Debug, Clone, Default)]
pub struct Proofs {
    pub proven: Vec<String>,
    pub open: Vec<String>,
}

/// `compile`, leaving out the runtime checks `proofs` (by function) shows
/// proven. Also returns how many checks were left out and how many kept.
pub fn compile_proven(prog: &Program, entry: &str, proofs: HashMap<String, Proofs>, traced: bool) -> Res<(String, usize, usize)> {
    PROOFS.with(|p| *p.borrow_mut() = (proofs, 0, 0));
    let r = if traced { compile_traced(prog, entry) } else { compile(prog, entry) };
    let (_, out, kept) = PROOFS.with(|p| std::mem::take(&mut *p.borrow_mut()));
    Ok((r?, out, kept))
}

/// `f` compiling without tracing: compile-time code made while a traced
/// program compiles (a literal pattern's matcher) is the compiler's work,
/// not the program's.
pub fn untraced<R>(f: impl FnOnce() -> R) -> R {
    let was = TRACING.with(|t| t.replace(false));
    let r = f();
    TRACING.with(|t| t.set(was));
    r
}

/// `compile_comptime`, traced (`strata run f [args] --trace`).
pub fn compile_traced_comptime(prog: &Program, fns: &[String]) -> Res<(String, Vec<Shape>, Shapes)> {
    TRACING.with(|t| t.set(true));
    let r = compile_comptime(prog, fns);
    TRACING.with(|t| t.set(false));
    r
}

/// `compile`, each function of the program reporting its calls and returns
/// on stderr as the interpreter's `--trace` did (`strata_trace_call`).
pub fn compile_traced(prog: &Program, entry: &str) -> Res<String> {
    TRACING.with(|t| t.set(true));
    let r = compile(prog, entry);
    TRACING.with(|t| t.set(false));
    r
}

/// Where each function is referred to (called, or taken as a value): the
/// bodies of the functions that do, and the functions also referred to
/// anywhere else (a contract, a type's refinement, a test, an impl's
/// method) or by a method call's name. A function's entry checks are
/// proven at each call, by its caller: they are left out only when every
/// reference is in a body whose proofs cover them.
#[derive(Default)]
struct Refs {
    /// The program's own functions (not std's, nor the prelude's): those whose proofs count.
    own: HashSet<String>,
    callers: HashMap<String, HashSet<String>>,
    elsewhere: HashSet<String>,
    methods: HashSet<String>,
}

impl Refs {
    fn of(prog: &Program) -> Refs {
        struct Walk<'r> {
            refs: &'r mut Refs,
            from: Option<String>,
        }
        impl crate::ast::Visit for Walk<'_> {
            fn expr(&mut self, e: &Expr) {
                match e {
                    Expr::Var(x) => match &self.from {
                        Some(f) => {
                            self.refs.callers.entry(x.to_string()).or_default().insert(f.clone());
                        }
                        None => {
                            self.refs.elsewhere.insert(x.to_string());
                        }
                    },
                    Expr::Method(_, m, _) => {
                        self.refs.methods.insert(crate::names::short(m).to_string());
                    }
                    _ => {}
                }
                crate::ast::walk_expr(self, e)
            }
            fn ty(&mut self, t: &TypeExpr) {
                let from = self.from.take();
                crate::ast::walk_ty(self, t);
                self.from = from;
            }
            fn pattern(&mut self, p: &Pattern) {
                let from = self.from.take();
                crate::ast::walk_pattern(self, p);
                self.from = from;
            }
        }
        let mut refs = Refs::default();
        for d in prog.decls.iter().filter(|d| d.module.as_str() != "std" && !d.module.as_str().starts_with("std.")) {
            match &d.kind {
                DeclKind::Fn(f) => {
                    refs.own.insert(f.name.to_string());
                }
                DeclKind::Impl(imp) if imp.is_inherent() => refs.own.extend(imp.methods.iter().map(|m| crate::types::static_fn(imp, m).name.to_string())),
                _ => {}
            }
        }
        for d in &prog.decls {
            let mut w = Walk { refs: &mut refs, from: None };
            match &d.kind {
                DeclKind::Fn(f) => {
                    use crate::ast::Visit;
                    let mut contract = f.clone();
                    contract.body = Expr::Unit;
                    crate::ast::walk_decl(&mut w, &DeclKind::Fn(contract));
                    w.from = Some(f.name.to_string());
                    w.expr(&f.body);
                }
                k => crate::ast::walk_decl(&mut w, k),
            }
        }
        refs
    }
}

/// Whether `verify`'s check `e` is the one called `k`: the same, or with
/// what it checks (`assigning \`x\`: Natural`) or how (` (early return)`).
fn same_check(e: &str, k: &str) -> bool {
    e.strip_prefix(k).is_some_and(|rest| rest.is_empty() || rest.starts_with(':') || rest.starts_with(" ("))
}

/// What a compilation starts from.
enum Entry<'e> {
    Main(&'e str),
    /// The tests, but those named here; with `true`, also those of what the
    /// program imports (std, dependencies), not only its own.
    Tests(&'e [String], bool),
    Fuzz(&'e [FuzzTarget]),
    /// std's functions to precompile (`jit::std_library`): only they are exported.
    Library(&'e [String]),
    /// Compile-time code: functions without parameters, whose results are kept.
    Comptime(&'e [String]),
    /// Functions called with arguments the host writes (`strata_given`):
    /// `@"strata.ct.N"` calls the Nth and keeps its result, as compile-time
    /// code does.
    Calls(&'e [CallEntry]),
    /// Nothing compiled: the shapes of these types, for values the host
    /// writes or reads (`native::write`, `native::read`).
    Shapes(&'e [TypeExpr]),
}

/// Tags from here on are union boxes' (`box_tag`); below, constructors' (0 is a tuple's).
const BOX_TAGS: i64 = 1 << 40;

/// 40 bits of a name's hash (SHA-256: the same in every strata).
fn hash40(name: &str) -> i64 {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(name.as_bytes());
    (u64::from_le_bytes(d[..8].try_into().unwrap()) & ((1 << 40) - 1)) as i64
}

/// A constructor's tag, from its qualified name: in [1, 2^40).
fn stable_tag(ctor: &str) -> i64 {
    1 + hash40(ctor) % (BOX_TAGS - 1)
}

/// The label of compile-time code (a probe): it has no frame on the call stack.
const COMPTIME: &str = "compile-time code";

/// How a value is laid out in memory, to read one the compiled code made
/// (a `comptime` result; the FFI's values later).
#[derive(Debug, Clone)]
pub enum Shape {
    Int,
    Float,
    Bool,
    Unit,
    Str,
    List(Box<Shape>),
    Set(Box<Shape>),
    Dict(Box<Shape>, Box<Shape>),
    Tuple(Vec<Shape>),
    /// A record or sum type (or a variant): its constructors in `Shapes`, by this key.
    Named(String),
    Distinct(String, Box<Shape>),
    /// A union: each member, with its box's tag if it is boxed.
    Union(Vec<(Option<i64>, Shape)>),
    /// A value the host only holds and hands back, never reads or writes
    /// (an app's model, which may hold functions): why it can't, otherwise.
    Opaque(String),
}

/// What `--trace` reads: each traced function instance's name, arguments'
/// shapes (`None`: no value) and result's, and the named shapes they use.
#[derive(Debug, Clone, Default)]
pub struct Traced {
    pub fns: Vec<(String, Vec<Option<Shape>>, Option<Shape>)>,
    pub named: Shapes,
}

/// A constructor of a named shape: its tag, name, whether it makes a record
/// (else a variant), and its fields.
#[derive(Debug, Clone)]
pub struct CtorShape {
    pub tag: i64,
    pub name: String,
    pub record: bool,
    pub fields: Vec<(Option<String>, Shape)>,
}

/// The named shapes a set of shapes refers to.
#[derive(Debug, Clone, Default)]
pub struct Shapes {
    pub named: HashMap<String, Vec<CtorShape>>,
}

/// The IR of compile-time code: for each of `fns` (functions of the program
/// without parameters), `@"strata.ct.N"` calls it and hands its result to
/// the runtime (`strata_ct_keep`), with steps counted; and the shape of each
/// result.
pub fn compile_comptime(prog: &Program, fns: &[String]) -> Res<(String, Vec<Shape>, Shapes)> {
    let (ir, _, shapes) = compile_with(prog, Entry::Comptime(fns), true)?;
    let (roots, named) = shapes.unwrap_or_default();
    Ok((ir, roots, named))
}

/// A function the host calls (`compile_calls`), and how it gets each argument.
#[derive(Debug, Clone)]
pub struct CallEntry {
    pub name: String,
    pub args: Vec<Arg>,
}

/// How a host-called function gets an argument.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Arg {
    /// Written by the host: checked all the way down, as nothing made it
    /// here, and dropped after the call.
    Given,
    /// Held by the host (a session's model, a capability): lent to the
    /// call, not checked, not dropped.
    Lent,
    /// Made by compiled code and handed back: not checked, dropped after the call.
    Moved,
}

/// The IR of calls of `calls` (functions of the resolved `prog`) with
/// arguments the host writes into memory: `@"strata.ct.N"` reads argument
/// `i` of the Nth with `strata_given(i)`, calls the function (which checks
/// its arguments' refinements and its contract), keeps its result
/// (`strata_ct_keep`: the host's from then on) and drops the arguments it
/// was given. The shapes, per call: the result's, then each parameter's.
pub fn compile_calls(prog: &Program, calls: &[CallEntry]) -> Res<(String, Vec<Vec<Shape>>, Shapes)> {
    let (ir, _, shapes) = compile_with(prog, Entry::Calls(calls), true)?;
    let (mut all, named) = shapes.unwrap_or_default();
    let mut per = vec![];
    for c in calls {
        let rest = all.split_off((c.args.len() + 1).min(all.len()));
        per.push(std::mem::replace(&mut all, rest));
    }
    Ok((ir, per, named))
}

/// How values of `types` are laid out in memory, in any program that has
/// them: tags come from names, so a shape found in `prog` holds wherever the
/// types mean the same (std's, in every program).
pub fn shapes_of(prog: &Program, types: &[TypeExpr]) -> Res<(Vec<Shape>, Shapes)> {
    Ok(compile_with(prog, Entry::Shapes(types), false)?.2.unwrap_or_default())
}

/// A function `strata verify --jit` fuzzes: its name, and its parameters'
/// names, written types (generic ones at the instance tried) and `mut`ness.
pub struct FuzzTarget {
    pub name: String,
    pub params: Vec<(String, TypeExpr, bool)>,
}

/// The LLVM IR of fuzz drivers (`@"strata.fuzz.N"` for target N): each
/// generates the target's arguments, hands them over (`strata_fuzz_keep`), and
/// calls it. Loops and calls count steps (`strata_step`).
pub fn compile_fuzz(prog: &Program, targets: &[FuzzTarget]) -> Res<(String, Vec<Vec<Shape>>, Shapes)> {
    let (ir, _, shapes) = compile_with(prog, Entry::Fuzz(targets), true)?;
    let (mut all, named) = shapes.unwrap_or_default();
    // split back into each driver's
    let mut per = vec![];
    for t in targets {
        let rest = all.split_off(t.params.len().min(all.len()));
        per.push(std::mem::replace(&mut all, rest));
    }
    Ok((ir, per, named))
}

/// The LLVM IR of a program's `test` blocks: a function per test
/// (`@"strata.test.N"`, taking and returning nothing), and their names.
/// Only the program's own tests (its files'), unless `with_deps`: what it
/// imports is tested on its own (std with `--file std`), and compiled and
/// verified there.
pub fn compile_tests(prog: &Program, with_deps: bool) -> Res<(String, Vec<String>)> {
    compile_for(prog, Entry::Tests(&[], with_deps))
}

/// `compile_tests`, leaving out the tests named in `skip`.
pub fn compile_tests_except(prog: &Program, skip: &[String], with_deps: bool) -> Res<(String, Vec<String>)> {
    compile_for(prog, Entry::Tests(skip, with_deps))
}

fn compile_for(prog: &Program, entry: Entry) -> Res<(String, Vec<String>)> {
    let (ir, tests, _) = compile_with(prog, entry, false)?;
    Ok((ir, tests))
}

/// The matcher of each literal pattern of `matches` (`std.regex.matcher`),
/// made while compiling (cached), as a function returning it as data
/// (`strata.matcher.N`), added to `p`: by pattern, the function's name. A
/// pattern that isn't a regex has none: matched when the program runs, it
/// faults saying why. The program's code is left as written (its messages
/// show `matches(it, "…")`).
fn literal_patterns(p: &mut Program) -> HashMap<String, String> {
    let mut patterns: Vec<String> = vec![];
    for d in &p.decls {
        let mut k = d.kind.clone();
        crate::ast::decl_all_exprs_mut(&mut k, |e| {
            if let Expr::Call(f, args) = e {
                if let (Expr::Var(n), [_, Expr::Str(s)]) = (&**f, args.as_slice()) {
                    if n.as_str() == "matches" && !patterns.contains(s) {
                        patterns.push(s.clone());
                    }
                }
            }
        });
    }
    // (std is there, under qualified names, when the program calls `matches`)
    let mut out = HashMap::default();
    if patterns.is_empty() || !p.decls.iter().any(|d| matches!(&d.kind, DeclKind::Fn(f) if f.name.as_str() == "std.regex.matcher")) {
        return out;
    }
    let calls: Vec<Expr> = patterns.iter().map(|s| Expr::Call(Box::new(Expr::Var(Symbol::new("std.regex.matcher"))), vec![Expr::Str(s.clone())])).collect();
    let made = crate::derive::comptime_literals(p, &calls);
    for (i, (s, m)) in patterns.iter().zip(made).enumerate() {
        let Ok(m) = m else { continue };
        if matches!(&m, Expr::Ctor(c, _) if c.as_str().ends_with("NotARegex")) {
            continue;
        }
        let name = format!("strata.matcher.{i}");
        let f = FnDecl {
            doc: vec![],
            comments: vec![],
            name: Symbol::new(&name),
            type_params: vec![],
            bounds: vec![],
            params: vec![],
            ret: TypeExpr::Named(Symbol::new("std.regex.Matcher"), vec![]),
            requires: vec![],
            ensures: vec![],
            terminates: false,
            body: m,
            comptime: false,
            derived: false,
        };
        p.decls.push(Decl { comments: vec![], doc: vec![], kind: DeclKind::Fn(f), module: Symbol::new(""), public: false });
        out.insert(s.clone(), name);
    }
    out
}

#[allow(clippy::type_complexity)]
/// `resolved`: `prog` is resolved already (`Checked::compiled`, with what was
/// added to it): only the prelude is.
fn compile_with(prog: &Program, entry: Entry, resolved: bool) -> Res<(String, Vec<String>, Option<(Vec<Shape>, Shapes)>)> {
    // the list built-ins in Strata, compiled with the program
    let mut with = prog.clone();
    let prelude = crate::binary::decode_module(PRELUDE).map_err(|e| format!("llvm/prelude.strata: {e}"))?;
    let mut with = if resolved {
        with.decls.extend(crate::resolve::resolve(&Program { decls: prelude }).program.decls);
        with
    } else {
        with.decls.extend(prelude);
        crate::resolve::resolve(&with).program
    };
    // a literal pattern's matcher (a DFA, usually) is made now: data in the
    // program (not for compile-time code or std's library, which have none)
    let matchers = if matches!(entry, Entry::Main(_) | Entry::Tests(..) | Entry::Fuzz(_)) { literal_patterns(&mut with) } else { HashMap::default() };
    let (resolved, types) = crate::types::elaborate_resolved(&with);
    let (env, _) = Env::build(&resolved.program);
    // every constructor gets a tag of its own, so a value's tag says what it is
    let mut ctors: Vec<&Symbol> = env.ctors.keys().collect();
    ctors.sort();
    // from the constructor's qualified name, not the program's numbering: the
    // same in every program, so code compiled once (std, precompiled; the FFI)
    // reads values alike
    let mut tags: HashMap<String, i64> = HashMap::default();
    let mut taken: HashMap<i64, String> = HashMap::default();
    for c in ctors {
        let t = stable_tag(c.as_str());
        if let Some(other) = taken.insert(t, c.to_string()) {
            return Err(format!("the constructors `{other}` and `{c}` have the same tag (rename one)"));
        }
        tags.insert(c.to_string(), t);
    }
    // bodies as the checker saw them (its types are by node); signatures from `env`
    let mut bodies: HashMap<String, &FnDecl> = HashMap::default();
    for d in &resolved.program.decls {
        match &d.kind {
            DeclKind::Fn(f) | DeclKind::Extern(ExternDecl { item: ExternItem::Fn(f), .. }) => {
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
    // (the prelude's traits too: `Debug`, `Display`)
    let traits = crate::types::prelude().decls.iter().chain(resolved.program.decls.iter()).filter_map(|d| match &d.kind {
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
    let mut m = Module { env: &env, types: &types, bodies, traits, impls, refinements, tags, globals: String::new(), strings: HashMap::default(), constants: 0, constant_of: HashMap::default(), body: String::new(), queue: VecDeque::new(), seen: HashSet::default(), helpers: HashSet::default(), lambdas: 0, fuzzing: false, fuzz_kept: Default::default(), traced: Default::default(), tracing: TRACING.with(|t| t.get()), proofs: if matches!(entry, Entry::Main(_)) { PROOFS.with(|p| std::mem::take(&mut p.borrow_mut().0)) } else { HashMap::default() }, refs: Refs::default(), left_out: 0, kept: 0, box_tags: HashMap::default(), gens: HashMap::default(), vtables: HashMap::default(), dispatch: vec![], foreign: Default::default(), precompiled: None, matchers, tasks: uses_tasks(&resolved.program.decls) || BUILDING_STD.with(|b| b.get()) };
    if !m.proofs.is_empty() {
        m.refs = Refs::of(prog);
    }
    // std's precompiled functions, but while making them, tracing, or in a library
    if !BUILDING_STD.with(|b| b.get()) && !m.tracing && !matches!(entry, Entry::Library(_)) {
        m.precompiled = jit::std_library().map(|l| &l.names);
    }
    let mut tests = vec![];
    // compile-time code counts steps too (its budget)
    m.fuzzing = matches!(entry, Entry::Fuzz(_) | Entry::Comptime(_));
    let mut comptime: Vec<(String, Ty)> = vec![];
    // `Entry::Calls`' results and parameter types, for their shapes
    let mut called: Option<Vec<Ty>> = None;
    match entry {
        Entry::Calls(calls) => {
            let mut tys = vec![];
            for (i, c) in calls.iter().enumerate() {
                let f = env.fns.get(&Symbol::new(&c.name)).ok_or(format!("no function `{}`", c.name))?.clone();
                if c.args.len() != f.params.len() {
                    return Err(format!("`{}` takes {} arguments", c.name, f.params.len()));
                }
                tys.extend(m.call_entry(i, &c.name, &f, &c.args)?);
            }
            called = Some(tys);
        }
        Entry::Fuzz(targets) => {
            for (i, t) in targets.iter().enumerate() {
                m.fuzz_driver(i, t).map_err(|e| format!("can't compile the fuzzing of `{}`: {e}", t.name))?;
            }
        }
        Entry::Main(entry) => {
            let main = env.fns.get(&Symbol::new(entry)).ok_or(format!("no `{entry}` function to compile"))?.clone();
            let ret = m.lower(&main.ret, &HashMap::default())?;
            // C's `main` first: it asks for the program's `main`, with its capabilities
            m.entry(entry, &main, &ret)?;
        }
        Entry::Library(names) => {
            for n in names.iter() {
                m.want_impls(n, vec![], vec![]);
            }
        }
        Entry::Comptime(fns) => {
            // each an ordinary function without parameters, its result handed over below
            for name in fns.iter() {
                let f = env.fns.get(&Symbol::new(name)).ok_or(format!("no compile-time function `{name}`"))?.clone();
                m.function(name, &[], &[], &f)?;
                let t = m.lower(&f.ret, &HashMap::default())?;
                comptime.push((sym_impls(name, &[], &[]), t));
            }
        }
        Entry::Shapes(types) => {
            let mut named = Shapes::default();
            let mut roots = vec![];
            for t in types.iter() {
                let t = m.lower(t, &HashMap::default())?;
                roots.push(m.shape(&t, &mut named)?);
            }
            return Ok((String::new(), vec![], Some((roots, named))));
        }
        Entry::Tests(skip, with_deps) => {
            // each test, a function of its own (no frame of its own on the call stack);
            // the program's own definitions come first, what it imports after them
            for (i, d) in resolved.program.decls.iter().enumerate() {
                let DeclKind::Test(t) = &d.kind else { continue };
                if !with_deps && i >= prog.decls.len() {
                    continue;
                }
                if skip.iter().any(|s| s == t.name.as_str()) {
                    continue;
                }
                let inst = Instance { label: format!("test {:?}", t.name.to_string()), symbol: format!("@\"strata.test.{}\"", tests.len()), impls: vec![], params: vec![], ret: TypeExpr::Unit, own_params: &[], own_ret: &TypeExpr::Unit, requires: vec![], ensures: vec![], body: &t.body, sub: HashMap::default(), given: &t.params };
                m.instance(&inst).map_err(|e| format!("can't compile test {:?}: {e}", t.name.to_string()))?;
                tests.push(t.name.to_string());
            }
        }
    }
    loop {
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
        // dispatchers cover every table; making one may compile more (and make tables)
        if !m.dispatchers()? && m.queue.is_empty() {
            break;
        }
    }
    for d in &m.dispatch {
        if let Some((_, text)) = &d.made {
            m.body += text;
        }
    }
    // compile-time code: a function per expression handing its value to the runtime, and its shape
    let mut shapes = None;
    if let Some(tys) = called {
        let mut named = Shapes::default();
        let roots = tys.iter().map(|t| m.shape(t, &mut named).unwrap_or_else(Shape::Opaque)).collect();
        shapes = Some((roots, named));
    }
    if !comptime.is_empty() {
        let mut named = Shapes::default();
        let mut roots = vec![];
        for (i, (sym, t)) in comptime.iter().enumerate() {
            roots.push(m.shape(t, &mut named)?);
            let ll = m.ll(t)?;
            let (call, slot) = match ll {
                "void" => (format!("  call void {sym}()"), "0".to_string()),
                "i64" => (format!("  %r = call i64 {sym}()"), "%r".into()),
                "double" => (format!("  %r = call double {sym}()\n  %s = bitcast double %r to i64"), "%s".into()),
                "i1" => (format!("  %r = call i1 {sym}()\n  %s = zext i1 %r to i64"), "%s".into()),
                _ => (format!("  %r = call ptr {sym}()\n  %s = ptrtoint ptr %r to i64"), "%s".into()),
            };
            writeln!(m.body, "define void @\"strata.ct.{i}\"() {{\nentry:\n{call}\n  call void @strata_ct_keep(i64 {slot})\n  ret void\n}}\n").unwrap();
        }
        shapes = Some((roots, named));
    }
    if matches!(entry, Entry::Fuzz(_)) {
        // each driver's arguments, one after another
        let (per, named) = std::mem::take(&mut m.fuzz_kept);
        shapes = Some((per.into_iter().flatten().collect(), named));
    }
    // what `--trace` reads values by (the JIT's hooks print them)
    if m.tracing {
        jit::set_traced(std::mem::take(&mut m.traced));
    }
    PROOFS.with(|p| {
        let mut p = p.borrow_mut();
        p.1 += m.left_out;
        p.2 += m.kept;
    });
    let mut out = String::from("; Strata, compiled by `strata build`\n\n");
    out += &m.globals;
    out += "\n";
    out += &m.body;
    out += DECLARATIONS;
    for line in m.foreign.values() {
        out += line;
    }
    Ok((out, tests, shapes))
}

const DECLARATIONS: &str = "\
declare ptr @strata_alloc(i64)
declare void @strata_dup(ptr)
declare ptr @strata_scope_begin()
declare void @strata_scope_end(ptr)
declare ptr @strata_spawn(ptr, ptr, ptr, ptr, ptr, i64)
declare i1 @strata_was_unwound()
declare void @strata_panic(i32, ptr)
declare ptr @strata_channel_new(i64, ptr)
declare void @strata_channel_drop(ptr)
declare void @strata_channel_send(ptr, i64)
declare i1 @strata_channel_recv(ptr, ptr)
declare void @strata_channel_close(ptr)
declare i64 @strata_select(ptr, i64)
declare ptr @strata_await(ptr)
declare i1 @strata_cancelled()
declare void @strata_unwound()
declare void @strata_scope_cancel(ptr)
@strata_cancel_any = external global i32
declare void @strata_int_dup(i64)
declare void @strata_int_drop(i64)
; an `Int` outside [-2^62, 2^62) is a big integer, an object: only those are counted
define internal void @strata.int_dup(i64 %v) alwaysinline {
  %t = add i64 %v, 4611686018427387904
  %big = icmp slt i64 %t, 0
  br i1 %big, label %held, label %done
held:
  call void @strata_int_dup(i64 %v)
  ret void
done:
  ret void
}
define internal void @strata.int_drop(i64 %v) alwaysinline {
  %t = add i64 %v, 4611686018427387904
  %big = icmp slt i64 %t, 0
  br i1 %big, label %held, label %done
held:
  call void @strata_int_drop(i64 %v)
  ret void
done:
  ret void
}
declare i1 @strata_release(ptr)
declare void @strata_free(ptr)
declare i1 @strata_unique(ptr)
declare void @strata_init(i32, ptr)
declare i64 @strata_pid()
declare void @strata_host_write(i64, ptr)
declare ptr @strata_host_read_line()
declare ptr @strata_host_read_file(ptr, ptr, ptr)
declare ptr @strata_host_code_version()
declare ptr @strata_host_code_snapshot(ptr)
declare ptr @strata_host_read_bytes(ptr, ptr, ptr)
declare i1 @strata_host_write_file(ptr, ptr, i1, ptr, ptr)
declare i1 @strata_host_exists(ptr)
declare ptr @strata_host_list_dir(ptr, ptr, ptr)
declare i1 @strata_host_make_dir(ptr, ptr, ptr)
declare i1 @strata_host_remove(ptr, ptr, ptr)
declare ptr @strata_host_args()
declare i64 @strata_host_now()
declare void @strata_host_sleep(i64)
declare ptr @strata_prog_name()
declare void @strata_options_error(i1, ptr)
declare ptr @strata_host_env_var(ptr)
declare ptr @strata_concat(ptr, ptr)
declare i1 @strata_str_eq(ptr, ptr)
declare i64 @strata_str_cmp(ptr, ptr)
declare i64 @strata_str_len(ptr)
declare void @strata_fault(i32, ptr) noreturn
declare void @strata_enter(ptr)
declare void @strata_step()
declare void @strata_fuzz_keep(i64, i64)
declare void @strata_ct_keep(i64)
declare i64 @strata_given(i64)
declare void @strata_leave()
declare void @strata_trace_arg(i64, i64, i64)
declare void @strata_trace_call(i64)
declare void @strata_trace_return(i64, i64)
declare void @strata_overflow() noreturn cold
declare void @strata_main_error(ptr)
declare i64 @strata_div(i64, i64)
declare i64 @strata_int_add(i64, i64)
declare i64 @strata_int_sub(i64, i64)
declare i64 @strata_int_mul(i64, i64)
declare i64 @strata_int_cmp(i64, i64)
declare i1 @strata_int_eq(i64, i64)
declare i64 @strata_int_lit(ptr)
declare i64 @strata_int_from_i64(i64)
declare i64 @strata_int_and(i64, i64)
declare i64 @strata_int_or(i64, i64)
declare i64 @strata_int_xor(i64, i64)
declare i64 @strata_shl(i64, i64)
declare i64 @strata_shr(i64, i64)
declare i64 @strata_int_from_u64(i64)
declare i64 @strata_int_bits(i64)
declare ptr @strata_cstr(ptr, ptr)
declare void @strata_cstr_free(ptr)
declare ptr @strata_str_from_c(ptr, ptr)
declare i64 @strata_random(i64, i64)
declare double @strata_random_unit()
declare i64 @strata_gen_int(ptr, i64)
declare double @strata_gen_float(ptr, i64, ptr, i64)
declare i64 @strata_gen_len(ptr, i64)
declare ptr @strata_gen_string(i64)
declare double @strata_int_to_float(i64)
declare i64 @strata_mod(i64, i64)
declare double @strata_fmod(double, double)
declare double @strata_finite(double, double, double, ptr)
declare i64 @strata_float_to_int(double, double)
declare double @strata_sqrt(double)
declare ptr @strata_list_new(i64)
declare ptr @strata_list_push(ptr, i64, i64)
declare ptr @strata_list_slice(ptr, i64, i64)
declare ptr @strata_list_concat(ptr, ptr, i64)
declare ptr @strata_concat_owned(ptr, ptr)
declare ptr @strata_range(i64, i64)
declare i64 @llvm.smax.i64(i64, i64)
declare i64 @llvm.smin.i64(i64, i64)
declare ptr @strata_show_int(i64)
declare ptr @strata_json_int(i64)
declare ptr @strata_show_float(double)
declare ptr @strata_show_bool(i1)
declare ptr @strata_json_str(ptr)
declare ptr @strata_debug_str(ptr)
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
declare i64 @strata_str_byte_len(ptr)
declare i64 @strata_str_byte_at(ptr, i64)
declare ptr @strata_str_byte_slice(ptr, i64, i64)
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

/// A dispatcher: method `m` of trait `trm` with type arguments `targs`,
/// on a value of trait type `tr_ty`, by its table. `made`: how many tables
/// it covered when last made, and its text.
struct Dispatch {
    name: String,
    tr_ty: Ty,
    trm: String,
    m: String,
    targs: Vec<Ty>,
    made: Option<(usize, String)>,
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
    /// Capabilities the runtime makes for it (a test's), bound before the body.
    given: &'a [Param],
}

/// A constructor's fields: name (positional ones have none), written type,
/// and type, with the type's parameters filled in.
struct FieldInfo {
    name: Option<String>,
    texpr: TypeExpr,
    ty: Ty,
}

/// A type as C has it, for `extern c`.
#[derive(Clone, Copy, PartialEq, Debug)]
enum CType {
    Int { bits: u32, signed: bool },
    Double,
    Bool,
    Str,
    Handle,
    /// `Option[Handle]`: a pointer that may be NULL (`None`).
    OptHandle,
    Void,
}

impl CType {
    fn ll(self) -> String {
        match self {
            CType::Int { bits, .. } => format!("i{bits}"),
            CType::Double => "double".into(),
            CType::Bool => "i1".into(),
            CType::Str | CType::Handle | CType::OptHandle => "ptr".into(),
            CType::Void => "void".into(),
        }
    }
    /// As a result: its type after how it is widened (`zeroext i1`).
    fn ret(self) -> String {
        format!("{}{}", self.attr().trim_start(), if self.attr().is_empty() { self.ll() } else { format!(" {}", self.ll()) })
    }
    /// How C's calling convention widens it (as clang declares it).
    fn attr(self) -> &'static str {
        match self {
            CType::Bool => " zeroext",
            CType::Int { bits, signed: true } if bits < 32 => " signext",
            CType::Int { bits, signed: false } if bits < 32 => " zeroext",
            _ => "",
        }
    }
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
    /// How many constant lists and blocks there are (they are named by number).
    constants: usize,
    /// The function returning each literal pattern's matcher (`literal_patterns`).
    matchers: HashMap<String, String>,
    /// Each constant list or block by its contents, so equal ones are one.
    constant_of: HashMap<String, String>,
    /// Compiled functions.
    body: String,
    /// Functions still to compile (with their type arguments): those the compiled ones call.
    queue: VecDeque<Job>,
    seen: HashSet<String>,
    /// Generated helpers (equality, JSON) already emitted.
    helpers: HashSet<String>,
    /// Lambdas compiled so far (each is a function of its own).
    lambdas: usize,
    /// The tag of each type a union boxes (by its key): unique in the
    /// program, so a smaller union's values are a larger one's as they are.
    box_tags: HashMap<String, i64>,
    /// Compiled for `strata verify --jit`: loops and calls count steps.
    fuzzing: bool,
    /// The shapes of each fuzz driver's arguments (handed to the runtime as
    /// they are made: `strata_fuzz_keep`), and the named shapes they use.
    fuzz_kept: (Vec<Vec<Shape>>, Shapes),
    /// `--trace`: each traced function instance (its id is its place): its
    /// name, its arguments' shapes (`None`: no value) and its result's.
    traced: Traced,
    /// `strata run --trace`: each function of the program reports its calls and returns.
    tracing: bool,
    /// What `verify` proved, by function (`compile_proven`); empty: every check is kept.
    proofs: HashMap<String, Proofs>,
    /// Who refers to each function (`Refs`), when there are proofs.
    refs: Refs,
    /// The checks of the program's own functions left out as proven, and kept.
    left_out: usize,
    kept: usize,
    /// Generators made (by the type and hints they generate), and mock methods.
    gens: HashMap<String, String>,
    /// The tables made for each trait type (by its key), with the implementation.
    vtables: HashMap<String, Vec<(String, Ty)>>,
    /// Dispatchers for methods with type parameters of their own, called
    /// on trait values (made once every table is known).
    dispatch: Vec<Dispatch>,
    /// The C functions `extern c` declarations call: symbol -> `declare` line.
    foreign: std::collections::BTreeMap<String, String>,
    /// std's functions precompiled (`jit::std_library`): declared, not compiled.
    precompiled: Option<&'static std::collections::HashSet<String>>,
    /// Whether the program starts tasks (`spawn`, `race`): then a call that
    /// may wait is followed by a check for cancellation (`cancel_point`).
    tasks: bool,
}

thread_local! {
    /// Set while std's library is being made: nothing is precompiled yet.
    static BUILDING_STD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// `f` run as part of making std's library: code compiled meanwhile (std's
/// own `comptime` expressions, evaluated when std is resolved) doesn't wait
/// for the library being made, which would wait for itself.
pub fn building_std<T>(f: impl FnOnce() -> T) -> T {
    let was = BUILDING_STD.with(|b| b.replace(true));
    let r = f();
    BUILDING_STD.with(|b| b.set(was));
    r
}

/// Make std's library: `names` compiled, everything else they use kept
/// internal to it (helpers, lambdas, generic instances: each program has its own).
pub fn compile_library(prog: &Program, names: &[String]) -> Res<String> {
    let r = building_std(|| compile_with(prog, Entry::Library(names), false));
    let ir = r?.0;
    let exported: Vec<String> = names.iter().map(|n| format!("{}(", sym_impls(n, &[], &[]))).collect();
    Ok(ir
        .lines()
        .map(|l| if l.starts_with("define ") && !l.starts_with("define internal ") && !exported.iter().any(|e| l.contains(e.as_str())) { l.replacen("define ", "define internal ", 1) } else { l.to_string() })
        .collect::<Vec<_>>()
        .join("\n"))
}

/// Whether std's function `f` can be precompiled: not generic, nothing it
/// takes is a trait (compiled per implementation) or known only while compiling.
pub fn precompilable(f: &FnDecl, env: &Env) -> bool {
    let traitish = |t: &TypeExpr| matches!(t, TypeExpr::Named(n, _) if env.traits.contains_key(n));
    f.name.as_str().starts_with("std.")
        && !f.name.as_str().contains(".for.")
        && f.type_params.is_empty()
        && !crate::stdlib::is_host(&f.name)
        && !traitish(&f.ret)
        && f.params.iter().all(|p| !p.comptime && !traitish(&p.ty))
}

impl<'a> Module<'a> {
    fn want_impls(&mut self, name: &str, targs: Vec<Ty>, impls: Vec<Option<Ty>>) -> String {
        let s = sym_impls(name, &targs, &impls);
        // precompiled in std's library: declared, its code is the library's
        if targs.is_empty() && impls.iter().all(|i| i.is_none()) && self.precompiled.is_some_and(|p| p.contains(name)) {
            if let Some(line) = self.declaration(name, &s) {
                self.foreign.insert(s.clone(), line);
                return s;
            }
        }
        if self.seen.insert(s.clone()) {
            self.queue.push_back(Job::Fn(name.to_string(), targs, impls));
        }
        s
    }

    /// The `declare` line of function `name`, compiled elsewhere, as its definition's signature is.
    fn declaration(&self, name: &str, sym: &str) -> Option<String> {
        let f = self.env.fns.get(&Symbol::new(name))?;
        let mut ps = vec![];
        for p in &f.params {
            let t = self.lower(&p.ty, &HashMap::default()).ok()?;
            if p.mutable {
                ps.push("ptr".to_string());
            } else if has_value(&t) {
                ps.push(self.ll(&t).ok()?.to_string());
            }
        }
        let r = self.lower(&f.ret, &HashMap::default()).ok()?;
        let ret = if has_value(&r) { self.ll(&r).ok()? } else { "void" };
        Some(format!("declare {ret} {sym}({})\n", ps.join(", ")))
    }

    /// Whether a written type is a trait (a parameter that takes any implementation).
    fn is_trait(&self, t: &TypeExpr) -> bool {
        matches!(t, TypeExpr::Named(n, _) if self.traits.contains_key(n))
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
        let label = format!("{}.{}", mi.head, crate::names::short(&mi.method));
        let inst = Instance { label, symbol: mi.symbol(), impls: vec![], params: own.params.clone(), ret: own.ret.clone(), own_params: &own.params, own_ret: &own.ret, requires, ensures, body: &own.body, sub, given: &[] };
        self.instance(&inst)
    }

    /// A string constant, laid out as the runtime's `Str`: its length, then
    /// its bytes, after a count of 0 (never freed). The operand points past
    /// the count, as the runtime's objects do.
    fn string(&mut self, s: &str) -> String {
        if let Some(g) = self.strings.get(s) {
            return g.clone();
        }
        let g = format!("@.str.{}", self.strings.len());
        let at = format!("getelementptr inbounds (i8, ptr {g}, i64 8)");
        let n = s.len();
        let mut bytes = String::new();
        for b in s.bytes() {
            if b.is_ascii_graphic() && b != b'"' && b != b'\\' || b == b' ' {
                bytes.push(b as char);
            } else {
                write!(bytes, "\\{b:02X}").unwrap();
            }
        }
        writeln!(self.globals, "{g} = private unnamed_addr constant {{ i64, i64, [{n} x i8] }} {{ i64 0, i64 {n}, [{n} x i8] c\"{bytes}\" }}, align 8").unwrap();
        self.strings.insert(s.to_string(), at.clone());
        at
    }

    /// A value of a constructor without fields: one shared block holding its
    /// tag, after a count of 0 (never freed).
    fn nullary(&mut self, c: &str) -> String {
        let g = format!("@\"ctor:{c}\"");
        if self.helpers.insert(g.clone()) {
            writeln!(self.globals, "{g} = private unnamed_addr constant [2 x i64] [i64 0, i64 {}], align 8", self.tags[c]).unwrap();
        }
        format!("getelementptr inbounds (i8, ptr {g}, i64 8)")
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
            // a refined alias (`Positive`, kept by name in a type argument): its base
            Ty::Con(n, args) if crate::types::refined_alias(self.env, *n) => {
                let base = crate::types::alias_base(self.env, *n, args);
                self.repr(&base)?
            }
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
            Ty::Union(_) => "ptr",
            other => return Err(format!("the type `{}` is not compiled yet", key(&other))),
        })
    }

    /// The C type of a type an `extern c` declaration names (the checker
    /// allowed only these).
    fn c_type(&self, t: &TypeExpr) -> Res<CType> {
        Ok(match t {
            TypeExpr::Unit => CType::Void,
            TypeExpr::Named(n, _) => match n.as_str() {
                "I8" => CType::Int { bits: 8, signed: true },
                "I16" => CType::Int { bits: 16, signed: true },
                "I32" => CType::Int { bits: 32, signed: true },
                "I64" => CType::Int { bits: 64, signed: true },
                "U8" => CType::Int { bits: 8, signed: false },
                "U16" => CType::Int { bits: 16, signed: false },
                "U32" => CType::Int { bits: 32, signed: false },
                "U64" => CType::Int { bits: 64, signed: false },
                "Float" => CType::Double,
                "Bool" => CType::Bool,
                "String" => CType::Str,
                _ if self.env.externs.contains_key(n) => CType::Handle,
                "Option" if matches!(t, TypeExpr::Named(_, a) if matches!(a.as_slice(), [TypeExpr::Named(h, x)] if x.is_empty() && self.env.externs.contains_key(h))) => CType::OptHandle,
                other => return Err(format!("`{other}` doesn't cross to C")),
            },
            other => return Err(format!("`{}` doesn't cross to C", crate::printer::ty(other))),
        })
    }

    /// An `extern c type`: a pointer C made, never counted, read or freed here.
    /// A value shown and compared by what it is, not what is in it: a
    /// handle, or a channel's end (`<Sender>`; equal when the same channel).
    fn opaque(&self, t: &Ty) -> bool {
        self.handle(t) || matches!(t, Ty::Con(n, a) if (n == "Sender" || n == "Receiver") && a.len() == 1)
    }

    fn handle(&self, t: &Ty) -> bool {
        // (a task's handle: its scope holds the task, and frees it)
        matches!(t, Ty::Con(n, args) if args.is_empty() && (self.env.externs.contains_key(n) || self.env.capabilities.contains(n)) || n == "Task" && args.len() == 1)
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

    // ---- unions ----
    //
    // A union of records and variants is one of their blocks: its tag says
    // which. A union with other members (`Int | String`) is a pointer too:
    // a record's or a variant's block as it is, any other member in a box
    // `{ tag, value }` whose tag is the member type's (`box_tag`, beyond the
    // constructors' tags).

    /// Whether values of `t` are a union with boxed members.
    fn boxed(&self, t: &Ty) -> bool {
        matches!(self.repr(t), Ok(Ty::Union(ms)) if ms.iter().any(|m| self.boxes(m)))
    }

    /// Whether a union holds values of member type `m` in a box.
    fn boxes(&self, m: &Ty) -> bool {
        !self.nominal(&self.repr(m).unwrap_or(Ty::Unit))
    }

    /// The named union a union type is (`Shape` for `Circle | Square`), as
    /// the checker finds it (`Checker::union_name`): its methods are its own.
    fn union_name(&self, t: &Ty) -> Option<String> {
        // a sum counts as its variants (`Circle | Shape$own` is `Circle | Square | Dot`)
        let exact = |ms: Vec<Ty>| {
            let mut k: Vec<String> = ms.iter().flat_map(|m| match self.ctors_of(m) {
                cs if cs.is_empty() => vec![key(m)],
                cs => cs,
            }).collect();
            k.sort();
            k.dedup();
            k
        };
        let want = exact(self.members(t));
        if want.len() < 2 {
            return None;
        }
        let mut names: Vec<&Symbol> = self.env.types.iter().filter(|(_, td)| crate::types::union_alias(td)).map(|(n, _)| n).collect();
        names.sort();
        // one without type parameters, exactly; else a generic one, its
        // members paired by what they are (`List[T]` with `List[Int]`): the
        // checker made sure no two of its members are alike
        if let Some(n) = names.iter().find(|n| self.env.types[**n].params.is_empty() && self.lower(&TypeExpr::Named(***n, vec![]), &HashMap::default()).is_ok_and(|u| exact(self.members(&u)) == want)) {
            return Some(n.to_string());
        }
        let heads = |ms: Vec<Ty>| {
            let mut k: Vec<String> = ms.iter().flat_map(|m| match self.ctors_of(m) {
                cs if cs.is_empty() => vec![Module::head(m).unwrap_or_else(|| key(m))],
                cs => cs,
            }).collect();
            k.sort();
            k
        };
        let want = heads(self.members(t));
        names.into_iter().filter(|n| !self.env.types[*n].params.is_empty()).find(|n| {
            let ps = &self.env.types[*n].params;
            let sub: HashMap<Symbol, Ty> = ps.iter().map(|p| (*p, Ty::Unit)).collect();
            self.lower(&TypeExpr::Named(**n, ps.iter().map(|p| TypeExpr::Named(*p, vec![])).collect()), &sub).is_ok_and(|u| heads(self.members(&u)) == want)
        }).map(|n| n.to_string())
    }

    /// The members of a union type (itself, for any other type).
    fn members(&self, t: &Ty) -> Vec<Ty> {
        match self.repr(t) {
            Ok(Ty::Union(ms)) => ms.iter().flat_map(|m| self.members(m)).collect(),
            _ => vec![t.clone()],
        }
    }

    fn box_tag(&mut self, m: &Ty) -> i64 {
        let k = key(&self.repr(m).unwrap_or(Ty::Unit));
        // (the member type's, beyond every constructor's)
        let t = BOX_TAGS | (hash40(&k) & (BOX_TAGS - 1));
        *self.box_tags.entry(k).or_insert(t)
    }

    /// The tags a value of member type `m` can have in a union.
    fn member_tags(&mut self, m: &Ty) -> Vec<i64> {
        if self.boxes(m) {
            return vec![self.box_tag(m)];
        }
        let r = self.repr(m).unwrap_or(Ty::Unit);
        if matches!(&r, Ty::Con(n, _) if n == "Tuple") {
            return vec![0];
        }
        self.ctors_of(&r).iter().map(|c| self.tags[c]).collect()
    }

    /// Whether `m` is (or is part of) one of `u`'s members.
    fn member_of(&self, m: &Ty, u: &Ty) -> bool {
        let keys: Vec<String> = self.members(u).iter().map(|x| key(&self.repr(x).unwrap_or(Ty::Unit))).collect();
        self.members(m).iter().all(|x| keys.contains(&key(&self.repr(x).unwrap_or(Ty::Unit))))
    }

    /// How values of `t` are laid out (`Shape`), the named ones added to `named`.
    fn shape(&mut self, t: &Ty, named: &mut Shapes) -> Res<Shape> {
        if let Ty::Con(n, _) = t {
            if self.is_distinct(n) {
                let inner = self.repr(t)?;
                return Ok(Shape::Distinct(n.to_string(), Box::new(self.shape(&inner, named)?)));
            }
        }
        let r = self.repr(t)?;
        Ok(match &r {
            Ty::Int => Shape::Int,
            Ty::Float => Shape::Float,
            Ty::Bool => Shape::Bool,
            Ty::Unit => Shape::Unit,
            Ty::Str => Shape::Str,
            Ty::Con(n, a) if n == "List" && a.len() == 1 => Shape::List(Box::new(self.shape(&a[0], named)?)),
            Ty::Con(n, a) if n == "Set" && a.len() == 1 => Shape::Set(Box::new(self.shape(&a[0], named)?)),
            Ty::Con(n, a) if n == "Dict" && a.len() == 2 => Shape::Dict(Box::new(self.shape(&a[0], named)?), Box::new(self.shape(&a[1], named)?)),
            Ty::Con(n, a) if n == "Tuple" => Shape::Tuple(a.iter().map(|x| self.shape(x, named)).collect::<Res<_>>()?),
            Ty::Union(ms) => {
                let mut out = vec![];
                for m in ms.clone() {
                    let tag = self.boxes(&m).then(|| self.box_tag(&m));
                    out.push((tag, self.shape(&m, named)?));
                }
                Shape::Union(out)
            }
            Ty::Con(n, _) if self.nominal(&r) => {
                let k = key(&r);
                if !named.named.contains_key(&k) {
                    // (a recursive type refers to itself by its key)
                    named.named.insert(k.clone(), vec![]);
                    let record = matches!(self.env.types.get(n).map(|t| &t.body), Some(TypeBody::Record(_)));
                    let mut cs = vec![];
                    for c in self.ctors_of(&r) {
                        let mut fields = vec![];
                        for f in self.fields(&c, &self.ctor_args(&c, &r))? {
                            fields.push((f.name.clone(), self.shape(&f.ty, named)?));
                        }
                        cs.push(CtorShape { tag: self.tags[&c], name: c.clone(), record, fields });
                    }
                    named.named.insert(k.clone(), cs);
                }
                Shape::Named(k)
            }
            other => return Err(format!("a `{}` made at compile time can't be kept in the program", key(other))),
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
                    Payload::Type(_) => unreachable!("a union member defined elsewhere is desugared away"),
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
            given: &[],
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
                None if p.mutable && self.is_trait(&p.ty) => return Err(format!("`mut {}` takes any `{}`: not compiled yet in a method", p.name, crate::printer::ty(&p.ty))),
                // no implementation given (a method, compiled once): a trait
                // value, its methods called through its table
                None => params.push(self.lower(&p.ty, &sub)?),
            }
        }
        let ret = self.lower(&f.ret, &sub)?;
        let mut c = Func::new(self, sub);
        if !c.m.proofs.is_empty() && c.m.refs.own.contains(name) {
            c.proofs_of = Some(name.to_string());
        }
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
        // on the call stack a fault shows (the program's functions, as the interpreter's)
        let framed = !name.starts_with(PRELUDE_MODULE) && !name.starts_with("test \"") && name != COMPTIME;
        if framed {
            // a prelude trait's method by its short name (`ListIter.next`)
            let n = c.m.string(&crate::types::shown_name(name));
            c.emit(format!("call void @strata_enter(ptr {n})"));
        }
        if c.m.fuzzing {
            c.emit("call void @strata_step()".into());
        }
        // parameters live in slots, like every variable
        for (i, (p, t)) in f.params.iter().zip(&params).enumerate() {
            if p.mutable {
                let declared = own.own_params.get(i).map(|q| q.ty.clone());
                c.scopes.last_mut().unwrap().vars.insert(p.name.to_string(), Local { t: t.clone(), slot: format!("%p.{}", p.name), declared });
                continue;
            }
            let slot = c.local(&p.name, t, None)?;
            c.store(&(t.clone(), format!("%p.{}", p.name)), &slot)?;
        }
        // what a test asks for: a capability holds nothing (the runtime's is a null
        // pointer), `Console` is the terminal (a test's output is its own), the run
        // mode is `Testing`
        for p in inst.given {
            let t = c.m.lower(&p.ty, &HashMap::default())?;
            let wanted = crate::printer::ty(&p.ty);
            let v = if wanted == crate::cmd::CONSOLE {
                c.coerce((Ty::Con(Symbol::new("std.io.Terminal"), vec![]), "null".into()), &t)?
            } else if wanted == crate::cmd::RUN_MODE {
                (t.clone(), c.m.nullary("std.process.Testing"))
            } else {
                (t.clone(), "null".into())
            };
            let v = c.take(v);
            let slot = c.local(&p.name, &t, None)?;
            c.store(&v, &slot)?;
        }
        if c.m.tracing && framed && !name.starts_with("strata_") {
            // each argument handed to strata (`strata_trace_arg`, read by its shape), then the call
            let id = c.m.traced.fns.len();
            let mut named = std::mem::take(&mut c.m.traced.named);
            let mut shapes = vec![];
            for (k, p) in f.params.iter().enumerate() {
                let v = c.load(&p.name)?;
                if has_value(&v.0) {
                    shapes.push(Some(c.m.shape(&v.0, &mut named)?));
                    let slot = c.as_slot(&v)?;
                    c.emit(format!("call void @strata_trace_arg(i64 {id}, i64 {k}, i64 {slot})"));
                } else {
                    shapes.push(None);
                }
            }
            let result = if has_value(&ret) { Some(c.m.shape(&ret, &mut named)?) } else { None };
            c.m.traced.named = named;
            c.m.traced.fns.push((name.to_string(), shapes, result));
            c.emit(format!("call void @strata_trace_call(i64 {id})"));
            c.trace_id = Some(id);
        }
        // the written types of the program's own declaration (inline refinements are its nodes)
        for p in own.own_params {
            let v = c.load(&p.name)?;
            if has_value(&v.0) && c.needs_check(&p.ty) && c.proven_at_calls(&crate::types::shown_name(&format!("argument `{}` of `{name}`", p.name))) {
                continue;
            }
            c.check(&p.ty, &v, &format!("argument `{}` of `{name}`", p.name), false)?;
        }
        for r in own.requires.clone() {
            if c.proven_at_calls(&crate::types::shown_name(&format!("precondition of `{name}`: requires {}", crate::printer::expr(r, 0)))) {
                continue;
            }
            c.push_scope(true);
            let ok = c.expr(r)?;
            c.pop_scope()?;
            let (pass, fail) = (c.fresh_label("requires.ok"), c.fresh_label("requires.fail"));
            c.cond_br(&ok, &pass, &fail);
            c.block(&fail);
            let mut parts = vec![Part::Lit(crate::types::shown_name(&format!("precondition of `{name}` violated: requires {} (with ", crate::printer::expr(r, 0))))];
            for (i, p) in f.params.iter().enumerate() {
                let v = c.load(&p.name)?;
                let shown = c.shown(&v)?;
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
        let v = if c.m.env.externs.contains_key(&Symbol::new(name)) { c.foreign_call(name, own.own_params, own.own_ret)? } else { c.expr(own.body)? };
        c.return_value(&v)?;
        // every way out: the result's refinement, then the postconditions
        // (each way out dropped what the body held)
        c.block("exit");
        c.in_exit = true;
        c.forget_drops();
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
            c.push_scope(false);
            let slot = c.local("it", &ret, None)?;
            c.store(&result, &slot)?;
            for en in own.ensures.clone() {
                let k = format!("ensures {}", crate::printer::expr(en, 0));
                if c.proven(|e| same_check(e, &k)) {
                    continue;
                }
                c.push_scope(true);
                let ok = c.expr(en)?;
                c.pop_scope()?;
                let (pass, fail) = (c.fresh_label("ensures.ok"), c.fresh_label("ensures.fail"));
                c.cond_br(&ok, &pass, &fail);
                c.block(&fail);
                let shown = c.shown(&result)?;
                c.fault(
                    Fault::Ensures,
                    vec![Part::Lit(format!("postcondition of `{name}` violated: ensures {} (it = ", crate::printer::expr(en, 0))), Part::Val(shown), Part::Lit(")".into())],
                )?;
                c.block(&pass);
            }
            c.pop_scope()?;
        }
        if let Some(id) = c.trace_id {
            let slot = if has_value(&ret) { c.as_slot(&result)? } else { "0".into() };
            c.emit(format!("call void @strata_trace_return(i64 {id}, i64 {slot})"));
        }
        if framed {
            c.emit("call void @strata_leave()".into());
        }
        if has_value(&ret) {
            c.emit(format!("ret {} {}", c.m.ll(&ret)?, result.1));
        } else {
            c.emit("ret void".into());
        }
        c.unwound_exit(&ret, framed)?;
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
            if crate::cli::is_options(c.m.env, &p.ty) {
                let v = c.options(&p.ty)?;
                if p.mutable {
                    // the slot holds the options (main may replace them)
                    let slot = c.fresh("opts");
                    writeln!(c.allocas, "  {slot} = alloca {}", c.m.ll(&v.0)?).unwrap();
                    let v = c.take(v);
                    c.emit(format!("store {} {}, ptr {slot}", c.m.ll(&v.0)?, v.1));
                    c.own_slot(&v.0, &slot);
                    args.push((Ty::Str, slot));
                } else {
                    args.push(v);
                }
                impls.push(None);
                continue;
            }
            // a capability of the program (`capability Sqlite { ... }`): it holds nothing
            if let TypeExpr::Named(n, a) = &p.ty {
                if a.is_empty() && c.m.env.capabilities.contains(n) {
                    let t = Ty::Con(*n, vec![]);
                    if p.mutable {
                        let slot = c.fresh("cap");
                        writeln!(c.allocas, "  {slot} = alloca ptr").unwrap();
                        c.emit(format!("store ptr null, ptr {slot}"));
                        args.push((Ty::Str, slot));
                    } else {
                        args.push((t, "null".into()));
                    }
                    impls.push(None);
                    continue;
                }
            }
            // how it runs, when the package lets it ask: `Running`, or `Tracing` under --trace
            if wanted == crate::cmd::RUN_MODE && crate::cmd::run_mode_allowed() {
                let which = if c.m.tracing { "std.process.Tracing" } else { "std.process.Running" };
                let v = (Ty::Con(Symbol::new(crate::cmd::RUN_MODE), vec![]), c.m.nullary(which));
                if p.mutable {
                    let slot = c.fresh("mode");
                    writeln!(c.allocas, "  {slot} = alloca ptr").unwrap();
                    c.emit(format!("store ptr {}, ptr {slot}", v.1));
                    args.push((Ty::Str, slot));
                } else {
                    args.push(v);
                }
                impls.push(None);
                continue;
            }
            // `Console`: the one capability the environment chooses, the terminal here
            if wanted != crate::cmd::CONSOLE || !c.m.env.capabilities.contains(&Symbol::new("std.io.Terminal")) {
                return Err(crate::cmd::not_given(&wanted, &format!("`{entry}`")) + &format!(" Its parameter `{}` is a `{wanted}`", p.name));
            }
            let t = Ty::Con(Symbol::new("std.io.Terminal"), vec![]);
            if p.mutable {
                let slot = c.fresh("cap");
                writeln!(c.allocas, "  {slot} = alloca ptr").unwrap();
                c.emit(format!("store ptr null, ptr {slot}"));
                args.push((Ty::Str, slot));
            } else {
                args.push((t.clone(), "null".into()));
            }
            impls.push(Some(t));
        }
        let f = c.m.want_impls(entry, vec![], impls);
        // before each way out: drop the capabilities (and the result)
        match ret {
            Ty::Unit => {
                c.call(&Ty::Unit, &f, &args)?;
                c.drop_from(0, None)?;
                c.emit("ret i32 0".into());
            }
            Ty::Int => {
                let r = c.call(&Ty::Int, &f, &args)?;
                c.drop_from(0, None)?;
                c.emit(format!("%r = add i64 {}, 0", r.1));
                c.emit("%c = trunc i64 %r to i32".into());
                c.emit("ret i32 %c".into());
            }
            Ty::Con(n, rargs) if n == "Result" && matches!(rargs[0], Ty::Unit | Ty::Int) => {
                let args0 = c.call(ret, &f, &args)?;
                c.temp(args0.clone());
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
                    c.drop_from(0, None)?;
                    c.emit(format!("ret i32 {code}"));
                } else {
                    c.drop_from(0, None)?;
                    c.emit("ret i32 0".into());
                }
                c.block("err");
                let e = c.field(&r, 0, &args[1])?;
                let shown = c.show(&e)?;
                c.emit(format!("call void @strata_main_error(ptr {shown})"));
                c.drop_value(&(Ty::Str, shown))?;
                c.drop_from(0, None)?;
                c.emit("ret i32 1".into());
            }
            other => return Err(format!("`{entry}` must return `()`, an exit code (`Int`) or a `Result` of either, not `{}`", key(other))),
        }
        let text = c.finish();
        writeln!(self.body, "define i32 @main(i32 %argc, ptr %argv) {{\n{text}}}\n").unwrap();
        Ok(())
    }
}

impl<'a> Module<'a> {
    /// `Entry::Calls`' function `i`: `name`'s arguments read from the host's
    /// slots, the call, its result kept, the arguments dropped but those
    /// lent. The result's type, then the parameters'.
    fn call_entry(&mut self, i: usize, name: &str, f: &FnDecl, how: &[Arg]) -> Res<Vec<Ty>> {
        if !f.type_params.is_empty() {
            return Err(format!("`{name}` is generic"));
        }
        let mut c = Func::new(self, HashMap::default());
        c.block("start");
        let ret = c.m.lower(&f.ret, &HashMap::default())?;
        let mut tys = vec![ret.clone()];
        let mut args = vec![];
        for (k, p) in f.params.iter().enumerate() {
            let t = c.m.lower(&p.ty, &HashMap::default())?;
            tys.push(t.clone());
            let raw = c.fresh("given");
            c.emit(format!("{raw} = call i64 @strata_given(i64 {k})"));
            let v = c.from_slot(&raw, &t)?;
            if how[k] == Arg::Given {
                c.validate(&v)?;
            }
            let lent = how[k] == Arg::Lent;
            if p.mutable {
                let slot = c.fresh("arg");
                writeln!(c.allocas, "  {slot} = alloca {}", c.m.ll(&t)?).unwrap();
                if has_value(&t) {
                    c.emit(format!("store {} {}, ptr {slot}", c.m.ll(&t)?, v.1));
                }
                // (what a lent one holds after the call stays the host's: a capability)
                if !lent {
                    c.own_slot(&t, &slot);
                }
                args.push((Ty::Str, slot));
            } else if lent {
                args.push(v);
            } else {
                args.push(c.temp(v));
            }
        }
        let sym = c.m.want_impls(name, vec![], vec![None; f.params.len()]);
        let r = c.call(&ret, &sym, &args)?;
        let slot = if has_value(&ret) { c.to_slot(&r)? } else { "0".into() };
        // (the result is the host's now: not dropped)
        c.emit(format!("call void @strata_ct_keep(i64 {slot})"));
        c.drop_from(0, None)?;
        c.emit("ret void".into());
        let text = c.finish();
        writeln!(self.body, "define void @\"strata.ct.{i}\"() {{\n{text}}}\n").unwrap();
        Ok(tys)
    }
}

impl Func<'_, '_> {
    /// `main`'s options, read from the arguments by the derived
    /// `from_args` (std.cli): `--help` prints the help and exits with 0, bad
    /// arguments print the message and exit with 2, as `strata run` does.
    fn options(&mut self, ty: &TypeExpr) -> Res<V> {
        let recv = self.lower(ty)?;
        let err_ty = Ty::Con(Symbol::new("std.cli.CliError"), vec![]);
        let res = Ty::Con(Symbol::new("Result"), vec![recv.clone(), err_ty.clone()]);
        let tm = self.m.traits.get(&Symbol::new(crate::cli::TRAIT)).and_then(|t| t.methods.iter().find(|m| crate::names::short(&m.decl.name) == "from_args")).ok_or("std.cli has no `from_args`")?;
        let name = crate::types::trait_call(crate::cli::TRAIT, &tm.decl.name);
        let prog = self.call(&Ty::Str, "@strata_prog_name", &[])?;
        let prog = self.temp(prog);
        let argv = self.call(&Ty::Con(Symbol::new("List"), vec![Ty::Str]), "@strata_host_args", &[])?;
        let argv = self.temp(argv);
        let r = self.call_method(&name, recv.clone(), vec![prog, argv], Some(res.clone()), &[])?;
        let r = (res, r.1);
        let (ok, err, help, bad) = (self.fresh_label("opts_ok"), self.fresh_label("opts_err"), self.fresh_label("opts_help"), self.fresh_label("opts_bad"));
        let tag = self.tag(&r);
        let is_ok = self.fresh("ok");
        self.emit(format!("{is_ok} = icmp eq i64 {tag}, {}", self.m.tags["Ok"]));
        self.emit(format!("br i1 {is_ok}, label %{ok}, label %{err}"));
        self.block(&err);
        let e = self.field(&r, 0, &err_ty)?;
        let etag = self.tag(&e);
        let is_help = self.fresh("help");
        let help_tag = *self.m.tags.get("std.cli.ShowHelp").ok_or("std.cli has no `ShowHelp`")?;
        self.emit(format!("{is_help} = icmp eq i64 {etag}, {help_tag}"));
        self.emit(format!("br i1 {is_help}, label %{help}, label %{bad}"));
        for (label, code) in [(&help, 0), (&bad, 2)] {
            self.block(label);
            let text = self.field(&e, 0, &Ty::Str)?;
            self.emit(format!("call void @strata_options_error(i1 {}, ptr {})", code == 0, text.1));
            self.drop_from(0, None)?;
            self.emit(format!("ret i32 {code}"));
        }
        self.block(&ok);
        self.field(&r, 0, &recv)
    }
}

/// A piece of a message built at run time.
#[derive(Clone)]
enum Part {
    Lit(String),
    /// An operand holding a string.
    Val(String),
    /// An `Int` operand, shown only if the message is made (a fault).
    Int(String),
}

/// A local variable: its slot, and the declared type every assignment must fit.
#[derive(Clone)]
struct Local {
    t: Ty,
    slot: String,
    declared: Option<TypeExpr>,
}

/// A scope: its variables, and what it drops when it ends.
///
/// Memory is reference-counted (runtime.c, "memory"). The conventions:
/// - a function borrows its arguments (the caller keeps them alive for the
///   call) and returns a value it owns, which the caller then owns;
/// - reading a variable, a field or an element borrows it (no counting);
/// - a value stored somewhere (a variable, a field, a list, a closure, the
///   result) is held once more there, unless it was a temporary of the
///   current scope, which moves (`take`);
/// - a scope drops, when it ends, its temporaries (values made in it that
///   nothing holds) and the variables it owns; `return`, `break` and
///   `continue` drop what they leave (`drop_from`).
///
/// Each branch, loop iteration and statement is a scope, so a temporary is
/// always dropped in code its making dominates.
#[derive(Default)]
struct Scope {
    vars: HashMap<String, Local>,
    /// Slots of the variables it owns.
    owned: Vec<(Ty, String)>,
    /// Values made in it that nothing holds yet.
    temps: Vec<V>,
    /// A statement's or a branch's: its variables belong to the enclosing scope.
    transparent: bool,
    /// A `scope { … }`'s: the runtime's record of its tasks, joined when it ends.
    tasks: Option<String>,
}

/// An operand that is a constant (never counted, never freed).
fn constant(op: &str) -> bool {
    // (a number is a small `Int`, never counted)
    op.starts_with('@') || op.starts_with("getelementptr") || op == "null" || op == "poison" || op.is_empty() || op.parse::<i64>().is_ok()
}

struct Func<'m, 'a> {
    m: &'m mut Module<'a>,
    /// The type arguments of the instance being compiled.
    sub: HashMap<Symbol, Ty>,
    /// The type arguments of the generic call being compiled, as written.
    named_targs: Option<Vec<(Symbol, Ty)>>,
    /// Slots of the variables, in the entry block (so LLVM keeps them in registers).
    allocas: String,
    code: String,
    tmp: usize,
    label: usize,
    scopes: Vec<Scope>,
    /// The enclosing loops: where `continue` and `break` go, and how many
    /// scopes were open outside the loop (what they leave).
    loops: Vec<(String, String, usize)>,
    /// The labels patterns jump to when they don't match, with how many of
    /// the scope's owned variables were bound before (what the jump drops).
    fail_marks: HashMap<String, usize>,
    /// The result's type and slot.
    ret_slot: Option<(Ty, String)>,
    /// `--trace`: this function's id among the traced (it reports its return).
    trace_id: Option<usize>,
    /// The current block, and whether it has ended (with a jump or `ret`).
    cur: String,
    terminated: bool,
    /// The places of the arguments of the call being compiled (for `mut` parameters).
    places: Vec<Option<(String, Vec<String>)>>,
    /// The program's function being compiled, whose proofs `proven` reads
    /// (none in a lambda, a helper, or std's and the prelude's code).
    proofs_of: Option<String>,
    /// Whether a cancellation check leaves through the `unwound` block.
    unwinds: bool,
    /// In the function's `exit` block (its result's checks): nothing left to
    /// drop there, so a panic can't leave as a cancellation does.
    in_exit: bool,
}

impl<'m, 'a> Func<'m, 'a> {
    fn new(m: &'m mut Module<'a>, sub: HashMap<Symbol, Ty>) -> Self {
        Func { m, sub, named_targs: None, allocas: String::new(), code: String::new(), tmp: 0, label: 0, scopes: vec![Scope::default()], loops: vec![], fail_marks: HashMap::default(), ret_slot: None, trace_id: None, cur: String::new(), terminated: true, places: vec![], unwinds: false, in_exit: false, proofs_of: None }
    }

    /// The function's text: the slots, then the code.
    fn finish(&mut self) -> String {
        if !self.terminated {
            self.emit("unreachable".into());
        }
        let first = self.code.lines().next().unwrap_or("").trim_end_matches(':').to_string();
        format!("entry:\n{}  br label %{first}\n{}", self.allocas, self.code)
    }

    /// After a call that may wait (a task's `recv`, `send`, `select`,
    /// `await`, or a function that may do one): if this task was cancelled,
    /// leave the function as `return` does, dropping what its scopes hold;
    /// its caller does the same after it. The result isn't stored: callers
    /// check before they use it. A program without tasks has no checks, and
    /// one with them reads a global (no cancellation pending: one branch).
    fn cancel_point(&mut self) -> Res<()> {
        if !self.m.tasks || self.ret_slot.is_none() || self.terminated {
            return Ok(());
        }
        let (any, maybe, go, out) = (self.fresh("cancels"), self.fresh_label("cancel.maybe"), self.fresh_label("cancel.go"), self.fresh_label("cancel.out"));
        let z = self.fresh("cancelling");
        self.emit(format!("{any} = load atomic i32, ptr @strata_cancel_any monotonic, align 4"));
        self.emit(format!("{z} = icmp ne i32 {any}, 0"));
        self.emit(format!("br i1 {z}, label %{maybe}, label %{go}, !prof !{{!\"branch_weights\", i32 1, i32 2000}}"));
        self.block(&maybe);
        let c = self.fresh("cancelled");
        self.emit(format!("{c} = call i1 @strata_cancelled()"));
        self.emit(format!("br i1 {c}, label %{out}, label %{go}"));
        self.block(&out);
        self.drop_from(0, None)?;
        self.br("unwound");
        self.unwinds = true;
        self.block(&go);
        Ok(())
    }

    /// Where a wait gave up because this task was cancelled (`strata_select`
    /// gave -1): leave as `cancel_point` does.
    fn unwind_here(&mut self) -> Res<()> {
        if !self.m.tasks || self.ret_slot.is_none() {
            self.emit("unreachable".into());
            return Ok(());
        }
        self.drop_from(0, None)?;
        self.br("unwound");
        self.unwinds = true;
        Ok(())
    }

    /// The way out taken by `cancel_point`: nothing stored, a zero value
    /// returned (never used), the thread marked as unwinding.
    fn unwound_exit(&mut self, ret: &Ty, framed: bool) -> Res<()> {
        if !self.unwinds {
            return Ok(());
        }
        self.block("unwound");
        self.emit("call void @strata_unwound()".into());
        if framed {
            self.emit("call void @strata_leave()".into());
        }
        if has_value(ret) {
            let ll = self.m.ll(ret)?;
            self.emit(format!("ret {ll} zeroinitializer"));
        } else {
            self.emit("ret void".into());
        }
        Ok(())
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

    /// `lower`, keeping refined aliases by name (`List[Positive]`): a type
    /// argument, which says which refinements its values have.
    fn lower_named(&self, t: &TypeExpr) -> Res<Ty> {
        match t {
            TypeExpr::Named(n, a) if crate::types::refined_alias(self.m.env, *n) => Ok(Ty::Con(*n, a.iter().map(|x| self.lower_named(x)).collect::<Res<_>>()?)),
            TypeExpr::Named(_, a) if !a.is_empty() => match self.lower(t)? {
                Ty::Con(h, la) if la.len() == a.len() => Ok(Ty::Con(h, a.iter().map(|x| self.lower_named(x)).collect::<Res<_>>()?)),
                other => Ok(other),
            },
            _ => self.lower(t),
        }
    }

    /// A type as written (for checking a value against it): `Positive`, `List[Int]`.
    fn written_of(t: &Ty) -> TypeExpr {
        match t {
            Ty::Int => TypeExpr::Named("Int".into(), vec![]),
            Ty::Float => TypeExpr::Named("Float".into(), vec![]),
            Ty::Bool => TypeExpr::Named("Bool".into(), vec![]),
            Ty::Str => TypeExpr::Named("String".into(), vec![]),
            Ty::Con(n, a) => TypeExpr::Named(*n, a.iter().map(Self::written_of).collect()),
            _ => TypeExpr::Unit,
        }
    }

    /// The type a type argument stands for, known here: its text
    /// (`"List[Int]"`), a type parameter of this instance (`type:T`), or
    /// `$type(head, args..)`.
    fn type_arg_ty(&self, e: &Expr, hint: Option<&Ty>) -> Res<Ty> {
        match e {
            Expr::Str(h) => {
                let t = crate::types::parse_type_text(h);
                match (&t, hint) {
                    // a generic type named by its head only: the call's own type says the rest
                    (TypeExpr::Named(n, args), Some(ht @ Ty::Con(m, _))) if args.is_empty() && n == m => Ok(ht.clone()),
                    _ => self.lower_named(&t),
                }
            }
            Expr::Var(x) if x.starts_with("type:") => self.sub.get(&Symbol::new(&x["type:".len()..])).cloned().ok_or(format!("the type `{x}` is not known here")),
            Expr::Call(f, args) if matches!(&**f, Expr::Var(n) if n == crate::types::TYPE_TEXT) => {
                let Some(Expr::Str(h)) = args.first() else { return Err("a type's text without its head".into()) };
                let targs = args[1..].iter().map(|a| self.type_arg_ty(a, None)).collect::<Res<Vec<_>>>()?;
                let head = self.lower(&TypeExpr::Named(Symbol::new(h), vec![]))?;
                Ok(match head {
                    Ty::Con(n, _) => Ty::Con(n, targs),
                    other => other,
                })
            }
            _ => Err("a type argument that isn't known here".into()),
        }
    }

    /// A variable that borrows its value (a parameter, `it`, a captured value).
    fn local(&mut self, name: &str, t: &Ty, declared: Option<TypeExpr>) -> Res<String> {
        let slot = self.fresh(&format!("v.{}", name.replace(|c: char| !c.is_ascii_alphanumeric() && c != '_', "_")));
        if has_value(t) {
            writeln!(self.allocas, "  {slot} = alloca {}", self.m.ll(t)?).unwrap();
        }
        let i = self.var_scope();
        self.scopes[i].vars.insert(name.to_string(), Local { t: t.clone(), slot: slot.clone(), declared });
        Ok(slot)
    }

    /// A variable holding `v`, which it owns (held once more, or moved).
    fn local_owned(&mut self, name: &str, v: V, declared: Option<TypeExpr>) -> Res<()> {
        let slot = self.local(name, &v.0, declared)?;
        let v = self.take(v);
        self.store(&v, &slot)?;
        self.own_slot(&v.0, &slot);
        Ok(())
    }

    /// The scope variables go in: the innermost that isn't a statement's or a branch's.
    fn var_scope(&self) -> usize {
        self.scopes.iter().rposition(|s| !s.transparent).unwrap_or(0)
    }

    /// The variable scope drops the slot's value when it ends.
    fn own_slot(&mut self, t: &Ty, slot: &str) {
        if self.counted(t) {
            let i = self.var_scope();
            if !self.scopes[i].owned.iter().any(|(_, s)| s == slot) {
                self.scopes[i].owned.push((t.clone(), slot.to_string()));
            }
        }
    }

    fn lookup(&self, name: &str) -> Option<Local> {
        self.scopes.iter().rev().find_map(|s| s.vars.get(name).cloned())
    }

    // ---- foreign calls ----

    /// The body of an `extern c` function: its arguments converted to C's
    /// types, the call, its result converted back. A `mut` parameter is an
    /// out-pointer: C's copy of the value, read back after the call.
    fn foreign_call(&mut self, name: &str, params: &[Param], ret: &TypeExpr) -> Res<V> {
        let symbol = crate::names::short(name).to_string();
        let callee = self.m.string(&symbol);
        let mut args = vec![];
        let mut frees = vec![];
        let mut outs = vec![];
        for p in params {
            let ct = self.m.c_type(&p.ty)?;
            let v = self.load(&p.name)?;
            let c = self.to_c(&v, ct, &callee)?;
            if ct == CType::Str {
                frees.push(c.clone());
            }
            if p.mutable {
                let slot = self.fresh("out");
                writeln!(self.allocas, "  {slot} = alloca {}", ct.ll()).unwrap();
                self.emit(format!("store {} {c}, ptr {slot}", ct.ll()));
                args.push(format!("ptr {slot}"));
                outs.push((p.name.to_string(), ct, slot, v));
            } else {
                args.push(format!("{}{} {c}", ct.ll(), ct.attr()));
            }
        }
        let rt = self.m.c_type(ret)?;
        let ret_ty = self.m.lower(ret, &HashMap::default())?;
        let decl = format!("declare {} @{symbol}({})\n", rt.ret(), params.iter().map(|p| self.m.c_type(&p.ty).map(|t| if p.mutable { "ptr".to_string() } else { format!("{}{}", t.ll(), t.attr()) })).collect::<Res<Vec<_>>>()?.join(", "));
        match self.m.foreign.get(&symbol) {
            Some(d) if *d != decl => return Err(format!("`{symbol}` is declared twice with different C types")),
            _ => {
                self.m.foreign.insert(symbol.clone(), decl);
            }
        }
        let r = if rt == CType::Void {
            self.emit(format!("call void @{symbol}({})", args.join(", ")));
            String::new()
        } else {
            let r = self.fresh("c");
            self.emit(format!("{r} = call {} @{symbol}({})", rt.ret(), args.join(", ")));
            r
        };
        for c in frees {
            self.emit(format!("call void @strata_cstr_free(ptr {c})"));
        }
        // what C wrote through the out-pointers, back in the caller's variables
        for (name, ct, slot, old) in outs {
            let raw = self.fresh("o");
            self.emit(format!("{raw} = load {}, ptr {slot}", ct.ll()));
            let v = self.from_c(&raw, ct, &old.0, &callee, &symbol)?;
            let v = self.take(v);
            self.drop_value(&old)?;
            let l = self.lookup(&name).ok_or(format!("no parameter `{name}`"))?;
            self.store(&v, &l.slot)?;
        }
        if rt == CType::Void {
            return Ok((ret_ty, String::new()));
        }
        let v = self.from_c(&r, rt, &ret_ty, &callee, &symbol)?;
        Ok(self.temp(v))
    }

    /// A value as C has it (a `String`: a copy to free after the call).
    fn to_c(&mut self, v: &V, ct: CType, callee: &str) -> Res<String> {
        let r = self.fresh("toc");
        match ct {
            CType::Int { bits: 64, .. } => self.emit(format!("{r} = call i64 @strata_int_bits(i64 {})", v.1)),
            CType::Int { bits, .. } => self.emit(format!("{r} = trunc i64 {} to i{bits}", v.1)),
            CType::Str => self.emit(format!("{r} = call ptr @strata_cstr(ptr {}, ptr {callee})", v.1)),
            CType::Double | CType::Bool | CType::Handle => return Ok(v.1.clone()),
            CType::OptHandle => {
                // `Some(h)` is `h`, `None` is NULL
                let Ty::Con(_, args) = &v.0 else { return Err("an `Option` of a handle expected".into()) };
                let h = args[0].clone();
                let tag = self.tag(v);
                let is = self.fresh("some");
                self.emit(format!("{is} = icmp eq i64 {tag}, {}", self.m.tags["Some"]));
                let (some, none, end) = (self.fresh_label("c.some"), self.fresh_label("c.none"), self.fresh_label("c.opt"));
                self.cond_br(&bool_v(&is), &some, &none);
                self.block(&some);
                let f = self.field(v, 0, &h)?;
                let from = self.cur.clone();
                self.br(&end);
                self.block(&none);
                self.br(&end);
                self.block(&end);
                self.emit(format!("{r} = phi ptr [ {}, %{from} ], [ null, %{none} ]", f.1));
            }
            CType::Void => return Ok(String::new()),
        }
        Ok(r)
    }

    /// A value C gave back, as a value of type `t`.
    fn from_c(&mut self, raw: &str, ct: CType, t: &Ty, callee: &str, symbol: &str) -> Res<V> {
        let r = self.fresh("fromc");
        match ct {
            CType::Int { bits: 64, signed: true } => self.emit(format!("{r} = call i64 @strata_int_from_i64(i64 {raw})")),
            CType::Int { bits: 64, signed: false } => self.emit(format!("{r} = call i64 @strata_int_from_u64(i64 {raw})")),
            CType::Int { bits, signed: true } => self.emit(format!("{r} = sext i{bits} {raw} to i64")),
            CType::Int { bits, signed: false } => self.emit(format!("{r} = zext i{bits} {raw} to i64")),
            CType::Str => self.emit(format!("{r} = call ptr @strata_str_from_c(ptr {raw}, ptr {callee})")),
            CType::Double | CType::Bool => return Ok((t.clone(), raw.to_string())),
            CType::Handle => {
                // NULL is no handle: such a result is declared `Option[...]`
                let (bad, good) = (self.fresh_label("c.null"), self.fresh_label("c.handle"));
                self.emit(format!("{r} = icmp eq ptr {raw}, null"));
                self.cond_br(&bool_v(&r), &bad, &good);
                self.block(&bad);
                let shown = key(t);
                let shown = crate::names::short(&shown);
                let msg = format!("`{symbol}` gave NULL for a `{shown}` (declare it `Option[{shown}]` if it can be NULL)");
                self.fault(Fault::Runtime, vec![Part::Lit(msg)])?;
                self.block(&good);
                return Ok((t.clone(), raw.to_string()));
            }
            CType::OptHandle => {
                let Ty::Con(_, args) = t else { return Err("an `Option` of a handle expected".into()) };
                return self.optional(&(args[0].clone(), raw.to_string()), t);
            }
            CType::Void => return Ok((t.clone(), String::new())),
        }
        Ok((t.clone(), r))
    }

    // ---- reference counting ----

    /// Whether values of type `t` are counted objects (pointers to the heap).
    /// An `Int` is: a big one is an object (a small one, most of them, is
    /// skipped by the inline test of `@strata.int_dup` / `@strata.int_drop`).
    fn counted(&self, t: &Ty) -> bool {
        has_value(t) && !is_never(t) && !self.m.handle(t) && !matches!(self.m.repr(t), Ok(Ty::Float | Ty::Bool | Ty::Unit) | Err(_))
    }

    fn is_int(&self, t: &Ty) -> bool {
        matches!(self.m.repr(t), Ok(Ty::Int))
    }

    /// One holder more of `v` (counted).
    fn dup_value(&mut self, v: &V) {
        if self.is_int(&v.0) {
            self.emit(format!("call void @strata.int_dup(i64 {})", v.1));
        } else {
            self.emit(format!("call void @strata_dup(ptr {})", v.1));
        }
    }

    /// What the runtime's list operations do with the elements they copy:
    /// 0 nothing, 1 hold each (objects), 2 hold the big ones (`Int`).
    fn elements_kind(&self, et: &Ty) -> V {
        let k = if !self.counted(et) { 0 } else if self.is_int(et) { 2 } else { 1 };
        (Ty::Int, k.to_string())
    }

    fn push_scope(&mut self, transparent: bool) {
        self.scopes.push(Scope { transparent, ..Scope::default() });
    }

    /// End the innermost scope: drop its temporaries, then its variables.
    fn pop_scope(&mut self) -> Res<()> {
        let s = self.scopes.pop().expect("a scope");
        if !self.terminated {
            self.drop_scope(&s, None)?;
        }
        Ok(())
    }

    /// End the innermost scope without dropping anything: what follows is
    /// unreachable (a fault), or its values were dropped on every way out.
    fn discard_scope(&mut self) {
        self.scopes.pop();
    }

    fn drop_scope(&mut self, s: &Scope, keep: Option<&str>) -> Res<()> {
        if let Some(t) = &s.tasks {
            // every task of the scope finishes before it does
            self.emit(format!("call void @strata_scope_end(ptr {t})"));
        }
        for v in s.temps.iter().rev() {
            if Some(v.1.as_str()) != keep {
                self.drop_value(v)?;
            }
        }
        for (t, slot) in s.owned.iter().rev() {
            let x = self.fresh("old");
            let ll = if self.is_int(t) { "i64" } else { "ptr" };
            self.emit(format!("{x} = load {ll}, ptr {slot}"));
            self.drop_value(&(t.clone(), x))?;
        }
        Ok(())
    }

    /// Leave scopes `depth..` (a `return`, a `break`): drop what they hold,
    /// except the temporary `keep` (the value leaving with the jump).
    fn drop_from(&mut self, depth: usize, keep: Option<&str>) -> Res<()> {
        let scopes = std::mem::take(&mut self.scopes);
        let r = scopes[depth.min(scopes.len())..].iter().rev().try_for_each(|s| self.drop_scope(s, keep));
        self.scopes = scopes;
        r
    }

    /// Note `v` as a temporary of the innermost scope (a value just made, that nothing holds).
    fn temp(&mut self, v: V) -> V {
        if self.counted(&v.0) && !constant(&v.1) && !self.is_temp(&v.1) {
            self.scopes.last_mut().unwrap().temps.push(v.clone());
        }
        v
    }

    fn is_temp(&self, op: &str) -> bool {
        self.scopes.iter().any(|s| s.temps.iter().any(|t| t.1 == op))
    }

    /// `v`, to be stored somewhere that will hold it: a temporary of the
    /// innermost scope moves there; anything else is held once more.
    fn take(&mut self, v: V) -> V {
        if !self.counted(&v.0) || constant(&v.1) {
            return v;
        }
        let temps = &mut self.scopes.last_mut().unwrap().temps;
        if let Some(i) = temps.iter().rposition(|t| t.1 == v.1) {
            temps.remove(i);
            return v;
        }
        self.dup_value(&v);
        v
    }

    /// `take` for a value leaving every scope (the result): a temporary of
    /// any scope moves; the operand to keep when they are dropped.
    fn take_out(&mut self, v: &V) -> Option<String> {
        if !self.counted(&v.0) || constant(&v.1) {
            return None;
        }
        if self.is_temp(&v.1) {
            return Some(v.1.clone());
        }
        self.dup_value(v);
        None
    }

    /// The values of `es`, in order. One that only borrows (a variable, a
    /// field) is held until the statement ends when a later operand may
    /// assign a variable: in `f(x, { x = g(); 1 })` the assignment drops
    /// the value `x` held, which the call still reads.
    fn operands(&mut self, es: &[&Expr]) -> Res<Vec<V>> {
        // whether some operand after each one may assign: from the right, once
        // (asking it of the rest for each operand was quadratic in a list literal)
        let mut later = vec![false; es.len()];
        for i in (0..es.len().saturating_sub(1)).rev() {
            later[i] = later[i + 1] || assigns(es[i + 1]);
        }
        let mut vs = vec![];
        for (i, e) in es.iter().enumerate() {
            let v = self.expr(e)?;
            let v = if later[i] { self.hold(v) } else { v };
            vs.push(v);
        }
        Ok(vs)
    }

    /// `v`, held by the innermost scope if it only borrows (a temporary already is).
    fn hold(&mut self, v: V) -> V {
        if !self.counted(&v.0) || constant(&v.1) || self.is_temp(&v.1) {
            return v;
        }
        self.dup_value(&v);
        self.temp(v)
    }

    /// One holder of `v` fewer (freed with what it holds, when it was the last).
    fn drop_value(&mut self, v: &V) -> Res<()> {
        if !self.counted(&v.0) || constant(&v.1) {
            return Ok(());
        }
        if self.is_int(&v.0) {
            self.emit(format!("call void @strata.int_drop(i64 {})", v.1));
            return Ok(());
        }
        let f = self.drop_fn(&v.0)?;
        self.emit(format!("call void {f}(ptr {})", v.1));
        Ok(())
    }

    /// A branch's value (an `if`'s, an arm's), coerced to `want`, in a scope
    /// of its own; taken, for the `phi` that joins the branches.
    fn branch(&mut self, want: &Ty, f: impl FnOnce(&mut Self) -> Res<V>) -> Res<V> {
        self.push_scope(true);
        let v = match f(self).and_then(|v| self.coerce(v, want)) {
            Ok(v) => v,
            Err(e) => {
                self.discard_scope();
                return Err(e);
            }
        };
        let v = self.take(v);
        self.pop_scope()?;
        Ok(v)
    }

    /// Evaluate `f` in a scope of its own whose value leaves it: the value is
    /// taken before the scope ends, and is a temporary of the enclosing one.
    fn scoped(&mut self, transparent: bool, f: impl FnOnce(&mut Self) -> Res<V>) -> Res<V> {
        self.push_scope(transparent);
        let v = f(self);
        let v = match v {
            Ok(v) => v,
            Err(e) => {
                self.discard_scope();
                return Err(e);
            }
        };
        let v = self.take(v);
        self.pop_scope()?;
        Ok(self.temp(v))
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

    /// A string built from pieces, which it consumes: a `Val` is a string
    /// the caller hands over (as `show` and `json` return). The caller owns
    /// the result.
    fn concat(&mut self, parts: Vec<Part>) -> Res<V> {
        let mut acc: Option<V> = None;
        for p in parts {
            let v = match p {
                Part::Lit(s) if s.is_empty() => continue,
                Part::Lit(s) => self.string(&s),
                Part::Val(op) => (Ty::Str, op),
                Part::Int(op) => {
                    let s = self.shown(&(Ty::Int, op))?;
                    (Ty::Str, s)
                }
            };
            acc = Some(match acc {
                None => v,
                Some(a) => self.call(&Ty::Str, "@strata_concat_owned", &[a, v])?,
            });
        }
        Ok(acc.unwrap_or_else(|| self.string("")))
    }

    /// Stop with a fault; the code after it is unreachable.
    ///
    /// In a program with tasks, inside a function's body, a panic may be a
    /// supervised task's own (`spawn supervised`): `strata_panic` then
    /// records it and returns, and the function leaves as a cancelled one
    /// does (dropping what it holds; its callers follow), so the dying task
    /// frees what it held. Anywhere else `strata_panic` ends the program.
    fn fault(&mut self, kind: Fault, msg: Vec<Part>) -> Res<()> {
        let m = self.concat(msg)?;
        if self.m.tasks && self.ret_slot.is_some() && !self.in_exit {
            self.emit(format!("call void @strata_panic(i32 {}, ptr {})", kind as i32, m.1));
            self.drop_from(0, None)?;
            self.br("unwound");
            self.unwinds = true;
        } else {
            self.emit(format!("call void @strata_fault(i32 {}, ptr {})", kind as i32, m.1));
            self.emit("unreachable".into());
        }
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

    /// A value's slot as an LLVM constant, if the value is a constant that
    /// fits one: a small `Int`, `()`, or a constant object (a string, a
    /// constant list or block: `@...` or `getelementptr`, never freed).
    fn constant_slot(&mut self, v: &V) -> Res<Option<(&'static str, String)>> {
        if is_never(&v.0) {
            return Ok(None);
        }
        Ok(match self.m.repr(&v.0)? {
            Ty::Int if v.1.parse::<i64>().is_ok() => Some(("i64", v.1.clone())),
            Ty::Unit => Some(("i64", "0".into())),
            // a float literal is its bits in hex (`0x3FF8000000000000`)
            Ty::Float => v.1.strip_prefix("0x").and_then(|h| u64::from_str_radix(h, 16).ok()).map(|b| ("i64", (b as i64).to_string())),
            Ty::Bool if v.1 == "true" || v.1 == "false" => Some(("i64", if v.1 == "true" { "1" } else { "0" }.into())),
            Ty::Int | Ty::Bool => None,
            _ if !constant(&v.1) || v.1 == "null" || v.1 == "poison" || v.1.is_empty() => None,
            _ => Some(("ptr", v.1.clone())),
        })
    }

    /// The slots as one constant array (`[n x i64]` or `[n x ptr]`; both are
    /// 64-bit words), if every value is a constant of one kind.
    fn constant_slots(&mut self, vs: &[V]) -> Res<Option<(String, String)>> {
        let mut kind = None;
        let mut items = vec![];
        for v in vs {
            let Some((k, c)) = self.constant_slot(v)? else { return Ok(None) };
            if kind.is_some_and(|k0| k0 != k) {
                return Ok(None);
            }
            kind = Some(k);
            items.push(format!("{k} {c}"));
        }
        let ty = format!("[{} x {}]", vs.len(), kind.unwrap_or("i64"));
        Ok(Some((ty.clone(), format!("{ty} [{}]", items.join(", ")))))
    }

    /// A block of constants (a tuple's) as a constant: its tag, then its
    /// slots, after a count of 0 (never freed).
    fn constant_block(&mut self, tag: i64, vs: &[V]) -> Res<Option<String>> {
        if vs.is_empty() {
            return Ok(None);
        }
        let mut tys = vec![];
        let mut items = vec![];
        for v in vs {
            let Some((k, c)) = self.constant_slot(v)? else { return Ok(None) };
            tys.push(k);
            items.push(format!("{k} {c}"));
        }
        let key = format!("block {tag} {}", items.join(", "));
        if let Some(at) = self.m.constant_of.get(&key) {
            return Ok(Some(at.clone()));
        }
        let g = format!("@.const.{}", self.m.constants);
        self.m.constants += 1;
        writeln!(self.m.globals, "{g} = private unnamed_addr constant {{ i64, i64, {} }} {{ i64 0, i64 {tag}, {} }}, align 8", tys.join(", "), items.join(", ")).unwrap();
        let at = format!("getelementptr inbounds (i8, ptr {g}, i64 8)");
        self.m.constant_of.insert(key, at.clone());
        Ok(Some(at))
    }

    /// A list of constants as a constant: its header (length, buffer,
    /// offset) and its full buffer (capacity, used, elements), each after a
    /// count of 0, so it is never freed and never changed in place (a push
    /// or an update copies it, as for any list held elsewhere).
    fn constant_list(&mut self, _et: &Ty, vs: &[V]) -> Res<Option<String>> {
        if vs.is_empty() {
            return Ok(None);
        }
        let Some((ty, arr)) = self.constant_slots(vs)? else { return Ok(None) };
        let key = format!("list {arr}");
        if let Some(at) = self.m.constant_of.get(&key) {
            return Ok(Some(at.clone()));
        }
        let n = vs.len();
        let (g, b) = (format!("@.const.{}", self.m.constants), format!("@.const.{}.buf", self.m.constants));
        self.m.constants += 1;
        writeln!(self.m.globals, "{b} = private unnamed_addr constant {{ i64, i64, i64, {ty} }} {{ i64 0, i64 {n}, i64 {n}, {arr} }}, align 8").unwrap();
        writeln!(self.m.globals, "{g} = private unnamed_addr constant {{ i64, i64, ptr, i64 }} {{ i64 0, i64 {n}, ptr getelementptr inbounds (i8, ptr {b}, i64 8), i64 0 }}, align 8").unwrap();
        let at = format!("getelementptr inbounds (i8, ptr {g}, i64 8)");
        self.m.constant_of.insert(key, at.clone());
        Ok(Some(at))
    }

    /// A new block: a tag, then slots. It holds its fields (taken); the
    /// caller owns it.
    fn block_of(&mut self, tag: i64, fields: &[V]) -> Res<String> {
        let fields: Vec<V> = fields.iter().map(|f| self.take(f.clone())).collect();
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

    /// A copy of a block whose fields have types `tys`: it holds them too,
    /// but for field `except` (about to be replaced). The caller owns it.
    fn copy_block(&mut self, v: &V, tys: &[Ty], except: Option<usize>) -> Res<String> {
        let p = self.fresh("copy");
        let size = 8 * (tys.len() + 1);
        self.emit(format!("{p} = call ptr @strata_alloc(i64 {size})"));
        self.emit(format!("call void @llvm.memcpy.p0.p0.i64(ptr {p}, ptr {}, i64 {size}, i1 false)", v.1));
        for (i, t) in tys.iter().enumerate() {
            if Some(i) != except && self.counted(t) {
                let f = self.field(&(v.0.clone(), p.clone()), i, t)?;
                self.dup_value(&f);
            }
        }
        Ok(p)
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
        if !all.is_empty() && !self.m.boxed(&v.0) && all.iter().all(|c| cs.contains(c)) {
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

    /// The program's own `Debug` / `Display` for a value's type, called: the
    /// string it returns (owned), if the type has one.
    fn text_impl(&mut self, tr: &str, m: &str, v: &V) -> Res<Option<String>> {
        let Ty::Con(n, _) = &v.0 else { return Ok(None) };
        let parent = self.m.env.variant_parent(n.sym()).map(|p| p.to_string());
        let has = |c: &Self, h: &str| c.m.env.impls.iter().any(|i| i.trait_name == tr && crate::types::texpr_head(&i.for_type) == h);
        if !has(self, n.as_str()) && !parent.as_deref().is_some_and(|p| has(self, p)) {
            return Ok(None);
        }
        let r = self.call_method(&crate::types::trait_call(tr, m), v.0.clone(), vec![v.clone()], Some(Ty::Str), &[])?;
        let r = self.take(r);
        Ok(Some(r.1))
    }

    /// A value as `print`, f-strings and `to_string` show it (`Display`): the
    /// type's own `to_string`, a string as itself, else its `debug`. The
    /// caller owns the string (as `json`'s): `concat` consumes it, or it is
    /// dropped.
    fn show(&mut self, v: &V) -> Res<String> {
        if let Some(s) = self.text_impl("Display", "to_string", v)? {
            return Ok(s);
        }
        if matches!(&v.0, Ty::Con(n, _) if self.m.is_distinct(n)) {
            return self.debug(v);
        }
        let t = self.m.repr(&v.0)?;
        if self.m.opaque(&t) {
            return self.shown(v);
        }
        if self.is_dyn(&t) {
            return self.dyn_entry_call(v, DYN_SHOW);
        }
        if self.m.boxed(&t) {
            let f = self.helper("show", &t)?;
            return Ok(self.call(&Ty::Str, &f, &[(t.clone(), v.1.clone())])?.1);
        }
        match t {
            Ty::Str => {
                // (owned, as every shown value: held once more)
                if !constant(&v.1) {
                    self.emit(format!("call void @strata_dup(ptr {})", v.1));
                }
                Ok(v.1.clone())
            }
            _ => self.debug(v),
        }
    }

    /// A value as it is written in code (`Debug`): `Some("hi")`,
    /// `Point { x: 1 }`, `[1, 2]`; the type's own `debug` if it has one.
    /// Owned by the caller, as `show`'s.
    fn debug(&mut self, v: &V) -> Res<String> {
        if let Some(s) = self.text_impl("Debug", "debug", v)? {
            return Ok(s);
        }
        if self.m.opaque(&self.m.repr(&v.0)?) {
            return self.shown(v);
        }
        // a `distinct` value: `UserId(7)` (it is represented as what it is made of)
        if let Ty::Con(n, _) = &v.0 {
            if self.m.is_distinct(n) {
                let inner = (self.m.repr(&v.0)?, v.1.clone());
                let d = self.debug(&inner)?;
                return Ok(self.concat(vec![Part::Lit(format!("{}(", crate::names::short(n))), Part::Val(d), Part::Lit(")".into())])?.1);
            }
        }
        let t = self.m.repr(&v.0)?;
        if self.is_dyn(&t) {
            return self.dyn_entry_call(v, DYN_DEBUG);
        }
        Ok(match &t {
            Ty::Int => self.call(&Ty::Str, "@strata_show_int", &[v.clone()])?.1,
            Ty::Float => self.call(&Ty::Str, "@strata_show_float", &[v.clone()])?.1,
            Ty::Bool => self.call(&Ty::Str, "@strata_show_bool", &[v.clone()])?.1,
            Ty::Str => self.call(&Ty::Str, "@strata_debug_str", &[v.clone()])?.1,
            Ty::Unit => self.m.string("()"),
            _ if is_never(&t) => self.m.string("()"),
            Ty::Fn(..) => {
                // how the closure shows
                let shown = self.field(v, 1, &Ty::Str)?;
                self.emit(format!("call void @strata_dup(ptr {})", shown.1));
                shown.1
            }
            Ty::Con(n, args) if n == "Set" => {
                let l = self.as_list(v)?;
                let lt = Ty::Con("List".into(), args.clone());
                let f = self.helper("debugset", &lt)?;
                self.call(&Ty::Str, &f, &[(lt, l.1)])?.1
            }
            t if self.m.boxed(t) || self.m.nominal(t) || matches!(t, Ty::Con(l, _) if l == "List" || l == "Dict") => {
                let f = self.helper("debug", t)?;
                self.call(&Ty::Str, &f, &[(t.clone(), v.1.clone())])?.1
            }
            t => return Err(format!("showing a `{}` is not compiled yet", key(t))),
        })
    }

    /// Check a value made outside compiled code (the host wrote it) as
    /// making it here would have: every constructor's fields and type, all
    /// the way down, and a `distinct` type's refinement. Faults on the first
    /// that fails.
    fn validate(&mut self, v: &V) -> Res<()> {
        if let Ty::Con(n, _) = &v.0 {
            if self.m.is_distinct(n) {
                if let Some(r) = self.m.refinement(n.as_str()) {
                    let ok = self.with_it(v, false, |s| s.expr(r))?;
                    self.fail_unless(&ok, crate::names::short(n), v, format!("does not satisfy `{}` (where {})", crate::names::short(n), crate::printer::expr(r, 0)))?;
                }
                let inner = (self.m.repr(&v.0)?, v.1.clone());
                return self.validate(&inner);
            }
        }
        let t = self.m.repr(&v.0)?;
        let deep = self.m.boxed(&t) || self.m.nominal(&t) || matches!(&t, Ty::Con(n, _) if n == "List" || n == "Set" || n == "Dict" || n == "Tuple");
        if deep && !self.m.handle(&t) && !self.is_dyn(&t) {
            let f = self.helper("valid", &t)?;
            self.call(&Ty::Unit, &f, &[(t, v.1.clone())])?;
        }
        Ok(())
    }

    /// A value as the 8-byte slot it is kept in (`native::read` reads it):
    /// itself, a float's bits, a Bool widened, a pointer's address.
    fn as_slot(&mut self, v: &V) -> Res<String> {
        let ll = self.m.ll(&v.0)?;
        Ok(match ll {
            "void" => "0".into(),
            "i64" => v.1.clone(),
            "double" => {
                let r = self.fresh("slot");
                self.emit(format!("{r} = bitcast double {} to i64", v.1));
                r
            }
            "i1" => {
                let r = self.fresh("slot");
                self.emit(format!("{r} = zext i1 {} to i64", v.1));
                r
            }
            _ => {
                let r = self.fresh("slot");
                self.emit(format!("{r} = ptrtoint ptr {} to i64", v.1));
                r
            }
        })
    }

    /// A value as messages show it: as `debug` does (the code that writes
    /// it, unless its type has a `Debug` of its own); a native handle by its
    /// type (`<Db>`: what it points to is C's).
    fn shown(&mut self, v: &V) -> Res<String> {
        let t = self.m.repr(&v.0)?;
        if self.m.opaque(&t) {
            return Ok(self.m.string(&format!("<{}>", crate::names::short(&key(&t)))));
        }
        self.debug(v)
    }

    /// A value as JSON (`to_json`, `derive Json`).
    fn json(&mut self, v: &V) -> Res<String> {
        let t = self.m.repr(&v.0)?;
        if self.m.opaque(&t) {
            // only its type: what it points to is C's
            return Ok(self.m.string(&format!("\"<{}>\"", crate::names::short(&key(&t)))));
        }
        if self.is_dyn(&t) {
            return self.dyn_entry_call(v, DYN_JSON);
        }
        Ok(match &t {
            Ty::Int => self.call(&Ty::Str, "@strata_json_int", &[v.clone()])?.1,
            Ty::Float => self.call(&Ty::Str, "@strata_show_float", &[v.clone()])?.1,
            Ty::Bool => self.call(&Ty::Str, "@strata_show_bool", &[v.clone()])?.1,
            Ty::Str => self.call(&Ty::Str, "@strata_json_str", &[v.clone()])?.1,
            Ty::Unit => self.m.string("null"),
            _ if is_never(&t) => self.m.string("null"),
            Ty::Fn(..) => {
                // how the closure shows, as a JSON string
                let shown = self.field(v, 1, &Ty::Str)?;
                self.call(&Ty::Str, "@strata_json_str", &[shown])?.1
            }
            Ty::Con(n, _) if n == "Set" => {
                let l = self.as_list(v)?;
                self.json(&l)?
            }
            t if self.m.boxed(t) => {
                let f = self.helper("json", t)?;
                self.call(&Ty::Str, &f, &[(t.clone(), v.1.clone())])?.1
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
            t if self.m.opaque(t) => {
                // the same pointer
                let r = self.fresh("eq");
                self.emit(format!("{r} = icmp eq ptr {}, {}", a.1, b.1));
                bool_v(&r)
            }
            Ty::Unit => bool_v("true"),
            Ty::Int => {
                // the same 64 bits, or two big integers of the same value
                let (same, end, deep) = (self.fresh("same"), self.fresh_label("int.eqd"), self.fresh_label("int.deep"));
                self.emit(format!("{same} = icmp eq i64 {}, {}", a.1, b.1));
                let from = self.cur.clone();
                self.emit(format!("br i1 {same}, label %{end}, label %{deep}"));
                self.block(&deep);
                let e = self.fresh("deq");
                self.emit(format!("{e} = call i1 @strata_int_eq(i64 {}, i64 {})", a.1, b.1));
                let from_deep = self.cur.clone();
                self.br(&end);
                self.block(&end);
                let r = self.fresh("eq");
                self.emit(format!("{r} = phi i1 [ true, %{from} ], [ {e}, %{from_deep} ]"));
                bool_v(&r)
            }
            Ty::Bool => {
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
            t if self.m.boxed(t) => {
                let f = self.helper("eq", t)?;
                self.call(&Ty::Bool, &f, &[(t.clone(), a.1.clone()), (t.clone(), b.1.clone())])?
            }
            t if collection(t).is_some() => {
                let (x, y) = (self.as_list(a)?, self.as_list(b)?);
                self.equal(&x, &y)?
            }
            t if self.m.nominal(t) || matches!(t, Ty::Con(l, _) if l == "List") => {
                let f = self.helper("eq", t)?;
                self.call(&Ty::Bool, &f, &[(t.clone(), a.1.clone()), (t.clone(), b.1.clone())])?
            }
            t if self.is_dyn(t) => {
                // the same implementation's table, then its equality (values
                // of different types are never equal)
                let (ra, rb) = (self.fresh("vta"), self.fresh("vtb"));
                self.emit(format!("{ra} = load i64, ptr {}", a.1));
                self.emit(format!("{rb} = load i64, ptr {}", b.1));
                let same = self.fresh("samevt");
                self.emit(format!("{same} = icmp eq i64 {ra}, {rb}"));
                let (cmp, end) = (self.fresh_label("dyn.eq"), self.fresh_label("dyn.eqd"));
                let from = self.cur.clone();
                self.emit(format!("br i1 {same}, label %{cmp}, label %{end}"));
                self.block(&cmp);
                let fp = self.dyn_entry(a, DYN_EQ);
                let (sa, sb) = (self.field(a, 0, &Ty::Int)?, self.field(b, 0, &Ty::Int)?);
                let r = self.fresh("deq");
                self.emit(format!("{r} = call i1 {fp}(i64 {}, i64 {})", sa.1, sb.1));
                let from_cmp = self.cur.clone();
                self.br(&end);
                self.block(&end);
                let p = self.fresh("eq");
                self.emit(format!("{p} = phi i1 [ false, %{from} ], [ {r}, %{from_cmp} ]"));
                bool_v(&p)
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
                // -1, 0 or 1
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
                        // a `distinct` value: as what it is made of (`UserId(9) < UserId(10)`)
                        let r = h.m.repr(t)?;
                        let f = h.helper("cmp", &r)?;
                        let c = h.call(&Ty::Int, &f, &[(r.clone(), "%a".into()), (r, "%b".into())])?;
                        h.emit(format!("ret i64 {}", c.1));
                    }
                    Ty::Int => {
                        let c = h.call(&Ty::Int, "@strata_int_cmp", &[a.clone(), b.clone()])?;
                        h.emit(format!("ret i64 {}", c.1));
                    }
                    Ty::Bool => {
                        let (lt, gt) = (h.fresh("l"), h.fresh("g"));
                        let (pl, pg) = ("ult", "ugt");
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
                    Ty::Con(sn, el) if sn == "Set" => {
                        // as its elements, in order
                        let list = Ty::Con("List".into(), el.clone());
                        let f = h.helper("cmp", &list)?;
                        let c = h.call(&Ty::Int, &f, &[(list.clone(), "%a".into()), (list, "%b".into())])?;
                        h.emit(format!("ret i64 {}", c.1));
                    }
                    Ty::Con(d, kv) if d == "Dict" => {
                        // as its pairs, in key order
                        let pairs = Ty::Con("List".into(), vec![Ty::Con("Tuple".into(), kv.clone())]);
                        let f = h.helper("cmp", &pairs)?;
                        let c = h.call(&Ty::Int, &f, &[(pairs.clone(), "%a".into()), (pairs, "%b".into())])?;
                        h.emit(format!("ret i64 {}", c.1));
                    }
                    r if h.m.boxed(&r) => {
                        // a union: by member, in the order written, then as that member
                        let ms = h.m.members(t);
                        let tc = t.clone();
                        h.on_member(&a, |h, ma, x| {
                            let ra = ms.iter().position(|m| m == ma).unwrap_or(0);
                            let (ms, ma) = (ms.clone(), ma.clone());
                            h.on_member(&(tc.clone(), "%b".into()), |h, mb, y| {
                                let rb = ms.iter().position(|m| m == mb).unwrap_or(0);
                                if ra != rb {
                                    h.emit(format!("ret i64 {}", if ra < rb { -1 } else { 1 }));
                                    return Ok(());
                                }
                                let f = h.helper("cmp", &ma)?;
                                let c = h.call(&Ty::Int, &f, &[x.clone(), y])?;
                                h.emit(format!("ret i64 {}", c.1));
                                Ok(())
                            })
                        })?;
                    }
                    r if h.m.nominal(&r) => {
                        // a record or variant: by variant, in the order declared, then field by field
                        let cs = h.m.ctors_of(&r);
                        let tc = t.clone();
                        h.on_ctor(&a, "", |h, ca| {
                            let ra = cs.iter().position(|c| c == ca).unwrap_or(0);
                            let (cs, ca) = (cs.clone(), ca.to_string());
                            h.on_ctor(&(tc.clone(), "%b".into()), "", |h, cb| {
                                let rb = cs.iter().position(|c| c == cb).unwrap_or(0);
                                if ra != rb {
                                    h.emit(format!("ret i64 {}", if ra < rb { -1 } else { 1 }));
                                    return Ok(());
                                }
                                let fs = h.m.fields(&ca, &h.m.ctor_args(&ca, &tc))?;
                                for (i, fi) in fs.iter().enumerate() {
                                    let (x, y) = (h.field(&(tc.clone(), "%a".into()), i, &fi.ty)?, h.field(&(tc.clone(), "%b".into()), i, &fi.ty)?);
                                    let f = h.helper("cmp", &fi.ty)?;
                                    let c = h.call(&Ty::Int, &f, &[x, y])?;
                                    let (z, next, out) = (h.fresh("z"), h.fresh_label("next"), h.fresh_label("differ"));
                                    h.emit(format!("{z} = icmp eq i64 {}, 0", c.1));
                                    h.emit(format!("br i1 {z}, label %{next}, label %{out}"));
                                    h.block(&out);
                                    h.emit(format!("ret i64 {}", c.1));
                                    h.block(&next);
                                }
                                h.emit("ret i64 0".into());
                                Ok(())
                            })
                        })?;
                    }
                    r => return Err(format!("comparing a `{}` is not compiled yet", key(&r))),
                }
                // (a type without a value, `()`, takes nothing: `call` passes nothing for it)
                if has_value(t) { format!("define i64 {name}({ll} %a, {ll} %b)") } else { format!("define i64 {name}()") }
            }
            "eq" if h.m.boxed(t) => {
                // the same member, then equal as it
                let (ta, tb) = (h.tag(&a), h.tag(&b));
                let st = h.fresh("sametag");
                h.emit(format!("{st} = icmp eq i64 {ta}, {tb}"));
                h.emit(format!("br i1 {st}, label %members, label %no"));
                h.block("members");
                h.on_member(&a, |h, m, x| {
                    let y = h.narrow(&b, m)?;
                    let e = h.equal(&x, &y)?;
                    h.emit(format!("ret i1 {}", e.1));
                    Ok(())
                })?;
                h.block("no");
                h.emit("ret i1 false".into());
                format!("define i1 {name}(ptr %a, ptr %b)")
            }
            "json" | "show" | "debug" if h.m.boxed(t) => {
                h.on_member(&a, |h, _, x| {
                    let s = match what {
                        "json" => h.json(&x)?,
                        "show" => h.show(&x)?,
                        _ => h.debug(&x)?,
                    };
                    h.emit(format!("ret ptr {s}"));
                    Ok(())
                })?;
                format!("define ptr {name}(ptr %a)")
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
            "debug" | "debugset" if list || matches!(t, Ty::Con(d, _) if d == "Dict") => {
                // `[a, b]`, `Set { a, b }`, `Dict { k: v }` (empty: `Set {}`, `Dict {}`)
                let dict = matches!(t, Ty::Con(d, _) if d == "Dict");
                let (open, close, empty) = match (what, dict) {
                    (_, true) => ("Dict { ", " }", "Dict {}"),
                    ("debugset", _) => ("Set { ", " }", "Set {}"),
                    _ => ("[", "]", "[]"),
                };
                let et = if dict { Ty::Con("Tuple".into(), args.clone()) } else { args[0].clone() };
                let n = h.list_len(&a);
                let none = h.fresh("none");
                h.emit(format!("{none} = icmp eq i64 {}, 0", n.1));
                h.emit(format!("br i1 {none}, label %none, label %some"));
                h.block("none");
                let e = h.m.string(empty);
                h.emit(format!("ret ptr {e}"));
                h.block("some");
                let acc = h.fresh("acc");
                writeln!(h.allocas, "  {acc} = alloca ptr").unwrap();
                let o = h.m.string(open);
                h.emit(format!("store ptr {o}, ptr {acc}"));
                let (nothing, comma) = (h.m.string(""), h.m.string(", "));
                h.each(&a, &et, |h, i, x| {
                    let first = h.fresh("first");
                    h.emit(format!("{first} = icmp eq i64 {i}, 0"));
                    let sep = h.fresh("sep");
                    h.emit(format!("{sep} = select i1 {first}, ptr {nothing}, ptr {comma}"));
                    let cur = h.fresh("cur");
                    h.emit(format!("{cur} = load ptr, ptr {acc}"));
                    let parts = if dict {
                        let (k, v) = (h.field(x, 0, &args[0])?, h.field(x, 1, &args[1])?);
                        let (kd, vd) = (h.debug(&k)?, h.debug(&v)?);
                        vec![Part::Val(cur), Part::Val(sep), Part::Val(kd), Part::Lit(": ".into()), Part::Val(vd)]
                    } else {
                        let d = h.debug(x)?;
                        vec![Part::Val(cur), Part::Val(sep), Part::Val(d)]
                    };
                    let s = h.concat(parts)?;
                    h.emit(format!("store ptr {}, ptr {acc}", s.1));
                    Ok(())
                })?;
                let cur = h.fresh("cur");
                h.emit(format!("{cur} = load ptr, ptr {acc}"));
                let s = h.concat(vec![Part::Val(cur), Part::Lit(close.into())])?;
                h.emit(format!("ret ptr {}", s.1));
                format!("define ptr {name}(ptr %a)")
            }
            "valid" => {
                // every part, then each constructor's fields and its type, as making it checks them
                let elements = |h: &mut Func, l: &V| -> Res<()> {
                    let et = h.elem_ty(&l.0)?;
                    h.each(l, &et, |h, _, x| h.validate(x))
                };
                match h.m.repr(t)? {
                    _ if tuple => {
                        for (i, et) in args.iter().enumerate() {
                            let x = h.field(&a, i, et)?;
                            h.validate(&x)?;
                        }
                        h.emit("ret void".into());
                    }
                    Ty::Con(n, _) if n == "List" => {
                        elements(&mut h, &a)?;
                        h.emit("ret void".into());
                    }
                    Ty::Con(n, _) if n == "Set" || n == "Dict" => {
                        let l = h.as_list(&a)?;
                        elements(&mut h, &l)?;
                        h.emit("ret void".into());
                    }
                    r if h.m.boxed(&r) => {
                        h.on_member(&a, |h, _, x| {
                            h.validate(&x)?;
                            h.emit("ret void".into());
                            Ok(())
                        })?;
                    }
                    _ => {
                        let tc = t.clone();
                        h.on_ctor(&a, "", |h, c| {
                            let fs = h.m.fields(c, &h.m.ctor_args(c, &tc))?;
                            for (i, f) in fs.iter().enumerate() {
                                let x = h.field(&a, i, &f.ty)?;
                                let fname = f.name.clone().unwrap_or(i.to_string());
                                h.check(&f.texpr, &x, &format!("{}.{fname}", crate::names::short(c)), false)?;
                                h.validate(&x)?;
                            }
                            h.check_type_refinement(c, &a)?;
                            h.emit("ret void".into());
                            Ok(())
                        })?;
                    }
                }
                format!("define void {name}(ptr %a)")
            }
            "debug" => {
                // as written in code: `(a, b)`, `Name { f: v }`, `Name(a, b)`, `Name`
                let fields_debug = |h: &mut Func, fs: &[FieldInfo], parts: &mut Vec<Part>| -> Res<()> {
                    for (i, f) in fs.iter().enumerate() {
                        if i > 0 {
                            parts.push(Part::Lit(", ".into()));
                        }
                        if let Some(n) = &f.name {
                            parts.push(Part::Lit(format!("{n}: ")));
                        }
                        let x = h.field(&a, i, &f.ty)?;
                        let s = h.debug(&x)?;
                        parts.push(Part::Val(s));
                    }
                    Ok(())
                };
                if tuple {
                    let fs: Vec<FieldInfo> = args.iter().map(|t| FieldInfo { name: None, texpr: TypeExpr::Unit, ty: t.clone() }).collect();
                    let mut parts = vec![Part::Lit("(".into())];
                    fields_debug(&mut h, &fs, &mut parts)?;
                    parts.push(Part::Lit(")".into()));
                    let s = h.concat(parts)?;
                    h.emit(format!("ret ptr {}", s.1));
                } else {
                    let tc = t.clone();
                    h.on_ctor(&a, "", |h, c| {
                        let fs = h.m.fields(c, &h.m.ctor_args(c, &tc))?;
                        let short = crate::names::short(c).to_string();
                        let mut parts = vec![];
                        if fs.is_empty() {
                            let is_record = matches!(h.m.env.types.get(&Symbol::new(c)).map(|td| &td.body), Some(TypeBody::Record(_)));
                            parts.push(Part::Lit(if is_record { format!("{short} {{}}") } else { short }));
                        } else if fs[0].name.is_none() {
                            parts.push(Part::Lit(format!("{short}(")));
                            fields_debug(h, &fs, &mut parts)?;
                            parts.push(Part::Lit(")".into()));
                        } else {
                            parts.push(Part::Lit(format!("{short} {{ ")));
                            fields_debug(h, &fs, &mut parts)?;
                            parts.push(Part::Lit(" }".into()));
                        }
                        let s = h.concat(parts)?;
                        h.emit(format!("ret ptr {}", s.1));
                        Ok(())
                    })?;
                }
                format!("define ptr {name}(ptr %a)")
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
                        // with fields: as the code that writes it (`debug`'s)
                        let d = h.debug(&a)?;
                        h.emit(format!("ret ptr {d}"));
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

    /// The function that drops one holder of a value of type `t`
    /// (`@"drop:T"(ptr)`): when it was the last, what the value holds is
    /// dropped and its memory freed. A constant (count 0) is never freed.
    fn drop_fn(&mut self, t: &Ty) -> Res<String> {
        let t = self.m.repr(t)?;
        let t = collection(&t).map(|c| c.1).unwrap_or(t);
        let name = format!("@\"drop:{}\"", key(&t));
        if !self.m.helpers.insert(name.clone()) {
            return Ok(name);
        }
        if matches!(&t, Ty::Con(n, _) if n == "Sender" || n == "Receiver") {
            writeln!(self.m.body, "define void {name}(ptr %a) {{\nstart:\n  tail call void @strata_channel_drop(ptr %a)\n  ret void\n}}\n").unwrap();
            return Ok(name);
        }
        let mut h = Func::new(self.m, HashMap::default());
        h.block("start");
        if h.m.boxed(&t) {
            // a record's or variant's block drops as itself; a box holds its value
            let a: V = (t.clone(), "%a".into());
            h.on_member(&a, |h, m, x| {
                if h.m.boxes(m) {
                    let (gone, unbox, kept) = (h.fresh("gone"), h.fresh_label("unbox"), h.fresh_label("kept"));
                    h.emit(format!("{gone} = call i1 @strata_release(ptr %a)"));
                    h.emit(format!("br i1 {gone}, label %{unbox}, label %{kept}"));
                    h.block(&unbox);
                    let x = h.field(&a, 0, m)?;
                    h.emit("call void @strata_free(ptr %a)".into());
                    h.drop_value(&x)?;
                    h.emit("ret void".into());
                    h.block(&kept);
                    h.emit("ret void".into());
                } else {
                    let f = h.drop_fn(m)?;
                    h.emit(format!("tail call void {f}(ptr {})", x.1));
                    h.emit("ret void".into());
                }
                Ok(())
            })?;
            let text = h.finish();
            writeln!(self.m.body, "define void {name}(ptr %a) {{\n{text}}}\n").unwrap();
            return Ok(name);
        }
        h.emit("%null = icmp eq ptr %a, null".into());
        h.emit("br i1 %null, label %kept, label %held".into());
        h.block("held");
        h.emit("%gone = call i1 @strata_release(ptr %a)".into());
        h.emit("br i1 %gone, label %last, label %kept".into());
        h.block("last");
        let a: V = (t.clone(), "%a".into());
        let args = Module::args_of(&t);
        // the fields to drop, then free the block and drop them (the last one
        // as a tail call, so a long chain doesn't grow the stack)
        let fields_then_free = |h: &mut Func, tys: &[Ty]| -> Res<()> {
            let mut held = vec![];
            for (i, ft) in tys.iter().enumerate() {
                if h.counted(ft) {
                    held.push(h.field(&a, i, ft)?);
                }
            }
            h.emit("call void @strata_free(ptr %a)".into());
            // the integers first, so an object field's drop can be the tail call
            let (ints, objects): (Vec<V>, Vec<V>) = held.into_iter().partition(|v| h.is_int(&v.0));
            for v in &ints {
                h.drop_value(v)?;
            }
            let n = objects.len();
            for (k, v) in objects.into_iter().enumerate() {
                let f = h.drop_fn(&v.0)?;
                let tail = if k + 1 == n { "tail " } else { "" };
                h.emit(format!("{tail}call void {f}(ptr {})", v.1));
            }
            h.emit("ret void".into());
            Ok(())
        };
        match &t {
            Ty::Str => {
                h.emit("call void @strata_free(ptr %a)".into());
                h.emit("ret void".into());
            }
            Ty::Fn(..) => {
                // the closure's own function drops what it captured, and frees it
                h.emit("%dp = getelementptr i64, ptr %a, i64 1".into());
                h.emit("%draw = load i64, ptr %dp".into());
                h.emit("%d = inttoptr i64 %draw to ptr".into());
                h.emit("tail call void %d(ptr %a)".into());
                h.emit("ret void".into());
            }
            _ if h.is_dyn(&t) => {
                // the table's entry for the value's type drops the value
                let fp = h.dyn_entry(&a, DYN_DROP);
                let s = h.field(&a, 0, &Ty::Int)?;
                h.emit("call void @strata_free(ptr %a)".into());
                h.emit(format!("tail call void {fp}(i64 {})", s.1));
                h.emit("ret void".into());
            }
            Ty::Con(l, _) if l == "List" => {
                // the header holds the buffer; the buffer its elements up to `used`
                let et = args[0].clone();
                h.emit("%bp = getelementptr i8, ptr %a, i64 8".into());
                h.emit("%buf = load ptr, ptr %bp".into());
                h.emit("call void @strata_free(ptr %a)".into());
                h.emit("%blast = call i1 @strata_release(ptr %buf)".into());
                h.emit("br i1 %blast, label %elements, label %kept".into());
                h.block("elements");
                if h.counted(&et) {
                    let f = if h.is_int(&et) { "@strata_int_drop".to_string() } else { h.drop_fn(&et)? };
                    h.emit("%up = getelementptr i8, ptr %buf, i64 8".into());
                    h.emit("%used = load i64, ptr %up".into());
                    h.emit("%data = getelementptr i64, ptr %buf, i64 2".into());
                    h.emit("br label %loop".into());
                    h.block("loop");
                    h.emit("%i = phi i64 [ 0, %elements ], [ %next, %body ]".into());
                    h.emit("%more = icmp slt i64 %i, %used".into());
                    h.emit("br i1 %more, label %body, label %done".into());
                    h.block("body");
                    h.emit("%ep = getelementptr i64, ptr %data, i64 %i".into());
                    h.emit("%eraw = load i64, ptr %ep".into());
                    if h.is_int(&et) {
                        h.emit(format!("call void {f}(i64 %eraw)"));
                    } else {
                        h.emit("%e = inttoptr i64 %eraw to ptr".into());
                        h.emit(format!("call void {f}(ptr %e)"));
                    }
                    h.emit("%next = add i64 %i, 1".into());
                    h.emit("br label %loop".into());
                    h.block("done");
                }
                h.emit("call void @strata_free(ptr %buf)".into());
                h.emit("ret void".into());
            }
            Ty::Con(n, _) if n == "Tuple" => fields_then_free(&mut h, &args)?,
            _ if h.m.nominal(&t) || matches!(t, Ty::Union(_)) => {
                let tc = t.clone();
                h.on_ctor(&a, "", |h, c| {
                    let tys: Vec<Ty> = h.m.fields(c, &h.m.ctor_args(c, &tc))?.into_iter().map(|f| f.ty).collect();
                    fields_then_free(h, &tys)
                })?;
            }
            other => return Err(format!("dropping a `{}` is not compiled yet", key(other))),
        }
        h.block("kept");
        h.emit("ret void".into());
        let text = h.finish();
        writeln!(self.m.body, "define void {name}(ptr %a) {{\n{text}}}\n").unwrap();
        Ok(name)
    }

    // ---- contracts ----

    /// Fault unless `v` is a value of type `t`: the refinements reachable
    /// through it (`Interp::check_type_in`). `outer` says whether the
    /// enclosing scope is visible to an inline refinement (a function's
    /// parameters, for its result type). A record or variant was checked
    /// when it was built, so only fields of a type argument's type are.
    fn check(&mut self, t: &TypeExpr, v: &V, ctx: &str, outer: bool) -> Res<()> {
        // what `verify` calls the same check
        let key = if ctx == "let binding" {
            Some(format!("`let` annotated `{}`", crate::printer::ty(t)))
        } else if ctx.starts_with("result of `") {
            Some(crate::types::shown_name(ctx))
        } else if ctx.starts_with("assigning `") || ctx.starts_with("constructing `") {
            // (a field's check is part of the value's: `constructing \`C\`: C.f`)
            let end = ctx[ctx.find('`').unwrap() + 1..].find('`').map(|i| ctx.find('`').unwrap() + 2 + i);
            Some(end.map_or(ctx, |e| &ctx[..e]).to_string())
        } else {
            // (an argument's type is the caller's to prove)
            None
        };
        self.check_keyed(t, v, ctx, outer, key)
    }

    /// `check`, left out when `verify` proved the check called `key`.
    fn check_keyed(&mut self, t: &TypeExpr, v: &V, ctx: &str, outer: bool, key: Option<String>) -> Res<()> {
        if !has_value(&v.0) || !self.needs_check(t) {
            return Ok(());
        }
        if let Some(k) = key {
            if self.proven(|e| same_check(e, &k)) {
                return Ok(());
            }
        }
        self.check_in(t, v, &[Part::Lit(ctx.to_string())], outer)
    }

    /// Whether every call of this function proves its entry check `key`
    /// (`argument \`n\` of \`f\``, `precondition of \`f\`: requires …`): each
    /// function referring to it proved it there. Counted.
    fn proven_at_calls(&mut self, key: &str) -> bool {
        let Some(name) = self.proofs_of.clone() else { return false };
        let refs = &self.m.refs;
        let ok = name != "main"
            && !refs.elsewhere.contains(&name)
            && !refs.methods.contains(crate::names::short(&name))
            && refs.callers.get(&name).is_some_and(|cs| {
                cs.iter().all(|g| self.m.proofs.get(g).is_some_and(|p| p.proven.iter().any(|e| same_check(e, key)) && !p.open.iter().any(|e| same_check(e, key))))
            });
        if ok {
            self.m.left_out += 1;
        } else {
            self.m.kept += 1;
        }
        ok
    }

    /// Whether `verify` proved every check of this function that `is` picks
    /// (at least one, none left open): such a check is left out. Counted.
    fn proven(&mut self, is: impl Fn(&str) -> bool) -> bool {
        let Some(name) = &self.proofs_of else { return false };
        let ok = self.m.proofs.get(name).is_some_and(|p| p.proven.iter().any(|e| is(e)) && !p.open.iter().any(|e| is(e)));
        if ok {
            self.m.left_out += 1;
        } else {
            self.m.kept += 1;
        }
        ok
    }

    /// `check`, with a context built at run time (a list element's index).
    fn check_in(&mut self, t: &TypeExpr, v: &V, ctx: &[Part], outer: bool) -> Res<()> {
        if !has_value(&v.0) || !self.needs_check(t) {
            return Ok(());
        }
        let within = |ctx: &[Part], more: Vec<Part>| -> Vec<Part> { ctx.iter().cloned().chain(more).collect() };
        match t {
            TypeExpr::Union(written) if matches!(self.m.repr(&v.0)?, Ty::Union(_)) => {
                // the value is one of the members whose refinements it satisfies
                let written: Vec<(Ty, TypeExpr)> = written.iter().map(|w| Ok((self.lower(w)?, w.clone()))).collect::<Res<_>>()?;
                let slot = self.fresh("inunion");
                writeln!(self.allocas, "  {slot} = alloca i1").unwrap();
                let end = self.fresh_label("union.checked");
                self.on_member(v, |s, m, x| {
                    let mk = key(&s.m.repr(m)?);
                    let mut ok = bool_v("false");
                    for (wt, w) in &written {
                        if s.m.members(wt).iter().any(|y| s.m.repr(y).map(|r| key(&r)).ok().as_deref() == Some(mk.as_str())) {
                            let one = s.satisfies(w, &x)?;
                            let r = s.fresh("or");
                            s.emit(format!("{r} = or i1 {}, {}", ok.1, one.1));
                            ok = bool_v(&r);
                        }
                    }
                    s.emit(format!("store i1 {}, ptr {slot}", ok.1));
                    s.br(&end);
                    Ok(())
                })?;
                self.block(&end);
                let ok = self.fresh("ok");
                self.emit(format!("{ok} = load i1, ptr {slot}"));
                self.fail_unless_in(&bool_v(&ok), ctx.to_vec(), v, format!("is not a `{}`", crate::printer::ty(t)))?;
            }
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
                // (the index is shown only when an element fails)
                self.each(v, &et, |c, i, x| {
                    if c.m.fuzzing {
                        // (a long list is work: it counts against a run's budget)
                        c.emit("call void @strata_step()".into());
                    }
                    let inner = within(&ctx, vec![Part::Lit(": [".into()), Part::Int(i.to_string()), Part::Lit("]".into())]);
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
                        let shown = c.shown(x)?;
                        let shown = c.temp((Ty::Str, shown)).1;
                        return c.check_in(&written[0], x, &within(&ctx, vec![Part::Lit(": element ".into()), Part::Val(shown)]), false);
                    }
                    let (k, val) = (c.field(x, 0, &lowered[0])?, c.field(x, 1, &lowered[1])?);
                    let shown = c.shown(&k)?;
                    let shown = c.temp((Ty::Str, shown)).1;
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
            TypeExpr::Union(ms) => ms.iter().any(|m| self.needs_check(m)),
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
        let k = format!("constructing `{c}`");
        if let Some(r) = self.m.refinement(&tname).filter(|_| !self.proven(|e| same_check(e, &k))) {
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
        let shown = self.shown(v)?;
        ctx.extend([Part::Lit(": ".into()), Part::Val(shown), Part::Lit(format!(" {msg}"))]);
        self.fault(Fault::Refinement, ctx)?;
        self.block(&pass);
        Ok(())
    }

    /// Whether `v` is a value of the type `written` names (a type pattern,
    /// `Positive`, `List[Int]`, or `is`): its constructor, and every refinement.
    fn holds(&mut self, written: &TypeExpr, v: &V) -> Res<V> {
        let TypeExpr::Named(name, _) = written else { return self.unsupported("this type pattern") };
        if self.m.boxed(&v.0) {
            // the member first, then its refinements on the value as it; a
            // generic one named without its arguments (`List =>`) is the
            // union's member of that name (`List[Int]`)
            let bare = match written {
                TypeExpr::Named(n, a) if a.is_empty() => self.m.members(&v.0).into_iter().find(|m| Module::head(m).as_deref() == Some(n.as_str()) && !Module::args_of(m).is_empty()),
                _ => None,
            };
            let target = match bare {
                Some(m) => m,
                None => self.lower(written)?,
            };
            let is = self.holds_member(v, &target)?;
            if self.m.boxed(&target) || self.m.members(&target).len() > 1 {
                return Ok(is);
            }
            let (yes, end) = (self.fresh_label("holds.member"), self.fresh_label("holds.end"));
            let from = self.cur.clone();
            self.cond_br(&is, &yes, &end);
            self.block(&yes);
            let x = self.narrow(v, &target)?;
            let ok = self.holds(written, &x)?;
            let from_yes = self.cur.clone();
            self.br(&end);
            self.block(&end);
            let r = self.fresh("holds");
            self.emit(format!("{r} = phi i1 [ false, %{from} ], [ {}, %{from_yes} ]", ok.1));
            return Ok(bool_v(&r));
        }
        let mut conds = vec![];
        if self.m.nominal(&self.m.repr(&v.0)?) {
            // an alias (`Choice` is a refined `Sum`): its underlying type's constructors
            let target = match self.m.env.types.get(name).map(|t| &t.body) {
                Some(TypeBody::Alias(_)) => self.m.repr(&self.lower(written)?)?,
                _ => Ty::Con(name.clone(), Module::args_of(&v.0)),
            };
            let cs = self.m.ctors_of(&target);
            conds.push(self.tag_in(v, &cs)?);
        }
        let mut t = written.clone();
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

    /// Whether `v` satisfies the refinements of the written type `t` (its
    /// conditions and those of the types it names, as `holds`).
    fn satisfies(&mut self, t: &TypeExpr, v: &V) -> Res<V> {
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
                    if let Some(r) = self.m.refinement(&n) {
                        conds.push(self.with_it(v, false, |c| c.expr(r))?);
                    }
                    let TypeBody::Alias(a) = td.body else { break };
                    t = a;
                }
                _ => break,
            }
        }
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
        let saved = if outer { None } else { Some(std::mem::replace(&mut self.scopes, vec![Scope::default()])) };
        self.push_scope(false);
        let slot = self.local("it", &v.0, None);
        let r = slot.and_then(|slot| self.store(v, &slot)).and_then(|_| f(self));
        let r = r.and_then(|r| self.pop_scope().map(|_| r));
        if let Some(s) = saved {
            self.scopes = s;
        }
        let r = r?;
        if r.0 != Ty::Bool {
            return Err("a refinement must be a `Bool`".into());
        }
        Ok(r)
    }

    /// Leave the function with `v`: it moves to the result (or is held once
    /// more), and every scope drops what it holds.
    fn return_value(&mut self, v: &V) -> Res<()> {
        if is_never(&v.0) {
            return Ok(());
        }
        let (t, slot) = self.ret_slot.clone().ok_or("`return` outside a function")?;
        let mut keep = None;
        if has_value(&t) {
            let v = self.coerce(v.clone(), &t)?;
            keep = self.take_out(&v);
            let ll = self.m.ll(&t)?;
            self.emit(format!("store {ll} {}, ptr {slot}", v.1));
        }
        self.drop_from(0, keep.as_deref())?;
        self.br("exit");
        Ok(())
    }

    /// After every way out of the body dropped what its scopes held: nothing
    /// left to drop (the variables stay known, for the postconditions).
    fn forget_drops(&mut self) {
        for s in &mut self.scopes {
            s.temps.clear();
            s.owned.clear();
        }
    }

    fn unsupported<X>(&self, what: &str) -> Res<X> {
        Err(format!("{what} is not compiled yet"))
    }

    // ---- expressions ----

    /// The value of `e`, as the checker typed it: a union's value where the
    /// checker narrowed it to a member (a matched variable in its arm) is
    /// that member's.
    fn expr(&mut self, e: &Expr) -> Res<V> {
        // a type passed at runtime (`type:T`): compiled code is made per type
        if matches!(e, Expr::Var(x) if x.starts_with("type:")) {
            return Ok((Ty::Unit, String::new()));
        }
        let v = self.expr_in(e)?;
        if !self.m.boxed(&v.0) {
            return Ok(v);
        }
        match self.ty_of(e) {
            Ok(t) if !self.m.boxed(&t) && self.m.member_of(&t, &v.0) => self.narrow(&v, &t),
            _ => Ok(v),
        }
    }

    /// A union's value `v`, known to be of member type `t`: unboxed (borrowed from the box).
    fn narrow(&mut self, v: &V, t: &Ty) -> Res<V> {
        if self.m.boxes(t) {
            let x = self.field(v, 0, t)?;
            return Ok((t.clone(), x.1));
        }
        Ok((t.clone(), v.1.clone()))
    }

    /// `v` (of a member type, or a smaller union) as a value of union `u`:
    /// boxed when `u` boxes its type (a temporary holding it).
    fn boxed_into(&mut self, v: V, u: &Ty) -> Res<V> {
        if self.m.boxed(&v.0) || matches!(self.m.repr(&v.0)?, Ty::Union(_)) || !self.m.boxes(&v.0) {
            return Ok((u.clone(), v.1));
        }
        let tag = self.m.box_tag(&v.0);
        let b = self.block_of(tag, &[v])?;
        Ok(self.temp((u.clone(), b)))
    }

    /// A `switch` on the member a union's value `v` holds: `each` gets the
    /// member type and the value as it (it must end its block).
    fn on_member(&mut self, v: &V, mut each: impl FnMut(&mut Self, &Ty, V) -> Res<()>) -> Res<()> {
        let ms = self.m.members(&v.0);
        let tag = self.tag(v);
        let labels: Vec<String> = ms.iter().map(|_| self.fresh_label("member")).collect();
        let mut cases = vec![];
        let mut seen = HashSet::default();
        for (m, l) in ms.iter().zip(&labels) {
            for t in self.m.member_tags(m) {
                if seen.insert(t) {
                    cases.push(format!("i64 {t}, label %{l}"));
                }
            }
        }
        let other = self.fresh_label("member.none");
        self.emit(format!("switch i64 {tag}, label %{other} [ {} ]", cases.join(" ")));
        for (m, l) in ms.iter().zip(&labels) {
            self.block(l);
            let x = self.narrow(v, m)?;
            each(self, m, x)?;
        }
        self.block(&other);
        self.emit("unreachable".into());
        self.dead();
        Ok(())
    }

    /// Whether union value `v` holds a member of type `t` (or of one of its members).
    fn holds_member(&mut self, v: &V, t: &Ty) -> Res<V> {
        let tags: Vec<i64> = self.m.members(t).iter().flat_map(|m| self.m.member_tags(m)).collect();
        let tag = self.tag(v);
        let mut acc = bool_v("false");
        for n in tags {
            let (e, o) = (self.fresh("is"), self.fresh("or"));
            self.emit(format!("{e} = icmp eq i64 {tag}, {n}"));
            self.emit(format!("{o} = or i1 {}, {e}", acc.1));
            acc = bool_v(&o);
        }
        Ok(acc)
    }

    fn expr_in(&mut self, e: &Expr) -> Res<V> {
        Ok(match e {
            Expr::Int(n) => self.int_lit(&n.to_string()),
            Expr::BigInt(n) => self.int_lit(&n.to_string()),
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
                    let mut es: Vec<&Expr> = vec![other];
                    es.extend(args.iter());
                    let mut vs = self.operands(&es)?;
                    let fv = vs.remove(0);
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
                let tv = self.branch(&want, |s| s.expr(t))?;
                let from_t = self.cur.clone();
                self.br(&end);
                self.block(&lf);
                let fv = self.branch(&want, |s| s.expr(f))?;
                let from_f = self.cur.clone();
                self.br(&end);
                self.block(&end);
                let r = self.join(&want, vec![(tv, from_t), (fv, from_f)])?;
                self.temp(r)
            }
            Expr::Block(stmts, last) => self.scoped(false, |s| {
                for st in stmts {
                    s.push_scope(true);
                    let r = s.stmt(st);
                    match r {
                        Ok(()) => s.pop_scope()?,
                        Err(e) => {
                            s.discard_scope();
                            return Err(e);
                        }
                    }
                }
                s.expr(last)
            })?,
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
                let s = self.concat(ps)?;
                self.temp(s)
            }
            Expr::Match(scrut, as_name, arms) => self.matching(e, scrut, as_name.as_deref(), arms)?,
            Expr::Tuple(xs) => {
                let t = self.ty_of(e)?;
                let vs = self.operands(&xs.iter().collect::<Vec<_>>())?;
                if vs.iter().any(|v| is_never(&v.0)) {
                    return Ok(never());
                }
                if let Some(b) = self.constant_block(0, &vs)? {
                    return Ok((t, b));
                }
                let b = self.block_of(0, &vs)?;
                self.temp((t, b))
            }
            Expr::Ctor(c, args) => self.construct(e, c, args)?,
            Expr::Field(r, f) => self.get_field(e, r, f)?,
            Expr::Is(x, name) => {
                let v = self.expr(x)?;
                if self.m.env.ctors.contains_key(name) && !self.m.is_distinct(name) {
                    self.tag_in(&v, &[name.to_string()])?
                } else if self.m.env.is_type_name(name) {
                    self.holds(&TypeExpr::Named(name.clone(), vec![]), &v)?
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
                let gv = self.branch(&want, |s| s.field(&v, 0, &want))?;
                let from_ok = self.cur.clone();
                self.br(&end);
                self.block(&fail);
                let fv = self.branch(&want, |s| {
                    s.push_scope(false);
                    if let Some(n) = name {
                        let et = match &vt {
                            Ty::Con(n, a) if n == "Result" => a[1].clone(),
                            _ => Ty::Unit,
                        };
                        let err = s.field(&v, 0, &et)?;
                        s.local_owned(n, err, None)?;
                    }
                    let fv = s.expr(fallback)?;
                    let fv = s.coerce(fv, &want)?;
                    let fv = s.take(fv);
                    s.pop_scope()?;
                    Ok(s.temp(fv))
                })?;
                let from_f = self.cur.clone();
                self.br(&end);
                self.block(&end);
                let r = self.join(&want, vec![(gv, from_ok), (fv, from_f)])?;
                self.temp(r)
            }
            Expr::List(xs) => {
                let t = self.ty_of(e)?;
                let vs = self.operands(&xs.iter().collect::<Vec<_>>())?;
                if vs.iter().any(|v| is_never(&v.0)) {
                    return Ok(never());
                }
                let et = self.elem_ty(&t)?;
                let vs: Vec<V> = vs.into_iter().map(|v| self.coerce(v, &et)).collect::<Res<_>>()?;
                // every element a constant: the list is one too, built once
                if let Some(l) = self.constant_list(&et, &vs)? {
                    return Ok((t, l));
                }
                // a new list's buffer holds the elements (taken)
                let l = self.call(&t, "@strata_list_new", &[(Ty::Int, vs.len().to_string())])?;
                let (bp, buf, data) = (self.fresh("bufp"), self.fresh("buf"), self.fresh("data"));
                self.emit(format!("{bp} = getelementptr i8, ptr {}, i64 8", l.1));
                self.emit(format!("{buf} = load ptr, ptr {bp}"));
                self.emit(format!("{data} = getelementptr i64, ptr {buf}, i64 2"));
                for (i, v) in vs.iter().enumerate() {
                    let v = self.take(v.clone());
                    let s = self.to_slot(&v)?;
                    let at = self.fresh("at");
                    self.emit(format!("{at} = getelementptr i64, ptr {data}, i64 {i}"));
                    self.emit(format!("store i64 {s}, ptr {at}"));
                }
                let up = self.fresh("usedp");
                self.emit(format!("{up} = getelementptr i8, ptr {buf}, i64 8"));
                self.emit(format!("store i64 {}, ptr {up}", vs.len()));
                self.emit(format!("store i64 {}, ptr {}", vs.len(), l.1));
                self.temp(l)
            }
            Expr::Lambda(ps, body) => self.lambda(e, ps, body)?,
            // the mark of a `mut` argument or receiver: the call writes back to its place
            Expr::Mut(x) => self.expr(x)?,
            // evaluated at build time once deriving lands; until then, as written
            Expr::Comptime(x) => self.expr(x)?,
            Expr::Scope(b) => {
                let s = self.fresh("tasks");
                self.emit(format!("{s} = call ptr @strata_scope_begin()"));
                let v = self.scoped(false, |c| {
                    c.scopes.last_mut().unwrap().tasks = Some(s.clone());
                    let v = c.expr(b)?;
                    // `scope { …; spawn e }` is `()` (a handle isn't counted)
                    Ok(if crate::types::ends_in_spawn(b) && !is_never(&v.0) { (Ty::Unit, String::new()) } else { v })
                })?;
                // (a task in it may have panicked inside a supervised one: then this one leaves)
                self.cancel_point()?;
                v
            }
            Expr::Spawn(body) => self.spawn(e, body)?,
            Expr::SupervisedSpawn(body) => self.supervised_spawn(e, body)?,
            Expr::Select(arms) => self.select(e, arms)?,
            Expr::Race(arms) => self.race(e, arms)?,
            Expr::Await(h) => {
                let ht = self.ty_of(h)?;
                let rt = self.ty_of(e)?;
                let v = self.expr(h)?;
                if is_never(&v.0) {
                    return Ok(never());
                }
                self.awaited(&v, &ht, &rt)?
            }
            Expr::Mock(t) => {
                let tr_ty = self.lower(t)?;
                if !self.is_dyn(&tr_ty) {
                    return self.unsupported(&format!("`mock` of `{}`, which is not a trait", key(&tr_ty)));
                }
                self.mock_value(&tr_ty)?
            }
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
    /// `+`, `-`, `*` on `Int`s: here when both and the result are small
    /// (runtime.c, "integers"), else in the runtime (a big integer).
    fn int_op(&mut self, op: &str, a: &V, b: &V) -> V {
        let (pair, r, ov) = (self.fresh("pair"), self.fresh("int"), self.fresh("ov"));
        self.emit(format!("{pair} = call {{ i64, i1 }} @llvm.{op}.with.overflow.i64(i64 {}, i64 {})", a.1, b.1));
        self.emit(format!("{r} = extractvalue {{ i64, i1 }} {pair}, 0"));
        self.emit(format!("{ov} = extractvalue {{ i64, i1 }} {pair}, 1"));
        let ok = self.all_small(&[&a.1, &b.1, &r]);
        let fine = self.fresh("fine");
        self.emit(format!("{fine} = and i1 {ok}, true"));
        let nov = self.fresh("nov");
        self.emit(format!("{nov} = xor i1 {ov}, true"));
        let both = self.fresh("fast");
        self.emit(format!("{both} = and i1 {fine}, {nov}"));
        let (fast, slow, end) = (self.fresh_label("int.small"), self.fresh_label("int.big"), self.fresh_label("int.done"));
        self.emit(format!("br i1 {both}, label %{fast}, label %{slow}"));
        self.block(&fast);
        self.br(&end);
        self.block(&slow);
        let f = match op {
            "sadd" => "add",
            "ssub" => "sub",
            _ => "mul",
        };
        let big = self.fresh("big");
        self.emit(format!("{big} = call i64 @strata_int_{f}(i64 {}, i64 {})", a.1, b.1));
        self.br(&end);
        self.block(&end);
        let v = self.fresh("n");
        self.emit(format!("{v} = phi i64 [ {r}, %{fast} ], [ {big}, %{slow} ]"));
        // a new value (a big one is an object)
        self.temp((Ty::Int, v))
    }

    /// `&`, `|`, `^` on `Int`s: here when both are small (so is the result:
    /// a small `Int` is its own 64-bit two's complement), else in the runtime.
    fn bit_op(&mut self, op: BinOp, a: &V, b: &V) -> V {
        let (ins, f) = match op {
            BinOp::BitAnd => ("and", "and"),
            BinOp::BitOr => ("or", "or"),
            _ => ("xor", "xor"),
        };
        let r = self.fresh("bits");
        self.emit(format!("{r} = {ins} i64 {}, {}", a.1, b.1));
        let ok = self.all_small(&[&a.1, &b.1]);
        let (fast, slow, end) = (self.fresh_label("bits.small"), self.fresh_label("bits.big"), self.fresh_label("bits.done"));
        self.emit(format!("br i1 {ok}, label %{fast}, label %{slow}"));
        self.block(&fast);
        self.br(&end);
        self.block(&slow);
        let big = self.fresh("big");
        self.emit(format!("{big} = call i64 @strata_int_{f}(i64 {}, i64 {})", a.1, b.1));
        self.br(&end);
        self.block(&end);
        let v = self.fresh("n");
        self.emit(format!("{v} = phi i64 [ {r}, %{fast} ], [ {big}, %{slow} ]"));
        self.temp((Ty::Int, v))
    }

    /// Whether every one of these `Int`s is small (in [-2^62, 2^62): its two
    /// top bits are equal).
    fn all_small(&mut self, xs: &[&str]) -> String {
        let mut acc = String::new();
        for x in xs {
            let (sh, xo) = (self.fresh("sh"), self.fresh("xo"));
            self.emit(format!("{sh} = shl i64 {x}, 1"));
            self.emit(format!("{xo} = xor i64 {x}, {sh}"));
            if acc.is_empty() {
                acc = xo;
            } else {
                let o = self.fresh("or");
                self.emit(format!("{o} = or i64 {acc}, {xo}"));
                acc = o;
            }
        }
        let r = self.fresh("small");
        self.emit(format!("{r} = icmp sge i64 {acc}, 0"));
        r
    }

    /// `a` `pred` `b` on `Int`s: as 64-bit values unless both are big.
    fn int_compare(&mut self, pred: &str, a: &str, b: &str) -> V {
        let (sa, sb, xa, xb, both, bb) = (self.fresh("sh"), self.fresh("sh"), self.fresh("xo"), self.fresh("xo"), self.fresh("both"), self.fresh("bigs"));
        self.emit(format!("{sa} = shl i64 {a}, 1"));
        self.emit(format!("{xa} = xor i64 {a}, {sa}"));
        self.emit(format!("{sb} = shl i64 {b}, 1"));
        self.emit(format!("{xb} = xor i64 {b}, {sb}"));
        self.emit(format!("{both} = and i64 {xa}, {xb}"));
        self.emit(format!("{bb} = icmp slt i64 {both}, 0"));
        let (fast, slow, end) = (self.fresh_label("cmp.small"), self.fresh_label("cmp.big"), self.fresh_label("cmp.done"));
        self.emit(format!("br i1 {bb}, label %{slow}, label %{fast}"));
        self.block(&fast);
        let r1 = self.fresh("c");
        self.emit(format!("{r1} = icmp {pred} i64 {a}, {b}"));
        self.br(&end);
        self.block(&slow);
        let (c, r2) = (self.fresh("c"), self.fresh("c"));
        self.emit(format!("{c} = call i64 @strata_int_cmp(i64 {a}, i64 {b})"));
        self.emit(format!("{r2} = icmp {pred} i64 {c}, 0"));
        self.br(&end);
        self.block(&end);
        let r = self.fresh("cmp");
        self.emit(format!("{r} = phi i1 [ {r1}, %{fast} ], [ {r2}, %{slow} ]"));
        bool_v(&r)
    }

    /// An integer literal: itself when small, else a big integer made at run time.
    fn int_lit(&mut self, digits: &str) -> V {
        if let Ok(n) = digits.parse::<i64>() {
            if (-(1i64 << 62)..(1i64 << 62)).contains(&n) {
                return (Ty::Int, n.to_string());
            }
        }
        let s = self.m.string(digits);
        let r = self.fresh("lit");
        self.emit(format!("{r} = call i64 @strata_int_lit(ptr {s})"));
        self.temp((Ty::Int, r))
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
            self.push_scope(true);
            let b = self.expr(r)?;
            self.pop_scope()?;
            let from_b = self.cur.clone();
            self.br(&end);
            self.block(&end);
            return self.join(&Ty::Bool, vec![(bool_v(short), from), (b, from_b)]);
        }
        // against a constructor without fields (`x == None`): its tag, as
        // `x is None`, whatever else the type holds (functions too)
        if matches!(op, Eq | Ne) {
            let bare = |e: &Expr| match e {
                Expr::Ctor(c, crate::ast::CtorArgs::None) if self.m.env.ctors.contains_key(c) => Some(*c),
                _ => None,
            };
            let test = match (bare(l), bare(r)) {
                (_, Some(c)) => Some((l, c)),
                (Some(c), None) => Some((r, c)),
                _ => None,
            };
            if let Some((x, c)) = test.filter(|(_, c)| !self.m.is_distinct(c)) {
                let v = self.expr(x)?;
                if is_never(&v.0) {
                    return Ok(v);
                }
                let is = self.tag_in(&v, &[c.to_string()])?;
                if op == Eq {
                    return Ok(is);
                }
                let r = self.fresh("ne");
                self.emit(format!("{r} = xor i1 {}, true", is.1));
                return Ok(bool_v(&r));
            }
        }
        let ab = self.operands(&[l, r])?;
        self.apply(op, &ab[0], &ab[1])
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
            (Add, Ty::Str) => {
                let r = self.call(&Ty::Str, "@strata_concat", &[a.clone(), b.clone()])?;
                self.temp(r)
            }
            (Add, Ty::Con(l, et)) if l == "List" => {
                let counted = self.elements_kind(&et[0]);
                let r = self.call(&a.0, "@strata_list_concat", &[a.clone(), b.clone(), counted])?;
                self.temp(r)
            }
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
            (Div, Ty::Int) => {
                let r = self.call(&Ty::Int, "@strata_div", &[a.clone(), b.clone()])?;
                self.temp(r)
            }
            (Mod, Ty::Int) => {
                let r = self.call(&Ty::Int, "@strata_mod", &[a.clone(), b.clone()])?;
                self.temp(r)
            }
            (BitAnd | BitOr | BitXor, Ty::Int) => self.bit_op(op, a, b),
            (Shl | Shr, Ty::Int) => {
                let f = if op == Shl { "@strata_shl" } else { "@strata_shr" };
                let r = self.call(&Ty::Int, f, &[a.clone(), b.clone()])?;
                self.temp(r)
            }
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
            (Lt, Ty::Int) => self.int_compare("slt", &a.1, &b.1),
            (Le, Ty::Int) => self.int_compare("sle", &a.1, &b.1),
            (Gt, Ty::Int) => self.int_compare("sgt", &a.1, &b.1),
            (Ge, Ty::Int) => self.int_compare("sge", &a.1, &b.1),
            (Lt, Ty::Float) => cmp(self, "fcmp olt")?,
            (Le, Ty::Float) => cmp(self, "fcmp ole")?,
            (Gt, Ty::Float) => cmp(self, "fcmp ogt")?,
            (Ge, Ty::Float) => cmp(self, "fcmp oge")?,
            (op, t) => return Err(format!("`{}` on a `{}`", op.symbol(), key(t))),
        })
    }

    fn call_named(&mut self, call: &Expr, name: &str, args: Vec<&Expr>) -> Res<V> {
        // a type passed at runtime (`$type("List", type:T)`): compiled code is
        // made per type, so the value is never used
        if name == crate::types::TYPE_TEXT {
            return Ok((Ty::Unit, String::new()));
        }
        // `T.check(x)`: `Ok(())` when `x` meets the refinements of the type
        if name == crate::types::CHECK && args.len() == 2 {
            let t = self.type_arg_ty(args[0], None)?;
            let written = Self::written_of(&t);
            let v = self.expr(args[1])?;
            let ok = match &written {
                TypeExpr::Named(..) => self.holds(&written, &v)?,
                _ => bool_v("true"),
            };
            let why = match &t {
                Ty::Con(n, _) if crate::types::refined_alias(self.m.env, *n) => {
                    let td = &self.m.env.types[n];
                    let pred = td.refinement.clone().or_else(|| if let TypeBody::Alias(TypeExpr::Refined(_, p)) = &td.body { Some((**p).clone()) } else { None });
                    match pred {
                        // a refinement written as a type argument: as written
                        Some(p) if n.as_str().starts_with(crate::desugar::WHERE) => match &td.body {
                            TypeBody::Alias(a) => format!("does not satisfy `{} where {}`", crate::printer::ty(a), crate::printer::expr(&p, 0)),
                            _ => format!("does not satisfy `{}`", crate::names::short(n)),
                        },
                        Some(p) => format!("does not satisfy `{}` (where {})", crate::names::short(n), crate::printer::expr(&p, 0)),
                        None => format!("does not satisfy `{}`", crate::names::short(n)),
                    }
                }
                t => format!("does not satisfy `{}`", key(t)),
            };
            let rt = Ty::Con("Result".into(), vec![Ty::Unit, Ty::Str]);
            let shown = self.shown(&v)?;
            let shown = self.temp((Ty::Str, shown));
            let why = self.m.string(&why);
            return self.call_fn(&prelude("check_result"), vec![ok, shown, (Ty::Str, why)], Some(rt));
        }
        // `$self_type(t, call)`: compiled code is made per type already
        if name == crate::types::SELF_TYPE && args.len() == 2 {
            return self.expr(args[1]);
        }
        if name == "matches" && args.len() == 2 && self.lookup(name).is_none() && !self.m.env.fns.contains_key(&Symbol::new(name)) {
            // a literal pattern: its matcher, made while compiling (a DFA, usually)
            if let Expr::Str(re) = args[1] {
                if let Some(f) = self.m.matchers.get(re.as_str()).cloned() {
                    let x = self.expr(args[0])?;
                    if is_never(&x.0) {
                        return Ok(x);
                    }
                    let m = self.call_fn(&f, vec![], None)?;
                    return self.call_fn("std.regex.run_matcher", vec![m, x], Some(Ty::Bool));
                }
            }
            // std.regex's engine: a pattern built at run time, or a literal one
            // that isn't a regex
            {
                let vs = self.operands(&args)?;
                if vs.iter().any(|v| is_never(&v.0)) {
                    return Ok(never());
                }
                let code = self.call_fn("std.regex.match_code", vs.clone(), Some(Ty::Int))?;
                // -1: not a regex, a fault saying why
                let bad = self.apply(BinOp::Eq, &code, &(Ty::Int, "-1".into()))?;
                let (fail, ok) = (self.fresh_label("bad_regex"), self.fresh_label("regex_ok"));
                self.cond_br(&bad, &fail, &ok);
                self.block(&fail);
                // (its temporaries are its own: the block ends in the fault)
                self.push_scope(true);
                let why = self.call_fn("std.regex.pattern_error", vec![vs[1].clone()], Some(Ty::Str))?;
                let msg = self.concat(vec![Part::Lit("bad regex: ".into()), Part::Val(why.1.clone())])?;
                self.emit(format!("call void @strata_fault(i32 {}, ptr {})", Fault::Runtime as i32 | 256, msg.1));
                self.emit("unreachable".into());
                self.pop_scope()?;
                self.block(&ok);
                return self.apply(BinOp::Eq, &code, &(Ty::Int, "1".into()));
            }
        }
        if self.lookup(name).is_some() {
            // a local closure
            let f = self.load(name)?;
            let vs = self.operands(&args)?;
            return self.call_closure(&f, vs);
        }
        // `Trait::m(type, ..)`: a trait method without `self`, for a type known here
        if let Some((tr, m)) = crate::types::split_trait_call(name) {
            let takes_self = self.m.trait_method(tr, m).map(|tm| crate::resolve::takes_self(&tm.decl)).unwrap_or(true);
            if !takes_self {
                let hint = self.ty_of(call).ok();
                let recv = match args.first() {
                    Some(e) => self.type_arg_ty(e, hint.as_ref())?,
                    None => return Err(format!("`{name}` needs the type it is called on")),
                };
                let vs = self.operands(&args[1..])?;
                return self.call_method(name, recv, vs, hint, &[]);
            }
        }
        if crate::stdlib::is_host(name) {
            let vs = self.operands(&args)?;
            let hint = self.ty_of(call).ok();
            return self.host(name, vs, hint);
        }
        // a generic function given its type arguments: as written (`Positive`),
        // so its instance knows their refinements
        if let Some(f) = self.m.env.fns.get(&Symbol::new(name)) {
            let mut named = vec![];
            for (p, a) in f.params.iter().zip(&args) {
                if let TypeExpr::Named(n, targs) = &p.ty {
                    if n == crate::types::TYPE_ARG {
                        if let (Some(TypeExpr::Named(tp, _)), Ok(t)) = (targs.first(), self.type_arg_ty(a, None)) {
                            named.push((*tp, t));
                        }
                    }
                }
            }
            if !named.is_empty() {
                self.named_targs = Some(named);
            }
        }
        let vs = self.operands(&args)?;
        if vs.iter().any(|v| is_never(&v.0)) {
            return Ok(never());
        }
        let hint = self.ty_of(call).ok();
        // where a `mut` parameter's changes go: the argument's place
        self.places = args.iter().map(|a| place_of(a)).collect();
        self.call_values(name, vs, hint)
    }

    /// The slot a `mut` parameter changes: a slot of its own holding the
    /// argument (held once more), written back to the place after the call.
    /// (Not the variable's own slot: the callee drops what it replaces, and
    /// the caller may still be reading the old value, as in `f(x, mut x.pop())`.)
    fn mut_slot(&mut self, place: &Option<(String, Vec<String>)>, v: &V, param: &str) -> Res<String> {
        let Some((root, _)) = place else { return Err(format!("the argument for the `mut` parameter `{param}` must be a variable or its field")) };
        self.lookup(root).ok_or(format!("`{root}` is not a local variable"))?;
        let tmp = self.fresh("mutarg");
        writeln!(self.allocas, "  {tmp} = alloca {}", self.m.ll(&v.0)?).unwrap();
        let v = self.take(v.clone());
        self.store(&v, &tmp)?;
        Ok(tmp)
    }

    /// After a call: a field passed `mut` goes back into its variable; the
    /// variable's declared type holds (`assigning \`x\``).
    fn write_back(&mut self, place: &Option<(String, Vec<String>)>, slot: &str, t: &Ty, param: &TypeExpr) -> Res<()> {
        let Some((root, path)) = place else { return Ok(()) };
        let l = self.lookup(root).ok_or(format!("`{root}` is not a local variable"))?;
        let shown = std::iter::once(root.clone()).chain(path.iter().cloned()).collect::<Vec<_>>().join(".");
        // the slot's value (owned) goes back into the place
        let r = self.fresh("back");
        let ll = self.m.ll(t)?;
        self.emit(format!("{r} = load {ll}, ptr {slot}"));
        let r = self.temp((t.clone(), r));
        let new = if path.is_empty() {
            r
        } else {
            let cur = self.load(root)?;
            self.set_path(&cur, path, r, &shown)?
        };
        self.replace(&l, (l.t.clone(), new.1))?;
        // the parameter's type held on every assignment in the callee: when it
        // fits in the variable's (the checker proved it), so does the result
        if let Some(d) = &l.declared {
            if !(path.is_empty() && crate::types::refinement_within(self.m.env, param, d) == Some(true)) {
                let v = self.load(root)?;
                // (`verify` calls it writing back)
                self.check_keyed(d, &v, &format!("assigning `{shown}`"), false, Some(format!("writing back `{shown}`")))?;
            }
        }
        Ok(())
    }

    /// Arguments for a function's parameters: `mut` ones as the slot of
    /// their place, the others as values (of the parameter's type; a
    /// trait-typed one keeps its own). Then what to write back.
    /// `per_impl`: a trait-typed parameter takes the implementation passed
    /// (a function, compiled once per implementation); otherwise (a method,
    /// compiled once) it takes a trait value.
    fn bind_args(&mut self, params: &[Param], vs: Vec<V>, sub: &HashMap<Symbol, Ty>, places: &[Option<(String, Vec<String>)>], per_impl: bool) -> Res<(Vec<V>, Vec<(usize, String, Ty, TypeExpr)>)> {
        let mut out = vec![];
        let mut backs = vec![];
        for (i, (p, v)) in params.iter().zip(vs).enumerate() {
            let as_passed = per_impl && self.m.is_trait(&p.ty);
            let t = if as_passed { v.0.clone() } else { self.m.lower(&p.ty, sub)? };
            if p.mutable {
                let place = places.get(i).cloned().flatten();
                let slot = self.mut_slot(&place, &(t.clone(), v.1), &p.name)?;
                out.push((Ty::Str, slot.clone()));
                backs.push((i, slot, t, p.ty.clone()));
            } else if as_passed {
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
                if matches!(self.m.repr(&recv)?, Ty::Union(_)) {
                    // a named union's own implementation, else each member's
                    if let Some(h) = self.m.union_name(&recv).filter(|h| self.m.resolve_method(name, h).is_some_and(|(tr, _)| self.m.impl_of(&tr, h).is_some())) {
                        return self.method_target(name, recv, h, vs, hint, &places);
                    }
                    return self.call_on_union(name, vs, hint, &places);
                }
                if self.is_dyn(&recv) || Module::head(&recv).is_some_and(|h| self.m.resolve_method(name, &h).is_some() || self.m.ctors_of(&recv).iter().any(|c| self.m.resolve_method(name, c).is_some())) {
                    return self.call_method(name, recv, vs, hint, &places);
                }
            }
        }
        if let Some(v) = self.string_builtin(name, &vs)? {
            return Ok(v);
        }
        if let Some(v) = self.channel_builtin(name, &vs, &hint)? {
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

    /// A trait method on a union's value: each member's implementation, by the member.
    fn call_on_union(&mut self, name: &str, vs: Vec<V>, hint: Option<Ty>, places: &[Option<(String, Vec<String>)>]) -> Res<V> {
        if vs.iter().any(|v| is_never(&v.0)) {
            return Ok(never());
        }
        let _ = places;
        let mutates = self.m.members(&vs[0].0).iter().filter_map(|m| Module::head(m)).find_map(|h| self.m.resolve_method(name, &h)).and_then(|(tr, m)| self.m.trait_method(&tr, &m)).is_some_and(|tm| tm.decl.params.first().is_some_and(|p| p.mutable));
        if mutates {
            // (it would write a member's value back where the union's goes)
            return self.unsupported(&format!("a `mut self` method (`{name}`) on a union"));
        }
        let ret = hint.clone().ok_or(format!("the result type of `{name}`"))?;
        let slot = self.fresh("ures");
        if has_value(&ret) {
            writeln!(self.allocas, "  {slot} = alloca {}", self.m.ll(&ret)?).unwrap();
        }
        let end = self.fresh_label("dispatched");
        let v0 = vs[0].clone();
        self.on_member(&v0, |c, m, x| {
            c.push_scope(true);
            let mut args = vec![x];
            args.extend(vs[1..].iter().cloned());
            let out = c.call_method(name, m.clone(), args, hint.clone(), &[])?;
            let out = c.coerce(out, &ret)?;
            let out = c.take(out);
            c.pop_scope()?;
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
        let r = self.fresh("ur");
        let ll = self.m.ll(&ret)?;
        self.emit(format!("{r} = load {ll}, ptr {slot}"));
        Ok(self.temp((ret, r)))
    }

    /// Call trait method `name` on a value of type `recv` (`vs` includes it,
    /// unless the method has no `self`). The implementation is chosen here,
    /// from the type; a variant with an implementation of its own is chosen
    /// by its tag.
    fn call_method(&mut self, name: &str, recv: Ty, vs: Vec<V>, hint: Option<Ty>, places: &[Option<(String, Vec<String>)>]) -> Res<V> {
        if vs.iter().any(|v| is_never(&v.0)) {
            return Ok(never());
        }
        let head = Module::head(&recv).or_else(|| self.m.union_name(&recv)).ok_or(format!("`{name}` on a `{}`", key(&recv)))?;
        // a refined alias (`Positive`) has its base type's methods
        let head = match &recv {
            Ty::Con(n, _) if crate::types::refined_alias(self.m.env, *n) => Module::head(&self.m.repr(&recv)?).unwrap_or(head),
            _ => head,
        };
        if self.m.traits.contains_key(&Symbol::new(&head)) {
            return self.dyn_call(name, recv, vs, hint, places);
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
            c.push_scope(true);
            let out = c.method_target(name, r, h, vs.clone(), hint.clone(), places)?;
            let out = c.take(out);
            c.pop_scope()?;
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
        Ok(self.temp((ret, r)))
    }

    /// Call the implementation of `name` for type `head` (on a `recv`).
    fn method_target(&mut self, name: &str, recv: Ty, head: String, vs: Vec<V>, hint: Option<Ty>, places: &[Option<(String, Vec<String>)>]) -> Res<V> {
        // `Debug` and `Display` without an impl of the type's own: the built-in forms
        let m = crate::types::split_trait_call(name).map_or(name, |(_, m)| m);
        if matches!(m, "debug" | "to_string") && vs.len() == 1 && self.m.resolve_method(name, &head).is_none_or(|(tr, _)| self.m.impl_of(&tr, &head).is_none()) {
            let s = if m == "debug" { self.debug(&vs[0])? } else { self.show(&vs[0])? };
            return Ok(self.temp((Ty::Str, s)));
        }
        let (tr, m) = self.m.resolve_method(name, &head).ok_or(format!("`{head}` has no method `{name}`"))?;
        let tm = self.m.trait_method(&tr, &m).ok_or(format!("trait `{tr}` has no method `{m}` (it has {:?})", self.m.traits.get(&Symbol::new(&tr)).map(|t| t.methods.iter().map(|x| x.decl.name.to_string()).collect::<Vec<_>>())))?;
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
        let (args, backs) = self.bind_args(&own_params, vs, &sub, places, false)?;
        let r = self.call(&ret, &symbol, &args)?;
        self.write_backs(backs, places)?;
        self.cancel_point()?;
        Ok(self.temp(r))
    }

    /// Call a function of the program (one instance of it, if generic).
    fn call_fn(&mut self, name: &str, vs: Vec<V>, hint: Option<Ty>) -> Res<V> {
        self.call_fn_at(name, vs, hint, &[])
    }

    /// `call_fn`, with the places of the arguments (for `mut` parameters).
    fn call_fn_at(&mut self, name: &str, vs: Vec<V>, hint: Option<Ty>, places: &[Option<(String, Vec<String>)>]) -> Res<V> {
        let f = self.m.env.fns.get(&Symbol::new(name)).ok_or_else(|| format!("`{name}` is not in the program"))?;
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
        // (type arguments passed as written: refined aliases by name)
        for (p, t) in self.named_targs.take().unwrap_or_default() {
            targs.insert(p, t);
        }
        let targs: Vec<Ty> = f.type_params.iter().map(|p| targs.get(p).cloned().unwrap_or(Ty::Unit)).collect();
        let sub: HashMap<Symbol, Ty> = f.type_params.iter().cloned().zip(targs.iter().cloned()).collect();
        let ret = self.m.lower(&f.ret, &sub)?;
        // a trait-typed parameter takes the implementation passed: an instance per implementation
        let impls: Vec<Option<Ty>> = f.params.iter().zip(&vs).map(|(p, v)| self.m.is_trait(&p.ty).then(|| v.0.clone())).collect();
        let params = f.params.clone();
        // an argument's own type may differ from the parameter's (a variant for its sum): same representation
        let (args, backs) = self.bind_args(&params, vs, &sub, places, true)?;
        let symbol = self.m.want_impls(name, targs.clone(), impls);
        let r = self.call(&ret, &symbol, &args)?;
        self.write_backs(backs, places)?;
        self.cancel_point()?;
        Ok(self.temp(r))
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
            "host_now" => self.call(&Ty::Int, "@strata_host_now", &[])?,
            "host_sleep" => {
                let r = self.call(&Ty::Unit, "@strata_host_sleep", &vs)?;
                // a wait: a cancelled task stops here
                self.cancel_point()?;
                r
            }
            "host_args" => {
                let r = self.call(&lst, "@strata_host_args", &[])?;
                self.temp(r)
            }
            "host_read_line" | "host_env_var" => {
                let r = self.call(&s_ty, &format!("@strata_{name}"), &vs)?;
                self.optional(&r, &t)?
            }
            "host_read_file" | "host_list_dir" | "host_read_bytes" => {
                // a value, or NULL and the failure
                let (k, m) = (self.fresh("kind"), self.fresh("msg"));
                writeln!(self.allocas, "  {k} = alloca ptr\n  {m} = alloca ptr").unwrap();
                let mut args = vs.clone();
                args.extend([(Ty::Str, k.clone()), (Ty::Str, m.clone())]);
                let r = self.call(&s_ty, &format!("@strata_{name}"), &args)?;
                let ok = self.fresh("ok");
                self.emit(format!("{ok} = icmp ne ptr {}, null", r.1));
                let okv = match name {
                    "host_list_dir" => lst,
                    "host_read_bytes" => Ty::Con(Symbol::new("List"), vec![Ty::Int]),
                    _ => Ty::Str,
                };
                let okv = (okv, r.1);
                self.outcome(&ok, Some(okv), &k, &m, &t)?
            }
            "host_code_version" => {
                let r = self.call(&s_ty, "@strata_host_code_version", &[])?;
                self.temp(r)
            }
            "host_code_snapshot" => {
                // the snapshot, or NULL and the failure (a string)
                let e = self.fresh("err");
                writeln!(self.allocas, "  {e} = alloca ptr").unwrap();
                let args = [(Ty::Str, e.clone())];
                let snap = Ty::Con(Symbol::new("std.code.Snapshot"), vec![]);
                let r = self.call(&snap, "@strata_host_code_snapshot", &args)?;
                let r = self.temp(r);
                let ok = self.fresh("ok");
                self.emit(format!("{ok} = icmp ne ptr {}, null", r.1));
                let (good, bad, end) = (self.fresh_label("ok"), self.fresh_label("err"), self.fresh_label("res"));
                self.cond_br(&bool_v(&ok), &good, &bad);
                self.block(&good);
                let okb = self.block_of(self.m.tags["Ok"], &[r])?;
                let from_ok = self.cur.clone();
                self.br(&end);
                self.block(&bad);
                let m = self.fresh("msg");
                self.emit(format!("{m} = load ptr, ptr {e}"));
                let m = self.temp((Ty::Str, m));
                let errb = self.block_of(self.m.tags["Err"], &[m])?;
                let from_err = self.cur.clone();
                self.br(&end);
                self.block(&end);
                let x = self.fresh("r");
                self.emit(format!("{x} = phi ptr [ {okb}, %{from_ok} ], [ {errb}, %{from_err} ]"));
                self.temp((t.clone(), x))
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
        // (`Some` takes it; when it is NULL there is nothing to drop)
        let v = &self.temp(v.clone());
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
        Ok(self.temp((t.clone(), r)))
    }

    /// `Ok(v)` (or `Ok(())`) when `ok`, else `Err((kind, message))` from the slots.
    fn outcome(&mut self, ok: &str, v: Option<V>, kslot: &str, mslot: &str, t: &Ty) -> Res<V> {
        // (`Ok` takes it; on a failure it is NULL)
        let v = v.map(|v| self.temp(v));
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
        let (k, m) = (self.temp((Ty::Str, k)), self.temp((Ty::Str, m)));
        let pair = self.block_of(0, &[k, m])?;
        let pair = self.temp((Ty::Con("Tuple".into(), vec![Ty::Str, Ty::Str]), pair));
        let errb = self.block_of(self.m.tags["Err"], &[pair])?;
        let from_err = self.cur.clone();
        self.br(&end);
        self.block(&end);
        let r = self.fresh("r");
        self.emit(format!("{r} = phi ptr [ {okb}, %{from_ok} ], [ {errb}, %{from_err} ]"));
        Ok(self.temp((t.clone(), r)))
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
                let et = list_elem(t).unwrap();
                let x = self.coerce(vs[1].clone(), &et)?;
                let x = self.take(x);
                let s = self.to_slot(&x)?;
                let counted = self.elements_kind(&et);
                let r = self.call(&types[0], "@strata_list_push", &[vs[0].clone(), (Ty::Int, s), counted])?;
                self.temp((vs[0].0.clone(), r.1))
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
                self.temp((opt, r))
            }
            ("take" | "drop", [t, Ty::Int]) if list_elem(t).is_some() => {
                // the count, within 0..len
                let n = self.list_len(&vs[0]);
                let (k0, k) = (self.fresh("k"), self.fresh("k"));
                self.emit(format!("{k0} = call i64 @llvm.smax.i64(i64 {}, i64 0)", vs[1].1));
                self.emit(format!("{k} = call i64 @llvm.smin.i64(i64 {k0}, i64 {})", n.1));
                let (from, to) = if name == "take" { ("0".to_string(), k) } else { (k, n.1.clone()) };
                let r = self.call(&types[0], "@strata_list_slice", &[vs[0].clone(), (Ty::Int, from), (Ty::Int, to)])?;
                self.temp((vs[0].0.clone(), r.1))
            }
            ("at", [t, Ty::Int]) if list_elem(t).is_some() && self.proven(|e| e == "index in range") => {
                let et = list_elem(t).unwrap();
                self.list_at(&vs[0], &vs[1].1, &et)?
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
                let (si, sn) = (self.shown(&vs[1])?, self.shown(&n)?);
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
                self.temp((vs[0].0.clone(), r.1))
            }
            ("to_json", [_]) => {
                let s = self.json(&vs[0])?;
                self.temp((Ty::Str, s))
            }
            ("range", [Ty::Int, Ty::Int]) => {
                let r = self.call(&Ty::Con("List".into(), vec![Ty::Int]), "@strata_range", &vs)?;
                self.temp(r)
            }
            ("print", [_]) => {
                let s = self.show(&vs[0])?;
                let r = self.call(&Ty::Unit, "@strata_print", &[(Ty::Str, s.clone())])?;
                self.drop_value(&(Ty::Str, s))?;
                r
            }
            ("to_string", [_]) => {
                let s = self.show(&vs[0])?;
                self.temp((Ty::Str, s))
            }
            ("debug", [_]) => {
                let s = self.debug(&vs[0])?;
                self.temp((Ty::Str, s))
            }
            ("len", [Ty::Str]) => self.call(&Ty::Int, "@strata_str_len", &vs)?,
            ("abs", [Ty::Int]) => {
                // |n| of a small n is small, but for -2^62
                let neg = self.int_op("ssub", &(Ty::Int, "0".into()), &vs[0]);
                let lt = self.int_compare("slt", &vs[0].1, "0");
                let r = self.fresh("abs");
                self.emit(format!("{r} = select i1 {}, i64 {}, i64 {}", lt.1, neg.1, vs[0].1));
                (Ty::Int, r)
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
            ("float", [Ty::Int]) => self.call(&Ty::Float, "@strata_int_to_float", &vs)?,
            ("float_bits", [Ty::Float]) => {
                let b = self.fresh("bits");
                self.emit(format!("{b} = bitcast double {} to i64", vs[0].1));
                let r = self.call(&Ty::Int, "@strata_int_from_u64", &[(Ty::Int, b)])?;
                self.temp(r)
            }
            ("float_from_bits", [Ty::Int]) => {
                // the low 64 bits; a NaN is not a `Float`
                let b = self.call(&Ty::Int, "@strata_int_bits", &vs)?;
                let f = self.fresh("f");
                self.emit(format!("{f} = bitcast i64 {} to double", b.1));
                let ok = self.fresh("num");
                self.emit(format!("{ok} = fcmp ord double {f}, 0.0"));
                self.option_if(&ok, &(Ty::Float, f), &Ty::Con("Option".into(), vec![Ty::Float]))?
            }
            ("sqrt", [Ty::Float]) => {
                // no NaN: the square root of a negative number is a fault
                self.call(&Ty::Float, "@strata_sqrt", &vs)?
            }
            // the float operations without their fault: `None` for a result that
            // isn't a finite number
            ("checked_add" | "checked_sub" | "checked_mul" | "checked_div" | "checked_rem", [Ty::Float, Ty::Float]) => {
                let (a, b) = (vs[0].clone(), vs[1].clone());
                let r = if name == "checked_rem" {
                    self.call(&Ty::Float, "@strata_fmod", &[a, b])?
                } else {
                    let ins = match name {
                        "checked_add" => "fadd",
                        "checked_sub" => "fsub",
                        "checked_mul" => "fmul",
                        _ => "fdiv",
                    };
                    let r = self.fresh("f");
                    self.emit(format!("{r} = {ins} double {}, {}", a.1, b.1));
                    (Ty::Float, r)
                };
                let ok = self.finite(&r.1);
                self.option_if(&ok, &r, &Ty::Con("Option".into(), vec![Ty::Float]))?
            }
            ("checked_sqrt", [Ty::Float]) => {
                let ok = self.fresh("nonneg");
                self.emit(format!("{ok} = fcmp oge double {}, 0.0", vs[0].1));
                let r = self.fresh("f");
                self.emit(format!("{r} = call double @llvm.sqrt.f64(double {})", vs[0].1));
                self.option_if(&ok, &(Ty::Float, r), &Ty::Con("Option".into(), vec![Ty::Float]))?
            }
            ("checked_floor" | "checked_ceil" | "checked_round", [Ty::Float]) => {
                // (on 0.0 when it isn't finite, so that nothing faults)
                let ok = self.finite(&vs[0].1);
                let x = self.fresh("x");
                self.emit(format!("{x} = select i1 {ok}, double {}, double 0.0", vs[0].1));
                let f = format!("@llvm.{}.f64", name.trim_start_matches("checked_"));
                let r = self.call(&Ty::Float, &f, &[(Ty::Float, x.clone())])?;
                let n = self.call(&Ty::Int, "@strata_float_to_int", &[r, (Ty::Float, x)])?;
                let n = self.temp(n);
                self.option_if(&ok, &n, &Ty::Con("Option".into(), vec![Ty::Int]))?
            }
            ("floor" | "ceil" | "round", [Ty::Float]) => {
                let r = self.call(&Ty::Float, &format!("@llvm.{name}.f64"), &vs)?;
                let r = self.call(&Ty::Int, "@strata_float_to_int", &[r, vs[0].clone()])?;
                self.temp(r)
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
        self.push_scope(true);
        f(self, &i, &x)?;
        self.pop_scope()?;
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
        if self.m.boxed(&want_r) && !self.m.boxed(&have) {
            return self.boxed_into(v, want);
        }
        if self.is_dyn(&want_r) && !self.is_dyn(&have) {
            let vt = self.vtable(&want_r, &v.0)?;
            // the pair holds the value
            let v = self.take(v);
            let s = self.to_slot(&v)?;
            let (p, a) = (self.fresh("dyn"), self.fresh("vt"));
            self.emit(format!("{p} = call ptr @strata_alloc(i64 16)"));
            self.emit(format!("{a} = ptrtoint ptr {vt} to i64"));
            self.emit(format!("store i64 {a}, ptr {p}"));
            self.set_field(&p, 0, &(Ty::Int, s))?;
            return Ok(self.temp((want.clone(), p)));
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
                let lt = want_r.clone();
                self.each(&v, &from, |c, _, x| {
                    let y = c.coerce(x.clone(), &et)?;
                    let y = c.take(y);
                    let s = c.to_slot(&y)?;
                    let cur = c.fresh("cur");
                    c.emit(format!("{cur} = load ptr, ptr {acc}"));
                    let next = c.call(&Ty::Str, "@strata_list_push", &[(Ty::Str, cur.clone()), (Ty::Int, s), (Ty::Int, "1".into())])?;
                    c.drop_value(&(lt.clone(), cur))?;
                    c.emit(format!("store ptr {}, ptr {acc}", next.1));
                    Ok(())
                })?;
                let r = self.fresh("conv");
                self.emit(format!("{r} = load ptr, ptr {acc}"));
                return Ok(self.temp((want.clone(), r)));
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
    ///
    /// The types are the trait's parameters (a supertrait's, through the
    /// subtrait's `supers`); `Self` is left to the caller (the trait type at
    /// a call, the implementation in its table). `Self` may be a
    /// parameter's or the result's whole type, not part of one.
    fn dyn_signature(&self, recv: &Ty, trm: &str, m: &str) -> Res<(&'a TraitMethod, HashMap<Symbol, Ty>)> {
        let tm = self.m.trait_method(trm, m).ok_or(format!("trait `{trm}` has no method `{m}`"))?;
        let Ty::Con(tr, args) = recv else { unreachable!() };
        let sub = self.super_sub(tr.as_str(), args, trm, 0).ok_or(format!("`{tr}` has no supertrait `{trm}`"))?;
        let whole = |t: &TypeExpr| matches!(t, TypeExpr::Named(n, a) if n == "Self" && a.is_empty());
        let mentions = |t: &TypeExpr| crate::types::mentions_self(&FnDecl { params: vec![], ret: t.clone(), ..tm.decl.clone() });
        if (mentions(&tm.decl.ret) && !whole(&tm.decl.ret)) || tm.decl.params.iter().skip(1).any(|p| mentions(&p.ty) && !(whole(&p.ty) && !p.mutable)) {
            return self.unsupported(&format!("calling `{m}` (it mentions `Self` inside another type) on a value of the trait type `{tr}`"));
        }
        if !tm.decl.type_params.is_empty() && (mentions(&tm.decl.ret) || tm.decl.params[0].mutable || tm.decl.params.iter().skip(1).any(|p| p.mutable || mentions(&p.ty))) {
            return self.unsupported(&format!("calling `{m}` (type parameters of its own, and `Self` or `mut`) on a value of the trait type `{tr}`"));
        }
        Ok((tm, sub))
    }

    /// The parameters of trait `trm` as seen from trait `tr` with arguments
    /// `args`: `tr`'s own, or a supertrait's through `tr`'s `supers`.
    fn super_sub(&self, tr: &str, args: &[Ty], trm: &str, depth: usize) -> Option<HashMap<Symbol, Ty>> {
        let td = self.m.traits.get(&Symbol::new(tr))?;
        let sub: HashMap<Symbol, Ty> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
        if tr == trm {
            return Some(sub);
        }
        if depth > 32 {
            return None;
        }
        td.supers.iter().find_map(|sup| {
            let TypeExpr::Named(n, sargs) = sup else { return None };
            let lowered: Vec<Ty> = sargs.iter().map(|a| self.m.lower(a, &sub)).collect::<Res<_>>().ok()?;
            self.super_sub(n.as_str(), &lowered, trm, depth + 1)
        })
    }

    /// Call `name` on a value of a trait type, through its table.
    fn dyn_call(&mut self, name: &str, recv: Ty, vs: Vec<V>, hint: Option<Ty>, places: &[Option<(String, Vec<String>)>]) -> Res<V> {
        let Ty::Con(tr, _) = &recv else { unreachable!() };
        let m = crate::types::split_trait_call(name).map_or(name, |(_, m)| m);
        let methods = self.dyn_methods(tr);
        let i = methods.iter().position(|(_, x)| x == m).ok_or(format!("`{tr}` has no method `{m}`"))?;
        let trm = methods[i].0.clone();
        let (tm, mut sub) = self.dyn_signature(&recv, &trm, m)?;
        sub.insert(Symbol::new("Self"), recv.clone());
        let fp = if tm.decl.type_params.is_empty() {
            self.dyn_entry(&vs[0], i + DYN_METHODS)
        } else {
            // type parameters of its own: a dispatcher for these, by the table
            let mut targs = HashMap::default();
            for (p, v) in tm.decl.params.iter().zip(&vs).skip(1) {
                self.m.bind_targs(&p.ty, &v.0, &tm.decl.type_params, &mut targs);
            }
            if let Some(rt) = &hint {
                self.m.bind_targs(&tm.decl.ret, rt, &tm.decl.type_params, &mut targs);
            }
            let targs: Vec<Ty> = tm.decl.type_params.iter().map(|p| targs.get(p).cloned().unwrap_or(Ty::Unit)).collect();
            for (p, t) in tm.decl.type_params.iter().zip(&targs) {
                sub.insert(p.clone(), t.clone());
            }
            let d = format!("@\"dispatch:{trm}::{m}@{}[{}]\"", key(&recv), targs.iter().map(key).collect::<Vec<_>>().join(", "));
            if !self.m.dispatch.iter().any(|x| x.name == d) {
                self.m.dispatch.push(Dispatch { name: d.clone(), tr_ty: recv.clone(), trm: trm.clone(), m: m.to_string(), targs, made: None });
            }
            d
        };
        let ret = self.m.lower(&tm.decl.ret, &sub)?;
        let dispatched = !tm.decl.type_params.is_empty();
        let mut args = vec![];
        let mut back = None;
        if tm.decl.params[0].mutable {
            // a copy of the pair, changed by the method, then stored in the place
            let place = places.first().cloned().flatten();
            let slot = self.mut_slot(&place, &vs[0], "self")?;
            let cur = self.fresh("cur");
            self.emit(format!("{cur} = load ptr, ptr {slot}"));
            // the copy holds the value too (through the table: its type isn't known here)
            let cur: V = (recv.clone(), cur);
            let copy = self.copy_block(&cur, &[Ty::Int], None)?;
            let dup = self.dyn_entry(&cur, DYN_DUP);
            let s = self.field(&cur, 0, &Ty::Int)?;
            self.emit(format!("call void {dup}(i64 {})", s.1));
            self.drop_value(&cur)?;
            self.emit(format!("store ptr {copy}, ptr {slot}"));
            let at = self.fresh("selfp");
            self.emit(format!("{at} = getelementptr i64, ptr {copy}, i64 1"));
            args.push((Ty::Str, at));
            back = Some((place, slot));
        } else if dispatched {
            // the dispatcher reads the table from the pair
            args.push((recv.clone(), vs[0].1.clone()));
        } else {
            let s = self.field(&vs[0], 0, &Ty::Int)?;
            args.push(s);
        }
        let mut backs = vec![];
        for (k, (p, v)) in tm.decl.params.iter().skip(1).zip(vs.into_iter().skip(1)).enumerate() {
            let t = self.m.lower(&p.ty, &sub)?;
            if p.mutable {
                let place = places.get(k + 1).cloned().flatten();
                let slot = self.mut_slot(&place, &(t.clone(), v.1), &p.name)?;
                args.push((Ty::Str, slot.clone()));
                backs.push((place, slot, t, p.ty.clone()));
                continue;
            }
            args.push(self.coerce(v, &t)?);
        }
        let r = self.call(&ret, &fp, &args)?;
        if let Some((place, slot)) = back {
            self.write_back(&place, &slot, &recv, &tm.decl.params[0].ty)?;
        }
        for (place, slot, t, pty) in backs {
            self.write_back(&place, &slot, &t, &pty)?;
        }
        self.cancel_point()?;
        Ok(self.temp(r))
    }

    /// A trait value `a` passed as `Self` to a method of `imp`'s table
    /// `vt`: its value, which must be of that same implementation.
    fn unwrap_self(&mut self, a: &V, vt: &str, imp: &Ty, m: &str) -> Res<V> {
        let (raw, mine, same) = (self.fresh("vt"), self.fresh("mine"), self.fresh("same"));
        self.emit(format!("{raw} = load i64, ptr {}", a.1));
        self.emit(format!("{mine} = ptrtoint ptr {vt} to i64"));
        self.emit(format!("{same} = icmp eq i64 {raw}, {mine}"));
        let (ok, bad) = (self.fresh_label("self.ok"), self.fresh_label("self.other"));
        self.cond_br(&bool_v(&same), &ok, &bad);
        self.block(&bad);
        let shown = self.shown(a)?;
        self.fault(Fault::Runtime, vec![Part::Lit(format!("`{m}` of `{}` takes a `{}` as `Self`, not ", key(imp), key(imp))), Part::Val(shown)])?;
        self.block(&ok);
        let s = self.field(a, 0, &Ty::Int)?;
        self.from_slot(&s.1, imp)
    }

    /// The table of `imp`'s implementation of trait type `tr_ty` (made once).
    fn vtable(&mut self, tr_ty: &Ty, imp: &Ty) -> Res<String> {
        let Ty::Con(tr, _) = tr_ty else { unreachable!() };
        let name = format!("@\"vt:{}@{}\"", key(tr_ty), key(imp));
        if !self.m.helpers.insert(name.clone()) {
            return Ok(name);
        }
        self.m.vtables.entry(key(tr_ty)).or_default().push((name.clone(), imp.clone()));
        let head = Module::head(imp).ok_or(format!("a `{}` can't be a `{tr}`", key(imp)))?;
        let ll = self.m.ll(imp)?;
        let mut entries = vec![];
        // showing it: as JSON, as `to_string` (`Display`), and as `debug`
        for (i, what) in ["json", "show", "debug"].iter().enumerate() {
            let f = format!("@\"dyn{what}:{}\"", key(imp));
            entries.push(f.clone());
            if !self.m.helpers.insert(f.clone()) {
                continue;
            }
            let mut h = Func::new(self.m, HashMap::default());
            h.block("start");
            let v = h.from_slot("%v", imp)?;
            let s = match i {
                0 => h.json(&v)?,
                1 => h.show(&v)?,
                _ => h.debug(&v)?,
            };
            h.emit(format!("ret ptr {s}"));
            let text = h.finish();
            writeln!(self.m.body, "define ptr {f}(i64 %v) {{\n{text}}}\n").unwrap();
        }
        // dropping and holding it, comparing two
        for what in ["drop", "dup", "eq"] {
            let f = format!("@\"dyn{what}:{}\"", key(imp));
            entries.push(f.clone());
            if !self.m.helpers.insert(f.clone()) {
                continue;
            }
            let mut h = Func::new(self.m, HashMap::default());
            h.block("start");
            let v = h.from_slot("%v", imp)?;
            if what == "eq" {
                let w = h.from_slot("%w", imp)?;
                let e = h.equal(&v, &w)?;
                h.emit(format!("ret i1 {}", e.1));
                let text = h.finish();
                writeln!(self.m.body, "define i1 {f}(i64 %v, i64 %w) {{\n{text}}}\n").unwrap();
                continue;
            }
            if what == "drop" {
                h.drop_value(&v)?;
            } else if h.counted(imp) {
                h.dup_value(&v);
            }
            h.emit("ret void".into());
            let text = h.finish();
            writeln!(self.m.body, "define void {f}(i64 %v) {{\n{text}}}\n").unwrap();
        }
        for (trm, m) in self.dyn_methods(tr) {
            let f = format!("@\"dyn:{trm}::{m}@{}\"", key(imp));
            entries.push(f.clone());
            let sig_ok = self.dyn_signature(tr_ty, &trm, &m).ok().filter(|(tm, _)| tm.decl.type_params.is_empty());
            let Some((tm, mut sub)) = sig_ok else {
                // not called through the table (a dispatcher calls one with
                // type parameters of its own): an entry that is never called
                writeln!(self.m.body, "define void {f}() {{\nentry:\n  unreachable\n}}\n").unwrap();
                continue;
            };
            // in the table, `Self` is the trait type: the wrapper unwraps and wraps
            sub.insert(Symbol::new("Self"), tr_ty.clone());
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
                if p.mutable {
                    // the caller's slot
                    sig.push(format!("ptr %a{k}"));
                    args.push((Ty::Str, format!("%a{k}")));
                    continue;
                }
                if has_value(&t) {
                    sig.push(format!("{} %a{k}", h.m.ll(&t)?));
                }
                let a: V = (t.clone(), format!("%a{k}"));
                if matches!(&p.ty, TypeExpr::Named(n, x) if n == "Self" && x.is_empty()) {
                    // a `Self`: a value of this same implementation, as it
                    args.push(h.unwrap_self(&a, &name, imp, &m)?);
                } else {
                    args.push(a);
                }
            }
            let self_ret = matches!(&tm.decl.ret, TypeExpr::Named(n, x) if n == "Self" && x.is_empty());
            let r = if self_ret {
                let r = h.call(imp, &symbol, &args)?;
                let r = h.temp(r);
                h.coerce(r, tr_ty)?
            } else {
                h.call(&ret, &symbol, &args)?
            };
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
    // A function value points to a block: the function's address, the
    // function that drops the block (what it captured, then the block
    // itself), then the values it captured. The function takes that block
    // first, then its parameters. One that captures nothing is a constant.

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
        let r = self.call(&rt, &fp, &all)?;
        self.cancel_point()?;
        Ok(self.temp(r))
    }

    /// A closure block: the function, its drop, how it shows (`shown`, as
    /// the interpreter's: `<closure (x) -> …>`, `<fn name>`), then the
    /// captured values (held by the block). A temporary of the current scope.
    fn closure(&mut self, fsym: &str, captured: &[V], shown: &str) -> Res<String> {
        let shown = self.m.string(shown);
        if captured.is_empty() {
            let g = format!("@\"closure:{}\"", fsym.trim_start_matches('@').trim_matches('"'));
            if self.m.helpers.insert(g.clone()) {
                writeln!(self.m.globals, "{g} = private constant {{ i64, ptr, ptr, ptr }} {{ i64 0, ptr {fsym}, ptr @strata_free, ptr {shown} }}, align 8").unwrap();
            }
            return Ok(format!("getelementptr inbounds (i8, ptr {g}, i64 8)"));
        }
        // the block's drop: what it captured, then itself
        let dsym = format!("@\"drop.{}\"", fsym.trim_start_matches('@').trim_matches('"'));
        let mut h = Func::new(self.m, HashMap::default());
        h.block("start");
        let env: V = (Ty::Str, "%a".into());
        let mut held = vec![];
        for (i, c) in captured.iter().enumerate() {
            if h.counted(&c.0) {
                held.push(h.field(&env, i + 2, &c.0)?);
            }
        }
        h.emit("call void @strata_free(ptr %a)".into());
        for v in &held {
            h.drop_value(v)?;
        }
        h.emit("ret void".into());
        let text = h.finish();
        writeln!(self.m.body, "define void {dsym}(ptr %a) {{\n{text}}}\n").unwrap();
        let p = self.fresh("clo");
        self.emit(format!("{p} = call ptr @strata_alloc(i64 {})", 8 * (captured.len() + 3)));
        let (a, d) = (self.fresh("fa"), self.fresh("fd"));
        self.emit(format!("{a} = ptrtoint ptr {fsym} to i64"));
        self.emit(format!("store i64 {a}, ptr {p}"));
        self.emit(format!("{d} = ptrtoint ptr {dsym} to i64"));
        self.set_field(&p, 0, &(Ty::Int, d))?;
        self.set_field(&p, 1, &(Ty::Str, shown))?;
        for (i, c) in captured.iter().enumerate() {
            let c = self.take(c.clone());
            self.set_field(&p, i + 2, &c)?;
        }
        self.temp((Ty::Fn(vec![], Box::new(Ty::Unit)), p.clone()));
        Ok(p)
    }

    /// `x -> body`: a function of its own, closing over the variables it uses.
    fn lambda(&mut self, e: &Expr, ps: &[LamParam], body: &Expr) -> Res<V> {
        let t = self.ty_of(e)?;
        self.closure_of(t, ps, body)
    }

    /// `spawn e`: `e` as a closure of no parameters, started as a task of
    /// the enclosing `scope`, which holds the closure and the result.
    fn spawn(&mut self, e: &Expr, body: &Expr) -> Res<V> {
        let t = self.ty_of(e)?;
        let Ty::Con(_, a) = &t else { return Err("a `spawn` without a task's type".into()) };
        let h = self.spawn_task(&a[0].clone(), body)?;
        Ok((t, h))
    }

    /// `body` started as a task (its result of type `rt`) of the innermost
    /// open scope: its handle.
    fn spawn_task(&mut self, rt: &Ty, body: &Expr) -> Res<String> {
        self.start_task(rt, body, false)
    }

    /// `spawn supervised e`: a task whose panic is its own; its result is
    /// `Ok(v)`, or `Err(Panic { … })` (built by its `fail` function).
    fn supervised_spawn(&mut self, e: &Expr, body: &Expr) -> Res<V> {
        let t = self.ty_of(e)?;
        let Ty::Con(_, a) = &t else { return Err("a `spawn supervised` without a task's type".into()) };
        let Ty::Con(_, r) = self.m.repr(&a[0])? else { return Err("a `spawn supervised` without a `Result`".into()) };
        let h = self.start_task(&r[0].clone(), body, true)?;
        Ok((t, h))
    }

    /// `body` started as a task of the innermost open scope (its result of
    /// type `rt`; a supervised task's is `Result[rt, Panic]`): its handle.
    fn start_task(&mut self, rt: &Ty, body: &Expr, supervised: bool) -> Res<String> {
        let rt = rt.clone();
        // the senders it captures: a copy of each is the task's, to close
        let mut names: Vec<String> = vars_in(body);
        names.sort();
        names.dedup();
        let mut senders = vec![];
        for x in &names {
            if let Some(l) = self.lookup(x) {
                if matches!(self.m.repr(&l.t)?, Ty::Con(n, _) if n == "Sender") {
                    senders.push(self.load(x)?);
                }
            }
        }
        let shares = self.fresh("shares");
        writeln!(self.allocas, "  {shares} = alloca [{} x ptr]", senders.len().max(1)).unwrap();
        for (i, s) in senders.iter().enumerate() {
            let p = self.fresh("share");
            self.emit(format!("{p} = getelementptr ptr, ptr {shares}, i64 {i}"));
            self.emit(format!("store ptr {}, ptr {p}", s.1));
        }
        let clo = self.closure_of(Ty::Fn(vec![], Box::new(rt.clone())), &[], body)?;
        let clo = self.take(clo);
        let (run, drop, fail) = if supervised { self.supervised_fns(&rt)? } else {
            let (run, drop) = self.task_fns(&rt)?;
            (run, drop, "null".to_string())
        };
        let h = self.fresh("task");
        self.emit(format!("{h} = call ptr @strata_spawn(ptr {}, ptr {run}, ptr {drop}, ptr {fail}, ptr {shares}, i64 {})", clo.1, senders.len()));
        Ok(h)
    }

    /// For a supervised task whose closure gives a `t`: the function that
    /// runs it and stores `Ok(v)` (nothing, if it was cancelled), the one
    /// dropping a stored `Result[t, Panic]`, and the one storing
    /// `Err(Panic { kind, message, stack })` after a panic (from the
    /// runtime's strings, which it takes).
    fn supervised_fns(&mut self, t: &Ty) -> Res<(String, String, String)> {
        let k = key(t);
        let panic = Ty::Con("Panic".into(), vec![]);
        let res = Ty::Con("Result".into(), vec![t.clone(), panic.clone()]);
        let (run, fail) = (format!("@\"task.srun:{k}\""), format!("@\"task.sfail:{k}\""));
        // (the result's drop takes the slot it is stored in)
        let (_, drop) = self.task_fns(&res)?;
        if self.m.helpers.insert(run.clone()) {
            let mut h = Func::new(self.m, HashMap::default());
            h.block("start");
            h.emit("%raw = load i64, ptr %c".into());
            h.emit("%f = inttoptr i64 %raw to ptr".into());
            let v = h.call(t, "%f", &[(Ty::Str, "%c".into())])?;
            h.emit("%unwound = call i1 @strata_was_unwound()".into());
            h.emit("br i1 %unwound, label %none, label %some".into());
            h.block("some");
            let ok = *h.m.tags.get("Ok").ok_or("no `Ok`")?;
            // (owned: moved into the block, not held once more)
            let v = h.temp(v);
            let b = h.block_of(ok, &[v])?;
            h.emit(format!("store ptr {b}, ptr %out"));
            h.emit("ret void".into());
            h.block("none");
            h.emit("ret void".into());
            let text = h.finish();
            writeln!(self.m.body, "define void {run}(ptr %c, ptr %out) {{\n{text}}}\n").unwrap();
        }
        if self.m.helpers.insert(fail.clone()) {
            let mut h = Func::new(self.m, HashMap::default());
            h.block("start");
            let ptag = *h.m.tags.get("Panic").ok_or("no `Panic`")?;
            let err = *h.m.tags.get("Err").ok_or("no `Err`")?;
            // (the strings are owned: moved into the record, as the record into `Err`)
            let fields: Vec<V> = ["%kind", "%msg", "%stack"].iter().map(|s| h.temp((Ty::Str, s.to_string()))).collect();
            let p = h.block_of(ptag, &fields)?;
            let p = h.temp((panic, p));
            let b = h.block_of(err, &[p])?;
            h.emit(format!("store ptr {b}, ptr %out"));
            h.emit("ret void".into());
            let text = h.finish();
            writeln!(self.m.body, "define void {fail}(ptr %out, ptr %kind, ptr %msg, ptr %stack) {{\n{text}}}\n").unwrap();
        }
        Ok((run, drop, fail))
    }

    /// `race { e as x => body, … }`: each `e` started as a task of a scope
    /// of the race's own; `strata_select` waits for the first to finish; the
    /// others are cancelled (they stop at their next wait), the winner's
    /// result moves out of its task, the scope ends (waiting for the others
    /// to stop), then the winner's arm runs.
    fn race(&mut self, e: &Expr, arms: &[SelectArm]) -> Res<V> {
        let want = self.ty_of(e)?;
        let n = arms.len();
        // a supervised arm's task gives a `Result[T, Panic]`
        let rts = arms
            .iter()
            .map(|a| match &a.op {
                Expr::SupervisedSpawn(x) => Ok(Ty::Con("Result".into(), vec![self.ty_of(x)?, Ty::Con("Panic".into(), vec![])])),
                op => self.ty_of(op),
            })
            .collect::<Res<Vec<Ty>>>()?;
        let mut slots = vec![];
        for t in &rts {
            slots.push(if has_value(t) {
                let s = self.fresh("won");
                writeln!(self.allocas, "  {s} = alloca {}", self.m.ll(t)?).unwrap();
                Some(s)
            } else {
                None
            });
        }
        let k = self.fresh("winner");
        let tasks = self.fresh("tasks");
        self.emit(format!("{tasks} = call ptr @strata_scope_begin()"));
        self.scoped(false, |c| {
            c.scopes.last_mut().unwrap().tasks = Some(tasks.clone());
            let ops = c.fresh("ops");
            writeln!(c.allocas, "  {ops} = alloca [{} x i64]", 4 * n.max(1)).unwrap();
            let mut handles = vec![];
            for (i, a) in arms.iter().enumerate() {
                let h = match &a.op {
                    Expr::SupervisedSpawn(x) => {
                        let inner = c.ty_of(x)?;
                        c.start_task(&inner, x, true)?
                    }
                    op => c.spawn_task(&rts[i], op)?,
                };
                let (p, q) = (c.fresh("op"), c.fresh("op"));
                c.emit(format!("{p} = getelementptr i64, ptr {ops}, i64 {}", 4 * i));
                c.emit(format!("store i64 2, ptr {p}"));
                c.emit(format!("{q} = getelementptr i64, ptr {ops}, i64 {}", 4 * i + 1));
                c.emit(format!("store ptr {h}, ptr {q}"));
                handles.push(h);
            }
            c.emit(format!("{k} = call i64 @strata_select(ptr {ops}, i64 {n})"));
            c.emit(format!("call void @strata_scope_cancel(ptr {tasks})"));
            // the winner's result, held here (its task keeps its own until the scope ends)
            let labels: Vec<String> = (0..n).map(|_| c.fresh_label("race.won")).collect();
            let (none, end) = (c.fresh_label("race.none"), c.fresh_label("race.got"));
            let cases: Vec<String> = labels.iter().enumerate().map(|(i, l)| format!("i64 {i}, label %{l}")).collect();
            c.emit(format!("switch i64 {k}, label %{none} [ {} ]", cases.join(" ")));
            for i in 0..n {
                c.block(&labels[i]);
                if let Some(slot) = &slots[i] {
                    let ht = Ty::Con("Task".into(), vec![rts[i].clone()]);
                    let v = c.await_one(&(ht, handles[i].clone()), &rts[i])?;
                    let v = c.take(v);
                    let ll = c.m.ll(&rts[i])?;
                    c.emit(format!("store {ll} {}, ptr {slot}", v.1));
                }
                c.br(&end);
            }
            // -1: the race itself was cancelled while it waited
            c.block(&none);
            c.unwind_here()?;
            c.block(&end);
            Ok((Ty::Unit, String::new()))
        })?;
        // the winner's arm, with its result
        let labels: Vec<String> = (0..n).map(|_| self.fresh_label("race.arm")).collect();
        let (none, end) = (self.fresh_label("race.none"), self.fresh_label("race.end"));
        let cases: Vec<String> = labels.iter().enumerate().map(|(i, l)| format!("i64 {i}, label %{l}")).collect();
        self.emit(format!("switch i64 {k}, label %{none} [ {} ]", cases.join(" ")));
        let mut incoming = vec![];
        for (i, a) in arms.iter().enumerate() {
            self.block(&labels[i]);
            let (rt, slot) = (rts[i].clone(), slots[i].clone());
            let v = self.branch(&want, |c| {
                c.scoped(false, |c| {
                    let got = match &slot {
                        Some(s) => {
                            let r = c.fresh("result");
                            c.emit(format!("{r} = load {}, ptr {s}", c.m.ll(&rt)?));
                            c.temp((rt.clone(), r))
                        }
                        None => (rt.clone(), String::new()),
                    };
                    if let Some(name) = &a.name {
                        c.local_owned(name, got, None)?;
                    }
                    c.expr(&a.body)
                })
            })?;
            incoming.push((v, self.cur.clone()));
            self.br(&end);
        }
        self.block(&none);
        self.emit("unreachable".into());
        self.block(&end);
        let r = self.join(&want, incoming)?;
        Ok(self.temp(r))
    }

    // ---- channels ----
    //
    // A channel is one runtime object (runtime.c, `Channel`), counted like
    // any value: its `Sender` and `Receiver` both point to it. What is sent
    // moves into it, as a slot, and moves out to the receiver.

    /// `channel()`, `channel(n)` and the operations the prelude's `Sending`
    /// and `Receiving` methods call.
    fn channel_builtin(&mut self, name: &str, vs: &[V], hint: &Option<Ty>) -> Res<Option<V>> {
        let elem = |m: &Module, t: &Ty| -> Res<Ty> {
            match m.repr(t)? {
                Ty::Con(n, a) if (n == "Sender" || n == "Receiver") && a.len() == 1 => Ok(a[0].clone()),
                other => Err(format!("`{}` is not a channel's end", key(&other))),
            }
        };
        Ok(Some(match (name, vs) {
            ("channel", [] | [_]) => {
                let Some(t) = hint.clone() else { return Err("`channel()` without its type".into()) };
                let Ty::Con(_, ends) = self.m.repr(&t)? else { return Err("`channel()` without its type".into()) };
                let et = elem(self.m, &ends[0])?;
                let cap = vs.first().map(|v| v.1.clone()).unwrap_or_else(|| "0".into());
                let drop = self.slot_drop_fn(&et)?;
                // held twice: by the sender and the receiver
                let c = self.fresh("chan");
                self.emit(format!("{c} = call ptr @strata_channel_new(i64 {cap}, ptr {drop})"));
                let b = self.fresh("ends");
                self.emit(format!("{b} = call ptr @strata_alloc(i64 24)"));
                self.emit(format!("store i64 0, ptr {b}"));
                self.set_field(&b, 0, &(ends[0].clone(), c.clone()))?;
                self.set_field(&b, 1, &(ends[1].clone(), c))?;
                self.temp((t, b))
            }
            ("channel_send", [tx, v]) => {
                let et = elem(self.m, &tx.0)?;
                let v = self.coerce(v.clone(), &et)?;
                let v = self.take(v);
                let s = self.to_slot(&v)?;
                self.emit(format!("call void @strata_channel_send(ptr {}, i64 {s})", tx.1));
                self.cancel_point()?;
                (Ty::Unit, String::new())
            }
            ("channel_close", [tx]) => {
                self.emit(format!("call void @strata_channel_close(ptr {})", tx.1));
                (Ty::Unit, String::new())
            }
            ("channel_recv", [rx]) => {
                let et = elem(self.m, &rx.0)?;
                let out = self.fresh("recvd");
                writeln!(self.allocas, "  {out} = alloca i64").unwrap();
                let ok = self.fresh("ok");
                self.emit(format!("{ok} = call i1 @strata_channel_recv(ptr {}, ptr {out})", rx.1));
                // (checked once what came is held, so leaving drops it)
                let r = self.option_from(&bool_v(&ok), &out, &et)?;
                self.cancel_point()?;
                r
            }
            _ => return Ok(None),
        }))
    }

    /// `Some` of the value in the slot at `out` (owned: it moves into the
    /// `Some`) when `ok`, else `None`.
    fn option_from(&mut self, ok: &V, out: &str, et: &Ty) -> Res<V> {
        let opt = Ty::Con("Option".into(), vec![et.clone()]);
        let (some, none, end) = (self.fresh_label("got.some"), self.fresh_label("got.none"), self.fresh_label("got.end"));
        self.cond_br(ok, &some, &none);
        self.block(&some);
        let s = self.fresh("slot");
        self.emit(format!("{s} = load i64, ptr {out}"));
        let x = self.from_slot(&s, et)?;
        let x = self.temp(x);
        let b = self.block_of(self.m.tags["Some"], &[x])?;
        let from_some = self.cur.clone();
        self.br(&end);
        self.block(&none);
        let nothing = self.m.nullary("None");
        self.br(&end);
        self.block(&end);
        let r = self.fresh("opt");
        self.emit(format!("{r} = phi ptr [ {b}, %{from_some} ], [ {nothing}, %{none} ]"));
        Ok(self.temp((opt, r)))
    }

    /// The function dropping a value of type `t` held as a slot (an `i64`),
    /// as a channel holds what was sent: `null` when there is nothing to drop.
    fn slot_drop_fn(&mut self, t: &Ty) -> Res<String> {
        if !self.counted(t) {
            return Ok("null".into());
        }
        let name = format!("@\"slot.drop:{}\"", key(t));
        if self.m.helpers.insert(name.clone()) {
            let mut h = Func::new(self.m, HashMap::default());
            h.block("start");
            let v = h.from_slot("%s", t)?;
            h.drop_value(&v)?;
            h.emit("ret void".into());
            let text = h.finish();
            writeln!(self.m.body, "define void {name}(i64 %s) {{\n{text}}}\n").unwrap();
        }
        Ok(name)
    }

    /// `select { … }`: each arm's operation described to the runtime
    /// (`strata_select`: 32 bytes each: kind (0 recv, 1 send, 2 await), the
    /// channel or task, the slot, whether a value came), which does the
    /// first that can happen; then that arm runs.
    fn select(&mut self, e: &Expr, arms: &[SelectArm]) -> Res<V> {
        let want = self.ty_of(e)?;
        let n = arms.len();
        let ops = self.fresh("ops");
        writeln!(self.allocas, "  {ops} = alloca [{} x i64]", 4 * n.max(1)).unwrap();
        // what each arm waits on (a send's value is taken: the channel or the
        // arm that loses drops it)
        let mut sent: Vec<Option<V>> = vec![];
        let mut targets: Vec<V> = vec![];
        // the timers of clocks other than the system's: (arm, clock, its type, deadline)
        let mut other_clocks: Vec<(usize, V, Ty, String)> = vec![];
        let instant = Ty::Con(Symbol::new("std.time.Instant"), vec![]);
        for (i, a) in arms.iter().enumerate() {
            let (kind, target, value) = match &a.op {
                Expr::Method(r, _, args) if args.is_empty() => (0, self.expr(r)?, None),
                // `clock.after(d)`: a timer, its deadline `d` after now in that
                // clock's time. The system clock's the runtime waits for itself
                // (kind 3); another clock is asked for its time again whenever
                // the select looks (kind 4: ready once it has passed).
                Expr::Method(clock, m, args) if m == "after" && args.len() == 1 => {
                    // (its type in this instance: a function taking a `Clock` is
                    // compiled per implementation, so the system's is seen here)
                    let cv = self.expr(clock)?;
                    let ct = cv.0.clone();
                    let d = self.expr(&args[0])?;
                    let p = self.fresh("op");
                    self.emit(format!("{p} = getelementptr i64, ptr {ops}, i64 {}", 4 * i + 2));
                    let deadline = self.fresh("deadline");
                    if matches!(&ct, Ty::Con(n, _) if n == "std.time.SystemClock") {
                        let now = self.fresh("now");
                        self.emit(format!("{now} = call i64 @strata_host_now()"));
                        self.emit(format!("{deadline} = add i64 {now}, {}", d.1));
                        self.emit(format!("store i64 {deadline}, ptr {p}"));
                        (3, (Ty::Unit, "null".to_string()), None)
                    } else {
                        let t0 = self.call_method(&crate::types::trait_call("std.time.Clock", "std.time.now"), ct.clone(), vec![cv.clone()], Some(instant.clone()), &[])?;
                        self.emit(format!("{deadline} = add i64 {}, {}", t0.1, d.1));
                        self.emit(format!("store i64 0, ptr {p}"));
                        other_clocks.push((i, cv, ct, deadline));
                        (4, (Ty::Unit, "null".to_string()), None)
                    }
                }
                Expr::Method(s, _, args) => {
                    let tx = self.expr(s)?;
                    let et = match self.m.repr(&tx.0)? {
                        Ty::Con(_, a) => a[0].clone(),
                        _ => return Err("`send` in a `select` without a sender".into()),
                    };
                    let v = self.expr(&args[0])?;
                    let v = self.coerce(v, &et)?;
                    let v = self.take(v);
                    (1, tx, Some(v))
                }
                Expr::Await(h) => (2, self.expr(h)?, None),
                _ => return Err("a `select` arm that isn't `recv`, `send` or `await`".into()),
            };
            let at = |k: usize| format!("{}", 4 * i + k);
            let p = self.fresh("op");
            self.emit(format!("{p} = getelementptr i64, ptr {ops}, i64 {}", at(0)));
            self.emit(format!("store i64 {kind}, ptr {p}"));
            let q = self.fresh("op");
            self.emit(format!("{q} = getelementptr i64, ptr {ops}, i64 {}", at(1)));
            self.emit(format!("store ptr {}, ptr {q}", target.1));
            if let Some(v) = &value {
                let s = self.to_slot(v)?;
                let r = self.fresh("op");
                self.emit(format!("{r} = getelementptr i64, ptr {ops}, i64 {}", at(2)));
                self.emit(format!("store i64 {s}, ptr {r}"));
            }
            sent.push(value);
            targets.push(target);
        }
        // another clock's timers: ask it its time, then look; again while
        // `strata_select` says so (-2)
        let poll = self.fresh_label("sel.poll");
        if !other_clocks.is_empty() {
            self.br(&poll);
            self.block(&poll);
            for (i, cv, ct, deadline) in &other_clocks {
                let now = self.call_method(&crate::types::trait_call("std.time.Clock", "std.time.now"), ct.clone(), vec![cv.clone()], Some(instant.clone()), &[])?;
                let (ready, flag, p) = (self.fresh("ready"), self.fresh("flag"), self.fresh("op"));
                self.emit(format!("{ready} = icmp sge i64 {}, {deadline}", now.1));
                self.emit(format!("{flag} = zext i1 {ready} to i64"));
                self.emit(format!("{p} = getelementptr i64, ptr {ops}, i64 {}", 4 * i + 2));
                self.emit(format!("store i64 {flag}, ptr {p}"));
            }
        }
        let k = self.fresh("chosen");
        self.emit(format!("{k} = call i64 @strata_select(ptr {ops}, i64 {n})"));
        if !other_clocks.is_empty() {
            let (again, go) = (self.fresh("again"), self.fresh_label("sel.go"));
            self.emit(format!("{again} = icmp eq i64 {k}, -2"));
            self.emit(format!("br i1 {again}, label %{poll}, label %{go}"));
            self.block(&go);
        }
        let labels: Vec<String> = (0..n).map(|_| self.fresh_label("sel.arm")).collect();
        let (none, end) = (self.fresh_label("sel.none"), self.fresh_label("sel.end"));
        let cases: Vec<String> = labels.iter().enumerate().map(|(i, l)| format!("i64 {i}, label %{l}")).collect();
        self.emit(format!("switch i64 {k}, label %{none} [ {} ]", cases.join(" ")));
        let mut incoming = vec![];
        for (i, a) in arms.iter().enumerate() {
            self.block(&labels[i]);
            let v = self.branch(&want, |c| {
                // the values the other arms would have sent
                for (j, s) in sent.iter().enumerate() {
                    if let (Some(s), true) = (s, j != i) {
                        c.drop_value(s)?;
                    }
                }
                c.scoped(false, |c| {
                    let got: Option<V> = match &a.op {
                        Expr::Method(_, _, args) if args.is_empty() => {
                            let et = match c.m.repr(&targets[i].0)? {
                                Ty::Con(_, a) => a[0].clone(),
                                _ => return Err("`recv` in a `select` without a receiver".into()),
                            };
                            let (okp, ok, slot) = (c.fresh("okp"), c.fresh("ok"), c.fresh("slotp"));
                            c.emit(format!("{okp} = getelementptr i64, ptr {ops}, i64 {}", 4 * i + 3));
                            let okw = c.fresh("okw");
                            c.emit(format!("{okw} = load i64, ptr {okp}"));
                            c.emit(format!("{ok} = icmp ne i64 {okw}, 0"));
                            c.emit(format!("{slot} = getelementptr i64, ptr {ops}, i64 {}", 4 * i + 2));
                            Some(c.option_from(&bool_v(&ok), &slot, &et)?)
                        }
                        Expr::Await(_) => {
                            let Ty::Con(_, r) = c.m.repr(&targets[i].0)? else { return Err("`await` in a `select` without a task".into()) };
                            Some(c.await_one(&targets[i], &r[0])?)
                        }
                        _ => None,
                    };
                    if let (Some(name), Some(v)) = (&a.name, got) {
                        c.local_owned(name, v, None)?;
                    }
                    c.expr(&a.body)
                })
            })?;
            incoming.push((v, self.cur.clone()));
            self.br(&end);
        }
        // -1: this task was cancelled while it waited
        self.block(&none);
        self.unwind_here()?;
        self.block(&end);
        let r = self.join(&want, incoming)?;
        Ok(self.temp(r))
    }

    /// For a task whose result has type `t`: the function that runs its
    /// closure and stores the result, and the one that drops a stored
    /// result (`null` when there is nothing to drop).
    fn task_fns(&mut self, t: &Ty) -> Res<(String, String)> {
        let k = key(t);
        let run = format!("@\"task.run:{k}\"");
        let drop = if self.counted(t) { format!("@\"task.drop:{k}\"") } else { "null".to_string() };
        if self.m.helpers.insert(run.clone()) {
            let ll = self.m.ll(t)?;
            let call = if ll == "void" {
                "  call void %f(ptr %c)\n".to_string()
            } else {
                format!("  %r = call {ll} %f(ptr %c)\n  store {ll} %r, ptr %out\n")
            };
            writeln!(self.m.body, "define void {run}(ptr %c, ptr %out) {{\nstart:\n  %raw = load i64, ptr %c\n  %f = inttoptr i64 %raw to ptr\n{call}  ret void\n}}\n").unwrap();
        }
        if drop != "null" && self.m.helpers.insert(drop.clone()) {
            let ll = self.m.ll(t)?;
            let mut h = Func::new(self.m, HashMap::default());
            h.block("start");
            h.emit(format!("%v = load {ll}, ptr %out"));
            h.drop_value(&(t.clone(), "%v".into()))?;
            h.emit("ret void".into());
            let text = h.finish();
            writeln!(self.m.body, "define void {drop}(ptr %out) {{\n{text}}}\n").unwrap();
        }
        Ok((run, drop))
    }

    /// The result of the task `h` (type `rt`), held: the task keeps its own
    /// until its scope ends, so `await` can be asked twice.
    fn await_one(&mut self, h: &V, rt: &Ty) -> Res<V> {
        let p = self.fresh("out");
        self.emit(format!("{p} = call ptr @strata_await(ptr {})", h.1));
        self.cancel_point()?;
        if !has_value(rt) {
            return Ok((rt.clone(), String::new()));
        }
        let ll = self.m.ll(rt)?;
        let r = self.fresh("result");
        self.emit(format!("{r} = load {ll}, ptr {p}"));
        Ok(self.hold((rt.clone(), r)))
    }

    /// `await` of `v`, of type `ht`: a handle, or a tuple or list of them; `rt` the result's type.
    fn awaited(&mut self, v: &V, ht: &Ty, rt: &Ty) -> Res<V> {
        match self.m.repr(ht)? {
            Ty::Con(n, a) if n == "Task" => self.await_one(v, &a[0]),
            Ty::Con(n, hs) if n == "Tuple" => {
                let Ty::Con(_, rs) = self.m.repr(rt)? else { return Err("`await` of a tuple without a tuple's type".into()) };
                let mut vs = vec![];
                for (i, (h, r)) in hs.iter().zip(&rs).enumerate() {
                    let x = self.field(v, i, h)?;
                    vs.push(self.await_one(&x, r)?);
                }
                let b = self.block_of(0, &vs)?;
                Ok(self.temp((rt.clone(), b)))
            }
            Ty::Con(n, a) if n == "List" => {
                let ht = a[0].clone();
                let et = self.elem_ty(rt)?;
                let acc = self.fresh("awaited");
                writeln!(self.allocas, "  {acc} = alloca ptr").unwrap();
                let len = self.list_len(v);
                let l0 = self.call(rt, "@strata_list_new", &[len])?;
                self.emit(format!("store ptr {}, ptr {acc}", l0.1));
                let (lt, kind) = (rt.clone(), self.elements_kind(&et));
                self.each(v, &ht, |c, _, x| {
                    let y = c.await_one(x, &et)?;
                    let y = c.take(y);
                    let s = c.to_slot(&y)?;
                    let cur = c.fresh("cur");
                    c.emit(format!("{cur} = load ptr, ptr {acc}"));
                    let next = c.call(&Ty::Str, "@strata_list_push", &[(Ty::Str, cur.clone()), (Ty::Int, s), kind.clone()])?;
                    c.drop_value(&(lt.clone(), cur))?;
                    c.emit(format!("store ptr {}, ptr {acc}", next.1));
                    Ok(())
                })?;
                let r = self.fresh("awaited");
                self.emit(format!("{r} = load ptr, ptr {acc}"));
                Ok(self.temp((rt.clone(), r)))
            }
            other => Err(format!("`await` of `{}`", key(&other))),
        }
    }

    /// A closure of type `t` (a function type) with parameters `ps` and `body`.
    fn closure_of(&mut self, t: Ty, ps: &[LamParam], body: &Expr) -> Res<V> {
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
            let val = l.field(&env, i + 2, &v.0)?;
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
        l.unwound_exit(&rt, false)?;
        let text = l.finish();
        let ll = self.m.ll(&rt)?;
        writeln!(self.m.body, "define {ll} {fsym}({}) {{\n{text}}}\n", sig.join(", ")).unwrap();
        let shown = format!("<closure ({}) -> …>", ps.iter().map(|p| p.name.to_string()).collect::<Vec<_>>().join(", "));
        let p = self.closure(&fsym, &captured, &shown)?;
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
        let keep = l.take_out(&r);
        l.drop_from(0, keep.as_deref())?;
        if has_value(&rt) {
            let ll = l.m.ll(&rt)?;
            l.emit(format!("ret {ll} {}", r.1));
        } else {
            l.emit("ret void".into());
        }
        let text = l.finish();
        let ll = self.m.ll(&rt)?;
        writeln!(self.m.body, "define {ll} {fsym}({}) {{\n{text}}}\n", sig.join(", ")).unwrap();
        let p = self.closure(&fsym, &[], &format!("<fn {name}>"))?;
        Ok((t, p))
    }

    /// The order `sort` uses, as a closure `(a, b) -> a < b` on values of type `t`.
    fn less(&mut self, t: &Ty) -> Res<V> {
        let cmp = self.helper("cmp", t)?;
        let fsym = format!("@\"less:{}\"", key(t));
        if self.m.helpers.insert(fsym.clone()) {
            if has_value(t) {
                let ll = self.m.ll(t)?;
                writeln!(self.m.body, "define i1 {fsym}(ptr %env, {ll} %a, {ll} %b) {{\nentry:\n  %c = call i64 {cmp}({ll} %a, {ll} %b)\n  %r = icmp slt i64 %c, 0\n  ret i1 %r\n}}\n").unwrap();
            } else {
                // values without one (`()`) are all equal: never less
                writeln!(self.m.body, "define i1 {fsym}(ptr %env) {{\nentry:\n  ret i1 false\n}}\n").unwrap();
            }
        }
        let p = self.closure(&fsym, &[], "<fn order>")?;
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
            let k = format!("constructing `{c}`");
            if let Some(r) = self.m.refinement(c).filter(|_| !self.proven(|e| same_check(e, &k))) {
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
            CtorArgs::Positional(xs) => self.operands(&xs.iter().collect::<Vec<_>>())?,
            CtorArgs::Record(given) => {
                // in declaration order
                let mut es = vec![];
                for f in &fs {
                    let x = given.iter().find(|(n, _)| f.name.as_ref().map(|s| s.as_str()) == Some(n.as_str())).ok_or(format!("`{c}` needs its field `{}`", f.name.clone().unwrap_or_default()))?;
                    es.push(&x.1);
                }
                self.operands(&es)?
            }
            CtorArgs::Update(base, changed) => {
                let b = self.expr(base)?;
                if is_never(&b.0) {
                    return Ok(b);
                }
                // the fields not changed are read from `b` after the changes are evaluated
                let b = if changed.iter().any(|(_, x)| assigns(x)) { self.hold(b) } else { b };
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
        // every field a constant: the value is one too, built once (its checks are made above)
        if let Some(b) = self.constant_block(self.m.tags[c], &vals)? {
            let v = (t, b);
            self.check_type_refinement(c, &v)?;
            return Ok(v);
        }
        let p = self.block_of(self.m.tags[c], &vals)?;
        let v = self.temp((t, p));
        self.check_type_refinement(c, &v)?;
        Ok(v)
    }

    fn get_field(&mut self, e: &Expr, r: &Expr, f: &str) -> Res<V> {
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
        // at different places (or not in every variant): by the variant
        let want = self.ty_of(e).unwrap_or(ft);
        let ll = self.m.ll(&want)?;
        let slot = self.fresh("fld");
        if has_value(&want) {
            writeln!(self.allocas, "  {slot} = alloca {ll}").unwrap();
        }
        let end = self.fresh_label("got");
        self.on_ctor(&v, &end, |s, c| {
            match at.iter().find(|x| x.0 == c) {
                Some((_, i, ft)) => {
                    let x = s.field(&v, *i, ft)?;
                    let x = s.coerce(x, &want)?;
                    if has_value(&want) {
                        s.emit(format!("store {ll} {}, ptr {slot}", x.1));
                    }
                    s.br(&end);
                }
                None => {
                    // as the interpreter: a variant with named fields, or another value
                    let named = s.m.fields(c, &s.m.ctor_args(c, &t))?.first().is_some_and(|x| x.name.is_some());
                    if named {
                        s.fault(Fault::Runtime, vec![Part::Lit(format!("no field `{f}`"))])?;
                    } else {
                        let shown = s.shown(&v)?;
                        s.fault(Fault::Runtime, vec![Part::Lit(format!("no field `{f}` on ")), Part::Val(shown)])?;
                    }
                }
            }
            Ok(())
        })?;
        self.block(&end);
        if !has_value(&want) {
            return Ok((want, String::new()));
        }
        let r = self.fresh("fv");
        self.emit(format!("{r} = load {ll}, ptr {slot}"));
        Ok((want, r))
    }

    fn matching(&mut self, e: &Expr, scrut: &Expr, as_name: Option<&str>, arms: &[Arm]) -> Res<V> {
        let want = self.ty_of(e)?;
        let v = self.expr(scrut)?;
        if is_never(&v.0) {
            return Ok(v);
        }
        // the arms borrow the matched value: it is a temporary of the
        // enclosing scope, or a variable an arm's bindings no longer need
        // once they hold what they bound
        self.push_scope(false);
        if let Some(n) = as_name {
            self.local_owned(n, v.clone(), None)?;
        }
        let end = self.fresh_label("endmatch");
        let mut incoming = vec![];
        for arm in arms {
            let next = self.fresh_label("next_arm");
            self.push_scope(false);
            self.fail_marks.insert(next.clone(), 0);
            self.pattern(&arm.pat, &v, &next)?;
            let r = self.expr(&arm.body)?;
            let r = self.coerce(r, &want)?;
            let r = self.take(r);
            self.pop_scope()?;
            incoming.push((r, self.cur.clone()));
            self.br(&end);
            self.block(&next);
        }
        let shown = self.shown(&v)?;
        self.fault(Fault::Runtime, vec![Part::Lit("no match arm for ".into()), Part::Val(shown)])?;
        self.block(&end);
        let r = self.join(&want, incoming)?;
        self.pop_scope()?;
        Ok(self.temp(r))
    }

    /// Match `v` against a pattern: jump to `fail` if it doesn't match, else
    /// go on with its names bound in the current scope.
    ///
    /// Bound names own what they hold. A jump to `fail` first drops what
    /// was bound since `fail` was noted (`fail_marks`; all of the scope's
    /// when it wasn't).
    fn pattern(&mut self, p: &Pattern, v: &V, fail: &str) -> Res<()> {
        let test = |s: &mut Self, ok: V| -> Res<()> {
            let next = s.fresh_label("matched");
            let i = s.var_scope();
            let mark = s.fail_marks.get(fail).copied().unwrap_or(0).min(s.scopes[i].owned.len());
            let bound: Vec<(Ty, String)> = s.scopes[i].owned[mark..].to_vec();
            if bound.is_empty() {
                s.cond_br(&ok, &next, fail);
            } else {
                let unbind = s.fresh_label("unbind");
                s.cond_br(&ok, &next, &unbind);
                s.block(&unbind);
                for (t, slot) in bound.iter().rev() {
                    let x = s.fresh("old");
                    let ll = if s.is_int(t) { "i64" } else { "ptr" };
                    s.emit(format!("{x} = load {ll}, ptr {slot}"));
                    s.drop_value(&(t.clone(), x))?;
                }
                s.br(fail);
            }
            s.block(&next);
            Ok(())
        };
        // on a trait value: a constructor's (or a type's) pattern tests the
        // implementation first (the pair's table), then matches its value
        if self.is_dyn(&v.0) {
            if let Pattern::Ctor(c, _) = p {
                let tn = self.m.env.ctors.get(c).cloned().or_else(|| self.m.env.types.contains_key(c).then(|| c.clone()));
                if let Some(tn) = tn {
                    let imp = Ty::Con(tn.clone(), vec![]);
                    let candidates: Vec<(String, Ty)> = if self.m.env.types.get(&tn).is_some_and(|td| td.params.is_empty()) {
                        vec![(self.vtable(&v.0, &imp)?, imp)]
                    } else {
                        self.m.vtables.get(&key(&v.0)).cloned().unwrap_or_default().into_iter().filter(|(_, t)| Module::head(t).as_deref() == Some(tn.as_str())).collect()
                    };
                    let raw = self.fresh("vt");
                    self.emit(format!("{raw} = load i64, ptr {}", v.1));
                    let mut acc = bool_v("false");
                    for (vt, _) in &candidates {
                        let (mine, e, o) = (self.fresh("mine"), self.fresh("is"), self.fresh("or"));
                        self.emit(format!("{mine} = ptrtoint ptr {vt} to i64"));
                        self.emit(format!("{e} = icmp eq i64 {raw}, {mine}"));
                        self.emit(format!("{o} = or i1 {}, {e}", acc.1));
                        acc = bool_v(&o);
                    }
                    test(self, acc)?;
                    let Some((_, imp)) = candidates.into_iter().next() else { return Ok(()) };
                    let s = self.field(v, 0, &Ty::Int)?;
                    let x = self.from_slot(&s.1, &imp)?;
                    return self.pattern(p, &x, fail);
                }
            }
        }
        // on a union: a pattern of one member's values tests the member first
        if self.m.boxed(&v.0) {
            let ms = self.m.members(&v.0);
            let member = match p {
                Pattern::Int(_) => ms.iter().find(|m| matches!(m, Ty::Int)).cloned(),
                Pattern::Bool(_) => ms.iter().find(|m| matches!(m, Ty::Bool)).cloned(),
                Pattern::Str(_) => ms.iter().find(|m| matches!(m, Ty::Str)).cloned(),
                Pattern::List(..) => ms.iter().find(|m| matches!(self.m.repr(m), Ok(Ty::Con(l, _)) if l == "List")).cloned(),
                Pattern::Tuple(..) => ms.iter().find(|m| matches!(self.m.repr(m), Ok(Ty::Con(t, _)) if t == "Tuple")).cloned(),
                _ => None,
            };
            if let Some(m) = member {
                let is = self.holds_member(v, &m)?;
                test(self, is)?;
                let x = self.narrow(v, &m)?;
                return self.pattern(p, &x, fail);
            }
        }
        match p {
            Pattern::Wild => {}
            Pattern::Bind(x) => self.bind(x, v)?,
            Pattern::Int(n) => {
                let lit = self.int_lit(&n.to_string());
                let ok = self.apply(BinOp::Eq, v, &lit)?;
                test(self, ok)?;
            }
            Pattern::Bool(b) => {
                let ok = self.apply(BinOp::Eq, v, &bool_v(&b.to_string()))?;
                test(self, ok)?;
            }
            Pattern::Str(s) => {
                let l = self.string(s);
                let ok = self.apply(BinOp::Eq, v, &l)?;
                test(self, ok)?;
            }
            Pattern::Cond(c) => {
                // the condition sees the variables in scope, and `it`
                self.push_scope(false);
                let slot = self.local("it", &v.0, None)?;
                self.store(v, &slot)?;
                let ok = self.expr(c)?;
                self.pop_scope()?;
                test(self, ok)?;
            }
            Pattern::For(..) => return self.unsupported("a match comprehension outside a derived method"),
            Pattern::Or(ps) => {
                // each alternative binds the same names, into the same slots;
                // one that fails drops what it bound before the next is tried
                let matched = self.fresh_label("alt.matched");
                let scope = self.var_scope();
                let mark = self.scopes[scope].owned.len();
                for (i, alt) in ps.iter().enumerate() {
                    let next = if i + 1 == ps.len() { fail.to_string() } else { self.fresh_label("alt.next") };
                    if i + 1 < ps.len() {
                        self.fail_marks.insert(next.clone(), mark);
                    }
                    self.pattern(alt, v, &next)?;
                    self.br(&matched);
                    if i + 1 < ps.len() {
                        self.scopes[scope].owned.truncate(mark);
                        self.block(&next);
                    }
                }
                self.block(&matched);
            }
            Pattern::Ctor(..) if self.m.env.type_pattern(p).is_some() => {
                let ok = self.holds(&self.m.env.type_pattern(p).unwrap(), v)?;
                test(self, ok)?;
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
                test(self, ok)?;
                let fs = self.m.fields(c, &self.m.ctor_args(c, &v.0))?;
                match cp {
                    CtorPat::None | CtorPat::Type(_) => {}
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
                test(self, bool_v(&ok))?;
                for (i, ip) in items.iter().enumerate() {
                    let x = self.list_at(v, &i.to_string(), &et)?;
                    self.pattern(ip, &x, fail)?;
                }
                if let Some(r) = rest.as_ref().filter(|r| *r != "_") {
                    let tail = self.call(&v.0, "@strata_list_slice", &[v.clone(), (Ty::Int, items.len().to_string()), n])?;
                    let tail = self.temp(tail);
                    self.bind(r, &tail)?;
                }
            }
        }
        Ok(())
    }

    /// Bind a name in the current scope (again, in the same slot, for another
    /// alternative of an or-pattern).
    fn bind(&mut self, x: &str, v: &V) -> Res<()> {
        let i = self.var_scope();
        let slot = match self.scopes[i].vars.get(x) {
            Some(l) => l.slot.clone(),
            None => self.local(x, &v.0, None)?,
        };
        let v = self.take(v.clone());
        self.store(&v, &slot)?;
        self.own_slot(&v.0, &slot);
        Ok(())
    }

    fn stmt(&mut self, s: &Stmt) -> Res<()> {
        match s {
            Stmt::ComptimeFor(..) => return self.unsupported("`comptime for` outside a derived method"),
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
                    Stmt::Var(n, ty, _) => self.local_owned(n, v, ty.clone())?,
                    Stmt::Let(Pattern::Bind(n), _, _) => self.local_owned(n, v, None)?,
                    Stmt::Let(p, _, _) => {
                        let (ok, fail) = (self.fresh_label("let.ok"), self.fresh_label("let.fail"));
                        let i = self.var_scope();
                        self.fail_marks.insert(fail.clone(), self.scopes[i].owned.len());
                        self.pattern(p, &v, &fail)?;
                        self.br(&ok);
                        self.block(&fail);
                        let shown = self.shown(&v)?;
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
                self.replace(&l, new)?;
            }
            Stmt::While(c, body) => {
                let (head, inner, end) = (self.fresh_label("while"), self.fresh_label("do"), self.fresh_label("endwhile"));
                self.br(&head);
                self.block(&head);
                self.loop_invariants(body)?;
                self.push_scope(true);
                let cv = self.expr(c)?;
                self.pop_scope()?;
                self.cond_br(&cv, &inner, &end);
                self.block(&inner);
                let depth = self.scopes.len();
                self.push_scope(true);
                self.loop_body(body, &head, &end, depth)?;
                self.block(&end);
            }
            Stmt::Loop(body) => {
                let (head, end) = (self.fresh_label("loop"), self.fresh_label("endloop"));
                self.br(&head);
                self.block(&head);
                self.loop_invariants(body)?;
                let depth = self.scopes.len();
                self.push_scope(true);
                self.loop_body(body, &head, &end, depth)?;
                self.block(&end);
            }
            Stmt::For(p, xs, body) => {
                let is_range = matches!(xs, Expr::Call(f, args) if matches!(&**f, Expr::Var(n) if n == "range") && args.len() == 2 && !self.m.env.fns.contains_key(&Symbol::new("range")));
                if !is_range {
                    let v = self.expr(xs)?;
                    if is_never(&v.0) {
                        return Ok(());
                    }
                    // the loop holds the list (the body may change the variable it came from)
                    let v = self.take(v);
                    let v = self.temp(v);
                    let et = self.elem_ty(&v.0)?;
                    let counter = self.fresh("idx");
                    writeln!(self.allocas, "  {counter} = alloca i64").unwrap();
                    self.emit(format!("store i64 0, ptr {counter}"));
                    let (head, inner, step, end) = (self.fresh_label("for"), self.fresh_label("for.body"), self.fresh_label("for.next"), self.fresh_label("endfor"));
                    self.br(&head);
                    self.block(&head);
                    self.loop_invariants(body)?;
                    let i = self.fresh("i");
                    self.emit(format!("{i} = load i64, ptr {counter}"));
                    let n = self.list_len(&v);
                    let more = self.apply(BinOp::Lt, &(Ty::Int, i.clone()), &n)?;
                    self.cond_br(&more, &inner, &end);
                    self.block(&inner);
                    let x = self.list_at(&v, &i, &et)?;
                    let depth = self.scopes.len();
                    self.push_scope(false);
                    let fail = self.fresh_label("for.nomatch");
                    self.fail_marks.insert(fail.clone(), 0);
                    self.pattern(p, &x, &fail)?;
                    self.loop_body(body, &step, &end, depth)?;
                    self.block(&fail);
                    let shown = self.shown(&x)?;
                    self.fault(Fault::Runtime, vec![Part::Lit("for-loop pattern did not match ".into()), Part::Val(shown)])?;
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
                let counter = self.fresh("i");
                writeln!(self.allocas, "  {counter} = alloca i64").unwrap();
                self.store(&lo, &counter)?;
                let (head, inner, step, end) = (self.fresh_label("for"), self.fresh_label("for.body"), self.fresh_label("for.next"), self.fresh_label("endfor"));
                self.br(&head);
                self.block(&head);
                self.loop_invariants(body)?;
                let i = self.fresh("i");
                self.emit(format!("{i} = load i64, ptr {counter}"));
                let more = self.apply(BinOp::Lt, &(Ty::Int, i.clone()), &hi)?;
                self.cond_br(&more, &inner, &end);
                self.block(&inner);
                let depth = self.scopes.len();
                self.push_scope(false);
                let x: V = (Ty::Int, i.clone());
                let fail = self.fresh_label("for.nomatch");
                self.fail_marks.insert(fail.clone(), 0);
                self.pattern(p, &x, &fail)?;
                self.loop_body(body, &step, &end, depth)?;
                self.block(&fail);
                let shown = self.shown(&x)?;
                self.fault(Fault::Runtime, vec![Part::Lit("for-loop pattern did not match ".into()), Part::Val(shown)])?;
                self.block(&step);
                let next = self.fresh("i");
                self.emit(format!("{next} = add i64 {i}, 1"));
                self.emit(format!("store i64 {next}, ptr {counter}"));
                self.br(&head);
                self.block(&end);
            }
            Stmt::Break | Stmt::Continue => {
                let (cont, brk, depth) = self.loops.last().cloned().ok_or("`break` or `continue` outside a loop")?;
                self.drop_from(depth, None)?;
                self.br(if matches!(s, Stmt::Break) { &brk } else { &cont });
                self.dead();
            }
            Stmt::Expr(e) => {
                self.expr(e)?;
            }
            Stmt::Assert(x) => {
                let k = format!("assert {}", crate::printer::expr(x, 0));
                if !self.proven(|e| same_check(e, &k)) {
                    self.check_true(x, "assertion failed")?
                }
            }
            // (checked at the loop's head: `loop_invariants`)
            Stmt::Invariant(_) => {}
            Stmt::Comment(_) => {}
        }
        Ok(())
    }

    /// A loop's `invariant`s, checked at its head: on entry, after every
    /// pass and `continue`, and when it ends normally.
    fn loop_invariants(&mut self, body: &Expr) -> Res<()> {
        let invs: Vec<&Expr> = crate::types::loop_invariants(body).collect();
        if invs.is_empty() {
            return Ok(());
        }
        self.push_scope(true);
        for x in invs {
            // proven on entry and where every pass ends
            let k = format!("invariant `{}` ", crate::printer::expr(x, 0));
            if !self.proven(|e| e.starts_with(&k)) {
                self.check_true(x, "invariant failed")?;
            }
        }
        self.pop_scope()
    }

    /// Fault with `what: x` unless `x` is true.
    fn check_true(&mut self, x: &Expr, what: &str) -> Res<()> {
        {
            {
                // a comparison shows both sides when it fails
                let (ok, sides) = match x {
                    Expr::Bin(op, l, r) if !matches!(op, BinOp::And | BinOp::Or | BinOp::Implies) => {
                        let ab = self.operands(&[l, r])?;
                        let (a, b) = (ab[0].clone(), ab[1].clone());
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
                let mut parts = vec![Part::Lit(format!("{what}: {}", crate::printer::expr(x, 0)))];
                if let Some((op, a, b)) = sides {
                    let (sa, sb) = (self.shown(&a)?, self.shown(&b)?);
                    parts.extend([Part::Lit(format!(" (left of {} = ", op.symbol())), Part::Val(sa), Part::Lit(", right = ".into()), Part::Val(sb), Part::Lit(")".into())]);
                }
                self.fault(Fault::Assert, parts)?;
                self.block(&pass);
            }
        }
        Ok(())
    }

    /// `cur` with the field at `path` replaced by `v` (a new block per level),
    /// each changed record checked again as the interpreter does.
    fn set_path(&mut self, cur: &V, path: &[String], v: V, shown: &str) -> Res<V> {
        let Some((f, _)) = path.split_first() else { return Ok(v) };
        let t = self.m.repr(&cur.0)?;
        if let Ty::Con(n, args) = &t {
            if n == "Tuple" {
                let i: usize = f.parse().map_err(|_| format!("tuples have no field `{f}`"))?;
                if i >= args.len() {
                    return Err(format!("`{}` has no field `{f}`", key(&t)));
                }
                return self.set_in(cur, None, args.clone(), vec![], i, path, v, shown);
            }
        }
        let cs = self.m.ctors_of(&t);
        let place = |s: &Self, c: &str| -> Res<Option<(Vec<Ty>, Vec<TypeExpr>, usize)>> {
            let fs = s.m.fields(c, &s.m.ctor_args(c, &t))?;
            Ok(fs.iter().position(|x| x.name.as_deref() == Some(f)).map(|i| (fs.iter().map(|x| x.ty.clone()).collect(), fs.iter().map(|x| x.texpr.clone()).collect(), i)))
        };
        if let [c] = cs.as_slice() {
            let (tys, texprs, i) = place(self, c)?.ok_or(format!("`{}` has no field `{f}`", key(&t)))?;
            return self.set_in(cur, Some(c), tys, texprs, i, path, v, shown);
        }
        // a sum: by its variant (one without the field can't be assigned, as the interpreter says)
        let ll = self.m.ll(&cur.0)?;
        let slot = self.fresh("setp");
        writeln!(self.allocas, "  {slot} = alloca {ll}").unwrap();
        let end = self.fresh_label("assigned");
        let v = self.temp(v);
        self.on_ctor(cur, &end, |s, c| {
            match place(s, c)? {
                Some((tys, texprs, i)) => {
                    s.push_scope(true);
                    let new = s.set_in(cur, Some(c), tys, texprs, i, path, v.clone(), shown)?;
                    let new = s.take(new);
                    s.pop_scope()?;
                    s.emit(format!("store {ll} {}, ptr {slot}", new.1));
                    s.br(&end);
                }
                None => s.fault(Fault::Runtime, vec![Part::Lit(format!("cannot assign to `{shown}`"))])?,
            }
            Ok(())
        })?;
        self.block(&end);
        let r = self.fresh("set");
        self.emit(format!("{r} = load {ll}, ptr {slot}"));
        Ok(self.temp((cur.0.clone(), r)))
    }

    /// `set_path` for a value whose constructor is `c` (a tuple's: `None`),
    /// with fields of types `tys` (written as `texprs`, when known): field
    /// `i` replaced, in a copy (a temporary).
    #[allow(clippy::too_many_arguments)]
    fn set_in(&mut self, cur: &V, c: Option<&str>, tys: Vec<Ty>, texprs: Vec<TypeExpr>, i: usize, path: &[String], v: V, shown: &str) -> Res<V> {
        let f = &path[0];
        let inner = self.field(cur, i, &tys[i])?;
        let new_inner = self.set_path(&inner, &path[1..], v, shown)?;
        let new_inner = self.coerce(new_inner, &tys[i])?;
        let p = self.copy_block(cur, &tys, Some(i))?;
        let held = self.take(new_inner.clone());
        self.set_field(&p, i, &held)?;
        let new = self.temp((cur.0.clone(), p));
        let Some(c) = c else { return Ok(new) };
        // the changed field's refinement, and the type's own
        let ctx = format!("assigning `{shown}`");
        self.check(&texprs[i], &new_inner, &format!("{ctx}: {}.{f}", crate::names::short(c)), false)?;
        let tname = self.m.env.ctors[&Symbol::new(c)].clone();
        if let Some(r) = self.m.refinement(&tname) {
            let ok = self.with_it(&new, false, |s| s.expr(r))?;
            self.fail_unless(&ok, &ctx, &new, format!("does not satisfy `{tname}` (where {})", crate::printer::expr(&r, 0)))?;
        }
        Ok(new)
    }

    /// A loop's body, in the iteration's scope (opened by the caller, above
    /// `depth` scopes): then the scope ends and it's back to `cont`. `break`
    /// goes to `end`, and both leave the scopes above `depth`.
    fn loop_body(&mut self, body: &Expr, cont: &str, end: &str, depth: usize) -> Res<()> {
        self.loops.push((cont.to_string(), end.to_string(), depth));
        if self.m.fuzzing {
            self.emit("call void @strata_step()".into());
        }
        self.push_scope(true);
        let r = self.expr(body);
        self.loops.pop();
        r?;
        self.pop_scope()?;
        self.pop_scope()?;
        self.br(cont);
        Ok(())
    }

    /// Store `new` in variable `l`, which owns it (taken); its old value is
    /// dropped when the enclosing statement ends (something in it may still
    /// read the old one, as `f(x, mut x.pop())` does).
    fn replace(&mut self, l: &Local, new: V) -> Res<()> {
        let new = self.take(new);
        if self.counted(&l.t) {
            let old = self.fresh("old");
            let ll = self.m.ll(&l.t)?;
            self.emit(format!("{old} = load {ll}, ptr {}", l.slot));
            self.temp((l.t.clone(), old));
        }
        self.store(&(l.t.clone(), new.1), &l.slot)
    }
}

impl Module<'_> {
    /// Fuzz driver `i`: generate the arguments, each handed to the runtime
    /// as it is made (`strata_fuzz_keep`: read by its shape, before the call
    /// can change it), then call.
    fn fuzz_driver(&mut self, i: usize, t: &FuzzTarget) -> Res<()> {
        let mut named = std::mem::take(&mut self.fuzz_kept.1);
        let mut shapes = vec![];
        let mut c = Func::new(self, HashMap::default());
        c.block("start");
        let mut vs = vec![];
        let mut places = vec![];
        for (k, (_, ty, mutable)) in t.params.iter().enumerate() {
            let v = c.generate(ty, &Hints::default(), "0")?;
            let v = c.temp(v);
            shapes.push(c.m.shape(&v.0, &mut named)?);
            let slot = c.as_slot(&v)?;
            c.emit(format!("call void @strata_fuzz_keep(i64 {k}, i64 {slot})"));
            let local = format!("fuzz.arg{k}");
            c.local_owned(&local, v, None)?;
            vs.push(c.load(&local)?);
            places.push(mutable.then(|| (local.clone(), vec![])));
        }
        c.call_fn_at(&t.name, vs, None, &places)?;
        c.drop_from(0, None)?;
        c.emit("ret void".into());
        let text = c.finish();
        writeln!(self.body, "define void @\"strata.fuzz.{i}\"() {{\n{text}}}\n").unwrap();
        self.fuzz_kept.0.push(shapes);
        self.fuzz_kept.1 = named;
        Ok(())
    }

    /// Make (again) the dispatchers whose trait has tables they don't cover
    /// yet: whether any changed.
    fn dispatchers(&mut self) -> Res<bool> {
        let mut changed = false;
        for i in 0..self.dispatch.len() {
            let tables = self.vtables.get(&key(&self.dispatch[i].tr_ty)).cloned().unwrap_or_default();
            if self.dispatch[i].made.as_ref().is_some_and(|(n, _)| *n == tables.len()) {
                continue;
            }
            let (name, tr_ty, trm, m, targs) = {
                let d = &self.dispatch[i];
                (d.name.clone(), d.tr_ty.clone(), d.trm.clone(), d.m.clone(), d.targs.clone())
            };
            let text = self.dispatcher(&name, &tr_ty, &trm, &m, &targs, &tables).map_err(|e| format!("can't compile `{trm}.{m}` on a `{}`: {e}", key(&tr_ty)))?;
            self.dispatch[i].made = Some((tables.len(), text));
            changed = true;
        }
        Ok(changed)
    }

    /// A dispatcher's text: by the pair's table, the implementation's method.
    fn dispatcher(&mut self, name: &str, tr_ty: &Ty, trm: &str, m: &str, targs: &[Ty], tables: &[(String, Ty)]) -> Res<String> {
        let mut h = Func::new(self, HashMap::default());
        let (tm, mut sub) = h.dyn_signature(tr_ty, trm, m)?;
        for (p, t) in tm.decl.type_params.iter().zip(targs) {
            sub.insert(p.clone(), t.clone());
        }
        let ret = h.m.lower(&tm.decl.ret, &sub)?;
        h.block("start");
        let mut sig = vec!["ptr %pair".to_string()];
        let mut rest = vec![];
        for (k, p) in tm.decl.params.iter().skip(1).enumerate() {
            let t = h.m.lower(&p.ty, &sub)?;
            if has_value(&t) {
                sig.push(format!("{} %a{k}", h.m.ll(&t)?));
            }
            rest.push((t, format!("%a{k}")));
        }
        let pair: V = (tr_ty.clone(), "%pair".into());
        h.emit("%vt = load i64, ptr %pair".into());
        let s = h.field(&pair, 0, &Ty::Int)?;
        for (vt, imp) in tables {
            if let Some(mocked) = mock_of(imp) {
                // a mock's: a generated result
                let f = h.mock_method(&mocked, trm, m, targs)?;
                let (is, yes, no, mine) = (h.fresh("is"), h.fresh_label("mock"), h.fresh_label("next"), h.fresh("mine"));
                h.emit(format!("{mine} = ptrtoint ptr {vt} to i64"));
                h.emit(format!("{is} = icmp eq i64 %vt, {mine}"));
                h.emit(format!("br i1 {is}, label %{yes}, label %{no}"));
                h.block(&yes);
                let r = h.call(&ret, &f, &rest)?;
                if has_value(&ret) {
                    h.emit(format!("ret {} {}", h.m.ll(&ret)?, r.1));
                } else {
                    h.emit("ret void".into());
                }
                h.block(&no);
                continue;
            }
            let head = Module::head(imp).ok_or(format!("a `{}` can't be a `{trm}`", key(imp)))?;
            let (_, imp_m) = h.m.resolve_method(&crate::types::trait_call(trm, m), &head).ok_or(format!("`{head}` has no method `{m}`"))?;
            let symbol = h.m.want_method(MethodInst { tr: trm.to_string(), method: imp_m, recv: imp.clone(), head, targs: targs.to_vec() });
            let (is, yes, no) = (h.fresh("is"), h.fresh_label("impl"), h.fresh_label("next"));
            let mine = h.fresh("mine");
            h.emit(format!("{mine} = ptrtoint ptr {vt} to i64"));
            h.emit(format!("{is} = icmp eq i64 %vt, {mine}"));
            h.emit(format!("br i1 {is}, label %{yes}, label %{no}"));
            h.block(&yes);
            let me = h.from_slot(&s.1, imp)?;
            let mut args = vec![me];
            args.extend(rest.iter().cloned());
            let r = h.call(&ret, &symbol, &args)?;
            if has_value(&ret) {
                h.emit(format!("ret {} {}", h.m.ll(&ret)?, r.1));
            } else {
                h.emit("ret void".into());
            }
            h.block(&no);
        }
        h.emit("unreachable".into());
        let text = h.finish();
        Ok(format!("define {} {name}({}) {{\n{text}}}\n\n", h.m.ll(&ret)?, sig.join(", ")))
    }

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
            // a union's members, paired by what they are (`List[T]` with `List[Int]`)
            (TypeExpr::Union(ws), Ty::Union(ts)) => {
                for w in ws {
                    let head = crate::types::texpr_head(w);
                    if let Some(x) = ts.iter().find(|x| Module::head(x).as_deref() == Some(head.as_str())) {
                        self.bind_targs(w, x, params, out);
                    }
                }
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
    /// Whether a double is a finite number (not a NaN, not infinite).
    fn finite(&mut self, x: &str) -> String {
        let (a, ok) = (self.fresh("abs"), self.fresh("finite"));
        self.emit(format!("{a} = call double @llvm.fabs.f64(double {x})"));
        self.emit(format!("{ok} = fcmp olt double {a}, 0x7FF0000000000000"));
        ok
    }

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
        Ok(self.temp((t.clone(), r)))
    }

    /// A string built-in on `vs` (the first a `String`), if `name` is one.
    fn string_builtin(&mut self, name: &str, vs: &[V]) -> Res<Option<V>> {
        let types: Vec<Ty> = vs.iter().map(|v| self.m.repr(&v.0)).collect::<Res<_>>()?;
        if types.first() != Some(&Ty::Str) && !(name == "chr" && types == [Ty::Int]) {
            return Ok(None);
        }
        let args: Vec<V> = vs.iter().zip(&types).map(|(v, t)| (t.clone(), v.1.clone())).collect();
        let r = self.string_builtin_in(name, &types, &args)?;
        Ok(r.map(|v| self.temp(v)))
    }

    fn string_builtin_in(&mut self, name: &str, types: &[Ty], args: &[V]) -> Res<Option<V>> {
        let args = args.to_vec();
        let s = Ty::Str;
        let strings = Ty::Con("List".into(), vec![Ty::Str]);
        let opt = |t: Ty| Ty::Con("Option".into(), vec![t]);
        Ok(Some(match (name, types) {
            ("split", [_, Ty::Str]) => self.call_fn(&prelude("str_split"), args, Some(strings))?,
            ("trim", [_]) => self.call(&s, "@strata_str_trim", &args)?,
            // written in Strata (llvm/prelude.strata), over the string's bytes
            ("contains", [_, Ty::Str]) => self.call_fn(&prelude("str_contains"), args, Some(Ty::Bool))?,
            ("starts_with", [_, Ty::Str]) => self.call_fn(&prelude("str_starts_with"), args, Some(Ty::Bool))?,
            ("ends_with", [_, Ty::Str]) => self.call_fn(&prelude("str_ends_with"), args, Some(Ty::Bool))?,
            ("reverse", [_]) => self.call_fn(&prelude("str_reverse"), args, Some(s))?,
            ("chars", [_]) => self.call_fn(&prelude("str_chars"), args, Some(strings))?,
            ("lines", [_]) => self.call_fn(&prelude("lines"), args, Some(strings))?,
            ("get", [_, Ty::Int]) => {
                let c = self.call(&s, "@strata_str_at", &args)?;
                let c = self.temp(c);
                let ok = self.fresh("found");
                self.emit(format!("{ok} = icmp ne ptr {}, null", c.1));
                self.option_if(&ok, &c, &opt(Ty::Str))?
            }
            ("at", [_, Ty::Int]) => self.call(&s, "@strata_str_at_or_fault", &args)?,
            ("slice", [_, Ty::Int, Ty::Int]) => self.call(&s, "@strata_str_slice", &args)?,
            ("index_of", [_, Ty::Str]) => self.call_fn(&prelude("str_index_of"), args, Some(opt(Ty::Int)))?,
            ("replace", [_, Ty::Str, Ty::Str]) => self.call_fn(&prelude("str_replace"), args, Some(s))?,
            ("repeat", [_, Ty::Int]) => self.call_fn(&prelude("str_repeat"), args, Some(s))?,
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
            ("byte_len", [_]) => self.call(&Ty::Int, "@strata_str_byte_len", &args)?,
            ("byte_at", [_, Ty::Int]) => self.call(&Ty::Int, "@strata_str_byte_at", &args)?,
            ("byte_slice", [_, Ty::Int, Ty::Int]) => self.call(&s, "@strata_str_byte_slice", &args)?,
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
                // the parsed number is new (a big one, an object), and moves into the option
                let x = self.temp((t.clone(), x));
                self.option_if(&ok.1, &x, &opt(t))?
            }
            _ => return Ok(None),
        }))
    }

    /// An f-string part with a format spec (`{x:.2}`, `{n:>5}`, `{n:05}`),
    /// as the old interpreter did.
    fn format_spec(&mut self, v: &V, f: &FmtSpec) -> Res<String> {
        // what the value is at run time: a distinct type's is not a number or a string
        let numeric = matches!(v.0, Ty::Int | Ty::Float);
        if (f.plus || f.zero) && !numeric {
            let shown = self.shown(v)?;
            self.fault(Fault::Runtime, vec![Part::Lit(format!("format spec `{f}`: `+` and `0` apply to numbers, not ")), Part::Val(shown)])?;
            return Ok(self.m.string(""));
        }
        let body = match (&v.0, f.precision) {
            (Ty::Float, Some(p)) => self.call(&Ty::Str, "@strata_show_float_fixed", &[v.clone(), (Ty::Int, p.to_string())])?.1,
            (Ty::Str, Some(p)) => self.call(&Ty::Str, "@strata_str_slice", &[v.clone(), (Ty::Int, "0".into()), (Ty::Int, p.to_string())])?.1,
            (_, Some(_)) => {
                let shown = self.shown(v)?;
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
// A `Set[A]` is a `List[A]` kept sorted in the order of the generated
// `cmp` without duplicates; a
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
        let p = self.closure(&fsym, &[], "<fn order>")?;
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

// ---- mocks and generated values ----
//
// `mock T` is a value of trait `T` whose methods generate their results, as
// the old interpreter's: `requires` checked, then a
// value of the result's type generated until `ensures` holds (200 tries).
// Values are generated as src/generate.rs does: integers from the intervals
// the refinements allow (often their ends, or 0), lengths likewise, strings
// matching a refinement's regex (from samples made when compiling), and
// rejection until the refinement holds (300 tries); past depth 3, the
// smallest choices, so recursive types end. The random numbers are the
// runtime's (runtime.c), not the interpreter's.

/// The marker type of a mock's table (among a trait's tables).
fn mock_marker(tr_ty: &Ty) -> Ty {
    Ty::Con(Symbol::new("<mock>"), vec![tr_ty.clone()])
}

fn mock_of(imp: &Ty) -> Option<Ty> {
    match imp {
        Ty::Con(n, a) if n == "<mock>" => a.first().cloned(),
        _ => None,
    }
}

/// What a type's refinements say about the values to generate (as generate.rs).
#[derive(Clone, Debug, Default)]
struct Hints {
    ints: Option<crate::types::ints::Ints>,
    lens: Option<crate::types::ints::Ints>,
    regex: Option<String>,
}

impl Hints {
    fn ints(&self) -> crate::types::ints::Ints {
        self.ints.clone().unwrap_or(vec![(crate::types::ints::MIN, crate::types::ints::MAX)])
    }
    fn lens(&self) -> crate::types::ints::Ints {
        self.lens.clone().unwrap_or(vec![(0, crate::types::ints::MAX)])
    }
    fn and(&self, pred: &Expr) -> Hints {
        use crate::types::ints;
        Hints { ints: Some(ints::and(&self.ints(), &ints::hull(pred))), lens: Some(ints::and(&self.lens(), &ints::len_hull(pred))), regex: regex_of(pred).or_else(|| self.regex.clone()) }
    }
}

/// The regex of a `matches(it, "...")` among a predicate's `&&`s.
fn regex_of(e: &Expr) -> Option<String> {
    let is_it = |e: &Expr| matches!(e, Expr::Var(v) if v == "it");
    match e {
        Expr::Bin(BinOp::And, l, r) => regex_of(l).or_else(|| regex_of(r)),
        Expr::Call(f, a) if matches!(&**f, Expr::Var(n) if n == "matches") && a.len() == 2 && is_it(&a[0]) => match &a[1] {
            Expr::Str(re) => Some(re.clone()),
            _ => None,
        },
        Expr::Method(r, m, a) if m == "matches" && is_it(r) && a.len() == 1 => match &a[0] {
            Expr::Str(re) => Some(re.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// An interval set with open ends brought within `below` / `above` of the
/// finite one (or around 0), as generate.rs's `interval`.
fn bounded(set: &crate::types::ints::Ints, below: i128, above: i128) -> Vec<(i64, i64)> {
    use crate::types::ints::{MAX, MIN};
    set.iter()
        .map(|&(a, b)| {
            let (lo, hi) = match (a == MIN, b == MAX) {
                (true, true) => (-below, below),
                (true, false) => (b - below, b),
                (false, true) => (a, a + above),
                (false, false) => (a, b),
            };
            let lo = lo.max(MIN);
            (lo as i64, hi.min(MAX).max(lo) as i64)
        })
        .collect()
}

impl Func<'_, '_> {
    /// A new mock of trait type `tr_ty`: a pair of its mock table and nothing.
    fn mock_value(&mut self, tr_ty: &Ty) -> Res<V> {
        let vt = self.mock_table(tr_ty)?;
        let (p, a) = (self.fresh("mock"), self.fresh("vt"));
        self.emit(format!("{p} = call ptr @strata_alloc(i64 16)"));
        self.emit(format!("{a} = ptrtoint ptr {vt} to i64"));
        self.emit(format!("store i64 {a}, ptr {p}"));
        self.set_field(&p, 0, &(Ty::Int, "0".into()))?;
        Ok(self.temp((tr_ty.clone(), p)))
    }

    /// The table of mocks of `tr_ty` (made once): shown as `<mock T>`, holding
    /// nothing, equal to each other, and each method a generated result.
    fn mock_table(&mut self, tr_ty: &Ty) -> Res<String> {
        let Ty::Con(tr, _) = tr_ty else { unreachable!() };
        let name = format!("@\"vt:{}@<mock>\"", key(tr_ty));
        if !self.m.helpers.insert(name.clone()) {
            return Ok(name);
        }
        self.m.vtables.entry(key(tr_ty)).or_default().push((name.clone(), mock_marker(tr_ty)));
        let tag = key(tr_ty).replace('"', "");
        let shown = self.m.string(&format!("\"<mock {tag}>\""));
        let plain = self.m.string(&format!("<mock {tag}>"));
        let mut entries = vec![];
        // as JSON (a string), as `to_string` and as `debug`
        for (what, text) in [("json", format!("  call void @strata_dup(ptr {shown})\n  ret ptr {shown}")), ("show", format!("  call void @strata_dup(ptr {plain})\n  ret ptr {plain}")), ("debug", format!("  call void @strata_dup(ptr {plain})\n  ret ptr {plain}"))] {
            let f = format!("@\"mock{what}:{}\"", key(tr_ty));
            writeln!(self.m.body, "define ptr {f}(i64 %v) {{\nentry:\n{text}\n}}\n").unwrap();
            entries.push(f);
        }
        let f = format!("@\"mocknone:{}\"", key(tr_ty));
        writeln!(self.m.body, "define void {f}(i64 %v) {{\nentry:\n  ret void\n}}\n").unwrap();
        entries.push(f.clone());
        entries.push(f);
        let f = format!("@\"mockeq:{}\"", key(tr_ty));
        writeln!(self.m.body, "define i1 {f}(i64 %v, i64 %w) {{\nentry:\n  ret i1 true\n}}\n").unwrap();
        entries.push(f);
        for (trm, m) in self.dyn_methods(tr) {
            let f = format!("@\"mockdyn:{trm}::{m}@{}\"", key(tr_ty));
            entries.push(f.clone());
            let sig_ok = self.dyn_signature(tr_ty, &trm, &m).ok().filter(|(tm, _)| tm.decl.type_params.is_empty());
            let Some((tm, mut sub)) = sig_ok else {
                writeln!(self.m.body, "define void {f}() {{\nentry:\n  unreachable\n}}\n").unwrap();
                continue;
            };
            sub.insert(Symbol::new("Self"), tr_ty.clone());
            let made = self.mock_method(tr_ty, &trm, &m, &[])?;
            let ret = self.m.lower(&tm.decl.ret, &sub)?;
            // the table's signature: `self` first (its slot, or a pointer to it)
            let mut sig = vec![if tm.decl.params[0].mutable { "ptr %self".to_string() } else { "i64 %self".to_string() }];
            let mut args = vec![];
            for (k, p) in tm.decl.params.iter().skip(1).enumerate() {
                let t = self.m.lower(&p.ty, &sub)?;
                if p.mutable {
                    sig.push(format!("ptr %a{k}"));
                    args.push(format!("ptr %a{k}"));
                } else if has_value(&t) {
                    sig.push(format!("{} %a{k}", self.m.ll(&t)?));
                    args.push(format!("{} %a{k}", self.m.ll(&t)?));
                }
            }
            let rl = self.m.ll(&ret)?;
            let body = if has_value(&ret) { format!("  %r = call {rl} {made}({})\n  ret {rl} %r", args.join(", ")) } else { format!("  call void {made}({})\n  ret void", args.join(", ")) };
            writeln!(self.m.body, "define {rl} {f}({}) {{\nentry:\n{body}\n}}\n", sig.join(", ")).unwrap();
        }
        let list: Vec<String> = entries.iter().map(|e| format!("ptr {e}")).collect();
        writeln!(self.m.globals, "{name} = private constant [{} x ptr] [{}], align 8", entries.len(), list.join(", ")).unwrap();
        Ok(name)
    }

    /// A mock's method `m` of trait `trm` (with these type arguments of its
    /// own): its parameters (no `self`), `requires` checked, then a result
    /// generated until `ensures` holds.
    fn mock_method(&mut self, tr_ty: &Ty, trm: &str, m: &str, targs: &[Ty]) -> Res<String> {
        let name = format!("@\"mock:{trm}::{m}@{}[{}]\"", key(tr_ty), targs.iter().map(key).collect::<Vec<_>>().join(", "));
        if !self.m.helpers.insert(name.clone()) {
            return Ok(name);
        }
        let (tm, mut sub) = self.dyn_signature(tr_ty, trm, m)?;
        sub.insert(Symbol::new("Self"), tr_ty.clone());
        for (p, t) in tm.decl.type_params.iter().zip(targs) {
            sub.insert(p.clone(), t.clone());
        }
        let ret = self.m.lower(&tm.decl.ret, &sub)?;
        let tn = crate::names::short(trm).to_string();
        let mut h = Func::new(self.m, sub.clone());
        h.block("start");
        let mut sig = vec![];
        for (k, p) in tm.decl.params.iter().enumerate().skip(1) {
            let t = h.m.lower(&p.ty, &sub)?;
            if p.mutable {
                sig.push(format!("ptr %a{k}"));
                h.scopes[0].vars.insert(p.name.to_string(), Local { t: t.clone(), slot: format!("%a{k}"), declared: None });
                continue;
            }
            if has_value(&t) {
                sig.push(format!("{} %a{k}", h.m.ll(&t)?));
            }
            let slot = h.local(&p.name, &t, None)?;
            h.store(&(t, format!("%a{k}")), &slot)?;
        }
        for r in &tm.decl.requires {
            h.push_scope(true);
            let ok = h.expr(r)?;
            h.pop_scope()?;
            let (pass, fail) = (h.fresh_label("requires.ok"), h.fresh_label("requires.fail"));
            h.cond_br(&ok, &pass, &fail);
            h.block(&fail);
            h.fault(Fault::Requires, vec![Part::Lit(crate::types::shown_name(&format!("precondition of `{tn}.{m}` violated: requires {}", crate::printer::expr(r, 0))))])?;
            h.block(&pass);
        }
        // the result's written type, with the trait's types in
        let written = h.written(&tm.decl.ret, &sub);
        let rl = h.m.ll(&ret)?;
        let count = h.fresh("tries");
        writeln!(h.allocas, "  {count} = alloca i64").unwrap();
        h.emit(format!("store i64 0, ptr {count}"));
        let (head, body, fail) = (h.fresh_label("mock.try"), h.fresh_label("mock.make"), h.fresh_label("mock.give_up"));
        h.br(&head);
        h.block(&head);
        let n = h.fresh("n");
        h.emit(format!("{n} = load i64, ptr {count}"));
        let more = h.fresh("more");
        h.emit(format!("{more} = icmp slt i64 {n}, 200"));
        h.cond_br(&bool_v(&more), &body, &fail);
        h.block(&body);
        let n1 = h.fresh("n");
        h.emit(format!("{n1} = add i64 {n}, 1"));
        h.emit(format!("store i64 {n1}, ptr {count}"));
        let v = h.generate(&written, &Hints::default(), "0")?;
        let v = (ret.clone(), v.1);
        // each `ensures`, with `it` the result
        let mut ok = bool_v("true");
        if !tm.decl.ensures.is_empty() {
            h.push_scope(false);
            let slot = h.local("it", &ret, None)?;
            h.store(&v, &slot)?;
            for en in &tm.decl.ensures {
                h.push_scope(true);
                let e = h.expr(en)?;
                h.pop_scope()?;
                let r = h.fresh("and");
                h.emit(format!("{r} = and i1 {}, {}", ok.1, e.1));
                ok = bool_v(&r);
            }
            h.pop_scope()?;
        }
        let (good, retry) = (h.fresh_label("mock.ok"), h.fresh_label("mock.retry"));
        h.cond_br(&ok, &good, &retry);
        h.block(&retry);
        h.drop_value(&v)?;
        h.br(&head);
        h.block(&good);
        if has_value(&ret) {
            h.emit(format!("ret {rl} {}", v.1));
        } else {
            h.emit("ret void".into());
        }
        h.block(&fail);
        h.fault(Fault::Runtime, vec![Part::Lit(format!("mock {tn}.{m}: could not generate a result satisfying its `ensures`"))])?;
        let text = h.finish();
        writeln!(self.m.body, "define {rl} {name}({}) {{\n{text}}}\n", sig.join(", ")).unwrap();
        Ok(name)
    }

    /// A written type with the instance's types written in (`Self`, a trait's parameters).
    fn written(&self, t: &TypeExpr, sub: &HashMap<Symbol, Ty>) -> TypeExpr {
        let texprs: HashMap<Symbol, TypeExpr> = sub.iter().map(|(k, v)| (k.clone(), ty_texpr(v))).collect();
        crate::types::subst_texpr(t, &texprs)
    }

    /// A value of written type `t` (owned, not a temporary), at `depth`.
    fn generate(&mut self, t: &TypeExpr, hints: &Hints, depth: &str) -> Res<V> {
        let f = self.gen_fn(t, hints)?;
        let ty = self.m.lower(t, &HashMap::default())?;
        if !has_value(&ty) {
            self.emit(format!("call void {f}(i64 {depth})"));
            return Ok((ty, String::new()));
        }
        let r = self.fresh("gen");
        self.emit(format!("{r} = call {} {f}(i64 {depth})", self.m.ll(&ty)?));
        Ok((ty, r))
    }

    /// The generator of written type `t` with `hints` (made once): `(i64 depth) -> t`.
    fn gen_fn(&mut self, t: &TypeExpr, hints: &Hints) -> Res<String> {
        let k = format!("{}|{:?}", crate::printer::ty(t), hints);
        if let Some(n) = self.m.gens.get(&k) {
            return Ok(n.clone());
        }
        let name = format!("@\"gen.{}\"", self.m.gens.len());
        self.m.gens.insert(k, name.clone());
        let ty = self.m.lower(t, &HashMap::default())?;
        let mut h = Func::new(self.m, HashMap::default());
        h.block("start");
        let v = h.produce(t, &ty, hints)?;
        if has_value(&ty) {
            h.emit(format!("ret {} {}", h.m.ll(&ty)?, v.1));
        } else {
            h.emit("ret void".into());
        }
        let text = h.finish();
        let ll = self.m.ll(&ty)?;
        writeln!(self.m.body, "define {ll} {name}(i64 %depth) {{\n{text}}}\n").unwrap();
        Ok(name)
    }

    /// Integer intervals as a constant table of pairs: (its operand, how many).
    fn pairs(&mut self, ps: &[(i64, i64)]) -> (String, usize) {
        let g = format!("@\"pairs.{}\"", self.m.helpers.len());
        self.m.helpers.insert(g.clone());
        let cells: Vec<String> = ps.iter().flat_map(|(a, b)| [format!("i64 {a}"), format!("i64 {b}")]).collect();
        writeln!(self.m.globals, "{g} = private unnamed_addr constant [{} x i64] [{}], align 8", cells.len(), cells.join(", ")).unwrap();
        (g, ps.len())
    }

    /// `depth > 3`: past it, the smallest choices.
    fn deep(&mut self) -> V {
        let r = self.fresh("deep");
        self.emit(format!("{r} = icmp sgt i64 %depth, 3"));
        bool_v(&r)
    }

    /// The body of a generator: an owned value of `t` (of type `ty`) in `%depth`.
    fn produce(&mut self, t: &TypeExpr, ty: &Ty, hints: &Hints) -> Res<V> {
        use crate::types::ints;
        let next_depth = |s: &mut Self| {
            let d = s.fresh("d");
            s.emit(format!("{d} = add i64 %depth, 1"));
            d
        };
        if let TypeExpr::Named(n, args) = t {
            if n == crate::types::TYPE_ARG {
                let s = self.string(&args.first().map(crate::printer::type_text).unwrap_or_default());
                return Ok(s);
            }
        }
        if let TypeExpr::Refined(b, pred) = t {
            let h2 = hints.and(pred);
            return self.until(t, ty, 300, |s| s.generate(b, &h2, "%depth"));
        }
        if let TypeExpr::Union(ms) = t {
            let i = self.fresh("which");
            self.emit(format!("{i} = call i64 @strata_random(i64 0, i64 {})", ms.len() - 1));
            let slot = self.fresh("u");
            writeln!(self.allocas, "  {slot} = alloca {}", self.m.ll(ty)?).unwrap();
            let end = self.fresh_label("u.end");
            let labels: Vec<String> = ms.iter().map(|_| self.fresh_label("u.member")).collect();
            let cases: Vec<String> = labels.iter().enumerate().map(|(k, l)| format!("i64 {k}, label %{l}")).collect();
            self.emit(format!("switch i64 {i}, label %{} [ {} ]", labels[0], cases.join(" ")));
            for (m, l) in ms.iter().zip(&labels) {
                self.block(l);
                self.push_scope(true);
                let v = self.generate(m, &Hints::default(), "%depth")?;
                let v = self.temp(v);
                let u = self.coerce(v, ty)?;
                let u = self.take(u);
                self.pop_scope()?;
                self.emit(format!("store ptr {}, ptr {slot}", u.1));
                self.br(&end);
            }
            self.block(&end);
            let r = self.fresh("uv");
            self.emit(format!("{r} = load ptr, ptr {slot}"));
            return Ok((ty.clone(), r));
        }
        let TypeExpr::Named(n, args) = t else {
            return match t {
                TypeExpr::Unit => Ok((Ty::Unit, String::new())),
                _ => self.unsupported(&format!("generating a value of `{}`", crate::printer::ty(t))),
            };
        };
        match n.as_str() {
            "Int" => {
                let (g, k) = self.pairs(&bounded(&hints.ints(), 1000, 100));
                let r = self.fresh("i");
                self.emit(format!("{r} = call i64 @strata_gen_int(ptr {g}, i64 {k})"));
                return Ok(self.temp((Ty::Int, r)));
            }
            "Float" => {
                let (g, k) = self.pairs(&bounded(&hints.ints(), 1000, 100));
                let raw: Vec<(i64, i64)> = hints.ints().iter().map(|&(a, b)| (a.max(ints::MIN) as i64, b.min(ints::MAX) as i64)).collect();
                let (rg, rk) = self.pairs(&raw);
                let r = self.fresh("f");
                self.emit(format!("{r} = call double @strata_gen_float(ptr {g}, i64 {k}, ptr {rg}, i64 {rk})"));
                return Ok((Ty::Float, r));
            }
            "Bool" => {
                let (x, r) = (self.fresh("b"), self.fresh("b"));
                self.emit(format!("{x} = call i64 @strata_random(i64 0, i64 1)"));
                self.emit(format!("{r} = trunc i64 {x} to i1"));
                return Ok(bool_v(&r));
            }
            "Unit" => return Ok((Ty::Unit, String::new())),
            "String" => {
                if let Some(re) = &hints.regex {
                    // samples made now, one picked at run time
                    let samples = regex_samples(re)?;
                    let consts: Vec<String> = samples.iter().map(|x| self.m.string(x)).collect();
                    let g = format!("@\"samples.{}\"", self.m.helpers.len());
                    self.m.helpers.insert(g.clone());
                    writeln!(self.m.globals, "{g} = private unnamed_addr constant [{} x ptr] [{}], align 8", consts.len(), consts.iter().map(|c| format!("ptr {c}")).collect::<Vec<_>>().join(", ")).unwrap();
                    let (i, at, r) = (self.fresh("which"), self.fresh("at"), self.fresh("s"));
                    self.emit(format!("{i} = call i64 @strata_random(i64 0, i64 {})", consts.len() - 1));
                    self.emit(format!("{at} = getelementptr ptr, ptr {g}, i64 {i}"));
                    self.emit(format!("{r} = load ptr, ptr {at}"));
                    return Ok((Ty::Str, r));
                }
                let (g, k) = self.pairs(&bounded(&hints.lens(), 0, 8));
                let (len, r) = (self.fresh("len"), self.fresh("s"));
                self.emit(format!("{len} = call i64 @strata_gen_len(ptr {g}, i64 {k})"));
                self.emit(format!("{r} = call ptr @strata_gen_string(i64 {len})"));
                return Ok((Ty::Str, r));
            }
            "Tuple" => {
                self.push_scope(true);
                let d = next_depth(self);
                let mut vs = vec![];
                for a in args {
                    let v = self.generate(a, &Hints::default(), &d)?;
                    vs.push(self.temp(v));
                }
                let b = self.block_of(0, &vs)?;
                self.pop_scope()?;
                return Ok((ty.clone(), b));
            }
            "List" | "Set" | "Dict" => {
                // a list of the elements (pairs for a dict), then the set or dict made of it
                let el = match n.as_str() {
                    "Dict" => TypeExpr::Named(Symbol::new("Tuple"), args.clone()),
                    _ => args.first().cloned().unwrap_or(TypeExpr::Unit),
                };
                let lty = Ty::Con(Symbol::new("List"), vec![self.m.lower(&el, &HashMap::default())?]);
                let deep = self.deep();
                let len = self.fresh("len");
                if n == "List" {
                    let (g, k) = self.pairs(&bounded(&hints.lens(), 0, 4));
                    let shortest = hints.lens().first().map_or(0, |&(lo, _)| lo.max(0) as i64);
                    let any = self.fresh("len");
                    self.emit(format!("{any} = call i64 @strata_gen_len(ptr {g}, i64 {k})"));
                    self.emit(format!("{len} = select i1 {}, i64 {shortest}, i64 {any}", deep.1));
                } else {
                    let any = self.fresh("len");
                    self.emit(format!("{any} = call i64 @strata_random(i64 0, i64 4)"));
                    self.emit(format!("{len} = select i1 {}, i64 0, i64 {any}", deep.1));
                }
                let d = next_depth(self);
                let acc = self.fresh("acc");
                writeln!(self.allocas, "  {acc} = alloca ptr").unwrap();
                let l0 = self.call(&lty, "@strata_list_new", &[(Ty::Int, len.clone())])?;
                self.emit(format!("store ptr {}, ptr {acc}", l0.1));
                let counter = self.fresh("k");
                writeln!(self.allocas, "  {counter} = alloca i64").unwrap();
                self.emit(format!("store i64 0, ptr {counter}"));
                let (head, body, end) = (self.fresh_label("gen.each"), self.fresh_label("gen.el"), self.fresh_label("gen.done"));
                self.br(&head);
                self.block(&head);
                let i = self.fresh("i");
                self.emit(format!("{i} = load i64, ptr {counter}"));
                let more = self.fresh("more");
                self.emit(format!("{more} = icmp slt i64 {i}, {len}"));
                self.cond_br(&bool_v(&more), &body, &end);
                self.block(&body);
                let x = self.generate(&el, &Hints::default(), &d)?;
                let s = self.to_slot(&x)?;
                let cur = self.fresh("cur");
                self.emit(format!("{cur} = load ptr, ptr {acc}"));
                let counted = self.elements_kind(&x.0);
                let next = self.call(&lty, "@strata_list_push", &[(lty.clone(), cur.clone()), (Ty::Int, s), counted])?;
                self.drop_value(&(lty.clone(), cur))?;
                self.emit(format!("store ptr {}, ptr {acc}", next.1));
                let i1 = self.fresh("i");
                self.emit(format!("{i1} = add i64 {i}, 1"));
                self.emit(format!("store i64 {i1}, ptr {counter}"));
                self.br(&head);
                self.block(&end);
                let r = self.fresh("l");
                self.emit(format!("{r} = load ptr, ptr {acc}"));
                if n == "List" {
                    return Ok((ty.clone(), r));
                }
                // a set or dict: from the list, as `set` and `dict` make one
                self.push_scope(true);
                let l = self.temp((lty.clone(), r));
                let kt = self.m.lower(&args[0], &HashMap::default())?;
                let less = self.less(&kt)?;
                let made = self.call_fn(&prelude(if n == "Set" { "set_from" } else { "dict_from" }), vec![l, less], None)?;
                let made = self.take(made);
                self.pop_scope()?;
                return Ok((ty.clone(), made.1));
            }
            _ => {}
        }
        if self.is_dyn(ty) {
            let v = self.mock_value(ty)?;
            return Ok(self.take(v));
        }
        // a variant's type: its sum's, only that variant
        let (tn, only) = match self.m.env.variant_parent(n.sym()) {
            Some(p) => (p.clone(), Some(n.clone())),
            None => (n.clone(), None),
        };
        let td = self.m.env.types.get(&tn).cloned().ok_or(format!("generating a value of `{tn}`"))?;
        let h2 = td.refinement.as_ref().map_or_else(Hints::default, |r| Hints::default().and(r));
        let whole = t.clone();
        self.until(&whole, ty, 300, |s| {
            match &td.body {
                TypeBody::Alias(a) => {
                    let sub: HashMap<Symbol, TypeExpr> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
                    let inner = crate::types::subst_texpr(a, &sub);
                    let v = s.generate(&inner, &h2, "%depth")?;
                    Ok((ty.clone(), v.1))
                }
                TypeBody::Record(_) | TypeBody::Sum(_) => {
                    let mut variants = s.m.env.variants_of(tn.as_str(), args).unwrap_or_default();
                    if let Some(o) = &only {
                        variants.retain(|v| &v.0 == o);
                    }
                    if variants.is_empty() {
                        // a capability or a C handle: only the runtime, or C, makes one
                        return Err(format!("a `{}` can't be made here (only the runtime, or C, makes one)", crate::names::short(&tn)));
                    }
                    let smallest = (0..variants.len()).min_by_key(|i| variants[*i].2.len()).unwrap_or(0);
                    let (deep, any, i) = (s.deep(), s.fresh("any"), s.fresh("which"));
                    s.emit(format!("{any} = call i64 @strata_random(i64 0, i64 {})", variants.len().max(1) - 1));
                    s.emit(format!("{i} = select i1 {}, i64 {smallest}, i64 {any}", deep.1));
                    let slot = s.fresh("made");
                    writeln!(s.allocas, "  {slot} = alloca ptr").unwrap();
                    let end = s.fresh_label("made.end");
                    let labels: Vec<String> = variants.iter().map(|_| s.fresh_label("variant")).collect();
                    let cases: Vec<String> = labels.iter().enumerate().map(|(k, l)| format!("i64 {k}, label %{l}")).collect();
                    s.emit(format!("switch i64 {i}, label %{} [ {} ]", labels[0], cases.join(" ")));
                    let d = s.fresh("d");
                    let first = labels[0].clone();
                    let _ = first;
                    for ((c, _, ftys), l) in variants.iter().zip(&labels) {
                        s.block(l);
                        s.emit(format!("{d}.{l} = add i64 %depth, 1"));
                        let dl = format!("{d}.{l}");
                        let b = if ftys.is_empty() {
                            s.m.nullary(c.as_str())
                        } else {
                            s.push_scope(true);
                            let mut vs = vec![];
                            let fs = s.m.fields(c.as_str(), &s.m.ctor_args(c.as_str(), ty))?;
                            for (ft, fi) in ftys.iter().zip(&fs) {
                                let v = s.generate(ft, &Hints::default(), &dl)?;
                                let v = s.temp(v);
                                vs.push(s.coerce(v, &fi.ty)?);
                            }
                            let b = s.block_of(s.m.tags[c.as_str()], &vs)?;
                            s.pop_scope()?;
                            b
                        };
                        s.emit(format!("store ptr {b}, ptr {slot}"));
                        s.br(&end);
                    }
                    s.block(&end);
                    let r = s.fresh("v");
                    s.emit(format!("{r} = load ptr, ptr {slot}"));
                    Ok((ty.clone(), r))
                }
            }
        })
    }

    /// `make` until the value satisfies `t`'s refinements (`tries` times),
    /// dropping each one that doesn't; then a fault, as the interpreter's.
    fn until(&mut self, t: &TypeExpr, ty: &Ty, tries: usize, mut make: impl FnMut(&mut Self) -> Res<V>) -> Res<V> {
        let count = self.fresh("tries");
        writeln!(self.allocas, "  {count} = alloca i64").unwrap();
        self.emit(format!("store i64 0, ptr {count}"));
        let (head, body, fail) = (self.fresh_label("gen.try"), self.fresh_label("gen.make"), self.fresh_label("gen.give_up"));
        self.br(&head);
        self.block(&head);
        let n = self.fresh("n");
        self.emit(format!("{n} = load i64, ptr {count}"));
        let more = self.fresh("more");
        self.emit(format!("{more} = icmp slt i64 {n}, {tries}"));
        self.cond_br(&bool_v(&more), &body, &fail);
        self.block(&body);
        let n1 = self.fresh("n");
        self.emit(format!("{n1} = add i64 {n}, 1"));
        self.emit(format!("store i64 {n1}, ptr {count}"));
        let v = make(self)?;
        let v = (ty.clone(), v.1);
        let ok = self.satisfies(t, &v)?;
        let (good, retry) = (self.fresh_label("gen.ok"), self.fresh_label("gen.retry"));
        self.cond_br(&ok, &good, &retry);
        self.block(&retry);
        self.drop_value(&v)?;
        self.br(&head);
        self.block(&fail);
        self.fault(Fault::Runtime, vec![Part::Lit(format!("could not generate a value satisfying `{}` after {tries} attempts", crate::printer::ty(t)))])?;
        self.block(&good);
        Ok(v)
    }
}

/// A type as written (for generators of a trait's types).
fn ty_texpr(t: &Ty) -> TypeExpr {
    let named = |n: &str, a: &[Ty]| TypeExpr::Named(Symbol::new(n), a.iter().map(ty_texpr).collect());
    match t {
        Ty::Int => named("Int", &[]),
        Ty::Float => named("Float", &[]),
        Ty::Bool => named("Bool", &[]),
        Ty::Str => named("String", &[]),
        Ty::Unit => TypeExpr::Unit,
        Ty::Con(n, a) => named(n.as_str(), a),
        Ty::Fn(ps, r) => TypeExpr::Fn(ps.iter().map(ty_texpr).collect(), Box::new(ty_texpr(r))),
        Ty::Union(ms) => TypeExpr::Union(ms.iter().map(ty_texpr).collect()),
        Ty::Rigid(n) => named(n.as_str(), &[]),
        Ty::Var(_) => TypeExpr::Unit,
    }
}

/// Strings matching `re`, made now (`std.regex.samples`: the same ones
/// every time), or why there are none.
fn regex_samples(re: &str) -> Res<Vec<String>> {
    let v = crate::stdlib::regex_value("samples", vec![Expr::Str(re.to_string()), Expr::Int(32), Expr::Int(7)])?;
    let texts = |xs: &[Expr]| xs.iter().map(|x| if let Expr::Str(s) = x { Ok(s.clone()) } else { Err(format!("a sample of {re:?} is not a string")) }).collect::<Res<Vec<String>>>();
    match v {
        Expr::Ctor(c, CtorArgs::Positional(xs)) if c.as_str().ends_with("Ok") => match xs.as_slice() {
            [Expr::List(xs)] => texts(xs),
            _ => Err(format!("the samples of {re:?} are not a list")),
        },
        Expr::Ctor(c, CtorArgs::Positional(xs)) if c.as_str().ends_with("Err") => match xs.as_slice() {
            [Expr::Str(m)] => Err(format!("can't generate strings matching {re:?}: {m}")),
            _ => Err(format!("can't generate strings matching {re:?}")),
        },
        _ => Err(format!("no samples of {re:?}")),
    }
}
