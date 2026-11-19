//! Static checking: Hindley–Milner style inference with explicit signatures
//! on top-level functions, effect checking, exhaustiveness checking, and
//! typed-hole reporting.

use crate::ast::*;
use crate::parser::Parser;
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};

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
    ("starts_with", "", "(String, String) -> Bool"),
    // strings are UTF-8 and indexed by character
    ("ends_with", "", "(String, String) -> Bool"),
    ("chars", "", "(String) -> List[String]"),
    ("get", "", "(String, Int) -> Option[String]"),
    ("slice", "", "(String, Int, Int) -> String"),
    ("slice", "A", "(List[A], Int, Int) -> List[A]"),
    ("index_of", "", "(String, String) -> Option[Int]"),
    ("replace", "", "(String, String, String) -> String"),
    ("lines", "", "(String) -> List[String]"),
    ("repeat", "", "(String, Int) -> String"),
    ("reverse", "", "(String) -> String"),
    ("to_upper", "", "(String) -> String"),
    ("to_lower", "", "(String) -> String"),
    ("is_digit", "", "(String) -> Bool"),
    ("is_alpha", "", "(String) -> Bool"),
    ("is_space", "", "(String) -> Bool"),
    ("ord", "", "(String) -> Int"),
    ("chr", "", "(Int) -> String"),
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

/// Everything known about the declarations of a program, shared by the
/// checker, interpreter and generators.
pub struct Env {
    pub types: HashMap<String, TypeDecl>,
    /// ctor name -> (type name)
    pub ctors: HashMap<String, String>,
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
        TypeExpr::Union(_) => "Union".into(),
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
                                // a clash with a record type is reported below, with the other name clashes
                                let prev = env.ctors.get(&v.name).cloned();
                                if prev.as_ref().is_some_and(|p| *p == v.name) {
                                    continue;
                                }
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
        // a variant is a type too, so its name must be unique in the whole
        // project: no other variant, type or trait (in any module) may share it
        let short = |q: &str| q.rsplit('.').next().unwrap_or(q).to_string();
        let mut owners: HashMap<String, Vec<String>> = HashMap::new();
        for n in env.types.keys().chain(env.traits.keys()) {
            owners.entry(short(n)).or_default().push(format!("the type `{n}`"));
        }
        let mut variants: Vec<(&String, &String)> = Vec::new();
        for t in env.types.values() {
            if let TypeBody::Sum(vs) = &t.body {
                variants.extend(vs.iter().map(|v| (&v.name, &t.name)));
            }
        }
        variants.sort();
        for (c, t) in &variants {
            owners.entry(short(c)).or_default().push(format!("the variant `{c}` of `{t}`"));
        }
        for (c, t) in &variants {
            let others: Vec<&String> = owners[&short(c)].iter().filter(|o| **o != format!("the variant `{c}` of `{t}`")).collect();
            if !others.is_empty() {
                let list = others.iter().map(|o| o.as_str()).collect::<Vec<_>>().join(", ");
                errs.push(Diag {
                    decl: (*t).clone(),
                    msg: format!("variant `{c}` of `{t}` has the same name as {list}: a variant is a type of its own, so its name must be unique across the project (rename one)"),
                });
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

    /// The sum type a variant belongs to (`Circle` -> `Shape`). A variant is a
    /// type of its own, with its sum's type parameters.
    pub fn variant_parent(&self, name: &str) -> Option<String> {
        let p = self.ctors.get(name)?;
        (p != name && matches!(self.types.get(p).map(|t| &t.body), Some(TypeBody::Sum(_)))).then(|| p.clone())
    }

    /// Whether `name` names a type (not only a constructor): a primitive, a
    /// declared type, a variant or a trait. Such a name in a pattern matches
    /// the values of that type.
    pub fn is_type_name(&self, name: &str) -> bool {
        matches!(name, "Int" | "Float" | "Bool" | "String" | "Unit" | "List" | "Set" | "Dict")
            || self.types.contains_key(name)
            || self.traits.contains_key(name)
            || self.variant_parent(name).is_some()
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
    matches: Vec<(Ty, Vec<(P, String)>)>,
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
    /// What `return`, `!` and `?` return from: the enclosing function or
    /// lambda's result type (`None` in tests, laws and refinements).
    ret: Option<Ty>,
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
                    self.unify(x, y)?;
                }
                Ok(())
            }
            _ => self.unify(&e, &a),
        }
    }

    fn expect(&mut self, expected: &Ty, actual: &Ty, what: &str) {
        if let Err(e) = self.fits(expected, actual) {
            self.err(format!("{what}: {e}"));
        }
    }

    /// The type of two things that must agree (`if` branches, list elements):
    /// whichever holds the other, else an error.
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
        self.err(format!("{what}: expected `{sa}`, found `{sb}` (if both are meant, write the type: `{sa} | {sb}`)"));
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
        let tr = self.env.traits[trait_name].clone();
        let m = tr.methods.iter().find(|m| m.decl.name == name).unwrap().decl.clone();
        let mut vars: HashMap<String, Ty> = tr.params.iter().map(|p| (p.clone(), self.fresh())).collect();
        for p in &m.type_params {
            vars.insert(p.clone(), self.fresh());
        }
        let self_t = self.fresh();
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
                    return self.lambda(ps, Some(pts), body);
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
            for (k, v) in s.iter().filter(|(k, _)| !k.starts_with("mut:")) {
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
                let want = match expected.map(|t| self.resolve(t)) {
                    Some(Ty::Con(n, a)) if n == "List" && a.len() == 1 => Some(a[0].clone()),
                    _ => None,
                };
                let mut t = want.clone().unwrap_or_else(|| self.fresh());
                for el in es {
                    let et = self.infer_with(el, Some(&t));
                    match &want {
                        Some(w) => self.expect(w, &et, "list element"),
                        None => t = self.join(&t, &et, "list element"),
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
                if let Expr::Var(n) = &**f {
                    if self.lookup(n).is_none() {
                        self.check_mut_args(n, args);
                    }
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
                if self.lookup(m).is_none() {
                    self.check_mut_args(m, &all);
                }
                if let Some(t) = self.overloaded(m, &all) {
                    return t;
                }
                let Some(ft) = self.lookup(m).or_else(|| self.global_fn(m)) else {
                    self.err(format!("unknown function `{m}`"));
                    return self.fresh();
                };
                self.call(ft, &all, &format!("call to `{m}`"))
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
                        self.join(&tt, &ft, "if branches")
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
                let mut rt: Option<Ty> = expected.cloned();
                let mut rows = Vec::new();
                for a in arms {
                    self.scopes.push(HashMap::new());
                    let (nt, p) = self.pattern(&a.pat, &st);
                    if let Some(x) = &target {
                        self.bind(x, nt.clone());
                        if name.is_none() {
                            self.bind(&format!("narrowed:{x}"), st.clone());
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
                        Some(r) => self.join(&r, &bt, "match arms"),
                        None => bt,
                    });
                    self.scopes.pop();
                    rows.push((p, crate::printer::pat(&a.pat)));
                }
                self.matches.push((st, rows));
                rt.unwrap_or_else(|| self.fresh())
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
                            let (_, pp) = self.pattern(p, &t);
                            if !irrefutable(p) {
                                self.matches.push((t, vec![(pp, crate::printer::pat(p))]));
                            }
                        }
                        Stmt::Var(n, ann, e) => {
                            let ann = ann.as_ref().map(|a| self.lower(a, &HashMap::new()));
                            let t = self.infer_with(e, ann.as_ref());
                            if let Some(a) = &ann {
                                self.expect(a, &t, "let annotation");
                            }
                            self.bind(n, t);
                            self.bind(&format!("mut:{n}"), Ty::Unit);
                        }
                        Stmt::Assign(place, e) => {
                            self.check_place(place, "assign to");
                            let pt = self.end_narrowing(place).unwrap_or_else(|| self.infer(place));
                            let et = self.infer_with(e, Some(&pt));
                            self.expect(&pt, &et, "assignment");
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
                    if let StrPart::Expr(e) = p {
                        self.infer(e);
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
                        if self.unify(&r, &passed).is_err() {
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

    /// Arguments passed to `mut` parameters are changed by the call.
    fn check_mut_args(&mut self, callee: &str, args: &[Expr]) {
        let params: Option<Vec<Param>> = self
            .env
            .fns
            .get(callee)
            .map(|f| f.params.clone())
            .or_else(|| {
                let tr = self.env.method_trait.get(callee)?;
                self.env.traits[tr].methods.iter().find(|m| m.decl.name == callee).map(|m| m.decl.params.clone())
            });
        for (p, a) in params.unwrap_or_default().iter().zip(args) {
            if p.mutable {
                self.check_place(a, &format!("let `{callee}` change (its `mut {}` parameter)", p.name));
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
            Pattern::Cond(c) => {
                self.scopes.push(HashMap::new());
                self.bind("it", t.clone());
                let ct = self.infer(c);
                self.expect(&Ty::Bool, &ct, &format!("pattern `where {}`", crate::printer::expr(c, 0)));
                self.scopes.pop();
                (t.clone(), ints::of_pred(c).map_or(P::Lit, P::Ints))
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
            TypeExpr::Refined(b, pred) => match (self.int_refinement(b), ints::of_pred(pred)) {
                (Refn::Plain, Some(s)) if self.int_based(b) => Refn::Ints(s),
                (Refn::Ints(a), Some(s)) => Refn::Ints(ints::and(&a, &s)),
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

    fn int_based(&self, t: &TypeExpr) -> bool {
        matches!(self.env.expand_alias(t), TypeExpr::Named(n, _) if n == "Int")
    }
}

/// `x`, `o.status`: the written path of a variable or field access.
fn place_path(e: &Expr) -> Option<String> {
    match e {
        Expr::Var(x) => Some(x.clone()),
        Expr::Field(r, f) => Some(format!("{}.{f}", place_path(r)?)),
        _ => None,
    }
}

/// A refinement as far as patterns can tell: none, a set of integers, or a
/// condition we cannot decide yet (it covers nothing for exhaustiveness).
enum Refn {
    Plain,
    Ints(ints::Ints),
    Opaque,
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
// Exhaustiveness and unreachable arms (Maranget's usefulness algorithm).
//
// A pattern stands for a set of values. A type is split into "atoms": its
// constructors (variants, records, `true`/`false`, `[]`/`::`, tuples), the
// members of a union, and, for `Int`, the intervals that the patterns' bounds
// cut it into. Anything else is one opaque atom.

/// The set of values a pattern matches.
#[derive(Clone, Debug)]
pub enum P {
    Any,
    /// A constructor, with patterns for its fields.
    C(String, Vec<P>),
    /// Some values we cannot describe (a string literal, an undecidable
    /// condition): it covers no atom fully.
    Lit,
    /// Every value of a type (`String`, `Circle`, `Shape` the trait).
    Ty(Ty),
    /// These integers (a literal, `where 3 <= it && it <= 5`, `Positive`).
    Ints(ints::Ints),
}

#[derive(Clone, PartialEq)]
enum AtomKey {
    Ctor(String),
    Region(i128, i128),
    Opaque(String),
}

struct Atom {
    key: AtomKey,
    args: Vec<Ty>,
    ty: Ty,
}

impl Checker<'_> {
    /// The atoms of type `t`, with `Int` cut by the integer sets in `col`.
    fn atoms(&mut self, t: &Ty, col: &[&P]) -> Vec<Atom> {
        let t = self.resolve(t);
        let atom = |k: AtomKey, args: Vec<Ty>, ty: &Ty| Atom { key: k, args, ty: ty.clone() };
        let ctor = |c: &str| AtomKey::Ctor(c.to_string());
        match &t {
            Ty::Bool => vec![atom(ctor("true"), vec![], &t), atom(ctor("false"), vec![], &t)],
            Ty::Unit => vec![atom(ctor("()"), vec![], &t)],
            Ty::Int => {
                let mut cuts: Vec<i128> = vec![ints::MIN];
                for p in col {
                    if let P::Ints(s) = p {
                        for (lo, hi) in s {
                            cuts.push(*lo);
                            cuts.push(hi + 1);
                        }
                    }
                }
                cuts.retain(|c| *c >= ints::MIN && *c <= ints::MAX);
                cuts.sort();
                cuts.dedup();
                cuts.iter().enumerate().map(|(i, lo)| atom(AtomKey::Region(*lo, cuts.get(i + 1).map_or(ints::MAX, |n| n - 1)), vec![], &t)).collect()
            }
            Ty::Union(ms) => ms.iter().flat_map(|m| self.atoms(m, col)).collect(),
            Ty::Con(n, args) if n == "Tuple" => vec![atom(ctor("(,)"), args.clone(), &t)],
            Ty::Con(n, args) if n == "List" => vec![atom(ctor("[]"), vec![], &t), atom(ctor("::"), vec![args[0].clone(), t.clone()], &t)],
            Ty::Con(n, args) if !self.env.traits.contains_key(n) && (self.env.types.contains_key(n) || self.env.variant_parent(n).is_some()) => {
                let ctors: Vec<String> = match self.env.types.get(n).map(|td| &td.body) {
                    Some(TypeBody::Sum(vs)) => vs.iter().map(|v| v.name.clone()).collect(),
                    _ => vec![n.clone()],
                };
                let parent = self.env.variant_parent(n).unwrap_or(n.clone());
                ctors
                    .into_iter()
                    .filter_map(|c| {
                        let (ct, fields) = self.ctor_type(&c)?;
                        let _ = self.unify(&ct, &Ty::Con(parent.clone(), args.clone()));
                        let fs = fields.into_iter().map(|f| self.resolve(&f.1)).collect();
                        let ty = self.variant_ty(&c, &ct);
                        Some(atom(AtomKey::Ctor(c), fs, &self.resolve(&ty)))
                    })
                    .collect()
            }
            _ => vec![atom(AtomKey::Opaque(show_ty(&t)), vec![], &t)],
        }
    }

    /// The patterns for an atom's fields if `p` covers the whole atom.
    fn covers(&mut self, p: &P, a: &Atom) -> Option<Vec<P>> {
        let any = || vec![P::Any; a.args.len()];
        match p {
            P::Any => Some(any()),
            P::C(name, subs) => (a.key == AtomKey::Ctor(name.clone())).then(|| subs.clone()),
            P::Lit => None,
            P::Ints(s) => match a.key {
                AtomKey::Region(lo, hi) if ints::contains(s, lo, hi) => Some(vec![]),
                _ => None,
            },
            P::Ty(pt) => {
                let pt = self.resolve(pt);
                if let Ty::Con(tr, targs) = &pt {
                    if self.env.traits.contains_key(tr) {
                        let snap = (self.subst.clone(), self.pending.len());
                        let ok = self.satisfies(&a.ty, tr, targs, 0).is_ok();
                        self.subst = snap.0;
                        self.pending.truncate(snap.1);
                        return ok.then(any);
                    }
                }
                let inside = self.atoms(&pt, &[]).iter().any(|b| match (&a.key, &b.key) {
                    (AtomKey::Region(..), AtomKey::Region(..)) => true,
                    (x, y) => x == y,
                });
                inside.then(any)
            }
        }
    }

    fn show_atom(&self, a: &Atom, args: &[String], in_union: bool) -> String {
        match &a.key {
            AtomKey::Opaque(k) => if in_union { k.clone() } else { "_".into() },
            AtomKey::Region(lo, hi) => {
                let full = *lo == ints::MIN && *hi == ints::MAX;
                match (*lo == ints::MIN, *hi == ints::MAX) {
                    _ if full => if in_union { "Int".into() } else { "_".into() },
                    _ if lo == hi => lo.to_string(),
                    (true, false) => format!("where it <= {hi}"),
                    (false, true) => format!("where it >= {lo}"),
                    _ => format!("where {lo} <= it && it <= {hi}"),
                }
            }
            AtomKey::Ctor(c) => {
                let n = args.len();
                let named = self.env.ctors.get(c).and_then(|t| self.env.variants_of(t, &[])).and_then(|vs| vs.into_iter().find(|v| &v.0 == c)).and_then(|v| v.1);
                match c.as_str() {
                    "::" => format!("[{}, ..]", args[0]),
                    "(,)" => format!("({})", args.join(", ")),
                    _ if n == 0 => c.clone(),
                    _ if args.iter().all(|a| a == "_") => c.clone(),
                    _ if named.is_some() => {
                        let ns = named.unwrap();
                        let parts: Vec<String> = ns.iter().zip(args).filter(|(_, a)| *a != "_").map(|(n, a)| format!("{n}: {a}")).collect();
                        format!("{c} {{ {}, .. }}", parts.join(", "))
                    }
                    _ => format!("{c}({})", args.join(", ")),
                }
            }
        }
    }

    /// A value (as pattern text, one per column) that no row matches, if any.
    fn missing(&mut self, rows: &[Vec<P>], tys: &[Ty], depth: usize) -> Option<Vec<String>> {
        if tys.is_empty() {
            return if rows.is_empty() { Some(vec![]) } else { None };
        }
        if depth > 12 {
            return None;
        }
        // only wildcards here: every value of this column behaves the same
        if rows.iter().all(|r| matches!(r[0], P::Any)) {
            let def: Vec<Vec<P>> = rows.iter().map(|r| r[1..].to_vec()).collect();
            return self.missing(&def, &tys[1..], depth + 1).map(|w| [vec!["_".to_string()], w].concat());
        }
        let t0 = self.resolve(&tys[0]);
        let in_union = matches!(t0, Ty::Union(_));
        let col: Vec<P> = rows.iter().map(|r| r[0].clone()).collect();
        let atoms = self.atoms(&t0, &col.iter().collect::<Vec<_>>());
        for a in atoms {
            let n = a.args.len();
            let spec: Vec<Vec<P>> = rows.iter().filter_map(|r| self.covers(&r[0], &a).map(|subs| [subs, r[1..].to_vec()].concat())).collect();
            let tys2 = [a.args.clone(), tys[1..].to_vec()].concat();
            if let Some(w) = self.missing(&spec, &tys2, depth + 1) {
                let (args, rest) = w.split_at(n);
                let shown = self.show_atom(&a, args, in_union);
                return Some([vec![shown], rest.to_vec()].concat());
            }
        }
        None
    }

    /// Whether the row `q` matches some value that none of `rows` matches.
    fn useful(&mut self, rows: &[Vec<P>], q: &[P], tys: &[Ty], depth: usize) -> bool {
        if tys.is_empty() {
            return rows.is_empty();
        }
        if depth > 12 || matches!(q[0], P::Lit) {
            return true;
        }
        if matches!(q[0], P::Any) && rows.iter().all(|r| matches!(r[0], P::Any)) {
            let def: Vec<Vec<P>> = rows.iter().map(|r| r[1..].to_vec()).collect();
            return self.useful(&def, &q[1..], &tys[1..], depth + 1);
        }
        let mut col: Vec<P> = rows.iter().map(|r| r[0].clone()).collect();
        col.push(q[0].clone());
        let atoms = self.atoms(&tys[0], &col.iter().collect::<Vec<_>>());
        for a in atoms {
            let Some(qs) = self.covers(&q[0], &a) else {
                // part of an opaque type (a `Log` among a trait's values): it may match something
                if matches!(a.key, AtomKey::Opaque(_)) && !matches!(q[0], P::Any) {
                    return true;
                }
                continue;
            };
            let spec: Vec<Vec<P>> = rows.iter().filter_map(|r| self.covers(&r[0], &a).map(|subs| [subs, r[1..].to_vec()].concat())).collect();
            let tys2 = [a.args.clone(), tys[1..].to_vec()].concat();
            if self.useful(&spec, &[qs, q[1..].to_vec()].concat(), &tys2, depth + 1) {
                return true;
            }
        }
        false
    }

    fn check_matches(&mut self) {
        let ms = std::mem::take(&mut self.matches);
        for (t, arms) in ms {
            let rows: Vec<Vec<P>> = arms.iter().map(|(p, _)| vec![p.clone()]).collect();
            for (i, (_, text)) in arms.iter().enumerate() {
                if !self.useful(&rows[..i], &rows[i], &[t.clone()], 0) {
                    self.err(format!("unreachable match arm `{text}`: the arms above already cover it"));
                }
            }
            if let Some(w) = self.missing(&rows, &[t.clone()], 0) {
                self.err(format!("non-exhaustive match on `{}`: `{}` is not covered", self.show(&t), w[0]));
            }
        }
    }
}

/// Sets of integers as sorted, disjoint, inclusive intervals (in i128 so that
/// the bounds of i64 never overflow).
pub mod ints {
    use crate::ast::{BinOp, Expr, UnOp};
    pub type Ints = Vec<(i128, i128)>;
    pub const MIN: i128 = i64::MIN as i128;
    pub const MAX: i128 = i64::MAX as i128;

    pub fn point(n: i128) -> Ints {
        vec![(n, n)]
    }
    fn norm(mut v: Ints) -> Ints {
        v.retain(|(a, b)| a <= b);
        v.sort();
        let mut out: Ints = Vec::new();
        for (a, b) in v {
            match out.last_mut() {
                Some((_, hi)) if a <= *hi + 1 => *hi = (*hi).max(b),
                _ => out.push((a, b)),
            }
        }
        out
    }
    pub fn or(a: &Ints, b: &Ints) -> Ints {
        norm([a.clone(), b.clone()].concat())
    }
    pub fn not(a: &Ints) -> Ints {
        let mut out = Vec::new();
        let mut lo = MIN;
        for (x, y) in a {
            if *x > lo {
                out.push((lo, x - 1));
            }
            lo = y + 1;
        }
        if lo <= MAX {
            out.push((lo, MAX));
        }
        out
    }
    pub fn and(a: &Ints, b: &Ints) -> Ints {
        not(&or(&not(a), &not(b)))
    }
    pub fn contains(s: &Ints, lo: i128, hi: i128) -> bool {
        s.iter().any(|(a, b)| *a <= lo && hi <= *b)
    }
    /// The integers `it` for which a condition holds, when it only compares
    /// `it` with constants (combined with `&&`, `||`, `!`, `==>`).
    pub fn of_pred(e: &Expr) -> Option<Ints> {
        let konst = |e: &Expr| match e {
            Expr::Int(n) => Some(*n as i128),
            Expr::Un(UnOp::Neg, x) => match &**x {
                Expr::Int(n) => Some(-(*n as i128)),
                _ => None,
            },
            _ => None,
        };
        let is_it = |e: &Expr| matches!(e, Expr::Var(x) if x == "it");
        Some(match e {
            Expr::Bool(true) => vec![(MIN, MAX)],
            Expr::Bool(false) => vec![],
            Expr::Un(UnOp::Not, x) => not(&of_pred(x)?),
            Expr::Bin(BinOp::And, l, r) => and(&of_pred(l)?, &of_pred(r)?),
            Expr::Bin(BinOp::Or, l, r) => or(&of_pred(l)?, &of_pred(r)?),
            Expr::Bin(BinOp::Implies, l, r) => or(&not(&of_pred(l)?), &of_pred(r)?),
            Expr::Bin(op, l, r) => {
                use BinOp::*;
                let (op, c) = if is_it(l) {
                    (*op, konst(r)?)
                } else if is_it(r) {
                    let flipped = match op {
                        Lt => Gt,
                        Le => Ge,
                        Gt => Lt,
                        Ge => Le,
                        o => *o,
                    };
                    (flipped, konst(l)?)
                } else {
                    return None;
                };
                match op {
                    Lt => vec![(MIN, c - 1)],
                    Le => vec![(MIN, c)],
                    Gt => vec![(c + 1, MAX)],
                    Ge => vec![(c, MAX)],
                    Eq => point(c),
                    Ne => not(&point(c)),
                    _ => return None,
                }
            }
            _ => return None,
        })
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
    };
    let vars: HashMap<String, Ty> = f.type_params.iter().map(|p| (p.clone(), c.fresh())).collect();
    let bounded: Vec<&Bound> = f.bounds.iter().collect();
    // candidate types per bounded parameter: implementations first, then a mock
    let mut cands: Vec<Vec<TypeExpr>> = Vec::new();
    for b in &bounded {
        let first = b.traits.first().map(texpr_head).unwrap_or_default();
        let mut cs: Vec<TypeExpr> = env.impls.iter().filter(|i| i.trait_name == first).map(|i| i.for_type.clone()).collect();
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
    let (env, mut errors) = Env::new(prog);
    errors.splice(0..0, resolved.errors);
    let mut holes = Vec::new();
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
        errors.extend(c.errors);
    }
    CheckReport { errors, holes }
}
