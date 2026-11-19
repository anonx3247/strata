//! Static checking: Hindley–Milner style inference with explicit signatures
//! on top-level functions, effect checking, exhaustiveness checking, and
//! typed-hole reporting.

use crate::ast::*;
use crate::parser::Parser;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub const PRELUDE: &str = r#"
/// An optional value.
type Option[T] =
  | None
  | Some(T)

/// The result of an operation that can fail.
type Result[T, E] =
  | Ok(T)
  | Err(E)

/// What a stateful handler clause answers: the value for the operation,
/// and the handler's next state.
type Resume[R, S] = { value: R, state: S }

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
"#;

/// Builtin functions and their type schemes.
pub const BUILTINS: &[(&str, &str, &str)] = &[
    ("len", "A", "(List[A]) -> Int"),
    ("len", "", "(String) -> Int"),
    ("len", "K,V", "(Dict[K, V]) -> Int"),
    ("len", "A", "(Set[A]) -> Int"),
    ("contains", "A", "(Set[A], A) -> Bool"),
    ("contains", "K,V", "(Dict[K, V], K) -> Bool"),
    ("contains", "", "(String, String) -> Bool"),
    ("get", "K,V", "(Dict[K, V], K) -> Option[V]"),
    ("dict", "K,V", "(List[(K, V)]) -> Dict[K, V]"),
    ("insert", "K,V", "(Dict[K, V], K, V) -> Dict[K, V]"),
    ("insert", "A", "(Set[A], A) -> Set[A]"),
    ("remove", "K,V", "(Dict[K, V], K) -> Dict[K, V]"),
    ("remove", "A", "(Set[A], A) -> Set[A]"),
    ("keys", "K,V", "(Dict[K, V]) -> List[K]"),
    ("values", "K,V", "(Dict[K, V]) -> List[V]"),
    ("items", "K,V", "(Dict[K, V]) -> List[(K, V)]"),
    ("map_values", "K,V,W", "(Dict[K, V], (V) -> W) -> Dict[K, W]"),
    ("set", "A", "(List[A]) -> Set[A]"),
    ("to_list", "A", "(Set[A]) -> List[A]"),
    ("union", "A", "(Set[A], Set[A]) -> Set[A]"),
    ("intersection", "A", "(Set[A], Set[A]) -> Set[A]"),
    ("difference", "A", "(Set[A], Set[A]) -> Set[A]"),
    ("zip", "A,B", "(List[A], List[B]) -> List[(A, B)]"),
    ("enumerate", "A", "(List[A]) -> List[(Int, A)]"),
    ("sort", "A", "(List[A]) -> List[A]"),
    ("sort_by", "A,B", "(List[A], (A) -> B) -> List[A]"),
    ("reverse", "A", "(List[A]) -> List[A]"),
    ("join", "", "(List[String], String) -> String"),
    ("split", "", "(String, String) -> List[String]"),
    ("trim", "", "(String) -> String"),
    ("parse_int", "", "(String) -> Option[Int]"),
    ("parse_float", "", "(String) -> Option[Float]"),
    ("float", "", "(Int) -> Float"),
    ("floor", "", "(Float) -> Int"),
    ("ceil", "", "(Float) -> Int"),
    ("round", "", "(Float) -> Int"),
    ("sqrt", "", "(Float) -> Float"),
    ("map", "A,B", "(List[A], (A) -> B) -> List[B]"),
    ("filter", "A", "(List[A], (A) -> Bool) -> List[A]"),
    ("fold", "A,B", "(List[A], B, (B, A) -> B) -> B"),
    ("all", "A", "(List[A], (A) -> Bool) -> Bool"),
    ("any", "A", "(List[A], (A) -> Bool) -> Bool"),
    ("contains", "A", "(List[A], A) -> Bool"),
    ("push", "A", "(List[A], A) -> List[A]"),
    ("take", "A", "(List[A], Int) -> List[A]"),
    ("drop", "A", "(List[A], Int) -> List[A]"),
    ("find", "A", "(List[A], (A) -> Bool) -> Option[A]"),
    ("count", "A", "(List[A], (A) -> Bool) -> Int"),
    ("get", "A", "(List[A], Int) -> Option[A]"),
    ("sum", "", "(List[Int]) -> Int"),
    ("sum", "", "(List[Float]) -> Float"),
    ("range", "", "(Int, Int) -> List[Int]"),
    ("abs", "", "(Int) -> Int"),
    ("abs", "", "(Float) -> Float"),
    ("min", "", "(Int, Int) -> Int"),
    ("min", "", "(Float, Float) -> Float"),
    ("max", "", "(Int, Int) -> Int"),
    ("max", "", "(Float, Float) -> Float"),
    ("matches", "", "(String, String) -> Bool"),
    ("str_len", "", "(String) -> Int"),
    ("starts_with", "", "(String, String) -> Bool"),
    ("to_string", "A", "(A) -> String"),
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
    /// Effects available at the hole.
    pub effects: Vec<String>,
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

/// Everything known about the declarations of a program, shared by the
/// checker, interpreter and generators.
pub struct Env {
    pub types: HashMap<String, TypeDecl>,
    /// ctor name -> (type name)
    pub ctors: HashMap<String, String>,
    pub effects: HashMap<String, EffectDecl>,
    pub fns: HashMap<String, FnDecl>,
    pub traits: HashMap<String, TraitDecl>,
    pub impls: Vec<ImplDecl>,
    /// trait method name -> trait name (method names are unique across traits)
    pub method_trait: HashMap<String, String>,
}

/// The name used to find an implementation: `List[Int]` -> `List`, `Line` -> `Line`.
pub fn texpr_head(t: &TypeExpr) -> String {
    match t {
        TypeExpr::Named(n, _) => n.clone(),
        TypeExpr::Unit => "Unit".into(),
        TypeExpr::Fn(..) => "Fn".into(),
        TypeExpr::Refined(b, _) => texpr_head(b),
    }
}

impl Env {
    pub fn new(prog: &Program) -> (Env, Vec<Diag>) {
        let resolved = crate::resolve::resolve(prog);
        let prog = &resolved.program;
        let prelude = crate::parser::parse_program(PRELUDE).expect("prelude parses");
        let mut env = Env {
            types: HashMap::new(),
            ctors: HashMap::new(),
            effects: HashMap::new(),
            fns: HashMap::new(),
            traits: HashMap::new(),
            impls: Vec::new(),
            method_trait: HashMap::new(),
        };
        let mut errs = Vec::new();
        let prelude_names: Vec<String> = prelude.decls.iter().map(|d| d.name()).collect();
        for d in prelude.decls.iter().chain(prog.decls.iter()) {
            let name = d.name();
            let dup = match &d.kind {
                DeclKind::Type(t) => {
                    match &t.body {
                        TypeBody::Sum(vs) => {
                            for v in vs {
                                if env.ctors.insert(v.name.clone(), t.name.clone()).is_some() {
                                    errs.push(Diag { decl: name.clone(), msg: format!("constructor `{}` defined twice", v.name) });
                                }
                            }
                        }
                        TypeBody::Record(_) => {
                            env.ctors.insert(t.name.clone(), t.name.clone());
                        }
                        TypeBody::Alias(_) if t.distinct => {
                            env.ctors.insert(t.name.clone(), t.name.clone());
                        }
                        TypeBody::Alias(_) => {}
                    }
                    env.types.insert(t.name.clone(), t.clone()).is_some()
                }
                DeclKind::Effect(e) => env.effects.insert(e.name.clone(), e.clone()).is_some(),
                DeclKind::Fn(f) => env.fns.insert(f.name.clone(), f.clone()).is_some(),
                DeclKind::Test(_) | DeclKind::Use(_) => false,
                DeclKind::Trait(t) => {
                    for m in &t.methods {
                        if let Some(other) = env.method_trait.insert(m.decl.name.clone(), t.name.clone()) {
                            errs.push(Diag { decl: name.clone(), msg: format!("method `{}` is also defined by trait `{other}`; method names must be unique across traits for now", m.decl.name) });
                        }
                    }
                    env.traits.insert(t.name.clone(), t.clone()).is_some()
                }
                DeclKind::Impl(i) => {
                    let head = texpr_head(&i.for_type);
                    if env.impls.iter().any(|o| o.trait_name == i.trait_name && texpr_head(&o.for_type) == head) {
                        errs.push(Diag { decl: name.clone(), msg: format!("`{}` is already implemented for `{head}`", i.trait_name) });
                    }
                    env.impls.push(i.clone());
                    false
                }
            };
            if dup {
                let msg = if prelude_names.contains(&name) {
                    format!("`{name}` is already defined in the prelude (use it, or pick another name)")
                } else {
                    format!("`{name}` is defined twice")
                };
                errs.push(Diag { decl: name.clone(), msg });
            }
        }
        for (m, tr) in &env.method_trait {
            if env.fns.contains_key(m) {
                errs.push(Diag { decl: m.clone(), msg: format!("`{m}` is both a function and a method of trait `{tr}`") });
            }
        }
        (env, errs)
    }

    /// The implementation of `method` for values whose type head is `head`:
    /// the impl's own method, or the trait's default.
    pub fn resolve_method(&self, method: &str, head: &str) -> Option<(FnDecl, Vec<Expr>, Vec<Expr>)> {
        let tr = self.traits.get(self.method_trait.get(method)?)?;
        let tm = tr.methods.iter().find(|m| m.decl.name == method)?;
        let imp = self.impls.iter().find(|i| i.trait_name == tr.name && texpr_head(&i.for_type) == head)?;
        let decl = match imp.methods.iter().find(|m| m.name == method) {
            Some(m) => m.clone(),
            None if tm.default => tm.decl.clone(),
            None => return None,
        };
        // the trait's contract binds every implementation
        Some((decl, tm.decl.requires.clone(), tm.decl.ensures.clone()))
    }

    /// Follow refinement aliases to the underlying (unrefined) type expression.
    pub fn expand_alias(&self, t: &TypeExpr) -> TypeExpr {
        if let TypeExpr::Refined(b, _) = t {
            return self.expand_alias(b);
        }
        if let TypeExpr::Named(n, args) = t {
            if let Some(TypeDecl { body: TypeBody::Alias(a), params, distinct: false, .. }) = self.types.get(n) {
                let sub: HashMap<String, TypeExpr> = params.iter().cloned().zip(args.iter().cloned()).collect();
                return self.expand_alias(&subst_texpr(a, &sub));
            }
        }
        t.clone()
    }

    /// Constructors of a named type with type args substituted: (ctor, field names, field types)
    pub fn variants_of(&self, name: &str, args: &[TypeExpr]) -> Option<Vec<(String, Option<Vec<String>>, Vec<TypeExpr>)>> {
        let td = self.types.get(name)?;
        let sub: HashMap<String, TypeExpr> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
        let s = |t: &TypeExpr| subst_texpr(t, &sub);
        match &td.body {
            TypeBody::Sum(vs) => Some(
                vs.iter()
                    .map(|v| match &v.payload {
                        Payload::None => (v.name.clone(), None, vec![]),
                        Payload::Positional(ts) => (v.name.clone(), None, ts.iter().map(s).collect()),
                        Payload::Record(fs) => (
                            v.name.clone(),
                            Some(fs.iter().map(|f| f.name.clone()).collect()),
                            fs.iter().map(|f| s(&f.ty)).collect(),
                        ),
                    })
                    .collect(),
            ),
            TypeBody::Record(fs) => Some(vec![(
                name.to_string(),
                Some(fs.iter().map(|f| f.name.clone()).collect()),
                fs.iter().map(|f| s(&f.ty)).collect(),
            )]),
            TypeBody::Alias(a) if td.distinct => Some(vec![(name.to_string(), None, vec![s(a)])]),
            TypeBody::Alias(_) => None,
        }
    }
}

pub fn subst_texpr(t: &TypeExpr, sub: &HashMap<String, TypeExpr>) -> TypeExpr {
    match t {
        TypeExpr::Named(n, args) if args.is_empty() && sub.contains_key(n) => sub[n].clone(),
        TypeExpr::Named(n, args) => TypeExpr::Named(n.clone(), args.iter().map(|a| subst_texpr(a, sub)).collect()),
        TypeExpr::Fn(ps, r) => TypeExpr::Fn(ps.iter().map(|a| subst_texpr(a, sub)).collect(), Box::new(subst_texpr(r, sub))),
        TypeExpr::Unit => TypeExpr::Unit,
        TypeExpr::Refined(b, p) => TypeExpr::Refined(Box::new(subst_texpr(b, sub)), p.clone()),
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
    /// Effects available here, with their type arguments.
    effects: BTreeMap<String, Vec<Ty>>,
    rigid: BTreeSet<String>,
    decl: String,
    errors: Vec<Diag>,
    holes: Vec<(String, Ty, Vec<(String, Ty)>, Vec<String>)>,
    matches: Vec<(Ty, Vec<Pattern>)>,
    /// What `Self` means here (inside a trait or impl).
    self_ty: Option<Ty>,
    /// Trait bounds on the rigid type parameters in scope (`T: Ord`).
    bounds: HashMap<String, Vec<(String, Vec<Ty>)>>,
    /// Obligations `type implements trait[args]`, solved at the end of the declaration.
    pending: Vec<(Ty, String, Vec<Ty>, String)>,
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
            _ => t.clone(),
        }
    }

    fn show(&self, t: &Ty) -> String {
        show_ty(&self.resolve(t))
    }

    fn occurs(&self, v: usize, t: &Ty) -> bool {
        match self.resolve(t) {
            Ty::Var(w) => v == w,
            Ty::Con(_, args) => args.iter().any(|a| self.occurs(v, a)),
            Ty::Fn(ps, r) => ps.iter().any(|a| self.occurs(v, a)) || self.occurs(v, &r),
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
            _ => Err(format!("expected `{}`, found `{}`", show_ty(&a), show_ty(&b))),
        }
    }

    fn expect(&mut self, expected: &Ty, actual: &Ty, what: &str) {
        if let Err(e) = self.unify(expected, actual) {
            self.err(format!("{what}: {e}"));
        }
    }

    /// Convert a written type to a checker type. `vars` maps scheme params to fresh vars.
    fn lower(&mut self, t: &TypeExpr, vars: &HashMap<String, Ty>) -> Ty {
        match t {
            TypeExpr::Unit => Ty::Unit,
            TypeExpr::Refined(b, _) => self.lower(b, vars),
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
                if self.env.traits.contains_key(n) {
                    self.err(format!("`{n}` is a trait, not a type: take a type parameter instead, e.g. `fn f[T: {n}](x: T)`"));
                    return self.fresh();
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
                        None => {
                            self.err(format!("unknown type `{n}`"));
                            self.fresh()
                        }
                    },
                }
            }
        }
    }

    /// The type of a trait method used anywhere: `Self` is a fresh type that
    /// must implement the trait (checked when the declaration is finished).
    fn trait_method(&mut self, name: &str, trait_name: &str) -> Ty {
        let tr = self.env.traits[trait_name].clone();
        let m = tr.methods.iter().find(|m| m.decl.name == name).unwrap().decl.clone();
        let mut vars: HashMap<String, Ty> = tr.params.iter().map(|p| (p.clone(), self.fresh())).collect();
        for p in &m.type_params {
            vars.insert(p.clone(), self.fresh());
        }
        let self_t = self.fresh();
        let saved = self.self_ty.replace(self_t.clone());
        let fty = self.lower(&Self::fn_scheme(&m), &vars);
        let mut missing = Vec::new();
        for e in &m.effects {
            if let Some((en, args)) = self.lower_effect(e, &vars) {
                match self.effects.get(&en).cloned() {
                    Some(have) => {
                        for (a, b) in have.iter().zip(&args) {
                            let _ = self.unify(a, b);
                        }
                    }
                    None => missing.push(en),
                }
            }
        }
        self.self_ty = saved;
        if !missing.is_empty() {
            self.err(format!("`{name}` performs {{{}}}, which is neither declared by `{}` nor handled here", missing.join(", "), self.decl));
        }
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
        let head = match &t {
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
        let cands: Vec<ImplDecl> = self.env.impls.iter().filter(|i| i.trait_name == tr && texpr_head(&i.for_type) == head).cloned().collect();
        for imp in cands {
            let snapshot = self.subst.clone();
            let vars: HashMap<String, Ty> = imp.type_params.iter().map(|p| (p.clone(), self.fresh())).collect();
            let saved_rigid = std::mem::take(&mut self.rigid);
            let ft = self.lower(&imp.for_type, &vars);
            let targs: Vec<Ty> = imp.trait_args.iter().map(|a| self.lower(a, &vars)).collect();
            self.rigid = saved_rigid;
            let mut ok = self.unify(&ft, &t).is_ok() && targs.len() == args.len() && targs.iter().zip(args).all(|(a, b)| self.unify(a, b).is_ok());
            if ok {
                for b in &imp.bounds {
                    for btr in &b.traits {
                        if let TypeExpr::Named(bn, bargs) = btr {
                            let ba: Vec<Ty> = bargs.iter().map(|a| self.lower(a, &vars)).collect();
                            if let Err(e) = self.satisfies(&vars[&b.param], bn, &ba, depth + 1) {
                                ok = false;
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
        let mut effs = BTreeMap::new();
        for e in &f.effects {
            if let Some((n, args)) = self.lower_effect(e, &HashMap::new()) {
                effs.insert(n, args);
            }
        }
        self.effects = effs;
        self.scopes.push(HashMap::new());
        for p in &f.params {
            self.check_refined(&p.ty);
        }
        for p in &f.params {
            let t = self.lower(&p.ty, &HashMap::new());
            self.bind(&p.name, t);
        }
        // the result's refinement may mention the parameters
        self.check_refined(&f.ret);
        let ret = self.lower(&f.ret, &HashMap::new());
        for r in &f.requires {
            let t = self.infer(r);
            self.expect(&Ty::Bool, &t, "requires clause");
            obligations.push(format!("requires {}", crate::printer::expr(r, 0)));
        }
        let bt = self.infer_with(&f.body, Some(&ret));
        self.expect(&ret, &bt, "function body");
        self.scopes.push(HashMap::new());
        self.bind("it", ret.clone());
        let saved = std::mem::take(&mut self.effects);
        for en in &f.ensures {
            let t = self.infer(en);
            self.expect(&Ty::Bool, &t, "ensures clause");
            obligations.push(format!("ensures {}", crate::printer::expr(en, 0)));
        }
        self.effects = saved;
        self.scopes.pop();
        self.scopes.pop();
    }

    /// Lower an effect reference such as `Queue[P, E]`. Omitted type arguments become fresh.
    fn lower_effect(&mut self, e: &TypeExpr, vars: &HashMap<String, Ty>) -> Option<(String, Vec<Ty>)> {
        let TypeExpr::Named(n, args) = e else {
            self.err(format!("`{}` is not an effect", crate::printer::ty(e)));
            return None;
        };
        let Some(ed) = self.env.effects.get(n).cloned() else {
            self.err(format!("unknown effect `{n}`"));
            return None;
        };
        if !args.is_empty() && args.len() != ed.params.len() {
            self.err(format!("effect `{n}` expects {} type arguments, got {}", ed.params.len(), args.len()));
        }
        let tys = if args.is_empty() {
            ed.params.iter().map(|_| self.fresh()).collect()
        } else {
            args.iter().map(|a| self.lower(a, vars)).collect()
        };
        Some((n.clone(), tys))
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
            TypeExpr::Named(_, args) => args.iter().for_each(|a| self.check_refined(a)),
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
            let mut missing = Vec::new();
            for e in &f.effects {
                let Some((en, args)) = self.lower_effect(e, &vars) else { continue };
                match self.effects.get(&en).cloned() {
                    Some(have) => {
                        for (a, b) in have.iter().zip(&args) {
                            if let Err(msg) = self.unify(a, b) {
                                self.err(format!("`{name}` performs `{en}` at a different type: {msg}"));
                            }
                        }
                    }
                    None => missing.push(en),
                }
            }
            if !missing.is_empty() {
                self.err(format!(
                    "`{name}` performs {{{}}}, which is neither declared by `{}` nor handled here",
                    missing.join(", "),
                    self.decl
                ));
            }
            return Some(fty);
        }
        if let Some(tn) = self.env.method_trait.get(name).cloned() {
            return Some(self.trait_method(name, &tn));
        }
        if let Some((_, ps, sig)) = BUILTINS.iter().find(|(n, ..)| *n == name) {
            let ps: Vec<String> = ps.split(',').filter(|s| !s.is_empty()).map(String::from).collect();
            return Some(self.instantiate(&ps, &parse_type_str(sig)));
        }
        None
    }

    fn call(&mut self, fty: Ty, args: &[Expr], what: &str) -> Ty {
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
            let at = self.infer_with(a, Some(p));
            self.expect(p, &at, &format!("{what}, argument {}", i + 1));
        }
        r
    }

    fn infer_with(&mut self, e: &Expr, expected: Option<&Ty>) -> Ty {
        // Bidirectional lite: lambdas take their parameter types from context.
        if let (Expr::Lambda(ps, body), Some(exp)) = (e, expected) {
            if let Ty::Fn(pts, _) = self.resolve(exp) {
                if pts.len() == ps.len() {
                    self.scopes.push(HashMap::new());
                    for (p, t) in ps.iter().zip(pts.iter()) {
                        self.bind(p, t.clone());
                    }
                    let r = self.infer(body);
                    self.scopes.pop();
                    return Ty::Fn(pts, Box::new(r));
                }
            }
        }
        if let (Expr::Hole(h), Some(exp)) = (e, expected) {
            self.record_hole(h, exp.clone());
            return exp.clone();
        }
        self.infer(e)
    }

    fn record_hole(&mut self, name: &str, t: Ty) {
        let mut ctx: Vec<(String, Ty)> = Vec::new();
        for s in &self.scopes {
            for (k, v) in s {
                ctx.retain(|(n, _)| n != k);
                ctx.push((k.clone(), v.clone()));
            }
        }
        ctx.sort_by(|a, b| a.0.cmp(&b.0));
        self.holes.push((name.to_string(), t, ctx, self.effects.keys().cloned().collect()));
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

    fn infer(&mut self, e: &Expr) -> Ty {
        match e {
            Expr::Int(_) => Ty::Int,
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
                let t = self.fresh();
                for el in es {
                    let et = self.infer(el);
                    self.expect(&t, &et, "list element");
                }
                Ty::Con("List".into(), vec![t])
            }
            Expr::Ctor(c, args) => {
                let Some((rt, fields)) = self.ctor_type(c) else {
                    self.err(format!("unknown constructor `{c}`"));
                    return self.fresh();
                };
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
                rt
            }
            Expr::Call(f, args) => {
                if let Expr::Var(n) = &**f {
                    if let Some(t) = self.overloaded(n, args) {
                        return t;
                    }
                }
                let what = match &**f {
                    Expr::Var(n) => format!("call to `{n}`"),
                    _ => "call".into(),
                };
                let ft = self.infer(f);
                self.call(ft, args, &what)
            }
            Expr::Method(recv, m, args) => {
                let mut all = vec![(**recv).clone()];
                all.extend(args.iter().cloned());
                if let Some(t) = self.overloaded(m, &all) {
                    return t;
                }
                let Some(ft) = self.lookup(m).or_else(|| self.global_fn(m)) else {
                    self.err(format!("unknown function `{m}`"));
                    return self.fresh();
                };
                self.call(ft, &all, &format!("call to `{m}`"))
            }
            Expr::Perform(eff, op, args) => {
                let Some(ed) = self.env.effects.get(eff).cloned() else {
                    self.err(format!("unknown effect `{eff}`"));
                    return self.fresh();
                };
                let Some(sig) = ed.ops.iter().find(|o| &o.name == op) else {
                    self.err(format!("effect `{eff}` has no operation `{op}`"));
                    return self.fresh();
                };
                let args_ty = match self.effects.get(eff).cloned() {
                    Some(a) => a,
                    None => {
                        self.err(format!(
                            "performs `{eff}.{op}` but `{eff}` is neither declared by `{}` nor handled here",
                            self.decl
                        ));
                        ed.params.iter().map(|_| self.fresh()).collect()
                    }
                };
                let vars: HashMap<String, Ty> = ed.params.iter().cloned().zip(args_ty).collect();
                let sig = sig.clone();
                let ft = self.lower(&TypeExpr::Fn(sig.params.iter().map(|p| p.ty.clone()).collect(), Box::new(sig.ret.clone())), &vars);
                self.call(ft, args, &format!("`{eff}.{op}`"))
            }
            Expr::Field(r, f) => {
                let rt = self.infer(r);
                self.field_type(&rt, f)
            }
            Expr::Lambda(ps, body) => {
                self.scopes.push(HashMap::new());
                let pts: Vec<Ty> = ps.iter().map(|_| self.fresh()).collect();
                for (p, t) in ps.iter().zip(&pts) {
                    self.bind(p, t.clone());
                }
                let r = self.infer(body);
                self.scopes.pop();
                Ty::Fn(pts, Box::new(r))
            }
            Expr::Bin(op, l, r) => {
                let (lt, rt) = (self.infer(l), self.infer(r));
                use BinOp::*;
                match op {
                    Add | Sub | Mul | Div | Mod => {
                        self.numeric(&lt, &rt, op);
                        lt
                    }
                    Concat => {
                        self.expect(&lt, &rt, "`++` operands");
                        match self.resolve(&lt) {
                            Ty::Str | Ty::Var(_) => {}
                            Ty::Con(n, _) if n == "List" => {}
                            t => self.err(format!("`++` works on String or List, found `{}`", show_ty(&t))),
                        }
                        lt
                    }
                    Eq | Ne => {
                        self.expect(&lt, &rt, &format!("`{}` operands", op.symbol()));
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
                match self.ctor_type(c) {
                    Some((ct, _)) => self.expect(&ct, &t, &format!("`is {c}`")),
                    None => self.err(format!("unknown constructor `{c}`")),
                }
                Ty::Bool
            }
            Expr::If(c, t, f) => {
                let ct = self.infer(c);
                self.expect(&Ty::Bool, &ct, "if condition");
                let tt = self.infer(t);
                let ft = self.infer_with(f, Some(&tt));
                self.expect(&tt, &ft, "if branches");
                tt
            }
            Expr::Match(s, arms) => {
                let st = self.infer(s);
                let rt = self.fresh();
                for a in arms {
                    self.scopes.push(HashMap::new());
                    self.pattern(&a.pat, &st);
                    let bt = self.infer_with(&a.body, Some(&rt));
                    self.expect(&rt, &bt, "match arms");
                    self.scopes.pop();
                }
                self.matches.push((st, arms.iter().map(|a| a.pat.clone()).collect()));
                rt
            }
            Expr::Block(stmts, last) => {
                self.scopes.push(HashMap::new());
                for s in stmts {
                    match s {
                        Stmt::Let(p, ann, e) => {
                            let ann = ann.as_ref().map(|a| self.lower(a, &HashMap::new()));
                            let t = self.infer_with(e, ann.as_ref());
                            if let Some(a) = &ann {
                                self.expect(a, &t, "let annotation");
                            }
                            self.pattern(p, &t);
                            if !irrefutable(p) {
                                self.matches.push((t, vec![p.clone()]));
                            }
                        }
                        Stmt::Expr(e) => {
                            self.infer(e);
                        }
                        Stmt::Assert(e) => {
                            let t = self.infer(e);
                            self.expect(&Ty::Bool, &t, "assert");
                        }
                    }
                }
                let t = self.infer(last);
                self.scopes.pop();
                t
            }
            Expr::Handle(body, handlers) => {
                let outer = self.effects.clone();
                let mut installed: Vec<(String, Vec<Ty>, Option<Ty>)> = Vec::new();
                for h in handlers {
                    let Some(ed) = self.env.effects.get(&h.effect).cloned() else {
                        self.err(format!("unknown effect `{}`", h.effect));
                        continue;
                    };
                    let args: Vec<Ty> = ed.params.iter().map(|_| self.fresh()).collect();
                    let vars: HashMap<String, Ty> = ed.params.iter().cloned().zip(args.iter().cloned()).collect();
                    let state_ty = h.state.as_ref().map(|(_, init)| self.infer(init));
                    if let HandlerKind::Clauses(cs) = &h.kind {
                        for op in &ed.ops {
                            if !cs.iter().any(|c| c.op == op.name) {
                                self.err(format!("handler for `{}` is missing operation `{}`", h.effect, op.name));
                            }
                        }
                        for c in cs {
                            let Some(sig) = ed.ops.iter().find(|o| o.name == c.op) else {
                                self.err(format!("effect `{}` has no operation `{}`", h.effect, c.op));
                                continue;
                            };
                            if sig.params.len() != c.params.len() {
                                self.err(format!("`{}.{}` takes {} parameters", h.effect, c.op, sig.params.len()));
                            }
                            self.scopes.push(HashMap::new());
                            for (p, sp) in c.params.iter().zip(&sig.params) {
                                let t = self.lower(&sp.ty, &vars);
                                self.bind(p, t);
                            }
                            let mut rt = self.lower(&sig.ret, &vars);
                            if let (Some((sv, _)), Some(st)) = (&h.state, &state_ty) {
                                self.bind(sv, st.clone());
                                rt = Ty::Con("Resume".into(), vec![rt, st.clone()]);
                            }
                            let bt = self.infer_with(&c.body, Some(&rt));
                            self.expect(&rt, &bt, &format!("result of handler `{}.{}`", h.effect, c.op));
                            self.scopes.pop();
                        }
                    }
                    installed.push((h.effect.clone(), args, state_ty));
                }
                for (e, args, _) in &installed {
                    self.effects.insert(e.clone(), args.clone());
                }
                let mut t = self.infer(body);
                self.effects = outer;
                // return clauses run innermost (last listed) first, with the final state in scope
                for (h, (_, _, st)) in handlers.iter().zip(&installed).rev() {
                    if let Some((p, b)) = &h.on_return {
                        self.scopes.push(HashMap::new());
                        self.bind(p, t.clone());
                        if let (Some((sv, _)), Some(st)) = (&h.state, st) {
                            self.bind(sv, st.clone());
                        }
                        t = self.infer(b);
                        self.scopes.pop();
                    }
                }
                t
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
    fn overloaded(&mut self, name: &str, args: &[Expr]) -> Option<Ty> {
        if self.lookup(name).is_some() || self.env.fns.contains_key(name) {
            return None;
        }
        let cands: Vec<_> = BUILTINS.iter().filter(|(n, ..)| *n == name).collect();
        if cands.len() < 2 {
            return None;
        }
        let arg_tys: Vec<Ty> = args.iter().map(|a| self.infer(a)).collect();
        for (_, ps, sig) in &cands {
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
        let sigs: Vec<String> = cands.iter().map(|(_, _, s)| format!("  {name}{}", s)).collect();
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

    fn pattern(&mut self, p: &Pattern, t: &Ty) {
        match p {
            Pattern::Wild => {}
            Pattern::Bind(x) => self.bind(x, t.clone()),
            Pattern::Int(_) => self.expect(&Ty::Int, t, "pattern"),
            Pattern::Tuple(ps) => {
                let items: Vec<Ty> = ps.iter().map(|_| self.fresh()).collect();
                self.expect(&Ty::Con("Tuple".into(), items.clone()), t, "tuple pattern");
                for (p, it) in ps.iter().zip(&items) {
                    self.pattern(p, it);
                }
            }
            Pattern::Str(_) => self.expect(&Ty::Str, t, "pattern"),
            Pattern::Bool(_) => self.expect(&Ty::Bool, t, "pattern"),
            Pattern::List(items, rest) => {
                let el = self.fresh();
                let lt = Ty::Con("List".into(), vec![el.clone()]);
                self.expect(&lt, t, "list pattern");
                for i in items {
                    self.pattern(i, &el);
                }
                if let Some(r) = rest {
                    if r != "_" {
                        self.bind(r, lt);
                    }
                }
            }
            Pattern::Ctor(c, cp) => {
                let Some((ct, fields)) = self.ctor_type(c) else {
                    self.err(format!("unknown constructor `{c}` in pattern"));
                    return;
                };
                self.expect(t, &ct, &format!("pattern `{c}`"));
                match cp {
                    CtorPat::None => {
                        if !fields.is_empty() {
                            self.err(format!("pattern `{c}` must bind its {} fields (use `{c}(..)` or `{c} {{ .. }}`)", fields.len()));
                        }
                    }
                    CtorPat::Positional(ps) => {
                        if ps.len() != fields.len() {
                            self.err(format!("pattern `{c}` expects {} fields, got {}", fields.len(), ps.len()));
                        }
                        for (sp, (_, ft)) in ps.iter().zip(fields.iter()) {
                            self.pattern(sp, ft);
                        }
                    }
                    CtorPat::Record(fs, open) => {
                        for (fname, sp) in fs {
                            match fields.iter().find(|(n, _)| n.as_deref() == Some(fname)) {
                                Some((_, ft)) => {
                                    let ft = ft.clone();
                                    self.pattern(sp, &ft)
                                }
                                None => self.err(format!("`{c}` has no field `{fname}`")),
                            }
                        }
                        if !open && fs.len() != fields.len() {
                            self.err(format!("pattern `{c} {{..}}` does not mention every field; add `..`"));
                        }
                    }
                }
            }
        }
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
    }
}

// ------------------------------------------------------------------
// Exhaustiveness (a small version of Maranget's usefulness algorithm)

#[derive(Clone)]
enum P {
    Any,
    C(String, Vec<P>),
    Lit,
}

fn norm(c: &Checker, p: &Pattern) -> P {
    match p {
        Pattern::Wild | Pattern::Bind(_) => P::Any,
        Pattern::Int(_) | Pattern::Str(_) => P::Lit,
        Pattern::Tuple(ps) => P::C("(,)".into(), ps.iter().map(|p| norm(c, p)).collect()),
        Pattern::Bool(b) => P::C(b.to_string(), vec![]),
        Pattern::List(items, rest) => {
            let mut acc = match rest {
                Some(_) => P::Any,
                None => P::C("[]".into(), vec![]),
            };
            for i in items.iter().rev() {
                acc = P::C("::".into(), vec![norm(c, i), acc]);
            }
            acc
        }
        Pattern::Ctor(name, cp) => {
            let tname = c.env.ctors.get(name).cloned().unwrap_or_default();
            let field_names: Vec<String> = c
                .env
                .variants_of(&tname, &[])
                .and_then(|vs| vs.into_iter().find(|v| &v.0 == name))
                .map(|v| v.1.unwrap_or_else(|| (0..v.2.len()).map(|i| i.to_string()).collect()))
                .unwrap_or_default();
            let subs = match cp {
                CtorPat::None => vec![P::Any; field_names.len()],
                CtorPat::Positional(ps) => ps.iter().map(|p| norm(c, p)).collect(),
                CtorPat::Record(fs, _) => field_names
                    .iter()
                    .map(|f| fs.iter().find(|(n, _)| n == f).map(|(_, p)| norm(c, p)).unwrap_or(P::Any))
                    .collect(),
            };
            P::C(name.clone(), subs)
        }
    }
}

impl Checker<'_> {
    /// The constructors of a type, or None if it has infinitely many values.
    fn signature(&mut self, t: &Ty) -> Option<Vec<(String, Vec<Ty>)>> {
        match self.resolve(t) {
            Ty::Bool => Some(vec![("true".into(), vec![]), ("false".into(), vec![])]),
            Ty::Con(n, args) if n == "Tuple" => Some(vec![("(,)".into(), args.clone())]),
            Ty::Con(n, _) if n == "Dict" || n == "Set" => None,
            Ty::Unit => Some(vec![("()".into(), vec![])]),
            Ty::Con(n, args) if n == "List" => {
                let l = Ty::Con("List".into(), args.clone());
                Some(vec![("[]".into(), vec![]), ("::".into(), vec![args[0].clone(), l])])
            }
            Ty::Con(n, args) => {
                let td = self.env.types.get(&n)?.clone();
                let ctors: Vec<String> = match &td.body {
                    TypeBody::Sum(vs) => vs.iter().map(|v| v.name.clone()).collect(),
                    TypeBody::Record(_) => vec![n.clone()],
                    TypeBody::Alias(_) if td.distinct => vec![n.clone()],
                    TypeBody::Alias(_) => return None,
                };
                let mut out = Vec::new();
                for c in ctors {
                    let (ct, fields) = self.ctor_type(&c)?;
                    let _ = self.unify(&ct, &Ty::Con(n.clone(), args.clone()));
                    out.push((c, fields.into_iter().map(|f| self.resolve(&f.1)).collect()));
                }
                Some(out)
            }
            _ => None,
        }
    }

    fn missing(&mut self, rows: &[Vec<P>], tys: &[Ty], depth: usize) -> Option<Vec<String>> {
        if tys.is_empty() {
            return if rows.is_empty() { Some(vec![]) } else { None };
        }
        if depth > 12 {
            return None;
        }
        let rest_tys = &tys[1..];
        match self.signature(&tys[0]) {
            Some(cs) => {
                for (c, arg_tys) in cs {
                    let n = arg_tys.len();
                    let spec: Vec<Vec<P>> = rows
                        .iter()
                        .filter_map(|r| match &r[0] {
                            P::Any => Some([vec![P::Any; n], r[1..].to_vec()].concat()),
                            P::C(name, subs) if *name == c => Some([subs.clone(), r[1..].to_vec()].concat()),
                            _ => None,
                        })
                        .collect();
                    let tys2 = [arg_tys.clone(), rest_tys.to_vec()].concat();
                    if let Some(w) = self.missing(&spec, &tys2, depth + 1) {
                        let (args, rest) = w.split_at(n);
                        let named = self.env.ctors.get(&c).and_then(|t| self.env.variants_of(t, &[])).and_then(|vs| vs.into_iter().find(|v| v.0 == c)).and_then(|v| v.1);
                        let shown = match c.as_str() {
                            "::" => format!("[{}, ..]", args[0]),
                            "(,)" => format!("({})", args.join(", ")),
                            _ if n == 0 => c.clone(),
                            _ if named.is_some() && args.iter().all(|a| a == "_") => format!("{c} {{ .. }}"),
                            _ if named.is_some() => {
                                let ns = named.unwrap();
                                let parts: Vec<String> = ns.iter().zip(args).filter(|(_, a)| *a != "_").map(|(n, a)| format!("{n}: {a}")).collect();
                                format!("{c} {{ {}, .. }}", parts.join(", "))
                            }
                            _ => format!("{c}({})", args.join(", ")),
                        };
                        return Some([vec![shown], rest.to_vec()].concat());
                    }
                }
                None
            }
            None => {
                let def: Vec<Vec<P>> = rows.iter().filter(|r| matches!(r[0], P::Any)).map(|r| r[1..].to_vec()).collect();
                self.missing(&def, rest_tys, depth + 1).map(|w| [vec!["_".to_string()], w].concat())
            }
        }
    }

    fn check_matches(&mut self) {
        let ms = std::mem::take(&mut self.matches);
        for (t, pats) in ms {
            let rows: Vec<Vec<P>> = pats.iter().map(|p| vec![norm(self, p)]).collect();
            let _ = P::Lit;
            if let Some(w) = self.missing(&rows, &[t.clone()], 0) {
                self.err(format!("non-exhaustive match on `{}`: `{}` is not covered", self.show(&t), w[0]));
            }
        }
    }
}

// ------------------------------------------------------------------

pub fn check(prog: &Program) -> CheckReport {
    let resolved = crate::resolve::resolve(prog);
    let prog = &resolved.program;
    let (env, mut errors) = Env::new(prog);
    errors.splice(0..0, resolved.errors);
    let mut holes = Vec::new();
    for d in &prog.decls {
        let mut c = Checker {
            env: &env,
            subst: Vec::new(),
            scopes: vec![HashMap::new()],
            effects: BTreeMap::new(),
            rigid: BTreeSet::new(),
            decl: d.name(),
            errors: Vec::new(),
            holes: Vec::new(),
            matches: Vec::new(),
            self_ty: None,
            bounds: HashMap::new(),
            pending: Vec::new(),
        };
        let mut obligations = Vec::new();
        match &d.kind {
            DeclKind::Type(t) => {
                if matches!(t.body, TypeBody::Alias(_)) && t.refinement.is_none() && !t.distinct {
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
                        Ty::Con(t.name.clone(), t.params.iter().map(|p| Ty::Rigid(p.clone())).collect())
                    }
                    TypeBody::Sum(vs) => {
                        for v in vs {
                            match &v.payload {
                                Payload::None => {}
                                Payload::Positional(ts) => ts.iter().for_each(|t| {
                                    c.lower(t, &HashMap::new());
                                }),
                                Payload::Record(fs) => fs.iter().for_each(|f| {
                                    c.lower(&f.ty, &HashMap::new());
                                }),
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
            DeclKind::Effect(e) => {
                c.rigid = e.params.iter().cloned().collect();
                for op in &e.ops {
                    c.scopes.push(HashMap::new());
                    for p in &op.params {
                        let t = c.lower(&p.ty, &HashMap::new());
                        c.bind(&p.name, t);
                    }
                    let rt = c.lower(&op.ret, &HashMap::new());
                    for r in &op.requires {
                        let t = c.infer(r);
                        c.expect(&Ty::Bool, &t, "requires clause");
                    }
                    c.bind("it", rt);
                    for en in &op.ensures {
                        let t = c.infer(en);
                        c.expect(&Ty::Bool, &t, "ensures clause");
                    }
                    c.scopes.pop();
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
                if env.types.get(&head).is_some_and(|t| matches!(t.body, TypeBody::Alias(_)) && !t.distinct) {
                    c.err(format!("`{head}` is a refined alias of another type; implement the trait for the underlying type"));
                }
                let self_ty = c.lower(&i.for_type, &HashMap::new());
                c.self_ty = Some(self_ty.clone());
                match env.traits.get(&i.trait_name).cloned() {
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
                        for tm in &tr.methods {
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
                                    let allowed = tm.decl.effect_names();
                                    for e in im.effect_names() {
                                        if !allowed.contains(&e) {
                                            c.err(format!("`{}` performs `{e}`, which trait `{}` does not allow for this method", im.name, tr.name));
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
        for (name, t, ctx, effs) in std::mem::take(&mut c.holes) {
            let inferred = c.show(&t);
            holes.push(HoleInfo {
                decl: d.name(),
                name,
                goal: match &declared_goal {
                    Some(g) if *g != inferred => format!("{g}  (= {inferred} + refinement)"),
                    _ => inferred,
                },
                context: ctx.iter().map(|(n, t)| (n.clone(), c.show(t))).collect(),
                effects: effs,
                obligations: obligations.clone(),
            });
        }
        errors.extend(c.errors);
    }
    CheckReport { errors, holes }
}
