//! Static checking: Hindley–Milner style inference with explicit signatures
//! on top-level functions, effect checking, exhaustiveness checking, and
//! typed-hole reporting.

use crate::ast::*;
use crate::parser::Parser;
use serde::Serialize;
use std::collections::{BTreeSet, HashMap, HashSet};

mod env;
mod exhaustive;
pub mod ints;
pub mod reals;
pub use env::Env;
use exhaustive::*;

thread_local! {
    /// While `field_owners` runs: for each tagged field access (`qty·tag`),
    /// the record types (or record variants) its receiver can be.
    static FIELD_PROBE: std::cell::RefCell<Option<HashMap<String, BTreeSet<String>>>> = const { std::cell::RefCell::new(None) };
}

/// Which record each field access belongs to, by type. `tagged` is a
/// program whose accesses of interest are written `field·tag`; the result
/// maps each tag to the record types (or record variants) of its receiver:
/// one, or several for a union. Names are qualified (`shapes.Line`).
pub fn field_owners(tagged: &Program) -> HashMap<String, BTreeSet<String>> {
    FIELD_PROBE.with(|p| *p.borrow_mut() = Some(HashMap::new()));
    let _ = check(tagged);
    FIELD_PROBE.with(|p| p.borrow_mut().take()).unwrap_or_default()
}

pub const PRELUDE: &str = r#"
/// An optional value.
type Option[T] =
  | None
  | Some(T)

/// The result of an operation that can fail.
type Result[T, E] =
  | Ok(T)
  | Err(E)


// Common refinements. They are structural: an `Int` can be passed wherever a
// `Natural` is expected (checked at the boundary), and a `Positive` is a `Natural`.

/// 0, 1, 2, ...
type Natural = Int where it >= 0
/// 1, 2, 3, ...
type Positive = Int where it > 0
/// -1, -2, ...
type Negative = Int where it < 0
type NonZero = Int where it != 0
/// A share between 0.0 and 1.0.
type Probability = Float where 0.0 <= it && it <= 1.0
type Percentage = Float where 0.0 <= it && it <= 100.0
/// A list with at least one element.
type NonEmpty[T] = List[T] where len(it) > 0
/// A string with at least one character.
type NonEmptyString = String where len(it) > 0

// Sized integers: `Int` with the range of a machine integer. They are checked
// like any refinement; a compiler can store each in its own width.

/// -128 ..= 127
type I8 = Int where -128 <= it && it <= 127
/// -32768 ..= 32767
type I16 = Int where -32768 <= it && it <= 32767
/// -2^31 ..= 2^31 - 1
type I32 = Int where -2147483648 <= it && it <= 2147483647
/// -2^63 ..= 2^63 - 1
type I64 = Int where -9223372036854775808 <= it && it <= 9223372036854775807
/// 0 ..= 255
type U8 = Int where 0 <= it && it <= 255
/// 0 ..= 65535
type U16 = Int where 0 <= it && it <= 65535
/// 0 ..= 2^32 - 1
type U32 = Int where 0 <= it && it <= 4294967295
/// 0 ..= 2^64 - 1
type U64 = Int where 0 <= it && it <= 18446744073709551615
"#;

/// A built-in function (one overload): its name, type parameters, type, and
/// whether it always returns, never failing, on well-typed arguments (which
/// lets contracts see through the functions calling it, see termination.rs).
pub struct Builtin {
    pub name: &'static str,
    pub params: &'static str,
    pub sig: &'static str,
    pub total: bool,
}

impl Builtin {
    pub const fn new(name: &'static str, params: &'static str, sig: &'static str) -> Builtin {
        Builtin { name, params, sig, total: false }
    }
    pub const fn total(name: &'static str, params: &'static str, sig: &'static str) -> Builtin {
        Builtin { name, params, sig, total: true }
    }
}

/// Whether every overload of the builtin `name` always returns.
pub fn total_builtin(name: &str) -> bool {
    let mut all = BUILTINS.iter().filter(|b| b.name == name).peekable();
    all.peek().is_some() && all.all(|b| b.total)
}

/// The prelude's definitions, parsed once.
pub fn prelude() -> &'static Program {
    static P: std::sync::OnceLock<Program> = std::sync::OnceLock::new();
    P.get_or_init(|| crate::parser::parse_program(PRELUDE).expect("prelude parses"))
}

/// Builtin functions and their type schemes.
pub const BUILTINS: &[Builtin] = &[
    Builtin::total("len", "A", "(List[A]) -> Int"),
    Builtin::total("len", "", "(String) -> Int"),
    Builtin::total("len", "K,V", "(Dict[K, V]) -> Int"),
    Builtin::total("len", "A", "(Set[A]) -> Int"),
    Builtin::total("contains", "A", "(Set[A], A) -> Bool"),
    Builtin::total("contains", "K,V", "(Dict[K, V], K) -> Bool"),
    Builtin::total("contains", "", "(String, String) -> Bool"),
    Builtin::total("get", "K,V", "(Dict[K, V], K) -> Option[V]"),
    Builtin::total("dict", "K,V", "(List[(K, V)]) -> Dict[K, V]"),
    Builtin::total("insert", "K,V", "(Dict[K, V], K, V) -> Dict[K, V]"),
    Builtin::total("insert", "A", "(Set[A], A) -> Set[A]"),
    Builtin::total("remove", "K,V", "(Dict[K, V], K) -> Dict[K, V]"),
    Builtin::total("remove", "A", "(Set[A], A) -> Set[A]"),
    Builtin::total("keys", "K,V", "(Dict[K, V]) -> List[K]"),
    Builtin::total("values", "K,V", "(Dict[K, V]) -> List[V]"),
    Builtin::total("items", "K,V", "(Dict[K, V]) -> List[(K, V)]"),
    Builtin::total("map_values", "K,V,W", "(Dict[K, V], (V) -> W) -> Dict[K, W]"),
    Builtin::total("set", "A", "(List[A]) -> Set[A]"),
    Builtin::total("to_list", "A", "(Set[A]) -> List[A]"),
    Builtin::total("union", "A", "(Set[A], Set[A]) -> Set[A]"),
    Builtin::total("intersection", "A", "(Set[A], Set[A]) -> Set[A]"),
    Builtin::total("difference", "A", "(Set[A], Set[A]) -> Set[A]"),
    Builtin::total("zip", "A,B", "(List[A], List[B]) -> List[(A, B)]"),
    Builtin::total("enumerate", "A", "(List[A]) -> List[(Int, A)]"),
    Builtin::total("sort", "A", "(List[A]) -> List[A]"),
    Builtin::total("sort_by", "A,B", "(List[A], (A) -> B) -> List[A]"),
    Builtin::total("reverse", "A", "(List[A]) -> List[A]"),
    Builtin::total("join", "", "(List[String], String) -> String"),
    Builtin::new("split", "", "(String, String) -> List[String]"),
    Builtin::total("trim", "", "(String) -> String"),
    Builtin::total("parse_int", "", "(String) -> Option[Int]"),
    Builtin::new("parse_float", "", "(String) -> Option[Float]"),
    Builtin::new("float", "", "(Int) -> Float"),
    Builtin::new("floor", "", "(Float) -> Int"),
    Builtin::new("ceil", "", "(Float) -> Int"),
    Builtin::new("round", "", "(Float) -> Int"),
    Builtin::new("sqrt", "", "(Float) -> Float"),
    Builtin::total("map", "A,B", "(List[A], (A) -> B) -> List[B]"),
    Builtin::total("filter", "A", "(List[A], (A) -> Bool) -> List[A]"),
    Builtin::total("fold", "A,B", "(List[A], B, (B, A) -> B) -> B"),
    Builtin::total("all", "A", "(List[A], (A) -> Bool) -> Bool"),
    Builtin::total("any", "A", "(List[A], (A) -> Bool) -> Bool"),
    Builtin::total("contains", "A", "(List[A], A) -> Bool"),
    Builtin::total("push", "A", "(List[A], A) -> List[A]"),
    Builtin::total("take", "A", "(List[A], Int) -> List[A]"),
    Builtin::total("drop", "A", "(List[A], Int) -> List[A]"),
    Builtin::total("find", "A", "(List[A], (A) -> Bool) -> Option[A]"),
    Builtin::total("count", "A", "(List[A], (A) -> Bool) -> Int"),
    Builtin::total("get", "A", "(List[A], Int) -> Option[A]"),
    // `xs[i]`: fails unless `0 <= i < len(xs)`
    Builtin::new("at", "A", "(List[A], Int) -> A"),
    Builtin::total("sum", "", "(List[Int]) -> Int"),
    Builtin::total("sum", "", "(List[Float]) -> Float"),
    Builtin::new("range", "", "(Int, Int) -> List[Int]"),
    Builtin::total("abs", "", "(Int) -> Int"),
    Builtin::total("abs", "", "(Float) -> Float"),
    Builtin::total("min", "", "(Int, Int) -> Int"),
    Builtin::total("min", "", "(Float, Float) -> Float"),
    Builtin::total("max", "", "(Int, Int) -> Int"),
    Builtin::total("max", "", "(Float, Float) -> Float"),
    Builtin::new("matches", "", "(String, String) -> Bool"),
    Builtin::total("starts_with", "", "(String, String) -> Bool"),
    // strings are UTF-8 and indexed by character
    Builtin::new("ends_with", "", "(String, String) -> Bool"),
    Builtin::new("chars", "", "(String) -> List[String]"),
    Builtin::total("get", "", "(String, Int) -> Option[String]"),
    Builtin::new("at", "", "(String, Int) -> String"),
    Builtin::new("slice", "", "(String, Int, Int) -> String"),
    Builtin::new("slice", "A", "(List[A], Int, Int) -> List[A]"),
    Builtin::new("index_of", "", "(String, String) -> Option[Int]"),
    Builtin::new("replace", "", "(String, String, String) -> String"),
    Builtin::new("lines", "", "(String) -> List[String]"),
    Builtin::new("repeat", "", "(String, Int) -> String"),
    Builtin::total("reverse", "", "(String) -> String"),
    Builtin::new("to_upper", "", "(String) -> String"),
    Builtin::new("to_lower", "", "(String) -> String"),
    Builtin::new("is_digit", "", "(String) -> Bool"),
    Builtin::new("is_alpha", "", "(String) -> Bool"),
    Builtin::new("is_space", "", "(String) -> Bool"),
    Builtin::new("ord", "", "(String) -> Int"),
    Builtin::new("chr", "", "(Int) -> String"),
    Builtin::total("to_string", "A", "(A) -> String"),
    // the value as compact JSON, as `strata run` writes it
    Builtin::total("to_json", "A", "(A) -> String"),
    // a line of output, shown as by `to_string`: on the terminal under `strata run`,
    // into the test's own buffer under `strata test`, nowhere under `verify`
    Builtin::new("print", "A", "(A) -> ()"),
];

#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    Int,
    Float,
    Bool,
    Str,
    Unit,
    Con(String, Vec<Ty>),
    Fn(Vec<Ty>, Box<Ty>),
    Var(usize),
    /// A type parameter of the declaration being checked.
    Rigid(String),
    /// Values of any member: kept normalized (see `Checker::union_of`).
    Union(Vec<Ty>),
}

#[derive(Debug, Clone, Serialize)]
pub struct Diag {
    pub decl: String,
    pub msg: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HoleInfo {
    pub decl: String,
    pub name: String,
    /// The type the hole must produce.
    pub goal: String,
    /// What is in scope at the hole.
    pub context: Vec<(String, String)>,
    /// Contracts the hole's enclosing function must satisfy.
    pub obligations: Vec<String>,
}

#[derive(Default, Serialize)]
pub struct CheckReport {
    pub errors: Vec<Diag>,
    pub holes: Vec<HoleInfo>,
}

impl CheckReport {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// The name used to find an implementation: `List[Int]` -> `List`, `Line` -> `Line`.
pub fn texpr_head(t: &TypeExpr) -> String {
    match t {
        TypeExpr::Named(n, _) => n.clone(),
        TypeExpr::Unit => "Unit".into(),
        TypeExpr::Fn(..) => "Fn".into(),
        TypeExpr::Refined(b, _) => texpr_head(b),
        TypeExpr::Union(_) => "Union".into(),
    }
}

/// What a method call resolved to, when that depends on the receiver's type.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// Through trait `.0`: dispatch on the runtime type among that trait's
    /// implementations only. Written as the method `Trait::method`.
    Trait(String),
    /// A type's own method, known statically: a direct call of the function
    /// `.0` (`Item.describe`).
    Own(String),
    /// `X.m(..)`: trait `.0`'s method without `self`, for the type `.1`.
    /// Written as `Trait::m(type, ..)`.
    Static(String, TypeRef),
    /// A call of a generic function whose type parameters are needed when it
    /// runs (they have a static method to call): the types, in order, are
    /// passed as extra arguments.
    TypeArgs(Vec<TypeRef>),
    /// A trait method's name called on a value whose type doesn't implement
    /// any trait that declares it, where a builtin has that name: the builtin
    /// `.0` (`d.remove(k)` on a dict, with `Files.remove` in scope).
    Builtin(String),
    /// The method `.1` of trait `.0`, where the call was resolved to another
    /// module's method of the same name (`console.write(s)` with `Files`
    /// imported after `Console`). Written as `Trait::method`.
    Other(String, String),
}

/// A type as passed at runtime: a concrete type's name, or the caller's own
/// type parameter (forwarded from its hidden argument `type:T`).
#[derive(Debug, Clone, PartialEq)]
pub enum TypeRef {
    Head(String),
    Param(String),
}

impl TypeRef {
    /// The expression that evaluates to this type at runtime.
    pub fn expr(&self) -> Expr {
        match self {
            TypeRef::Head(h) => Expr::Str(h.clone()),
            TypeRef::Param(p) => Expr::Var(type_arg(p)),
        }
    }
}

/// The hidden parameter carrying type parameter `p` at runtime.
pub fn type_arg(p: &str) -> String {
    format!("type:{p}")
}

/// The type of a hidden type argument: `$Type[T]` is the name of `T`.
pub const TYPE_ARG: &str = "$Type";

/// The method name a call through a specific trait is elaborated to.
pub fn trait_call(tr: &str, method: &str) -> String {
    format!("{tr}::{method}")
}

/// `Trait::method` -> (`Trait`, `method`).
pub fn split_trait_call(name: &str) -> Option<(&str, &str)> {
    name.split_once("::")
}

/// The program as it runs: resolved, then each method call whose meaning
/// depends on the receiver's static type made explicit, as the checker
/// decided it. A call through a trait becomes `x.Trait::m(..)`, a call to a
/// type's own method a direct call `Item.m(x, ..)`. Everything that runs or
/// verifies code works on this.
pub fn elaborate(prog: &Program) -> crate::resolve::Resolution {
    elaborate_with(prog, false).0
}

/// The elaborated program, and the type the checker inferred for each of its
/// expressions, by node address (in a generic function, with its type
/// parameters as `Ty::Rigid`). The compiler works from these.
pub fn elaborate_typed(prog: &Program) -> (crate::resolve::Resolution, HashMap<usize, Ty>) {
    elaborate_with(prog, true)
}

fn elaborate_with(prog: &Program, typed: bool) -> (crate::resolve::Resolution, HashMap<usize, Ty>) {
    let mut r = crate::resolve::resolve(prog);
    let (env, _) = Env::build(&r.program);
    let shared = env.method_traits.values().any(|v| v.len() > 1);
    let statics = env.traits.values().any(|t| t.methods.iter().any(|m| !crate::resolve::takes_self(&m.decl)));
    // a trait method named like a builtin (`Files.remove`) may mean the builtin
    // or two modules' traits share a method name (`Console.write`, `Files.write`)
    let mut shorts: Vec<&str> = env.traits.values().flat_map(|t| t.methods.iter().map(|m| crate::names::short(&m.decl.name))).collect();
    let like_builtin = shorts.iter().any(|s| BUILTINS.iter().any(|b| b.name == *s));
    shorts.sort();
    let across = shorts.windows(2).any(|w| w[0] == w[1]);
    if env.inherent.is_empty() && !shared && !statics && !like_builtin && !across && !typed {
        return (r, HashMap::new());
    }
    let (_, _, targets, mut types) = check_decls(&r.program, &env);
    let targets: HashMap<usize, Target> = targets.into_iter().collect();
    for d in &mut r.program.decls {
        // a type parameter with a static method to call is passed when the function runs
        if let DeclKind::Fn(f) = &mut d.kind {
            for p in env.type_args(f) {
                f.params.push(Param { name: type_arg(&p), ty: TypeExpr::Named(TYPE_ARG.into(), vec![TypeExpr::Named(p.clone(), vec![])]), mutable: false });
            }
        }
        // children first, so a node's address is the one the checker saw when it is rewritten
        decl_exprs_mut_post(&mut d.kind, |e| {
            let Some(t) = targets.get(&(e as *const Expr as usize)) else { return };
        match (t, &mut *e) {
            (Target::Trait(tr), Expr::Method(_, m, _)) => *m = trait_call(tr, m),
            (Target::Trait(tr), Expr::Call(f, _)) => {
                if let Expr::Var(m) = &mut **f {
                    *m = trait_call(tr, m);
                }
            }
            (Target::Own(f), Expr::Method(recv, _, args)) => {
                let moved = &**recv as *const Expr as usize;
                let mut all = vec![std::mem::replace(&mut **recv, Expr::Unit)];
                all.append(args);
                *e = Expr::Call(Box::new(Expr::Var(f.clone())), all);
                // the receiver now lives in the argument list
                if let (Some(t), Expr::Call(_, all)) = (types.remove(&moved), &*e) {
                    types.insert(&all[0] as *const Expr as usize, t);
                }
            }
            (Target::Own(f), Expr::Call(g, _)) => **g = Expr::Var(f.clone()),
            (Target::Static(tr, ty), Expr::Method(_, m, args)) => {
                let mut all = vec![ty.expr()];
                all.append(args);
                *e = Expr::Call(Box::new(Expr::Var(trait_call(tr, m))), all);
            }
            (Target::TypeArgs(tys), Expr::Call(_, args) | Expr::Method(_, _, args)) => args.extend(tys.iter().map(|t| t.expr())),
            (Target::Other(tr, mq), Expr::Method(_, m, _)) => *m = trait_call(tr, mq),
            (Target::Other(tr, mq), Expr::Call(f, _)) => **f = Expr::Var(trait_call(tr, mq)),
            (Target::Builtin(b), Expr::Method(_, m, _)) => *m = b.clone(),
            (Target::Builtin(b), Expr::Call(f, _)) => **f = Expr::Var(b.clone()),
            _ => {}
        }
        });
    }
    (r, types)
}

/// `f(mut x)`: an argument marked for a `mut` parameter (not `mut q.claim()`,
/// a call marked as changing its receiver, which is passed as a value).
fn marks_arg(a: &Expr) -> bool {
    matches!(a, Expr::Mut(x) if !matches!(**x, Expr::Method(..)))
}

pub fn subst_texpr(t: &TypeExpr, sub: &HashMap<String, TypeExpr>) -> TypeExpr {
    match t {
        TypeExpr::Named(n, args) if args.is_empty() && sub.contains_key(n) => sub[n].clone(),
        TypeExpr::Named(n, args) => TypeExpr::Named(n.clone(), args.iter().map(|a| subst_texpr(a, sub)).collect()),
        TypeExpr::Fn(ps, r) => TypeExpr::Fn(ps.iter().map(|a| subst_texpr(a, sub)).collect(), Box::new(subst_texpr(r, sub))),
        TypeExpr::Unit => TypeExpr::Unit,
        TypeExpr::Refined(b, p) => TypeExpr::Refined(Box::new(subst_texpr(b, sub)), p.clone()),
        TypeExpr::Union(ms) => TypeExpr::Union(ms.iter().map(|m| subst_texpr(m, sub)).collect()),
    }
}

pub fn parse_type_str(s: &str) -> TypeExpr {
    let mut p = Parser::from_str(s);
    p.type_expr().expect("builtin type parses")
}

// ------------------------------------------------------------------

struct Checker<'a> {
    env: &'a Env,
    subst: Vec<Option<Ty>>,
    scopes: Vec<HashMap<String, Ty>>,
    rigid: BTreeSet<String>,
    decl: String,
    errors: Vec<Diag>,
    holes: Vec<(String, Ty, Vec<(String, Ty)>)>,
    /// Each `match` (and refutable `let`): the scrutinee's type, and each arm's
    /// value set with its text (for messages).
    matches: Vec<Coverage>,
    /// What `Self` means here (inside a trait or impl).
    self_ty: Option<Ty>,
    /// Trait bounds on the rigid type parameters in scope (`T: Ord`).
    bounds: HashMap<String, Vec<(String, Vec<Ty>)>>,
    /// Obligations `type implements trait[args]`, solved at the end of the declaration.
    pending: Vec<(Ty, String, Vec<Ty>, String)>,
    /// Scopes below this index belong outside the closure being checked.
    lambda_floor: usize,
    /// How many loops enclose the code being checked (for `break`/`continue`).
    loops: usize,
    /// Written types of immutable variables, for the refinements a `match`
    /// can rely on (`n: Positive`); scopes hold `written:x` -> index here.
    written: Vec<TypeExpr>,
    /// What `return`, `!` and `?` return from: the enclosing function or
    /// lambda's result type (`None` in tests, laws and refinements).
    ret: Option<Ty>,
    /// Which implementation each method call whose meaning depends on the
    /// receiver's type resolved to, by call node (see `elaborate`).
    targets: Vec<(usize, Target)>,
    /// The instantiation of the last named function `global_fn` gave a type for.
    last_inst: Option<(String, HashMap<String, Ty>)>,
    /// Calls of generic functions that need their type arguments at runtime:
    /// call node, and the type of each such parameter (resolved at the end).
    insts: Vec<(usize, Vec<(String, Ty)>)>,
    /// Set just before typing a call's callee, so a function needing type
    /// arguments can tell a call from its use as a value.
    callee: bool,
    /// Method calls marked `mut` (`mut q.claim(2)`), by node: they change their receiver.
    marked: HashSet<usize>,
    /// The type of each expression, by node address (resolved at the end).
    types: Vec<(usize, Ty)>,
}

impl<'a> Checker<'a> {
    fn err(&mut self, msg: impl Into<String>) {
        self.errors.push(Diag { decl: self.decl.clone(), msg: msg.into() });
    }

    fn fresh(&mut self) -> Ty {
        self.subst.push(None);
        Ty::Var(self.subst.len() - 1)
    }

    fn resolve(&self, t: &Ty) -> Ty {
        match t {
            Ty::Var(v) => match &self.subst[*v] {
                Some(t) => self.resolve(t),
                None => t.clone(),
            },
            Ty::Con(n, args) => Ty::Con(n.clone(), args.iter().map(|a| self.resolve(a)).collect()),
            Ty::Fn(ps, r) => Ty::Fn(ps.iter().map(|a| self.resolve(a)).collect(), Box::new(self.resolve(r))),
            Ty::Union(ms) => self.union_of(ms.iter().map(|m| self.resolve(m)).collect()),
            _ => t.clone(),
        }
    }

    /// A normalized union: nested unions are flattened, duplicates dropped, a
    /// variant is absorbed by its sum when the sum is there too, and a full set
    /// of a sum's variants is the sum itself. One member is just that member.
    fn union_of(&self, ms: Vec<Ty>) -> Ty {
        let mut out: Vec<Ty> = Vec::new();
        for m in ms {
            match m {
                Ty::Union(xs) => xs.into_iter().for_each(|x| if !out.contains(&x) { out.push(x) }),
                t => {
                    if !out.contains(&t) {
                        out.push(t)
                    }
                }
            }
        }
        let parent_of = |t: &Ty| match t {
            Ty::Con(v, a) => self.env.variant_parent(v).map(|p| (p, a.clone())),
            _ => None,
        };
        let present = out.clone();
        out.retain(|t| !parent_of(t).is_some_and(|(p, a)| present.contains(&Ty::Con(p, a))));
        loop {
            let full = out.iter().filter_map(parent_of).find(|(p, a)| {
                let Some(TypeDecl { body: TypeBody::Sum(vs), .. }) = self.env.types.get(p) else { return false };
                vs.iter().all(|v| out.contains(&Ty::Con(v.name.clone(), a.clone())))
            });
            let Some((p, a)) = full else { break };
            out.retain(|t| parent_of(t).is_none_or(|(q, b)| q != p || b != a));
            out.push(Ty::Con(p, a));
        }
        out.sort_by_key(show_ty);
        if out.len() == 1 { out.pop().unwrap() } else { Ty::Union(out) }
    }

    fn show(&self, t: &Ty) -> String {
        show_ty(&self.resolve(t))
    }

    fn occurs(&self, v: usize, t: &Ty) -> bool {
        match self.resolve(t) {
            Ty::Var(w) => v == w,
            Ty::Con(_, args) => args.iter().any(|a| self.occurs(v, a)),
            Ty::Fn(ps, r) => ps.iter().any(|a| self.occurs(v, a)) || self.occurs(v, &r),
            Ty::Union(ms) => ms.iter().any(|a| self.occurs(v, a)),
            _ => false,
        }
    }

    fn unify(&mut self, a: &Ty, b: &Ty) -> Result<(), String> {
        let (a, b) = (self.resolve(a), self.resolve(b));
        match (&a, &b) {
            _ if a == b => Ok(()),
            (Ty::Var(v), t) | (t, Ty::Var(v)) => {
                if self.occurs(*v, t) {
                    return Err(format!("infinite type {} ~ {}", show_ty(&a), show_ty(&b)));
                }
                self.subst[*v] = Some(t.clone());
                Ok(())
            }
            (Ty::Con(n1, a1), Ty::Con(n2, a2)) if n1 == n2 && a1.len() == a2.len() => {
                for (x, y) in a1.iter().zip(a2) {
                    self.unify(x, y)?;
                }
                Ok(())
            }
            (Ty::Fn(p1, r1), Ty::Fn(p2, r2)) if p1.len() == p2.len() => {
                for (x, y) in p1.iter().zip(p2) {
                    self.unify(x, y)?;
                }
                self.unify(r1, r2)
            }
            // A trait used as a type (`Console`) accepts any value whose type
            // implements it; that obligation is checked with the others.
            (Ty::Con(t, targs), other) | (other, Ty::Con(t, targs)) if self.env.traits.contains_key(t) => {
                self.pending.push((other.clone(), t.clone(), targs.clone(), format!("using it as a `{t}`")));
                Ok(())
            }
            _ => Err(format!("expected `{}`, found `{}`", show_ty(&a), show_ty(&b))),
        }
    }

    /// Whether a value of type `actual` can be used where `expected` is wanted:
    /// equal types, a member of a union, or a variant where its sum is wanted.
    /// Type arguments must still be equal (`List[Circle]` is not a `List[Shape]`).
    fn fits(&mut self, expected: &Ty, actual: &Ty) -> Result<(), String> {
        let (e, a) = (self.resolve(expected), self.resolve(actual));
        if e == a {
            return Ok(());
        }
        let mismatch = || format!("expected `{}`, found `{}`", show_ty(&e), show_ty(&a));
        match (&e, &a) {
            (Ty::Var(_), _) | (_, Ty::Var(_)) => self.unify(&e, &a),
            (_, Ty::Union(ms)) => {
                for m in ms {
                    self.fits(&e, m).map_err(|_| mismatch())?;
                }
                Ok(())
            }
            (Ty::Union(es), _) => {
                for m in es {
                    let snap = (self.subst.clone(), self.pending.len());
                    if self.fits(m, &a).is_ok() {
                        return Ok(());
                    }
                    self.subst = snap.0;
                    self.pending.truncate(snap.1);
                }
                Err(mismatch())
            }
            (Ty::Con(s, ea), Ty::Con(v, aa)) if self.env.variant_parent(v).as_deref() == Some(s.as_str()) && ea.len() == aa.len() => {
                for (x, y) in ea.iter().zip(aa) {
                    self.fits(x, y).map_err(|_| mismatch())?;
                }
                Ok(())
            }
            // Values are never shared, so a `List[Circle]` can be read as a
            // `List[Shape]` (type arguments fit member-wise). Only a `mut`
            // parameter, which writes back into the caller's variable, needs
            // the exact type; `call` checks that with `unify`. A trait's type
            // arguments stay exact.
            (Ty::Con(n1, ea), Ty::Con(n2, aa)) if n1 == n2 && ea.len() == aa.len() && !self.env.traits.contains_key(n1) => {
                for (x, y) in ea.iter().zip(aa) {
                    self.fits(x, y).map_err(|_| mismatch())?;
                }
                Ok(())
            }
            // a function that accepts more and returns less fits
            (Ty::Fn(ep, er), Ty::Fn(ap, ar)) if ep.len() == ap.len() => {
                for (x, y) in ep.iter().zip(ap) {
                    self.fits(y, x).map_err(|_| mismatch())?;
                }
                self.fits(er, ar).map_err(|_| mismatch())
            }
            _ => self.unify(&e, &a),
        }
    }

    fn expect(&mut self, expected: &Ty, actual: &Ty, what: &str) {
        if let Err(e) = self.fits(expected, actual) {
            self.err(format!("{what}: {e}"));
        }
    }

    /// The type of alternatives (`if` branches, `match` arms, list elements):
    /// whichever holds the other, else, when both are fully known, their union.
    fn join_union(&mut self, a: &Ty, b: &Ty, what: &str) -> Ty {
        for (x, y) in [(a, b), (b, a)] {
            let snap = (self.subst.clone(), self.pending.len());
            if self.fits(x, y).is_ok() {
                return x.clone();
            }
            self.subst = snap.0;
            self.pending.truncate(snap.1);
        }
        let (ra, rb) = (self.resolve(a), self.resolve(b));
        if !self.has_var(&ra) && !self.has_var(&rb) {
            return self.union_of(vec![ra, rb]);
        }
        self.join(a, b, what)
    }

    fn has_var(&self, t: &Ty) -> bool {
        match self.resolve(t) {
            Ty::Var(_) => true,
            Ty::Con(_, xs) | Ty::Union(xs) => xs.iter().any(|x| self.has_var(x)),
            Ty::Fn(ps, r) => ps.iter().any(|x| self.has_var(x)) || self.has_var(&r),
            _ => false,
        }
    }

    /// The type of two things that must be the same (`==` operands): whichever
    /// holds the other, else an error.
    fn join(&mut self, a: &Ty, b: &Ty, what: &str) -> Ty {
        for (x, y) in [(a, b), (b, a)] {
            let snap = (self.subst.clone(), self.pending.len());
            if self.fits(x, y).is_ok() {
                return x.clone();
            }
            self.subst = snap.0;
            self.pending.truncate(snap.1);
        }
        let (sa, sb) = (self.show(a), self.show(b));
        self.err(format!("{what}: expected `{sa}`, found `{sb}`"));
        a.clone()
    }

    /// Convert a written type to a checker type. `vars` maps scheme params to fresh vars.
    fn lower(&mut self, t: &TypeExpr, vars: &HashMap<String, Ty>) -> Ty {
        match t {
            TypeExpr::Unit => Ty::Unit,
            TypeExpr::Refined(b, _) => self.lower(b, vars),
            TypeExpr::Union(ms) => {
                let ms: Vec<Ty> = ms.iter().map(|m| self.lower(m, vars)).collect();
                self.union_of(ms)
            }
            TypeExpr::Fn(ps, r) => Ty::Fn(ps.iter().map(|p| self.lower(p, vars)).collect(), Box::new(self.lower(r, vars))),
            TypeExpr::Named(n, args) => {
                if let Some(v) = vars.get(n) {
                    return v.clone();
                }
                if n == "Self" && args.is_empty() {
                    return match &self.self_ty {
                        Some(t) => t.clone(),
                        None => {
                            self.err("`Self` only means something inside a trait or an impl");
                            self.fresh()
                        }
                    };
                }
                if self.rigid.contains(n) {
                    return Ty::Rigid(n.clone());
                }
                if let Some(tr) = self.env.traits.get(n) {
                    // a trait as a type: any implementation of it
                    let tr = tr.clone();
                    let largs: Vec<Ty> = if args.is_empty() { tr.params.iter().map(|_| self.fresh()).collect() } else { args.iter().map(|a| self.lower(a, vars)).collect() };
                    if tr.params.len() != largs.len() {
                        self.err(format!("trait `{n}` takes {} type arguments", tr.params.len()));
                    }
                    return Ty::Con(n.clone(), largs);
                }
                let largs: Vec<Ty> = args.iter().map(|a| self.lower(a, vars)).collect();
                match n.as_str() {
                    "Int" => Ty::Int,
                    "Float" => Ty::Float,
                    "Tuple" if largs.len() >= 2 => Ty::Con("Tuple".into(), largs),
                    "Dict" if largs.len() == 2 => Ty::Con("Dict".into(), largs),
                    "Set" if largs.len() == 1 => Ty::Con("Set".into(), largs),
                    "Bool" => Ty::Bool,
                    "String" => Ty::Str,
                    "Unit" => Ty::Unit,
                    "List" if largs.len() == 1 => Ty::Con("List".into(), largs),
                    _ => match self.env.types.get(n) {
                        Some(td) => {
                            if td.params.len() != largs.len() {
                                self.err(format!("type `{n}` expects {} type arguments, got {}", td.params.len(), largs.len()));
                            }
                            if let (TypeBody::Alias(a), false) = (&td.body, td.distinct) {
                                let sub: HashMap<String, Ty> = td.params.iter().cloned().zip(largs).collect();
                                let a = a.clone();
                                return self.lower(&a, &sub);
                            }
                            Ty::Con(n.clone(), largs)
                        }
                        None => match self.env.variant_parent(n) {
                            // a variant is a type, with its sum's parameters
                            Some(p) => {
                                let want = self.env.types[&p].params.len();
                                let largs = if largs.is_empty() && want > 0 { (0..want).map(|_| self.fresh()).collect() } else { largs };
                                if largs.len() != want {
                                    self.err(format!("`{n}` is a variant of `{p}`, which takes {want} type arguments"));
                                }
                                Ty::Con(n.clone(), largs)
                            }
                            None => {
                                self.err(format!("unknown type `{n}`"));
                                self.fresh()
                            }
                        },
                    },
                }
            }
        }
    }

    /// The type of a trait method used anywhere: `Self` is a fresh type that
    /// must implement the trait (checked when the declaration is finished).
    fn trait_method(&mut self, name: &str, trait_name: &str) -> Ty {
        self.trait_method_for(name, trait_name, None)
    }

    /// `trait_method`, with `Self` the given type when it is known (a static
    /// call names it: `Money.empty()`, `T.empty()`).
    fn trait_method_for(&mut self, name: &str, trait_name: &str, self_ty: Option<Ty>) -> Ty {
        let tr = self.env.traits[trait_name].clone();
        let m = tr.methods.iter().find(|m| m.decl.name == name).unwrap().decl.clone();
        let mut vars: HashMap<String, Ty> = tr.params.iter().map(|p| (p.clone(), self.fresh())).collect();
        for p in &m.type_params {
            vars.insert(p.clone(), self.fresh());
        }
        let self_t = self_ty.unwrap_or_else(|| self.fresh());
        let saved = self.self_ty.replace(self_t.clone());
        let fty = self.lower(&Self::fn_scheme(&m), &vars);
        self.self_ty = saved;
        let args: Vec<Ty> = tr.params.iter().map(|p| vars[p].clone()).collect();
        self.pending.push((self_t, trait_name.to_string(), args, format!("calling `{name}`")));
        fty
    }

    /// Supertraits of `tr[args]`, transitively, with arguments substituted.
    fn supers_of(&mut self, tr: &str, args: &[Ty], depth: usize) -> Vec<(String, Vec<Ty>)> {
        let Some(td) = self.env.traits.get(tr).cloned() else { return vec![] };
        if depth > 8 {
            return vec![];
        }
        let vars: HashMap<String, Ty> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
        let mut out = Vec::new();
        for s in &td.supers {
            if let TypeExpr::Named(sn, sargs) = s {
                let sa: Vec<Ty> = sargs.iter().map(|a| self.lower(a, &vars)).collect();
                out.extend(self.supers_of(sn, &sa, depth + 1));
                out.push((sn.clone(), sa));
            }
        }
        out
    }

    /// Does `t` implement `tr[args]`?
    fn satisfies(&mut self, t: &Ty, tr: &str, args: &[Ty], depth: usize) -> Result<(), String> {
        if depth > 8 {
            return Err(format!("trait resolution for `{tr}` is too deep"));
        }
        let t = self.resolve(t);
        // a union implements a trait when every member does
        if let Ty::Union(ms) = &t {
            for m in ms.clone() {
                self.satisfies(&m, tr, args, depth + 1)?;
            }
            return Ok(());
        }
        // a variant implements what its sum implements, unless it has an impl of its own
        if let Ty::Con(v, va) = &t {
            if let Some(p) = self.env.variant_parent(v) {
                if !self.env.impls.iter().any(|i| i.trait_name == tr && texpr_head(&i.for_type) == *v) {
                    return self.satisfies(&Ty::Con(p, va.clone()), tr, args, depth + 1);
                }
            }
        }
        let head = match &t {
            Ty::Union(_) => unreachable!(),
            Ty::Rigid(n) => {
                let known = self.bounds.get(n).cloned().unwrap_or_default();
                let mut all = known.clone();
                for (b, bargs) in &known {
                    all.extend(self.supers_of(b, bargs, 0));
                }
                return match all.iter().find(|(b, _)| b == tr) {
                    Some((_, bargs)) => {
                        for (a, b) in args.iter().zip(bargs) {
                            self.unify(a, b)?;
                        }
                        Ok(())
                    }
                    None => Err(format!("`{n}` is not known to implement `{tr}` (add a bound: `[{n}: {tr}]`)")),
                };
            }
            Ty::Var(_) => return Err(format!("could not work out which type must implement `{tr}`; add a type annotation")),
            Ty::Int => "Int".to_string(),
            Ty::Float => "Float".into(),
            Ty::Bool => "Bool".into(),
            Ty::Str => "String".into(),
            Ty::Unit => "Unit".into(),
            Ty::Fn(..) => "Fn".into(),
            Ty::Con(n, _) => n.clone(),
        };
        // a value typed by a trait implements that trait (and its supertraits)
        if let Ty::Con(n, nargs) = &t {
            if self.env.traits.contains_key(n) {
                let mut all = vec![(n.clone(), nargs.clone())];
                all.extend(self.supers_of(n, nargs, 0));
                return match all.iter().find(|(b, _)| b == tr) {
                    Some((_, bargs)) => {
                        for (a, b) in args.iter().zip(bargs) {
                            self.unify(a, b)?;
                        }
                        Ok(())
                    }
                    None => Err(format!("a `{n}` is not known to be a `{tr}`")),
                };
            }
        }
        let cands: Vec<ImplDecl> = self.env.impls.iter().filter(|i| i.trait_name == tr && texpr_head(&i.for_type) == head).cloned().collect();
        for imp in cands {
            let snapshot = self.subst.clone();
            let vars: HashMap<String, Ty> = imp.type_params.iter().map(|p| (p.clone(), self.fresh())).collect();
            let saved_rigid = std::mem::take(&mut self.rigid);
            let ft = self.lower(&imp.for_type, &vars);
            let targs: Vec<Ty> = imp.trait_args.iter().map(|a| self.lower(a, &vars)).collect();
            self.rigid = saved_rigid;
            let ok = self.unify(&ft, &t).is_ok() && targs.len() == args.len() && targs.iter().zip(args).all(|(a, b)| self.unify(a, b).is_ok());
            if ok {
                for b in &imp.bounds {
                    for btr in &b.traits {
                        if let TypeExpr::Named(bn, bargs) = btr {
                            let ba: Vec<Ty> = bargs.iter().map(|a| self.lower(a, &vars)).collect();
                            if let Err(e) = self.satisfies(&vars[&b.param], bn, &ba, depth + 1) {
                                self.subst = snapshot.clone();
                                return Err(format!("`{}` implements `{tr}` only when {e}", show_ty(&t)));
                            }
                        }
                    }
                }
            }
            if ok {
                return Ok(());
            }
            self.subst = snapshot;
        }
        Err(format!("`{}` does not implement `{tr}`", show_ty(&t)))
    }

    fn solve_pending(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        for (t, tr, args, ctx) in pending {
            if let Err(e) = self.satisfies(&t, &tr, &args, 0) {
                self.err(format!("{ctx}: {e}"));
            }
        }
    }

    /// Bounds written on type parameters (`[T: Ord + Show]`), lowered.
    fn add_bounds(&mut self, bounds: &[Bound]) {
        for b in bounds {
            for tr in &b.traits {
                match tr {
                    TypeExpr::Named(tn, targs) if self.env.traits.contains_key(tn) => {
                        let args: Vec<Ty> = targs.iter().map(|a| self.lower(a, &HashMap::new())).collect();
                        self.bounds.entry(b.param.clone()).or_default().push((tn.clone(), args));
                    }
                    other => self.err(format!("`{}` is not a trait", crate::printer::ty(other))),
                }
            }
        }
    }

    /// Check a function body against its signature (top-level functions,
    /// impl methods and default trait methods).
    fn check_fn(&mut self, f: &FnDecl, obligations: &mut Vec<String>) {
        self.rigid.extend(f.type_params.iter().cloned());
        self.add_bounds(&f.bounds);
        self.scopes.push(HashMap::new());
        for p in &f.params {
            self.check_refined(&p.ty);
        }
        for p in &f.params {
            let t = self.lower(&p.ty, &HashMap::new());
            self.bind(&p.name, t);
            if p.mutable {
                self.bind(&format!("mut:{}", p.name), Ty::Unit);
            }
            self.bind_written(&p.name, &p.ty);
        }
        // the result's refinement may mention the parameters
        self.check_refined(&f.ret);
        let ret = self.lower(&f.ret, &HashMap::new());
        for r in &f.requires {
            let t = self.infer(r);
            self.expect(&Ty::Bool, &t, "requires clause");
            obligations.push(format!("requires {}", crate::printer::expr(r, 0)));
        }
        let outer = self.ret.replace(ret.clone());
        let bt = self.infer_with(&f.body, Some(&ret));
        self.ret = outer;
        self.expect(&ret, &bt, "function body");
        self.scopes.push(HashMap::new());
        self.bind("it", ret.clone());
        for en in &f.ensures {
            let t = self.infer(en);
            self.expect(&Ty::Bool, &t, "ensures clause");
            obligations.push(format!("ensures {}", crate::printer::expr(en, 0)));
        }
        self.scopes.pop();
        self.scopes.pop();
    }

    /// A field's default has the field's type, and a literal one satisfies its
    /// refinement. It is checked where the type is defined, with no variables
    /// in scope (not even the other fields).
    fn check_defaults(&mut self, fs: &[Field]) {
        for f in fs {
            let Some(x) = &f.default else { continue };
            let want = self.lower(&f.ty, &HashMap::new());
            let got = self.infer_with(x, Some(&want));
            self.expect(&want, &got, &format!("default of field `{}`", f.name));
            if self.literal_outside(&f.ty, x) {
                self.err(format!("the default of `{}`, {}, does not satisfy `{}`", f.name, crate::printer::expr(x, 0), crate::printer::ty(&f.ty)));
            }
        }
    }

    /// Type-check the predicates of inline refinements inside a written type.
    fn check_refined(&mut self, t: &TypeExpr) {
        match t {
            TypeExpr::Refined(b, pred) => {
                self.check_refined(b);
                let bt = self.lower(b, &HashMap::new());
                self.scopes.push(HashMap::new());
                self.bind("it", bt);
                let pt = self.infer(pred);
                self.expect(&Ty::Bool, &pt, &format!("refinement `where {}`", crate::printer::expr(pred, 0)));
                self.scopes.pop();
            }
            TypeExpr::Named(_, args) | TypeExpr::Union(args) => args.iter().for_each(|a| self.check_refined(a)),
            TypeExpr::Fn(ps, r) => {
                ps.iter().for_each(|a| self.check_refined(a));
                self.check_refined(r);
            }
            TypeExpr::Unit => {}
        }
    }

    fn instantiate(&mut self, params: &[String], t: &TypeExpr) -> Ty {
        let vars: HashMap<String, Ty> = params.iter().map(|p| (p.clone(), self.fresh())).collect();
        self.lower(t, &vars)
    }

    fn lookup(&self, x: &str) -> Option<Ty> {
        self.scopes.iter().rev().find_map(|s| s.get(x).cloned())
    }

    fn bind(&mut self, x: &str, t: Ty) {
        self.scopes.last_mut().unwrap().insert(x.to_string(), t);
    }

    fn fn_scheme(f: &FnDecl) -> TypeExpr {
        TypeExpr::Fn(f.params.iter().map(|p| p.ty.clone()).collect(), Box::new(f.ret.clone()))
    }

    /// Type of a named function value (top-level or builtin), freshly instantiated.
    fn global_fn(&mut self, name: &str) -> Option<Ty> {
        if let Some(f) = self.env.fns.get(name) {
            let f = f.clone();
            let vars: HashMap<String, Ty> = f.type_params.iter().map(|p| (p.clone(), self.fresh())).collect();
            let fty = self.lower(&Self::fn_scheme(&f), &vars);
            for b in &f.bounds {
                for tr in &b.traits {
                    if let TypeExpr::Named(tn, targs) = tr {
                        let args: Vec<Ty> = targs.iter().map(|a| self.lower(a, &vars)).collect();
                        self.pending.push((vars[&b.param].clone(), tn.clone(), args, format!("calling `{name}`")));
                    }
                }
            }
            let callee = std::mem::take(&mut self.callee);
            if !callee && !self.env.type_args(&f).is_empty() {
                let short = crate::names::short(&name);
                self.err(format!("`{short}` needs to know its type arguments when it runs (it calls methods on the type), so it can't be passed as a value: pass a lambda (`x -> {short}(x)`)"));
            }
            self.last_inst = Some((name.to_string(), vars));
            return Some(fty);
        }
        if let Some((tr, _)) = self.env.method_contract(name) {
            let tn = tr.name.clone();
            return Some(self.trait_method(name, &tn));
        }
        if self.env.is_method(name) {
            let short = crate::names::short(&name);
            self.err(format!("which `{short}` runs depends on the type of the value it is called on (several types or traits have one): call it on a value, or pass a lambda such as `x -> x.{short}()`"));
            return Some(self.fresh());
        }
        if let Some(Builtin { params: ps, sig, .. }) = BUILTINS.iter().chain(crate::stdlib::HOST).find(|b| b.name == name) {
            let ps: Vec<String> = ps.split(',').filter(|s| !s.is_empty()).map(String::from).collect();
            return Some(self.instantiate(&ps, &parse_type_str(sig)));
        }
        None
    }

    /// A method name means the method of that name among the traits of the
    /// value it is called on, whichever module they come from. Names are
    /// qualified by module when resolved (`std.fs.write`), and a module's
    /// scope can only map `write` to one of them, so this re-reads the call by
    /// the receiver: `console.write(s)` is `Console`'s `write` even where
    /// `Files` (with its own `write`) was imported last. When none of the
    /// receiver's traits has the name and a builtin does, the call is the
    /// builtin's (`d.remove(k)` on a dict, with `Files.remove` in scope).
    /// The choice is recorded for `elaborate`. `None`: the call is as resolved.
    /// Whether `recv`'s type is known and has a method of its own named `m`
    /// (in `impl Type`, or its sum's for a variant).
    fn has_own_method(&mut self, recv: &Expr, m: &str) -> bool {
        let n = self.errors.len();
        let rt = self.infer(recv);
        self.errors.truncate(n);
        let Ty::Con(head, _) = self.resolve(&rt) else { return false };
        let sm = m.rsplit('.').next().unwrap_or(m);
        std::iter::once(head.clone()).chain(self.env.variant_parent(&head)).any(|h| {
            self.env.inherent.get(&h).is_some_and(|imp| imp.methods.iter().any(|x| x.name.rsplit('.').next() == Some(sm) && crate::resolve::takes_self(x)))
        })
    }

    /// `dotted`: written `x.m(..)`, where a type's own method comes before a
    /// function of the same name (`m(x, ..)` always means the function).
    fn by_receiver(&mut self, m: &str, all: &[&Expr], site: usize, dotted: bool, recv_marked: Option<bool>) -> Option<Ty> {
        // a function of that name takes the call, unless the receiver's type
        // has a method of its own by that name (`app.handle(req)` inside a
        // `fn handle`)
        if self.lookup(m).is_some() || self.env.fns.contains_key(m) {
            if !dotted || !all.first().is_some_and(|r| self.has_own_method(r, m)) {
                return None;
            }
        } else if split_trait_call(m).is_some() || !self.env.is_method(m) {
            return None;
        }
        let short = |q: &str| crate::names::short(q).to_string();
        let sm = short(m);
        let recv = all.first()?;
        let n = self.errors.len();
        let rt = self.infer(recv);
        self.errors.truncate(n);
        let rt = self.resolve(&rt);
        // the traits the receiver is known to have; `what` names it in errors
        let (head, what, traits): (Option<String>, String, Vec<String>) = match &rt {
            Ty::Rigid(p) => {
                let mut all = vec![];
                for (tr, args) in self.bounds.get(p).cloned().unwrap_or_default() {
                    all.push(tr.clone());
                    all.extend(self.supers_of(&tr, &args, 0).into_iter().map(|(s, _)| s));
                }
                (None, p.clone(), all)
            }
            Ty::Con(t, args) if self.env.traits.contains_key(t) => {
                let mut all = vec![t.clone()];
                all.extend(self.supers_of(t, args, 0).into_iter().map(|(s, _)| s));
                (None, short(t), all)
            }
            Ty::Int | Ty::Float | Ty::Bool | Ty::Str | Ty::Con(..) => {
                let head = match &rt {
                    Ty::Int => "Int".to_string(),
                    Ty::Float => "Float".into(),
                    Ty::Bool => "Bool".into(),
                    Ty::Str => "String".into(),
                    Ty::Con(h, _) => h.clone(),
                    _ => unreachable!(),
                };
                // the type's own method of that name comes first (a variant's before its sum's)
                let heads: Vec<String> = std::iter::once(head.clone()).chain(self.env.variant_parent(&head)).collect();
                for h in &heads {
                    if let Some(imp) = self.env.inherent.get(h).cloned() {
                        if let Some(f) = imp.methods.iter().find(|x| short(&x.name) == sm && crate::resolve::takes_self(x)).cloned() {
                            self.targets.push((site, Target::Own(crate::resolve::static_name(h, &f.name))));
                            let ft = self.own_method_ty(&imp, &f);
                            let muts: Vec<Option<TypeExpr>> = f.params.iter().map(|p| p.mutable.then(|| p.ty.clone())).collect();
                            return Some(self.call_with(ft, all, &format!("call to `{sm}`"), &muts));
                        }
                    }
                }
                let trs = self.env.impls.iter().filter(|i| heads.contains(&texpr_head(&i.for_type))).map(|i| i.trait_name.clone()).collect();
                (Some(head.clone()), short(&head), trs)
            }
            _ => return None,
        };
        let mut cands: Vec<String> = traits.into_iter().filter(|t| self.env.traits.get(t).is_some_and(|td| td.methods.iter().any(|x| short(&x.decl.name) == sm))).collect();
        cands.sort();
        cands.dedup();
        let declares_m = |c: &Checker, t: &str| c.env.traits.get(t).is_some_and(|td| td.methods.iter().any(|x| x.decl.name == m));
        match cands.as_slice() {
            // the resolved name is right (or the usual errors apply)
            [t] if declares_m(self, t) => None,
            [t] => {
                let t = t.clone();
                let mq = self.env.traits[&t].methods.iter().find(|x| short(&x.decl.name) == sm).unwrap().decl.name.clone();
                self.targets.push((site, Target::Other(t.clone(), mq.clone())));
                self.check_mut_args(&mq, all, recv_marked);
                let ft = self.trait_method(&mq, &t);
                let muts = self.mut_mask(&mq);
                Some(self.call_with(ft, all, &format!("call to `{sm}`"), &muts))
            }
            [] if head.as_deref().is_some_and(|h| ["Int", "Float", "Bool", "String", "List", "Dict", "Set"].contains(&h)) && BUILTINS.iter().any(|b| b.name == sm) => {
                self.targets.push((site, Target::Builtin(sm.clone())));
                if let Some(t) = self.overloaded(&sm, all) {
                    return Some(t);
                }
                let ft = self.global_fn(&sm)?;
                Some(self.call_with(ft, all, &format!("call to `{sm}`"), &[]))
            }
            [] => None,
            // two traits of the receiver have the name, from any modules: the same errors as within one
            _ => {
                let names = cands.iter().map(|t| short(t)).collect::<Vec<_>>().join("` and `");
                match head {
                    Some(_) => self.err(format!(
                        "`{what}` implements `{names}`, which both have `{sm}`: define `{sm}` in `impl {what}` to say what it means on a `{what}`, or call it where the value is typed as one of the traits"
                    )),
                    None => self.err(format!(
                        "`{sm}` is ambiguous here: `{what}` is known only as `{names}`, which both have `{sm}`, and nothing says which one this call means (even a type that defines `{sm}` itself doesn't say that)"
                    )),
                }
                Some(self.fresh())
            }
        }
    }

    /// The type of method `m` called on `recv`, when that depends on the
    /// receiver: the name is shared by several traits, or some type defines
    /// it itself. The decision is recorded for the call node `site`:
    /// - the receiver's concrete type is known: its own method if it has
    ///   one, else the one trait of the type that declares `m`;
    /// - only traits are known (a trait-typed value, a type parameter's
    ///   bounds): the one trait among them that declares `m`.
    /// Two candidates and nothing to choose between them is an error: even if
    /// the actual type defines `m` itself, that doesn't say which trait's
    /// meaning the call wants. `None` when `m` is not such a method.
    fn receiver_method(&mut self, m: &str, recv: Option<&Expr>, site: usize, dotted: bool) -> Option<Ty> {
        if self.lookup(m).is_some() || self.env.fns.contains_key(m) {
            if !dotted || !recv.is_some_and(|r| self.has_own_method(r, m)) {
                return None;
            }
        } else if !self.env.is_method(m) || self.env.method_contract(m).is_some() {
            return None;
        }
        let short = |q: &str| crate::names::short(q).to_string();
        let sm = short(m);
        let Some(recv) = recv else {
            self.err(format!("`{sm}` is a method: call it on a value (`x.{sm}()`)"));
            return Some(self.fresh());
        };
        // look at the receiver's type without reporting its errors twice (the call checks it again)
        let n = self.errors.len();
        let rt = self.infer(recv);
        self.errors.truncate(n);
        let rt = self.resolve(&rt);
        let declares = |c: &Checker, tr: &str| c.env.traits.get(tr).is_some_and(|t| t.methods.iter().any(|x| x.decl.name == m));
        let via_trait = |c: &mut Checker, tr: &str| {
            c.targets.push((site, Target::Trait(tr.to_string())));
            c.trait_method(m, tr)
        };
        // only traits are known: a type parameter's bounds, or a trait-typed value
        let known: Option<(String, Vec<String>)> = match &rt {
            Ty::Rigid(p) => {
                let mut all = vec![];
                for (tr, args) in self.bounds.get(p).cloned().unwrap_or_default() {
                    all.push(tr.clone());
                    all.extend(self.supers_of(&tr, &args, 0).into_iter().map(|(s, _)| s));
                }
                Some((p.clone(), all))
            }
            Ty::Con(t, args) if self.env.traits.contains_key(t) => {
                let mut all = vec![t.clone()];
                all.extend(self.supers_of(t, args, 0).into_iter().map(|(s, _)| s));
                Some((short(t), all))
            }
            _ => None,
        };
        if let Some((what, trs)) = known {
            let mut cands: Vec<String> = trs.into_iter().filter(|t| declares(self, t)).collect();
            cands.sort();
            cands.dedup();
            return Some(match cands.as_slice() {
                [tr] => {
                    let tr = tr.clone();
                    via_trait(self, &tr)
                }
                [] => {
                    self.err(format!("`{what}` has no method `{sm}`"));
                    self.fresh()
                }
                _ => {
                    let names = cands.iter().map(|t| short(t)).collect::<Vec<_>>().join("` and `");
                    self.err(format!("`{sm}` is ambiguous here: `{what}` is known only as `{names}`, which both have `{sm}`, and nothing says which one this call means (even a type that defines `{sm}` itself doesn't say that)"));
                    self.fresh()
                }
            });
        }
        let head = match &rt {
            Ty::Int => "Int".to_string(),
            Ty::Float => "Float".into(),
            Ty::Bool => "Bool".into(),
            Ty::Str => "String".into(),
            Ty::Unit => "Unit".into(),
            Ty::Con(h, _) => h.clone(),
            Ty::Union(_) => {
                self.err(format!("`{sm}` is ambiguous here: the value is a `{}`, and nothing says which member's `{sm}` this call means (narrow it with `match` first)", show_ty(&rt)));
                return Some(self.fresh());
            }
            _ => {
                self.err(format!("several types or traits have a method `{sm}`, and the type of the value it is called on is not known here: give it a type (e.g. on the `let` or the parameter)"));
                return Some(self.fresh());
            }
        };
        // the concrete type is known: its own method first, a variant's own before its sum's
        let heads: Vec<String> = std::iter::once(head.clone()).chain(self.env.variant_parent(&head)).collect();
        for h in &heads {
            if let Some(imp) = self.env.inherent.get(h).cloned() {
                if let Some(f) = imp.methods.iter().find(|x| x.name == m && crate::resolve::takes_self(x)).cloned() {
                    self.targets.push((site, Target::Own(crate::resolve::static_name(h, m))));
                    return Some(self.own_method_ty(&imp, &f));
                }
            }
            let trs = self.env.traits_with(m, h);
            match trs.as_slice() {
                [tr] => {
                    let tr = tr.clone();
                    return Some(via_trait(self, &tr));
                }
                [] => {}
                _ => {
                    let names = trs.iter().map(|t| short(t)).collect::<Vec<_>>().join("` and `");
                    self.err(format!(
                        "`{}` implements `{names}`, which both have `{sm}`: define `{sm}` in `impl {}` to say what it means on a `{}`, or call it where the value is typed as one of the traits",
                        short(h), short(h), short(h)
                    ));
                    return Some(self.fresh());
                }
            }
        }
        match self.env.method_traits.get(m).map(|v| v.as_slice()) {
            // the obligation reports that the type doesn't implement it
            Some([tr]) => {
                let tr = tr.clone();
                Some(via_trait(self, &tr))
            }
            _ => {
                self.err(format!("`{}` has no method `{sm}`", show_ty(&rt)));
                Some(self.fresh())
            }
        }
    }

    /// After the callee of a call was typed: if it is a generic function whose
    /// type parameters are needed at runtime, remember their instantiation.
    fn record_inst(&mut self, callee: &Expr, site: usize) {
        let Expr::Var(n) = callee else { return };
        let Some((name, vars)) = self.last_inst.take() else { return };
        if &name != n || self.lookup(n).is_some() {
            return;
        }
        let Some(f) = self.env.fns.get(n) else { return };
        let need = self.env.type_args(f);
        if !need.is_empty() {
            self.insts.push((site, need.into_iter().map(|p| { let t = vars[&p].clone(); (p, t) }).collect()));
        }
    }

    /// The runtime type of each recorded instantiation, once the declaration
    /// is fully inferred: a concrete type, or the caller's own type parameter.
    fn finish_insts(&mut self) {
        for (site, pairs) in std::mem::take(&mut self.insts) {
            let mut refs = Vec::new();
            for (p, t) in pairs {
                let t = self.resolve(&t);
                refs.push(match &t {
                    Ty::Rigid(q) => TypeRef::Param(q.clone()),
                    Ty::Con(h, _) if self.env.traits.contains_key(h) => {
                        self.err(format!("`{p}` must be a concrete type here (it has methods called on the type), not any `{}`", crate::names::short(&h)));
                        continue;
                    }
                    _ => match ty_head(&t) {
                        Some(h) => TypeRef::Head(h),
                        // unknown: the bound's obligation already asks for an annotation
                        None => continue,
                    },
                });
            }
            self.targets.push((site, Target::TypeArgs(refs)));
        }
    }

    /// `X.m(..)` where `X` is a type, a type parameter or `Self`, and `m` a
    /// trait method without `self`: the trait is the one of `X`'s that
    /// declares it. `None` when this is not such a call.
    fn static_call(&mut self, recv: &Expr, m: &str, site: usize) -> Option<Ty> {
        let x = match recv {
            Expr::Ctor(x, CtorArgs::None) | Expr::Var(x) if self.lookup(x).is_none() => x.clone(),
            _ => return None,
        };
        let statics: Vec<String> = self.env.method_traits.get(m)?.iter().filter(|t| self.env.traits.get(*t).is_some_and(|td| td.methods.iter().any(|tm| tm.decl.name == m && !crate::resolve::takes_self(&tm.decl)))).cloned().collect();
        if statics.is_empty() {
            return None;
        }
        let short = |q: &str| crate::names::short(q).to_string();
        let sm = short(m);
        // the type, how it is passed at runtime, and the traits it is known to have
        let rigid = |c: &mut Checker, p: &str| {
            let mut all = vec![];
            for (tr, args) in c.bounds.get(p).cloned().unwrap_or_default() {
                all.push(tr.clone());
                all.extend(c.supers_of(&tr, &args, 0).into_iter().map(|(s, _)| s));
            }
            all
        };
        let (ty, tref, known): (Ty, TypeRef, Vec<String>) = if x == "Self" {
            match self.self_ty.clone() {
                Some(Ty::Rigid(p)) => {
                    let k = rigid(self, &p);
                    (Ty::Rigid(p.clone()), TypeRef::Param(p), k)
                }
                Some(t) => {
                    let h = ty_head(&self.resolve(&t))?;
                    let k = self.env.impls.iter().filter(|i| texpr_head(&i.for_type) == h).map(|i| i.trait_name.clone()).collect();
                    (t, TypeRef::Head(h), k)
                }
                None => return None,
            }
        } else if self.rigid.contains(&x) {
            // only functions are given their type arguments when they run
            if self.self_ty.is_some() {
                self.err(format!("`{}.{sm}()`: methods can't be called on an impl's or trait's own type parameters yet (only a function's)", short(&x)));
                return Some(self.fresh());
            }
            let k = rigid(self, &x);
            (Ty::Rigid(x.clone()), TypeRef::Param(x.clone()), k)
        } else {
            let t = match x.as_str() {
                "Int" => Ty::Int,
                "Float" => Ty::Float,
                "Bool" => Ty::Bool,
                "String" => Ty::Str,
                _ => match self.env.types.get(&x) {
                    Some(td) => {
                        let args = td.params.iter().map(|_| self.fresh()).collect();
                        Ty::Con(x.clone(), args)
                    }
                    None => return None,
                },
            };
            let k = self.env.impls.iter().filter(|i| texpr_head(&i.for_type) == x).map(|i| i.trait_name.clone()).collect();
            (t, TypeRef::Head(x.clone()), k)
        };
        let mut cands: Vec<String> = known.into_iter().filter(|t| statics.contains(t)).collect();
        cands.sort();
        cands.dedup();
        let tr = match cands.as_slice() {
            [tr] => tr.clone(),
            // not known to have it: with a single candidate, the obligation says what is missing
            [] if statics.len() == 1 && matches!(tref, TypeRef::Head(_)) => statics[0].clone(),
            [] => {
                self.err(format!("`{}` has no method `{sm}` called on the type (none of its traits declares one)", short(&x)));
                return Some(self.fresh());
            }
            _ => {
                let names = cands.iter().map(|t| short(t)).collect::<Vec<_>>().join("` and `");
                self.err(format!("`{}.{sm}()` is ambiguous: `{names}` both have `{sm}`", short(&x)));
                return Some(self.fresh());
            }
        };
        self.targets.push((site, Target::Static(tr.clone(), tref)));
        Some(self.trait_method_for(m, &tr, Some(ty)))
    }

    /// The type of a type's own method, freshly instantiated: `Self` is the
    /// type, and the impl's and method's type parameters are fresh.
    fn own_method_ty(&mut self, imp: &ImplDecl, f: &FnDecl) -> Ty {
        let vars: HashMap<String, Ty> = imp.type_params.iter().chain(&f.type_params).map(|p| (p.clone(), self.fresh())).collect();
        let self_t = self.lower(&imp.for_type, &vars);
        let saved = self.self_ty.replace(self_t);
        let fty = self.lower(&Self::fn_scheme(f), &vars);
        self.self_ty = saved;
        for b in imp.bounds.iter().chain(&f.bounds) {
            for tr in &b.traits {
                if let TypeExpr::Named(tn, targs) = tr {
                    let args: Vec<Ty> = targs.iter().map(|a| self.lower(a, &vars)).collect();
                    if let Some(v) = vars.get(&b.param) {
                        self.pending.push((v.clone(), tn.clone(), args, format!("calling `{}`", f.name)));
                    }
                }
            }
        }
        fty
    }

    /// The written type of each `mut` parameter of a named function or trait method.
    fn mut_mask(&self, name: &str) -> Vec<Option<TypeExpr>> {
        if self.lookup(name).is_some() {
            return vec![];
        }
        let ps = match self.env.fns.get(name) {
            Some(f) => f.params.clone(),
            None => self.env.method_params(name).unwrap_or_default(),
        };
        ps.iter().map(|p| p.mutable.then(|| p.ty.clone())).collect()
    }

    /// Whether `e` may change the variable `root` (or one of its fields).
    fn assigns_to(&self, e: &Expr, root: &str) -> bool {
        let mut_params = |n: &str| self.mut_mask(n).iter().map(Option::is_some).collect();
        changed_places(e, &mut_params).iter().any(|q| q == root || q.starts_with(&format!("{root}.")))
    }

    /// Whether a literal is outside a written type's refinement (decided for
    /// integer, float and string-length refinements).
    fn literal_outside(&self, w: &TypeExpr, e: &Expr) -> bool {
        let int = match e {
            Expr::Int(n) => Some(*n as i128),
            Expr::BigInt(d) => d.parse::<i128>().ok(),
            Expr::Un(UnOp::Neg, x) => match &**x {
                Expr::Int(n) => Some(-(*n as i128)),
                _ => None,
            },
            _ => None,
        };
        match (self.int_refinement(w), int, e) {
            (Refn::Ints(s), Some(n), _) => !ints::contains(&s, n, n),
            (Refn::Reals(r), _, Expr::Float(f)) => !r.holds(*f),
            (Refn::Lens(s), _, Expr::Str(x)) => !ints::contains(&s, x.chars().count() as i128, x.chars().count() as i128),
            _ => false,
        }
    }

    /// Whether every value of `narrow` is a value of `wide`, as far as their
    /// refinements can be compared (`None`: they cannot be).
    fn refinement_within(&self, narrow: &TypeExpr, wide: &TypeExpr) -> Option<bool> {
        // a type parameter (`T`, `Self`) or anything not built on Int, Float,
        // String or List is left to the runtime check of the write-back
        if self.base_of(narrow) == Base::Other || self.base_of(narrow) != self.base_of(wide) {
            return None;
        }
        match (self.int_refinement(narrow), self.int_refinement(wide)) {
            (_, Refn::Plain) => Some(true),
            (Refn::Plain, Refn::Ints(_) | Refn::Lens(_) | Refn::Reals(_)) => Some(false),
            (Refn::Ints(a), Refn::Ints(b)) | (Refn::Lens(a), Refn::Lens(b)) => Some(ints::and(&a, &ints::not(&b)).is_empty()),
            (Refn::Reals(a), Refn::Reals(b)) => {
                let mut cuts = [a.cuts.clone(), b.cuts.clone()].concat();
                cuts.sort_by(|x, y| x.partial_cmp(y).unwrap());
                let mut points: Vec<f64> = cuts.clone();
                points.extend(cuts.windows(2).map(|w| (w[0] + w[1]) / 2.0));
                points.extend(cuts.first().map(|c| c - 1.0));
                points.extend(cuts.last().map(|c| c + 1.0));
                points.push(0.0);
                Some(points.iter().all(|x| !a.holds(*x) || b.holds(*x)))
            }
            _ => None,
        }
    }

    fn call_with(&mut self, fty: Ty, args: &[&Expr], what: &str, muts: &[Option<TypeExpr>]) -> Ty {
        let fty = self.resolve(&fty);
        let (ps, r) = match fty {
            Ty::Fn(ps, r) => (ps, *r),
            Ty::Var(_) => {
                let ps: Vec<Ty> = args.iter().map(|_| self.fresh()).collect();
                let r = self.fresh();
                self.expect(&fty, &Ty::Fn(ps.clone(), Box::new(r.clone())), what);
                (ps, r)
            }
            other => {
                self.err(format!("{what}: `{}` is not a function", show_ty(&other)));
                return self.fresh();
            }
        };
        if ps.len() != args.len() {
            self.err(format!("{what}: expects {} arguments, got {}", ps.len(), args.len()));
            return r;
        }
        for (i, (p, a)) in ps.iter().zip(args).enumerate() {
            let at = self.infer_arg(a, Some(p));
            if let Some(Some(pw)) = muts.get(i) {
                // the callee may put any value of its parameter type into the caller's variable
                if self.unify(p, &at).is_err() {
                    let (want, got) = (self.show(p), self.show(&at));
                    self.err(format!("{what}, argument {}: expected exactly `{want}`, found `{got}` (the parameter is `mut`, so the function can put any `{want}` into the caller's variable)", i + 1));
                } else if let Some(x) = place_path(a).map(|q| q.split('.').next().unwrap_or("").to_string()) {
                    // ... including values the variable's own refinement rules out
                    let bare = match a {
                        Expr::Mut(x) => &**x,
                        x => x,
                    };
                    if let (Expr::Var(_), Some(vw)) = (bare, self.written_of(&x)) {
                        if self.refinement_within(pw, &vw) == Some(false) {
                            self.err(format!(
                                "{what}, argument {}: `{x}` is declared `{}`, but the `mut` parameter is `{}`: the function could store a value `{x}` must never hold",
                                i + 1,
                                crate::printer::ty(&vw),
                                crate::printer::ty(pw)
                            ));
                        }
                    }
                }
            } else {
                self.expect(p, &at, &format!("{what}, argument {}", i + 1));
            }
        }
        r
    }

    /// A call's argument: `f(mut x)` is `x`, marked for a `mut` parameter
    /// (`check_mut_args` says whether it may be).
    fn infer_arg(&mut self, a: &Expr, expected: Option<&Ty>) -> Ty {
        match a {
            Expr::Mut(x) if marks_arg(a) => self.infer_with(x, expected),
            _ => self.infer_with(a, expected),
        }
    }

    fn infer_with(&mut self, e: &Expr, expected: Option<&Ty>) -> Ty {
        // Bidirectional lite: lambdas take their parameter types from context.
        if let (Expr::Lambda(ps, body), Some(exp)) = (e, expected) {
            if let Ty::Fn(pts, _) = self.resolve(exp) {
                if pts.len() == ps.len() {
                    let t = self.lambda(ps, Some(pts), body);
                    self.types.push((e as *const Expr as usize, t.clone()));
                    return t;
                }
            }
        }
        if let (Expr::Hole(h), Some(exp)) = (e, expected) {
            self.record_hole(h, exp.clone());
            return exp.clone();
        }
        self.infer_at(e, expected)
    }

    fn infer(&mut self, e: &Expr) -> Ty {
        self.infer_at(e, None)
    }

    /// A lambda, with parameter types from context when they are known.
    /// Written parameter types must agree with them.
    fn lambda(&mut self, ps: &[LamParam], from_context: Option<Vec<Ty>>, body: &Expr) -> Ty {
        let pts: Vec<Ty> = match from_context {
            Some(ts) => ts,
            None => ps.iter().map(|_| self.fresh()).collect(),
        };
        for (p, t) in ps.iter().zip(&pts) {
            if let Some(w) = &p.ty {
                self.check_refined(w);
                let wt = self.lower(w, &HashMap::new());
                self.expect(&wt, t, &format!("lambda parameter `{}`", p.name));
            }
        }
        self.scopes.push(HashMap::new());
        for (p, t) in ps.iter().zip(&pts) {
            self.bind(&p.name, t.clone());
            if let Some(w) = &p.ty {
                self.bind_written(&p.name, w);
            }
        }
        let floor = std::mem::replace(&mut self.lambda_floor, self.scopes.len() - 1);
        let loops = std::mem::take(&mut self.loops);
        let r = self.fresh();
        let outer = self.ret.replace(r.clone());
        let bt = self.infer(body);
        self.expect(&r, &bt, "lambda body");
        self.ret = outer;
        self.loops = loops;
        self.lambda_floor = floor;
        self.scopes.pop();
        Ty::Fn(pts, Box::new(r))
    }

    fn record_hole(&mut self, name: &str, t: Ty) {
        let mut ctx: Vec<(String, Ty)> = Vec::new();
        for s in &self.scopes {
            for (k, v) in s.iter().filter(|(k, _)| !k.contains(':')) {
                ctx.retain(|(n, _)| n != k);
                ctx.push((k.clone(), v.clone()));
            }
        }
        ctx.sort_by(|a, b| a.0.cmp(&b.0));
        self.holes.push((name.to_string(), t, ctx));
    }

    fn ctor_type(&mut self, c: &str) -> Option<(Ty, Vec<(Option<String>, Ty)>)> {
        let tname = self.env.ctors.get(c)?.clone();
        let td = self.env.types[&tname].clone();
        let vars: HashMap<String, Ty> = td.params.iter().map(|p| (p.clone(), self.fresh())).collect();
        let result = Ty::Con(tname.clone(), td.params.iter().map(|p| vars[p].clone()).collect());
        let fields = match &td.body {
            TypeBody::Record(fs) => fs.iter().map(|f| (Some(f.name.clone()), self.lower(&f.ty, &vars))).collect(),
            TypeBody::Sum(vs) => {
                let v = vs.iter().find(|v| v.name == c).unwrap();
                match &v.payload {
                    Payload::None => vec![],
                    Payload::Positional(ts) => ts.iter().map(|t| (None, self.lower(t, &vars))).collect(),
                    Payload::Record(fs) => fs.iter().map(|f| (Some(f.name.clone()), self.lower(&f.ty, &vars))).collect(),
                }
            }
            TypeBody::Alias(a) => vec![(None, self.lower(a, &vars))],
        };
        Some((result, fields))
    }

    /// The type of `e`; `expected` (when known) is what the context wants, used
    /// to pick a variant's own type and to check branches against it.
    fn infer_at(&mut self, e: &Expr, expected: Option<&Ty>) -> Ty {
        let t = self.infer_node(e, expected);
        self.types.push((e as *const Expr as usize, t.clone()));
        t
    }

    fn infer_node(&mut self, e: &Expr, expected: Option<&Ty>) -> Ty {
        match e {
            Expr::Int(_) | Expr::BigInt(_) => Ty::Int,
            Expr::Float(_) => Ty::Float,
            Expr::Tuple(es) => Ty::Con("Tuple".into(), es.iter().map(|e| self.infer(e)).collect()),
            Expr::Str(_) => Ty::Str,
            Expr::Bool(_) => Ty::Bool,
            Expr::Unit => Ty::Unit,
            Expr::Hole(h) => {
                let t = self.fresh();
                self.record_hole(h, t.clone());
                t
            }
            // rewritten into builtin calls when the program is resolved
            Expr::Range(..) | Expr::Index(..) | Expr::Comp(..) | Expr::Dict(..) | Expr::Set(..) => {
                self.err("internal: a range, slice or comprehension was not desugared");
                self.fresh()
            }
            Expr::Var(x) => {
                if let Some(t) = self.lookup(x) {
                    return t;
                }
                if let Some(t) = self.global_fn(x) {
                    return t;
                }
                self.err(format!("unknown variable `{x}`"));
                self.fresh()
            }
            Expr::List(es) => {
                let want = match expected.map(|t| self.resolve(t)) {
                    Some(Ty::Con(n, a)) if n == "List" && a.len() == 1 => Some(a[0].clone()),
                    _ => None,
                };
                let mut t = want.clone().unwrap_or_else(|| self.fresh());
                for el in es {
                    let et = self.infer_with(el, Some(&t));
                    match &want {
                        Some(w) => self.expect(w, &et, "list element"),
                        None => t = self.join_union(&t, &et, "list element"),
                    }
                }
                Ty::Con("List".into(), vec![t])
            }
            Expr::Ctor(c, args) => {
                let Some((rt, fields)) = self.ctor_type(c) else {
                    if self.env.traits.contains_key(c) {
                        self.err(format!("`{c}` is a trait, not a value: call its methods on a value that implements it (e.g. a `{}` parameter)", c.to_lowercase()));
                        return self.fresh();
                    }
                    self.err(format!("unknown constructor `{c}`"));
                    return self.fresh();
                };
                // the expected type gives the type arguments (so `Err(NonEmpty)` checks its
                // argument against the error type), and asks for the variant's own type
                let own = self.variant_ty(c, &rt);
                let mut result = rt.clone();
                if let Some(exp) = expected.map(|t| self.resolve(t)) {
                    let members = match &exp {
                        Ty::Union(ms) => ms.clone(),
                        t => vec![t.clone()],
                    };
                    let head = |t: &Ty| match t { Ty::Con(n, _) => Some(n.clone()), _ => None };
                    let rt_head = head(&rt);
                    if let Some(m) = members.iter().find(|m| head(m).as_deref() == Some(c.as_str()) || head(m) == rt_head) {
                        if let (Ty::Con(_, want), Ty::Con(_, have)) = (m, &rt) {
                            let snap = self.subst.clone();
                            if want.len() != have.len() || want.iter().zip(have).any(|(w, h)| self.unify(h, w).is_err()) {
                                self.subst = snap;
                            }
                        }
                        if head(m).as_deref() == Some(c.as_str()) {
                            result = own.clone();
                        }
                    }
                }
                match args {
                    CtorArgs::None => {
                        if !fields.is_empty() {
                            self.err(format!("constructor `{c}` needs {} fields", fields.len()));
                        }
                    }
                    CtorArgs::Positional(es) => {
                        if fields.len() != es.len() || fields.iter().any(|f| f.0.is_some()) {
                            self.err(format!("constructor `{c}` expects {} positional arguments", fields.len()));
                        }
                        for ((_, ft), a) in fields.iter().zip(es) {
                            let at = self.infer_with(a, Some(ft));
                            self.expect(ft, &at, &format!("argument of `{c}`"));
                        }
                    }
                    CtorArgs::Update(base, fs) => {
                        let tname = self.env.ctors.get(c).cloned().unwrap_or_default();
                        if !matches!(self.env.types.get(&tname).map(|t| &t.body), Some(TypeBody::Record(_))) {
                            self.err(format!("`{c} {{ ..base }}` update syntax works on record types only"));
                        }
                        let bt = self.infer_with(base, Some(&rt));
                        self.expect(&rt, &bt, &format!("base of `{c} {{ .. }}`"));
                        for (fname, fe) in fs {
                            match fields.iter().find(|(n, _)| n.as_deref() == Some(fname)) {
                                Some((_, ft)) => {
                                    let ft = ft.clone();
                                    let at = self.infer_with(fe, Some(&ft));
                                    self.expect(&ft, &at, &format!("field `{c}.{fname}`"));
                                }
                                None => self.err(format!("`{c}` has no field `{fname}`")),
                            }
                        }
                    }
                    CtorArgs::Record(fs) => {
                        for (fname, fe) in fs {
                            match fields.iter().find(|(n, _)| n.as_deref() == Some(fname)) {
                                Some((_, ft)) => {
                                    let ft = ft.clone();
                                    let at = self.infer_with(fe, Some(&ft));
                                    self.expect(&ft, &at, &format!("field `{c}.{fname}`"));
                                }
                                None => self.err(format!("`{c}` has no field `{fname}`")),
                            }
                        }
                        for (n, _) in &fields {
                            if let Some(n) = n {
                                if !fs.iter().any(|(f, _)| f == n) {
                                    self.err(format!("missing field `{n}` in `{c}`"));
                                }
                            }
                        }
                    }
                }
                result
            }
            Expr::Call(f, args) => {
                let args: Vec<&Expr> = args.iter().collect();
                let args = &args[..];
                if let Expr::Var(n) = &**f {
                    if let Some(t) = self.by_receiver(n, args, e as *const Expr as usize, false, None) {
                        return t;
                    }
                    if self.lookup(n).is_none() {
                        self.check_mut_args(n, args, None);
                    } else {
                        for a in args.iter().filter(|a| marks_arg(a)) {
                            self.err(format!("`{}`: the closure `{n}` has no `mut` parameters", crate::printer::expr(a, 0)));
                        }
                    }
                    if let Some(t) = self.overloaded(n, args) {
                        return t;
                    }
                }
                let what = match &**f {
                    Expr::Var(n) => format!("call to `{n}`"),
                    _ => "call".into(),
                };
                let muts = match &**f {
                    Expr::Var(n) => self.mut_mask(n),
                    _ => vec![],
                };
                if let Expr::Var(n) = &**f {
                    if let Some(ft) = self.receiver_method(n, args.first().copied(), e as *const Expr as usize, false) {
                        return self.call_with(ft, args, &what, &muts);
                    }
                }
                self.last_inst = None;
                self.callee = matches!(&**f, Expr::Var(_));
                let ft = self.infer(f);
                self.callee = false;
                self.record_inst(f, e as *const Expr as usize);
                self.call_with(ft, args, &what, &muts)
            }
            Expr::Method(recv, m, args) => {
                // the program's own nodes, not copies: decisions are recorded by node
                let all: Vec<&Expr> = std::iter::once(&**recv).chain(args.iter()).collect();
                let marked = Some(self.marked.contains(&(e as *const Expr as usize)));
                if let Some(t) = self.by_receiver(m, &all, e as *const Expr as usize, true, marked) {
                    return t;
                }
                if self.lookup(m).is_none() {
                    self.check_mut_args(m, &all, marked);
                } else if marked == Some(true) {
                    self.err(format!("`mut` marks a call that changes its receiver, but `{m}` is a closure"));
                }
                if let Some(t) = self.overloaded(m, &all) {
                    return t;
                }
                if let Some(ft) = self.static_call(recv, m, e as *const Expr as usize) {
                    let rest: Vec<&Expr> = args.iter().collect();
                    let muts = self.mut_mask(m);
                    return self.call_with(ft, &rest, &format!("call to `{m}`"), &muts);
                }
                self.last_inst = None;
                let ft = match self.receiver_method(m, Some(recv), e as *const Expr as usize, true) {
                    Some(ft) => ft,
                    None => match self.lookup(m).or_else(|| {
                        self.callee = true;
                        let t = self.global_fn(m);
                        self.callee = false;
                        t
                    }) {
                        Some(ft) => ft,
                        None => {
                            match &**recv {
                                // `Item.new()` that `impl Item` doesn't define
                                Expr::Ctor(t, CtorArgs::None) if self.env.types.contains_key(t) => {
                                    let short = |q: &str| crate::names::short(q).to_string();
                                    self.err(format!("`{}` has no method `{}` called on the type (one without `self`, in `impl {}`)", short(t), short(m), short(t)))
                                }
                                _ => self.err(format!("unknown function `{m}`")),
                            }
                            return self.fresh();
                        }
                    },
                };
                let muts = self.mut_mask(m);
                if self.lookup(m).is_none() {
                    self.record_inst(&Expr::Var(m.clone()), e as *const Expr as usize);
                }
                self.call_with(ft, &all, &format!("call to `{m}`"), &muts)
            }
            Expr::Field(r, f) if f.contains('·') => {
                // a tagged access (see `field_owners`): type it, and record its receiver's records
                let (base, tag) = f.split_once('·').unwrap();
                let rt = self.infer(r);
                let t = self.field_type(&rt, base);
                let owners: BTreeSet<String> = match self.resolve(&rt) {
                    Ty::Con(n, _) => [n].into(),
                    Ty::Union(ms) => ms.iter().filter_map(|m| if let Ty::Con(n, _) = self.resolve(m) { Some(n) } else { None }).collect(),
                    _ => BTreeSet::new(),
                };
                FIELD_PROBE.with(|p| {
                    if let Some(m) = p.borrow_mut().as_mut() {
                        m.entry(tag.to_string()).or_default().extend(owners);
                    }
                });
                t
            }
            Expr::Field(r, f) => {
                // `o.status` narrowed by an enclosing `match o.status { … }`
                if let Some(p) = place_path(e) {
                    if let Some(t) = self.lookup(&format!("path:{p}")) {
                        return t;
                    }
                }
                let rt = self.infer(r);
                self.field_type(&rt, f)
            }
            Expr::Lambda(ps, body) => self.lambda(ps, None, body),
            Expr::Bin(op, l, r) => {
                let (lt, rt) = (self.infer(l), self.infer(r));
                use BinOp::*;
                match op {
                    // `+` also joins strings and lists
                    Add if [&lt, &rt].iter().any(|t| joinable(&self.resolve(t))) => {
                        self.expect(&lt, &rt, "`+` operands");
                        lt
                    }
                    Add | Sub | Mul | Div | Mod => {
                        self.numeric(&lt, &rt, op);
                        lt
                    }
                    Eq | Ne => {
                        self.join(&lt, &rt, &format!("`{}` operands", op.symbol()));
                        Ty::Bool
                    }
                    Lt | Le | Gt | Ge => {
                        self.numeric(&lt, &rt, op);
                        Ty::Bool
                    }
                    And | Or | Implies => {
                        self.expect(&Ty::Bool, &lt, &format!("left of `{}`", op.symbol()));
                        self.expect(&Ty::Bool, &rt, &format!("right of `{}`", op.symbol()));
                        Ty::Bool
                    }
                }
            }
            Expr::Un(UnOp::Neg, x) => {
                let t = self.infer(x);
                match self.resolve(&t) {
                    Ty::Int | Ty::Float => {}
                    Ty::Var(_) => self.expect(&Ty::Int, &t, "operand of `-`"),
                    other => self.err(format!("operand of `-` must be Int or Float, found `{}`", show_ty(&other))),
                }
                t
            }
            Expr::Un(UnOp::Not, x) => {
                let t = self.infer(x);
                self.expect(&Ty::Bool, &t, "operand of `!`");
                Ty::Bool
            }
            Expr::Is(x, c) => {
                let t = self.infer(x);
                // the value must be able to be one: `shape is Circle`, `n is Positive`
                match self.ctor_type(c) {
                    Some((ct, _)) => {
                        let own = self.variant_ty(c, &ct);
                        self.expect(&t, &own, &format!("`is {c}`"))
                    }
                    None if self.env.is_type_name(c) => {
                        let pt = self.type_named(c);
                        if !self.is_trait_ty(&pt) {
                            self.expect(&t, &pt, &format!("`is {c}`"));
                        }
                    }
                    None => self.err(format!("unknown constructor `{c}`")),
                }
                Ty::Bool
            }
            Expr::If(c, t, f) => {
                let ct = self.infer(c);
                self.expect(&Ty::Bool, &ct, "if condition");
                match expected {
                    Some(exp) => {
                        let exp = exp.clone();
                        for b in [t, f] {
                            let bt = self.infer_with(b, Some(&exp));
                            self.expect(&exp, &bt, "if branch");
                        }
                        exp
                    }
                    None => {
                        let tt = self.infer(t);
                        let ft = self.infer_with(f, Some(&tt));
                        self.join_union(&tt, &ft, "if branches")
                    }
                }
            }
            Expr::Match(s, name, arms) => {
                let st = self.infer(s);
                // the matched variable (or the `as` name, or a field path) is narrowed in each arm
                let target = name.clone().or(match &**s {
                    Expr::Var(x) if self.lookup(x).is_some() => Some(x.clone()),
                    _ => None,
                });
                let path = if name.is_none() { place_path(s).filter(|p| p.contains('.')) } else { None };
                // what the matched value's written type already rules out (`n: Positive`)
                let written = self.written_scrutinee(s);
                let excluded = written.as_ref().and_then(|w| self.excluded_by(w).map(|p| (p, crate::printer::ty(w))));
                let mut rt: Option<Ty> = expected.cloned();
                let mut rows = Vec::new();
                for a in arms {
                    self.scopes.push(HashMap::new());
                    let (nt, p) = self.pattern(&a.pat, &st);
                    // narrowing follows the code: an arm that changes the matched variable
                    // (even later, or on a later pass of a loop) keeps its declared type
                    let changed = |c: &Self, root: &str| c.lookup(&format!("mut:{root}")).is_some() && c.assigns_to(&a.body, root);
                    let target = target.clone().filter(|x| name.is_some() || !changed(self, x));
                    let path = path.clone().filter(|p| !changed(self, p.split('.').next().unwrap_or("")));
                    if let Some(x) = &target {
                        self.bind(x, nt.clone());
                        if name.is_none() {
                            self.bind(&format!("narrowed:{x}"), st.clone());
                        }
                        // the arm's own refinement, for matches inside it
                        let arm_written = match &a.pat {
                            Pattern::Ctor(c, CtorPat::None) if !self.env.ctors.contains_key(c) && self.env.is_type_name(c) => Some(TypeExpr::Named(c.clone(), vec![])),
                            Pattern::Cond(pred) => Some(TypeExpr::Refined(Box::new(written.clone().unwrap_or_else(|| ty_to_texpr(&self.resolve(&st)))), pred.clone())),
                            Pattern::Wild | Pattern::Bind(_) => written.clone(),
                            _ => None,
                        };
                        if let Some(w) = arm_written {
                            self.bind_written(x, &w);
                        }
                    }
                    if let Some(pth) = &path {
                        self.bind(&format!("path:{pth}"), nt);
                    }
                    let bt = self.infer_with(&a.body, rt.as_ref());
                    rt = Some(match rt {
                        Some(r) if expected.is_some() => {
                            self.expect(&r, &bt, "match arm");
                            r
                        }
                        Some(r) => self.join_union(&r, &bt, "match arms"),
                        None => bt,
                    });
                    self.scopes.pop();
                    rows.push((p, crate::printer::pat(&a.pat)));
                }
                self.matches.push(Coverage { ty: st, arms: rows, excluded });
                rt.unwrap_or_else(|| self.fresh())
            }
            Expr::Block(stmts, last) => {
                self.scopes.push(HashMap::new());
                for s in stmts {
                    match s {
                        Stmt::Let(p, ann, e) => {
                            let ann_written = ann.as_ref();
                            let ann = ann.as_ref().map(|a| self.lower(a, &HashMap::new()));
                            let t = self.infer_with(e, ann.as_ref());
                            if let Some(a) = &ann {
                                self.expect(a, &t, "let annotation");
                            }
                            let (_, pp) = self.pattern(p, &t);
                            if let (Pattern::Bind(x), Some(w)) = (p, ann_written) {
                                self.bind_written(x, w);
                            }
                            if !irrefutable(p) {
                                self.matches.push(Coverage { ty: t, arms: vec![(pp, crate::printer::pat(p))], excluded: None });
                            }
                        }
                        Stmt::Var(n, ann, e) => {
                            let written = ann.clone();
                            let ann = ann.as_ref().map(|a| self.lower(a, &HashMap::new()));
                            let t = self.infer_with(e, ann.as_ref());
                            if let Some(a) = &ann {
                                self.expect(a, &t, "let annotation");
                            }
                            self.bind(n, t);
                            self.bind(&format!("mut:{n}"), Ty::Unit);
                            if let Some(w) = &written {
                                self.bind_written(n, w);
                            }
                        }
                        Stmt::Assign(place, e) => {
                            self.check_place(place, "assign to");
                            let pt = self.end_narrowing(place).unwrap_or_else(|| self.infer(place));
                            let et = self.infer_with(e, Some(&pt));
                            self.expect(&pt, &et, "assignment");
                            // a literal that breaks the variable's declared refinement
                            if let Expr::Var(x) = place {
                                if let Some(w) = self.written_of(x) {
                                    if self.literal_outside(&w, e) {
                                        self.err(format!("refinement violated at compile time in assigning `{x}`: {} does not satisfy `{}`", crate::printer::expr(e, 0), crate::printer::ty(&w)));
                                    }
                                }
                            }
                        }
                        Stmt::While(c, b) => {
                            let ct = self.infer(c);
                            self.expect(&Ty::Bool, &ct, "while condition");
                            self.loops += 1;
                            self.infer(b);
                            self.loops -= 1;
                        }
                        Stmt::Loop(b) => {
                            self.loops += 1;
                            self.infer(b);
                            self.loops -= 1;
                        }
                        Stmt::Break | Stmt::Continue => {
                            if self.loops == 0 {
                                self.err(format!("`{}` outside a loop", if matches!(s, Stmt::Break) { "break" } else { "continue" }));
                            }
                        }
                        Stmt::For(p, it, b) => {
                            let itt = self.infer(it);
                            let el = match self.resolve(&itt) {
                                Ty::Con(n, args) if (n == "List" || n == "Set") && args.len() == 1 => args[0].clone(),
                                Ty::Con(n, args) if n == "Dict" && args.len() == 2 => Ty::Con("Tuple".into(), args.clone()),
                                Ty::Var(_) => {
                                    let el = self.fresh();
                                    self.expect(&Ty::Con("List".into(), vec![el.clone()]), &itt, "for loop");
                                    el
                                }
                                other => {
                                    self.err(format!("`for` needs a List, Set or Dict, found `{}`", show_ty(&other)));
                                    self.fresh()
                                }
                            };
                            self.scopes.push(HashMap::new());
                            self.pattern(p, &el);
                            self.loops += 1;
                            self.infer(b);
                            self.loops -= 1;
                            self.scopes.pop();
                        }
                        Stmt::Expr(e) => {
                            self.infer(e);
                        }
                        Stmt::Comment(_) => {}
                        Stmt::Assert(e) => {
                            let t = self.infer(e);
                            self.expect(&Ty::Bool, &t, "assert");
                        }
                    }
                }
                let t = self.infer_with(last, expected);
                self.scopes.pop();
                t
            }
            Expr::Interp(parts) => {
                for p in parts {
                    if let StrPart::Expr(e, spec) = p {
                        let t = self.infer(e);
                        if let Some(f) = spec {
                            self.check_spec(f, &t);
                        }
                    }
                }
                Ty::Str
            }
            Expr::Return(x) => {
                match self.ret.clone() {
                    Some(r) => {
                        let t = self.infer_with(x, Some(&r));
                        self.expect(&r, &t, "`return` value");
                    }
                    None => {
                        self.err("`return` outside a function");
                        self.infer(x);
                    }
                }
                // `return` never produces a value where it stands
                self.fresh()
            }
            Expr::Try(x, kind) => {
                let t = self.infer(x);
                let (op, ty_name) = match kind {
                    TryKind::Result => ("!", "Result"),
                    TryKind::Option => ("?", "Option"),
                };
                let inner = self.fresh();
                let (wrapped, passed) = match kind {
                    TryKind::Result => {
                        let e = self.fresh();
                        let out = self.fresh();
                        (Ty::Con("Result".into(), vec![inner.clone(), e.clone()]), Ty::Con("Result".into(), vec![out, e]))
                    }
                    TryKind::Option => {
                        let out = self.fresh();
                        (Ty::Con("Option".into(), vec![inner.clone()]), Ty::Con("Option".into(), vec![out]))
                    }
                };
                if self.unify(&wrapped, &t).is_err() {
                    let shown = self.show(&t);
                    let hint = if *kind == TryKind::Result { "use `?` on an Option" } else { "use `!` on a Result" };
                    let article = if *kind == TryKind::Result { "a" } else { "an" };
                    self.err(format!("`{op}` needs {article} {ty_name}, found `{shown}` ({hint})"));
                    return inner;
                }
                match self.ret.clone() {
                    Some(r) => {
                        if self.fits(&r, &passed).is_err() {
                            let (want, shown) = (self.show(&passed), self.show(&r));
                            let what = if *kind == TryKind::Result { "the error" } else { "`None`" };
                            self.err(format!("`{op}` returns {what} early, so the result type must be `{want}`, found `{shown}` (handle it here with `except`)"));
                        }
                    }
                    None => self.err(format!("`{op}` outside a function: handle the {ty_name} with `except` or `match`")),
                }
                inner
            }
            Expr::Except(x, name, fb) => {
                let t = self.infer(x);
                let (inner, err_ty) = match self.resolve(&t) {
                    Ty::Con(n, args) if n == "Result" && args.len() == 2 => (args[0].clone(), Some(args[1].clone())),
                    Ty::Con(n, args) if n == "Option" && args.len() == 1 => {
                        if let Some(n) = name {
                            self.err(format!("`except {n}`: an Option has no error to name (`None` carries no value), write `except {{ ... }}`"));
                        }
                        (args[0].clone(), None)
                    }
                    other => {
                        let shown = show_ty(&other);
                        self.err(format!("`except` handles a Result or an Option, found `{shown}`"));
                        (self.fresh(), None)
                    }
                };
                self.scopes.push(HashMap::new());
                if let (Some(n), Some(et)) = (name, err_ty) {
                    self.bind(n, et);
                }
                let ft = self.infer_with(fb, Some(&inner));
                self.expect(&inner, &ft, "`except` fallback");
                self.scopes.pop();
                inner
            }
            // `mut q.claim(2)`: the call changes `q` (`check_mut_args` says whether it may)
            Expr::Mut(x) if matches!(**x, Expr::Method(..)) => {
                self.marked.insert(&**x as *const Expr as usize);
                self.infer_at(x, expected)
            }
            // `mut x` belongs only in an argument (`infer_arg` takes it off there)
            Expr::Mut(x) => {
                let shown = crate::printer::expr(x, 0);
                self.err(format!("`mut {shown}` marks an argument passed to a `mut` parameter (`f(mut {shown})`) or a call that changes its receiver (`mut {shown}.m()`)"));
                self.infer(x)
            }
            Expr::Mock(t) => {
                let ty = self.lower(t, &HashMap::new());
                match self.resolve(&ty) {
                    Ty::Con(n, _) if self.env.traits.contains_key(&n) => ty,
                    other => {
                        self.err(format!("`mock` needs a trait, found `{}`", show_ty(&other)));
                        self.fresh()
                    }
                }
            }
        }
    }

    /// A place (`x`, `x.f.g`) that is about to change must be rooted at a
    /// mutable local, declared outside no closure.
    fn check_place(&mut self, place: &Expr, what: &str) {
        let mut root = place;
        while let Expr::Field(r, _) = root {
            root = r;
        }
        let Expr::Var(x) = root else {
            self.err(format!("can only {what} a variable or one of its fields"));
            return;
        };
        // a narrowed binding (inside a match arm) is not where the variable lives
        let narrowed = format!("narrowed:{x}");
        let Some(idx) = self.scopes.iter().rposition(|s| s.contains_key(x) && !s.contains_key(&narrowed)) else {
            self.err(format!("can only {what} a local variable, not `{x}`"));
            return;
        };
        if !self.scopes[idx].contains_key(&format!("mut:{x}")) {
            self.err(format!("cannot {what} `{x}`: it is not mutable (declare it with `let mut {x}`, or make it a `mut` parameter)"));
        } else if idx < self.lambda_floor {
            self.err(format!("a closure cannot change `{x}`, which belongs to the enclosing function"));
        }
    }

    /// Arguments passed to `mut` parameters are changed by the call, and say
    /// so: `f(mut x)`. `recv` is set for a method call: whether it is marked
    /// `mut` (`mut q.claim(2)`), which it must be when it changes its
    /// receiver, and only then.
    fn check_mut_args(&mut self, callee: &str, args: &[&Expr], recv: Option<bool>) {
        let params: Option<Vec<Param>> = self
            .env
            .fns
            .get(callee)
            .map(|f| f.params.clone())
            .or_else(|| self.env.method_params(callee));
        let Some(params) = params else {
            // a builtin or a closure: no parameter is `mut`
            for a in args.iter().filter(|a| marks_arg(a)) {
                self.err(format!("`{}`: `{callee}` has no `mut` parameters", crate::printer::expr(a, 0)));
            }
            if recv == Some(true) {
                self.err(format!("`mut` marks a call that changes its receiver, but `{callee}` doesn't change it"));
            }
            return;
        };
        for (i, (p, a)) in params.iter().zip(args).enumerate() {
            let receiver = i == 0 && recv.is_some();
            let marked = if receiver { recv == Some(true) } else { marks_arg(a) };
            let place = match a {
                Expr::Mut(x) if marks_arg(a) => &**x,
                x => x,
            };
            let shown = crate::printer::expr(place, 0);
            if p.mutable {
                self.check_place(place, &format!("let `{callee}` change (its `mut {}` parameter)", p.name));
                if !marked && receiver {
                    let m = crate::names::short(callee);
                    self.err(format!("`{m}` changes its receiver (`mut self`): write `mut {shown}.{m}(…)` to show that `{shown}` may change"));
                } else if !marked {
                    self.err(format!("`{callee}` can change its `mut {}` parameter: write `mut {shown}` to show that `{shown}` may change", p.name));
                }
            } else if marked && receiver {
                let m = crate::names::short(callee);
                self.err(format!("`mut {shown}.{m}(…)`: `{m}` doesn't change its receiver (it doesn't take `mut self`)"));
            } else if marked {
                self.err(format!("`mut` on an argument of `{callee}`, but its parameter `{}` is not `mut`", p.name));
            }
        }
    }

    /// Arithmetic and comparison: both sides the same numeric type, no implicit
    /// Int/Float mixing. Unknown operands default to Int.
    fn numeric(&mut self, lt: &Ty, rt: &Ty, op: &BinOp) {
        self.expect(lt, rt, &format!("`{}` operands", op.symbol()));
        match self.resolve(lt) {
            Ty::Int | Ty::Float => {}
            Ty::Var(_) => self.expect(&Ty::Int, lt, &format!("`{}` operands", op.symbol())),
            other => self.err(format!("`{}` needs Int or Float operands, found `{}` (convert with `float(n)` / `round(x)`)", op.symbol(), show_ty(&other))),
        }
    }

    /// Builtins with several signatures (`len`, `contains`, `get`, ...): pick the
    /// first whose parameters unify with the argument types.
    fn overloaded(&mut self, name: &str, args: &[&Expr]) -> Option<Ty> {
        if self.lookup(name).is_some() || self.env.fns.contains_key(name) {
            return None;
        }
        let cands: Vec<_> = BUILTINS.iter().filter(|b| b.name == name).collect();
        if cands.len() < 2 {
            return None;
        }
        let arg_tys: Vec<Ty> = args.iter().map(|a| self.infer_arg(a, None)).collect();
        for Builtin { params: ps, sig, .. } in cands.iter().copied() {
            let snapshot = self.subst.clone();
            let ps: Vec<String> = ps.split(',').filter(|s| !s.is_empty()).map(String::from).collect();
            if let Ty::Fn(params, ret) = self.instantiate(&ps, &parse_type_str(sig)) {
                if params.len() == arg_tys.len() && params.iter().zip(&arg_tys).all(|(p, a)| self.unify(p, a).is_ok()) {
                    return Some(*ret);
                }
            }
            self.subst = snapshot;
        }
        let shown: Vec<String> = arg_tys.iter().map(|t| self.show(t)).collect();
        let sigs: Vec<String> = cands.iter().map(|b| format!("  {name}{}", b.sig)).collect();
        self.err(format!("no version of `{name}` accepts ({}); available:\n{}", shown.join(", "), sigs.join("\n")));
        Some(self.fresh())
    }

    fn field_type(&mut self, rt: &Ty, f: &str) -> Ty {
        let mut rt = self.resolve(rt);
        if let Ty::Var(_) = rt {
            // Unknown receiver: if exactly one record type has this field, use it.
            let cands: Vec<String> = self
                .env
                .types
                .values()
                .filter(|t| matches!(&t.body, TypeBody::Record(fs) if fs.iter().any(|x| x.name == f)))
                .map(|t| t.name.clone())
                .collect();
            if cands.len() == 1 {
                let (ct, _) = self.ctor_type(&cands[0]).unwrap();
                self.expect(&ct, &rt, "field access");
                rt = self.resolve(&ct);
            }
        }
        if let (Ty::Con(n, items), Ok(i)) = (&rt, f.parse::<usize>()) {
            if n == "Tuple" {
                return match items.get(i) {
                    Some(t) => t.clone(),
                    None => {
                        self.err(format!("tuple `{}` has no element {i}", show_ty(&rt)));
                        self.fresh()
                    }
                };
            }
        }
        if let Ty::Con(n, _) = &rt {
            if f == "value" && self.env.types.get(n).is_some_and(|t| t.distinct) {
                let (ct, fields) = self.ctor_type(&n.clone()).unwrap();
                self.expect(&ct, &rt, "field access");
                return fields[0].1.clone();
            }
        }
        // a field every member of a union has (with the same type)
        if let Ty::Union(ms) = &rt {
            let mut out: Option<Ty> = None;
            for m in ms.clone() {
                let has = match &m {
                    Ty::Con(n, _) => self.ctor_type(n).is_some_and(|(_, fs)| fs.iter().any(|(x, _)| x.as_deref() == Some(f))),
                    _ => false,
                };
                if !has {
                    self.err(format!("`{}` has no field `{f}`, so not every `{}` does (match on it first)", show_ty(&m), show_ty(&rt)));
                    return self.fresh();
                }
                let ft = self.field_type(&m, f);
                out = Some(match out {
                    None => ft,
                    Some(o) => self.join(&o, &ft, &format!("field `{f}` of `{}`", show_ty(&rt))),
                });
            }
            return out.unwrap_or_else(|| self.fresh());
        }
        // a variant with named fields, as its own type (`e: TooBig` has `e.max`)
        if let Ty::Con(v, args) = &rt {
            if let Some(p) = self.env.variant_parent(v) {
                let (ct, fields) = self.ctor_type(v).unwrap();
                let _ = self.unify(&ct, &Ty::Con(p, args.clone()));
                if let Some((_, t)) = fields.iter().find(|(x, _)| x.as_deref() == Some(f)) {
                    return t.clone();
                }
                self.err(format!("variant `{v}` has no field `{f}`"));
                return self.fresh();
            }
        }
        match &rt {
            Ty::Con(n, _) => {
                let n = n.clone();
                if let Some(TypeDecl { body: TypeBody::Record(_), .. }) = self.env.types.get(&n) {
                    let (ct, fields) = self.ctor_type(&n).unwrap();
                    self.expect(&ct, &rt, "field access");
                    if let Some((_, t)) = fields.iter().find(|(x, _)| x.as_deref() == Some(f)) {
                        return t.clone();
                    }
                }
                self.err(format!("type `{}` has no field `{f}` (match on variants to access their fields)", show_ty(&rt)));
                self.fresh()
            }
            _ => {
                self.err(format!("cannot access field `{f}` on `{}`", show_ty(&rt)));
                self.fresh()
            }
        }
    }

    /// Check a pattern against the scrutinee's type `t`. Returns the type of the
    /// values it matches (what a matched variable narrows to) and its value set.
    fn pattern(&mut self, p: &Pattern, t: &Ty) -> (Ty, P) {
        match p {
            Pattern::Wild => (t.clone(), P::Any),
            Pattern::Bind(x) => {
                self.bind(x, t.clone());
                (t.clone(), P::Any)
            }
            Pattern::Int(n) => {
                self.expect(t, &Ty::Int, "pattern");
                (Ty::Int, P::Ints(ints::point(*n as i128)))
            }
            Pattern::Str(_) => {
                self.expect(t, &Ty::Str, "pattern");
                (Ty::Str, P::Lit)
            }
            Pattern::Bool(b) => {
                self.expect(t, &Ty::Bool, "pattern");
                (Ty::Bool, P::C(b.to_string(), vec![]))
            }
            Pattern::Or(ps) => {
                // each alternative in its own scope; they must bind the same names
                let mut results = Vec::new();
                let mut binds: Vec<HashMap<String, Ty>> = Vec::new();
                for alt in ps {
                    self.scopes.push(HashMap::new());
                    results.push(self.pattern(alt, t));
                    binds.push(self.scopes.pop().unwrap());
                }
                let mut names: Vec<&String> = binds[0].keys().collect();
                names.sort();
                for (alt, b) in ps.iter().zip(&binds).skip(1) {
                    let mut other: Vec<&String> = b.keys().collect();
                    other.sort();
                    if other != names {
                        self.err(format!("the alternatives of `{}` must bind the same names (`{}` binds {:?})", crate::printer::pat(p), crate::printer::pat(alt), other));
                    }
                }
                for n in names {
                    let mut t0 = binds[0][n].clone();
                    for b in &binds[1..] {
                        if let Some(t1) = b.get(n) {
                            t0 = self.join_union(&t0, t1, &format!("`{n}` in `{}`", crate::printer::pat(p)));
                        }
                    }
                    self.bind(n, t0);
                }
                let narrowed = self.union_of(results.iter().map(|r| self.resolve(&r.0)).collect());
                (narrowed, P::Or(results.into_iter().map(|r| r.1).collect()))
            }
            Pattern::Cond(c) => {
                self.scopes.push(HashMap::new());
                self.bind("it", t.clone());
                let ct = self.infer(c);
                self.expect(&Ty::Bool, &ct, &format!("pattern `where {}`", crate::printer::expr(c, 0)));
                self.scopes.pop();
                (t.clone(), self.cond_set(t, c))
            }
            Pattern::Tuple(ps) => {
                let items: Vec<Ty> = ps.iter().map(|_| self.fresh()).collect();
                let tt = Ty::Con("Tuple".into(), items.clone());
                self.expect(t, &tt, "tuple pattern");
                let subs = ps.iter().zip(&items).map(|(p, it)| self.pattern(p, it).1).collect();
                (tt, P::C("(,)".into(), subs))
            }
            Pattern::List(items, rest) => {
                let el = self.fresh();
                let lt = Ty::Con("List".into(), vec![el.clone()]);
                self.expect(t, &lt, "list pattern");
                let subs: Vec<P> = items.iter().map(|i| self.pattern(i, &el).1).collect();
                if let Some(r) = rest {
                    if r != "_" {
                        self.bind(r, lt.clone());
                    }
                }
                let mut acc = if rest.is_some() { P::Any } else { P::C("[]".into(), vec![]) };
                for sp in subs.into_iter().rev() {
                    acc = P::C("::".into(), vec![sp, acc]);
                }
                (lt, acc)
            }
            // a type's name matches the values of that type: `Positive`, `String`, `Shape`
            Pattern::Ctor(c, CtorPat::None) if !self.env.ctors.contains_key(c) && self.env.is_type_name(c) => {
                let pt = self.type_named(c);
                if self.is_trait_ty(&pt) {
                    return (pt.clone(), P::Ty(pt));
                }
                let n_err = self.errors.len();
                self.expect(t, &pt, &format!("pattern `{c}`"));
                if self.errors.len() > n_err {
                    // already reported; don't also call it unreachable
                    return (pt, P::Lit);
                }
                let set = match self.int_refinement(&TypeExpr::Named(c.clone(), vec![])) {
                    Refn::Plain => P::Ty(pt.clone()),
                    Refn::Ints(s) => P::Ints(s),
                    Refn::Lens(s) => P::Lens(s),
                    Refn::Reals(r) => P::Reals(r),
                    Refn::Opaque => P::Lit,
                };
                (pt, set)
            }
            Pattern::Ctor(c, cp) => {
                let Some((ct, fields)) = self.ctor_type(c) else {
                    self.err(format!("unknown constructor `{c}` in pattern"));
                    return (t.clone(), P::Any);
                };
                let own = self.variant_ty(c, &ct);
                let n_err = self.errors.len();
                self.expect(t, &own, &format!("pattern `{c}`"));
                let bad = self.errors.len() > n_err;
                let field_names: Vec<Option<String>> = fields.iter().map(|f| f.0.clone()).collect();
                let subs: Vec<P> = match cp {
                    // `Circle` alone: every value of that variant, whatever its fields
                    CtorPat::None => vec![P::Any; fields.len()],
                    CtorPat::Positional(ps) => {
                        if ps.len() != fields.len() {
                            self.err(format!("pattern `{c}` expects {} fields, got {}", fields.len(), ps.len()));
                        }
                        ps.iter().zip(fields.iter()).map(|(sp, (_, ft))| self.pattern(sp, ft).1).collect()
                    }
                    CtorPat::Record(fs, open) => {
                        let mut subs = vec![P::Any; fields.len()];
                        for (fname, sp) in fs {
                            match field_names.iter().position(|n| n.as_deref() == Some(fname)) {
                                Some(i) => {
                                    let ft = fields[i].1.clone();
                                    subs[i] = self.pattern(sp, &ft).1;
                                }
                                None => self.err(format!("`{c}` has no field `{fname}`")),
                            }
                        }
                        if !open && fs.len() != fields.len() {
                            self.err(format!("pattern `{c} {{..}}` does not mention every field; add `..`"));
                        }
                        subs
                    }
                };
                (own, if bad { P::Lit } else { P::C(c.clone(), subs) })
            }
        }
    }

    /// The type of the constructor `c`'s values: its variant type for a variant
    /// of a sum, else `rt` (a record or distinct type is its own constructor).
    fn variant_ty(&self, c: &str, rt: &Ty) -> Ty {
        match (self.env.variant_parent(c), rt) {
            (Some(_), Ty::Con(_, args)) => Ty::Con(c.to_string(), args.clone()),
            _ => rt.clone(),
        }
    }

    /// The type a name denotes, with fresh type arguments if it takes any
    /// (`NonEmpty` in a pattern means `NonEmpty[_]`).
    fn type_named(&mut self, n: &str) -> Ty {
        let params: Vec<String> = match self.env.types.get(n) {
            Some(td) => td.params.clone(),
            None => match self.env.traits.get(n) {
                Some(_) => vec![],
                None => match self.env.variant_parent(n) {
                    Some(p) => self.env.types[&p].params.clone(),
                    None => match n {
                        "List" | "Set" => vec!["A".into()],
                        "Dict" => vec!["K".into(), "V".into()],
                        _ => vec![],
                    },
                },
            },
        };
        let args = params.iter().map(|p| TypeExpr::Named(p.clone(), vec![])).collect();
        self.instantiate(&params, &TypeExpr::Named(n.to_string(), args))
    }

    /// A format spec must suit the part's type: `.N` a Float or a String,
    /// `+` and `0` a number. (A part whose type is still unknown is checked
    /// when it runs.)
    fn check_spec(&mut self, f: &FmtSpec, t: &Ty) {
        let t = self.resolve(t);
        let base = match &t {
            Ty::Con(n, _) => self.env.types.get(n).and_then(|td| match &td.body {
                TypeBody::Alias(a) if !td.distinct => Some(texpr_head(a)),
                _ => None,
            }),
            _ => None,
        };
        let is = |want: &Ty, name: &str| t == *want || base.as_deref() == Some(name);
        let (int, float, string) = (is(&Ty::Int, "Int"), is(&Ty::Float, "Float"), is(&Ty::Str, "String"));
        if matches!(t, Ty::Var(_)) {
            return;
        }
        if f.precision.is_some() && !float && !string {
            self.err(format!("format spec `{f}`: `.N` applies to a Float (decimal places) or a String (most characters), not `{}`", show_ty(&t)));
        }
        if (f.plus || f.zero) && !int && !float {
            self.err(format!("format spec `{f}`: `+` and `0` apply to numbers, not `{}`", show_ty(&t)));
        }
    }

    fn is_trait_ty(&self, t: &Ty) -> bool {
        matches!(self.resolve(t), Ty::Con(n, _) if self.env.traits.contains_key(&n))
    }

    /// Assigning to a narrowed variable ends its narrowing (the new value only
    /// has to fit the declared type); assigning to it or a field drops any
    /// narrowed field paths below it. Returns the declared type of a variable.
    fn end_narrowing(&mut self, place: &Expr) -> Option<Ty> {
        let path = place_path(place)?;
        let root = path.split('.').next().unwrap_or("").to_string();
        for s in self.scopes.iter_mut() {
            s.retain(|k, _| !(k.starts_with("path:") && (k[5..] == path || k[5..].starts_with(&format!("{path}.")) || k[5..].starts_with(&format!("{root}.")))));
        }
        if path.contains('.') {
            return None;
        }
        let key = format!("narrowed:{path}");
        let orig = self.lookup(&key)?;
        for s in self.scopes.iter_mut() {
            if s.remove(&key).is_some() {
                s.insert(path.clone(), orig.clone());
            }
        }
        Some(orig)
    }

    /// What a written type's refinements say, as far as patterns are concerned.
    fn int_refinement(&self, t: &TypeExpr) -> Refn {
        match t {
            TypeExpr::Refined(b, pred) => match (self.int_refinement(b), self.base_of(b)) {
                (Refn::Plain, Base::Int) => ints::of_pred(pred).map_or(Refn::Opaque, Refn::Ints),
                (Refn::Ints(a), _) => ints::of_pred(pred).map_or(Refn::Opaque, |s| Refn::Ints(ints::and(&a, &s))),
                (Refn::Plain, Base::Sized) => ints::of_len_pred(pred).map_or(Refn::Opaque, Refn::Lens),
                (Refn::Lens(a), _) => ints::of_len_pred(pred).map_or(Refn::Opaque, |s| Refn::Lens(ints::and(&a, &s))),
                (Refn::Plain, Base::Float) => reals::of_pred(pred).map_or(Refn::Opaque, Refn::Reals),
                (Refn::Reals(a), _) => reals::of_pred(pred).and_then(|s| a.and(s)).map_or(Refn::Opaque, Refn::Reals),
                _ => Refn::Opaque,
            },
            TypeExpr::Named(n, args) => match self.env.types.get(n) {
                Some(td) if !td.distinct => {
                    let base = match &td.body {
                        TypeBody::Alias(a) => {
                            let sub: HashMap<String, TypeExpr> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
                            Some(subst_texpr(a, &sub))
                        }
                        _ => None,
                    };
                    match (base, &td.refinement) {
                        (Some(b), Some(r)) => self.int_refinement(&TypeExpr::Refined(Box::new(b), Box::new(r.clone()))),
                        (Some(b), None) => self.int_refinement(&b),
                        (None, Some(_)) => Refn::Opaque,
                        (None, None) => Refn::Plain,
                    }
                }
                _ => Refn::Plain,
            },
            TypeExpr::Union(ms) => {
                if ms.iter().all(|m| matches!(self.int_refinement(m), Refn::Plain)) { Refn::Plain } else { Refn::Opaque }
            }
            _ => Refn::Plain,
        }
    }

    fn base_of(&self, t: &TypeExpr) -> Base {
        match self.env.expand_alias(t) {
            TypeExpr::Named(n, _) => match n.as_str() {
                "Int" => Base::Int,
                "Float" => Base::Float,
                "String" | "List" => Base::Sized,
                _ => Base::Other,
            },
            _ => Base::Other,
        }
    }

    /// The value set a condition on a value of type `t` stands for.
    fn cond_set(&self, t: &Ty, pred: &Expr) -> P {
        let set = match self.resolve(t) {
            Ty::Int => ints::of_pred(pred).map(P::Ints),
            Ty::Float => reals::of_pred(pred).map(P::Reals),
            Ty::Str => ints::of_len_pred(pred).map(P::Lens),
            Ty::Con(n, _) if n == "List" => ints::of_len_pred(pred).map(P::Lens),
            _ => None,
        };
        set.unwrap_or(P::Lit)
    }

    /// The values a written type leaves out, as a set (for `match` coverage):
    /// `Positive` leaves out `it <= 0`, `NonEmpty` leaves out length 0.
    fn excluded_by(&self, t: &TypeExpr) -> Option<P> {
        match self.int_refinement(t) {
            Refn::Ints(s) => Some(P::Ints(ints::not(&s))),
            Refn::Lens(s) => Some(P::Lens(ints::and(&ints::not(&s), &vec![(0, ints::MAX)]))),
            Refn::Reals(r) => Some(P::Reals(r.not())),
            Refn::Plain | Refn::Opaque => None,
        }
    }

    /// Remember the written type of a variable. A declared type holds for the
    /// variable's whole life (every assignment is checked against it), so a
    /// `match` can rely on its refinement.
    fn bind_written(&mut self, x: &str, t: &TypeExpr) {
        self.written.push(t.clone());
        let i = self.written.len() - 1;
        self.bind(&format!("written:{x}"), Ty::Rigid(format!("#{i}")));
    }

    fn written_of(&self, x: &str) -> Option<TypeExpr> {
        match self.lookup(&format!("written:{x}"))? {
            Ty::Rigid(i) => self.written.get(i[1..].parse::<usize>().ok()?).cloned(),
            _ => None,
        }
    }

    /// The written type of what a `match` looks at: an immutable variable, a
    /// field of one, or the result of a function.
    fn written_scrutinee(&mut self, s: &Expr) -> Option<TypeExpr> {
        match s {
            Expr::Var(x) => self.written_of(x),
            Expr::Field(r, f) => {
                let Expr::Var(x) = &**r else { return None };
                let Ty::Con(n, args) = self.resolve(&self.lookup(x)?) else { return None };
                let td = self.env.types.get(&n)?;
                let TypeBody::Record(fs) = &td.body else { return None };
                let sub: HashMap<String, TypeExpr> = td.params.iter().cloned().zip(args.iter().map(ty_to_texpr)).collect();
                fs.iter().find(|x| x.name == *f).map(|x| subst_texpr(&x.ty, &sub))
            }
            Expr::Call(f, _) => match &**f {
                Expr::Var(n) if self.lookup(n).is_none() => self.env.fns.get(n).map(|f| f.ret.clone()),
                _ => None,
            },
            Expr::Method(_, m, _) if self.lookup(m).is_none() => self.env.fns.get(m).map(|f| f.ret.clone()),
            _ => None,
        }
    }
}

/// One `match` to check: the matched type, each arm's value set and text,
/// and the values the matched value's own refinement rules out (with that
/// refinement, for messages).
struct Coverage {
    ty: Ty,
    arms: Vec<(P, String)>,
    excluded: Option<(P, String)>,
}

/// A refinement as far as patterns can tell: none, a set of integers, or a
/// condition we cannot decide yet (it covers nothing for exhaustiveness).
enum Refn {
    Plain,
    Ints(ints::Ints),
    Lens(ints::Ints),
    Reals(reals::Reals),
    Opaque,
}

/// What kind of values a refinement's `it` ranges over, for patterns.
#[derive(PartialEq)]
enum Base {
    Int,
    Float,
    Sized,
    Other,
}

/// Types that `+` joins rather than adds.
fn joinable(t: &Ty) -> bool {
    match t {
        Ty::Str => true,
        Ty::Con(n, _) => n == "List",
        _ => false,
    }
}

pub fn irrefutable(p: &Pattern) -> bool {
    matches!(p, Pattern::Wild | Pattern::Bind(_))
}

pub fn show_ty(t: &Ty) -> String {
    match t {
        Ty::Int => "Int".into(),
        Ty::Float => "Float".into(),
        Ty::Con(n, a) if n == "Tuple" => format!("({})", a.iter().map(show_ty).collect::<Vec<_>>().join(", ")),
        Ty::Bool => "Bool".into(),
        Ty::Str => "String".into(),
        Ty::Unit => "()".into(),
        Ty::Var(v) => format!("?T{v}"),
        Ty::Rigid(n) => n.clone(),
        Ty::Con(n, a) if a.is_empty() => n.clone(),
        Ty::Con(n, a) => format!("{n}[{}]", a.iter().map(show_ty).collect::<Vec<_>>().join(", ")),
        Ty::Fn(ps, r) => format!("({}) -> {}", ps.iter().map(show_ty).collect::<Vec<_>>().join(", "), show_ty(r)),
        Ty::Union(ms) => ms.iter().map(|m| if matches!(m, Ty::Fn(..)) { format!("({})", show_ty(m)) } else { show_ty(m) }).collect::<Vec<_>>().join(" | "),
    }
}

// ------------------------------------------------------------------

fn ty_to_texpr(t: &Ty) -> TypeExpr {
    let n = |s: &str| TypeExpr::Named(s.into(), vec![]);
    match t {
        Ty::Int | Ty::Var(_) | Ty::Rigid(_) => n("Int"),
        Ty::Float => n("Float"),
        Ty::Bool => n("Bool"),
        Ty::Str => n("String"),
        Ty::Unit => TypeExpr::Unit,
        Ty::Con(c, args) => TypeExpr::Named(c.clone(), args.iter().map(ty_to_texpr).collect()),
        Ty::Fn(ps, r) => TypeExpr::Fn(ps.iter().map(ty_to_texpr).collect(), Box::new(ty_to_texpr(r))),
        Ty::Union(ms) => TypeExpr::Union(ms.iter().map(ty_to_texpr).collect()),
    }
}

/// Concrete types to test a generic function with: every bounded type
/// parameter gets an implementation of its bounds (or a mock of the trait),
/// chosen so that all the bounds agree (`Q: Queue[P, E]` and `W: Worker[P, E]`
/// must share `P` and `E`). Unbounded parameters become `Int`.
pub fn test_instance(env: &Env, f: &FnDecl) -> Option<HashMap<String, TypeExpr>> {
    let mut c = Checker {
        env,
        subst: Vec::new(),
        scopes: vec![HashMap::new()],
        rigid: BTreeSet::new(),
        decl: f.name.clone(),
        errors: Vec::new(),
        holes: Vec::new(),
        matches: Vec::new(),
        self_ty: None,
        bounds: HashMap::new(),
        pending: Vec::new(),
        lambda_floor: 0,
        loops: 0,
        ret: None,
        written: Vec::new(),
        targets: Vec::new(),
        last_inst: None,
        insts: Vec::new(),
        callee: false,
        marked: HashSet::new(),
        types: Vec::new(),
    };
    let vars: HashMap<String, Ty> = f.type_params.iter().map(|p| (p.clone(), c.fresh())).collect();
    let bounded: Vec<&Bound> = f.bounds.iter().collect();
    // candidate types per bounded parameter: implementations first, then a mock
    let mut cands: Vec<Vec<TypeExpr>> = Vec::new();
    for b in &bounded {
        let first = b.traits.first().map(texpr_head).unwrap_or_default();
        // never the real terminal, disk or process: fuzzing must not reach the system
        let mut cs: Vec<TypeExpr> = env.impls.iter().filter(|i| i.trait_name == first && !crate::stdlib::HOST_TYPES.contains(&texpr_head(&i.for_type).as_str())).map(|i| i.for_type.clone()).collect();
        cs.push(b.traits[0].clone());
        cands.push(cs);
    }
    let total: usize = cands.iter().map(|c| c.len()).product();
    for k in 0..total.min(256) {
        let snapshot = c.subst.clone();
        let mut idx = k;
        let mut ok = true;
        for (bi, b) in bounded.iter().enumerate() {
            let choice = &cands[bi][idx % cands[bi].len()];
            idx /= cands[bi].len();
            // an impl's own type parameters are left open, to be fixed by the other bounds
            let mut open: HashMap<String, Ty> = vars.clone();
            if let Some(imp) = env.impls.iter().find(|i| &i.for_type == choice) {
                for p in &imp.type_params {
                    open.insert(p.clone(), c.fresh());
                }
            }
            let t = c.lower(choice, &open);
            if c.unify(&vars[&b.param], &t).is_err() {
                ok = false;
                break;
            }
            for tr in &b.traits {
                if let TypeExpr::Named(tn, targs) = tr {
                    let args: Vec<Ty> = targs.iter().map(|a| c.lower(a, &vars)).collect();
                    if c.satisfies(&vars[&b.param], tn, &args, 0).is_err() {
                        ok = false;
                    }
                }
            }
            if !ok {
                break;
            }
        }
        c.pending.clear();
        if ok {
            return Some(f.type_params.iter().map(|p| (p.clone(), ty_to_texpr(&c.resolve(&vars[p])))).collect());
        }
        c.subst = snapshot;
    }
    None
}

pub fn check(prog: &Program) -> CheckReport {
    let resolved = crate::resolve::resolve(prog);
    let prog = &resolved.program;
    let (env, mut errors) = Env::build(prog);
    errors.splice(0..0, resolved.errors);
    let (errs, holes, _, _) = check_decls(prog, &env);
    errors.extend(errs);
    CheckReport { errors, holes }
}

/// Check every declaration of a resolved program: errors, holes, and the
/// method-call decisions `elaborate` writes into the program.
fn check_decls(prog: &Program, env: &Env) -> (Vec<Diag>, Vec<HoleInfo>, Vec<(usize, Target)>, HashMap<usize, Ty>) {
    let (mut errors, mut holes, mut targets, mut types) = (Vec::new(), Vec::new(), Vec::new(), HashMap::new());
    for d in &prog.decls {
        let mut c = Checker {
            env: &env,
            subst: Vec::new(),
            scopes: vec![HashMap::new()],
                rigid: BTreeSet::new(),
            decl: d.name(),
            errors: Vec::new(),
            holes: Vec::new(),
            matches: Vec::new(),
            self_ty: None,
            bounds: HashMap::new(),
            pending: Vec::new(),
            lambda_floor: 0,
            loops: 0,
            ret: None,
            written: Vec::new(),
            targets: Vec::new(),
        last_inst: None,
        insts: Vec::new(),
        callee: false,
        marked: HashSet::new(),
        types: Vec::new(),
        };
        let mut obligations = Vec::new();
        match &d.kind {
            DeclKind::Type(t) => {
                if matches!(t.body, TypeBody::Alias(ref a) if !matches!(a, TypeExpr::Union(_))) && t.refinement.is_none() && !t.distinct {
                    c.err(format!(
                        "`type {0} = {1}` adds nothing: constrain it (`type {0} = {1} where …`) or make it a separate type (`distinct type {0} = {1}`)",
                        t.name,
                        match &t.body { TypeBody::Alias(a) => crate::printer::ty(a), _ => String::new() }
                    ));
                }
                c.rigid = t.params.iter().cloned().collect();
                let self_ty = match &t.body {
                    TypeBody::Alias(a) => c.lower(a, &HashMap::new()),
                    TypeBody::Record(fs) => {
                        for f in fs {
                            c.lower(&f.ty, &HashMap::new());
                            c.check_refined(&f.ty);
                        }
                        c.check_defaults(fs);
                        Ty::Con(t.name.clone(), t.params.iter().map(|p| Ty::Rigid(p.clone())).collect())
                    }
                    TypeBody::Sum(vs) => {
                        for v in vs {
                            match &v.payload {
                                Payload::None => {}
                                Payload::Positional(ts) => ts.iter().for_each(|t| {
                                    c.lower(t, &HashMap::new());
                                }),
                                Payload::Record(fs) => {
                                    fs.iter().for_each(|f| {
                                        c.lower(&f.ty, &HashMap::new());
                                    });
                                    c.check_defaults(fs);
                                }
                            }
                        }
                        Ty::Con(t.name.clone(), t.params.iter().map(|p| Ty::Rigid(p.clone())).collect())
                    }
                };
                if let Some(r) = &t.refinement {
                    c.bind("it", self_ty);
                    let rt = c.infer(r);
                    c.expect(&Ty::Bool, &rt, "refinement");
                }
            }
            DeclKind::Fn(f) => {
                c.check_fn(f, &mut obligations);
            }
            DeclKind::Trait(t) => {
                c.rigid = t.params.iter().cloned().collect();
                c.rigid.insert("Self".into());
                c.self_ty = Some(Ty::Rigid("Self".into()));
                let own: Vec<Ty> = t.params.iter().map(|p| Ty::Rigid(p.clone())).collect();
                c.bounds.insert("Self".into(), vec![(t.name.clone(), own)]);
                for s in &t.supers {
                    match s {
                        TypeExpr::Named(sn, _) if env.traits.contains_key(sn) => {}
                        other => c.err(format!("supertrait `{}` is not a trait", crate::printer::ty(other))),
                    }
                }
                for m in &t.methods {
                    // without `self`, the type it is called on must matter: else it is just a function
                    if !crate::resolve::takes_self(&m.decl) && !mentions_self(&m.decl) {
                        let short = crate::names::short(&m.decl.name);
                        c.err(format!("trait method `{short}` has no `self` and doesn't mention `Self`, so no implementation could be chosen for a call: make it a function"));
                    }
                    let mut ob = Vec::new();
                    let saved = c.rigid.clone();
                    if m.default {
                        c.check_fn(&m.decl, &mut ob);
                    } else {
                        c.rigid.extend(m.decl.type_params.iter().cloned());
                        for p in &m.decl.params {
                            c.check_refined(&p.ty);
                            c.lower(&p.ty, &HashMap::new());
                        }
                        c.lower(&m.decl.ret, &HashMap::new());
                        c.scopes.push(HashMap::new());
                        for p in &m.decl.params {
                            let t = c.lower(&p.ty, &HashMap::new());
                            c.bind(&p.name, t);
                        }
                        for r in &m.decl.requires {
                            let t = c.infer(r);
                            c.expect(&Ty::Bool, &t, "requires clause");
                        }
                        let rt = c.lower(&m.decl.ret, &HashMap::new());
                        c.bind("it", rt);
                        for en in &m.decl.ensures {
                            let t = c.infer(en);
                            c.expect(&Ty::Bool, &t, "ensures clause");
                        }
                        c.scopes.pop();
                    }
                    c.rigid = saved;
                }
                for l in &t.laws {
                    c.scopes.push(HashMap::new());
                    for p in &l.params {
                        let t = c.lower(&p.ty, &HashMap::new());
                        c.bind(&p.name, t);
                    }
                    let lt = c.infer(&l.body);
                    c.expect(&Ty::Bool, &lt, &format!("law `{}`", l.name));
                    c.scopes.pop();
                }
            }
            DeclKind::Impl(i) => {
                c.rigid = i.type_params.iter().cloned().collect();
                c.add_bounds(&i.bounds);
                let head = texpr_head(&i.for_type);
                let alias = env.types.get(&head).is_some_and(|t| matches!(t.body, TypeBody::Alias(_)) && !t.distinct);
                if alias && !i.is_inherent() {
                    c.err(format!("`{head}` is a refined alias of another type; implement the trait for the underlying type"));
                }
                let self_ty = c.lower(&i.for_type, &HashMap::new());
                c.self_ty = Some(self_ty.clone());
                match env.traits.get(&i.trait_name).cloned() {
                    None if i.is_inherent() => {
                        // a type's own methods: only for a type this program defines, in its module
                        let short = |q: &str| crate::names::short(q).to_string();
                        if !env.types.contains_key(&head) || alias {
                            c.err(format!("`impl {}`: methods can only be added to a record, sum or `distinct` type defined in this program (use a function, or a trait)", short(&head)));
                        }
                        let mut seen = std::collections::HashSet::new();
                        for im in &i.methods {
                            if !seen.insert(&im.name) {
                                c.err(format!("`{}` is defined twice in `impl {}`", short(&im.name), short(&head)));
                            }
                        }
                    }
                    None => c.err(format!("unknown trait `{}`", i.trait_name)),
                    Some(tr) => {
                        if tr.params.len() != i.trait_args.len() {
                            c.err(format!("trait `{}` takes {} type arguments", tr.name, tr.params.len()));
                        }
                        let targs: Vec<Ty> = i.trait_args.iter().map(|a| c.lower(a, &HashMap::new())).collect();
                        let vars: HashMap<String, Ty> = tr.params.iter().cloned().zip(targs.iter().cloned()).collect();
                        // supertraits must be implemented too
                        for (sn, sa) in c.supers_of(&tr.name, &targs, 0) {
                            c.pending.push((self_ty.clone(), sn.clone(), sa, format!("`{}` requires `{sn}`", tr.name)));
                        }
                        let short = |q: &str| crate::names::short(q).to_string();
                        for tm in &tr.methods {
                            let _ = short;
                            // each trait's implementation is its own, even when the type has a method of the same name
                            match i.methods.iter().find(|m| m.name == tm.decl.name) {
                                None if !tm.default => c.err(format!("missing method `{}` required by `{}`", tm.decl.name, tr.name)),
                                None => {}
                                Some(im) => {
                                    // same shape as the trait's signature, with Self and the trait's parameters filled in
                                    let mut mvars = vars.clone();
                                    for p in &tm.decl.type_params {
                                        mvars.insert(p.clone(), Ty::Rigid(p.clone()));
                                    }
                                    c.rigid.extend(im.type_params.iter().cloned());
                                    let want = c.lower(&Checker::fn_scheme(&tm.decl), &mvars);
                                    let got = c.lower(&Checker::fn_scheme(im), &HashMap::new());
                                    if let Err(e) = c.unify(&want, &got) {
                                        c.err(format!("`{}` does not match the signature in trait `{}`: {e}", im.name, tr.name));
                                    }
                                    for (tp, ip) in tm.decl.params.iter().zip(&im.params) {
                                        if tp.mutable != ip.mutable {
                                            c.err(format!("`{}`: parameter `{}` must be {}as in trait `{}`", im.name, ip.name, if tp.mutable { "`mut` " } else { "not `mut` " }, tr.name));
                                        }
                                    }
                                }
                            }
                        }
                        for im in &i.methods {
                            if !tr.methods.iter().any(|m| m.decl.name == im.name) {
                                c.err(format!("`{}` is not a method of trait `{}`", im.name, tr.name));
                            }
                        }
                    }
                }
                for im in &i.methods {
                    let saved = c.rigid.clone();
                    let mut ob = Vec::new();
                    c.check_fn(im, &mut ob);
                    c.rigid = saved;
                }
            }
            DeclKind::Test(t) => {
                c.infer(&t.body);
            }
            DeclKind::Use(_) => {}
        }
        c.solve_pending();
        c.check_matches();
        let declared_goal = match &d.kind {
            DeclKind::Fn(f) if matches!(f.body, Expr::Hole(_)) => Some(crate::printer::ty(&f.ret)),
            _ => None,
        };
        for (name, t, ctx) in std::mem::take(&mut c.holes) {
            let inferred = c.show(&t);
            holes.push(HoleInfo {
                decl: d.name(),
                name,
                goal: match &declared_goal {
                    Some(g) if *g != inferred => format!("{g}  (= {inferred} + refinement)"),
                    _ => inferred,
                },
                context: ctx.iter().map(|(n, t)| (n.clone(), c.show(t))).collect(),
                obligations: obligations.clone(),
            });
        }
        c.finish_insts();
        for (at, t) in std::mem::take(&mut c.types) {
            types.insert(at, c.resolve(&t));
        }
        errors.extend(c.errors);
        targets.extend(c.targets);
    }
    (errors, holes, targets, types)
}

/// A type's own method as the function it is called as (`Item.new`, or
/// `Item.describe` once elaborated): `Self` is the type, and the impl's type
/// parameters are the function's.
pub fn static_fn(i: &ImplDecl, m: &FnDecl) -> FnDecl {
    let sub: HashMap<String, TypeExpr> = [("Self".to_string(), i.for_type.clone())].into();
    let mut f = m.clone();
    f.name = crate::resolve::static_name(&texpr_head(&i.for_type), &m.name);
    f.type_params = i.type_params.iter().chain(&m.type_params).cloned().collect();
    f.bounds = i.bounds.iter().chain(&m.bounds).cloned().collect();
    for p in &mut f.params {
        p.ty = subst_texpr(&p.ty, &sub);
    }
    f.ret = subst_texpr(&f.ret, &sub);
    f
}

/// Whether `Self` appears in a method's signature.
pub fn mentions_self(f: &FnDecl) -> bool {
    fn t(x: &TypeExpr) -> bool {
        match x {
            TypeExpr::Named(n, args) => n == "Self" || args.iter().any(t),
            TypeExpr::Fn(ps, r) => ps.iter().any(t) || t(r),
            TypeExpr::Refined(b, _) => t(b),
            TypeExpr::Union(ms) => ms.iter().any(t),
            TypeExpr::Unit => false,
        }
    }
    f.params.iter().any(|p| t(&p.ty)) || t(&f.ret)
}

/// The name a type is known by at runtime (`Money`, `Int`), when it is concrete.
pub fn ty_head(t: &Ty) -> Option<String> {
    Some(match t {
        Ty::Int => "Int".into(),
        Ty::Float => "Float".into(),
        Ty::Bool => "Bool".into(),
        Ty::Str => "String".into(),
        Ty::Unit => "Unit".into(),
        Ty::Con(h, _) => h.clone(),
        _ => return None,
    })
}
