//! SMT verification: a function's contracts become proof obligations that an
//! SMT solver (Z3) proves or refutes.
//!
//! The body is executed symbolically. Values are SMT terms (`Int`, `Bool`,
//! `String`), records and sums are kept as structures of terms (a sum is a tag
//! plus the payload of every constructor), and lists are tracked by their
//! length only. A branch splits the path condition and the two results are
//! merged with `ite`. A loop forgets what it changes: those variables become
//! fresh values that keep only what their types guarantee (loop invariants
//! come later).
//!
//! Every runtime check of the interpreter is an obligation: a callee's
//! parameter refinements and `requires`, constructor and field refinements,
//! annotated `let`s, `assert`, division by zero, and the function's own result
//! type and `ensures`. Calls are checked against the callee's contract, never
//! its body: the result is a fresh value about which only the contract is
//! assumed.
//!
//! Anything not modelled (floats, dicts, lambdas, most list contents) becomes
//! a fresh unconstrained value. That is sound (it can only make a proof fail),
//! and the reason is remembered, so an unproven obligation says why.
//!
//! Integers are mathematical integers, as `Int` is meant to be (a big integer
//! when it needs to be). Division and modulo are Euclidean, like SMT-LIB's
//! `div` and `mod` and like the interpreter.

use crate::ast::*;
use crate::interp::{Fields, Value};
use crate::types::{subst_texpr, Env};
use crate::fx::{HashMap, HashSet};
use std::io::Write;
use std::process::{Command, Stdio};

/// An SMT-LIB term.
type T = String;

#[derive(Clone, Debug)]
pub enum Val {
    Int(T),
    Bool(T),
    Str(T),
    Unit,
    Tuple(Vec<Val>),
    /// A record value: type name and fields in declaration order.
    Rec(String, Vec<(String, Val)>),
    /// A value of a sum type: which constructor (`tag`, an index), and the
    /// payload of every constructor (only the active one means anything).
    Sum { ty: String, args: Vec<TypeExpr>, tag: T, ctors: Vec<(String, Vec<Val>)> },
    /// A value of a union type (`Square | Circle`): which member shape it has
    /// (`tag`, an index into `slots`), and a value for each shape (only the
    /// active one means anything; `Absent` for a shape it can't have). Shapes
    /// are named by `val_key` / `Vc::type_key` and are pairwise disjoint, so
    /// the tag is a function of the value, as in the interpreter, where a
    /// union value is just the member's value.
    Union { tag: T, slots: Vec<(String, Val)> },
    /// A list: its length, and `id`, which says how its elements are made and
    /// what is known of every one of them (`Vc::lists`).
    List { len: T, id: usize },
    Distinct(String, Box<Val>),
    /// The payload of a constructor that is not the active one.
    Absent,
    /// Any value: something not modelled, with the reason.
    Opaque(String),
    /// A function value of a known type: what it does is unknown, but what
    /// it returns is a valid value of `ret`, as any value of that type is.
    FnOf { ret: TypeExpr },
}

/// A function applied to list elements (`map`, `filter`, `all`, `find`),
/// kept so that it can be applied to any element later.
#[derive(Clone, Debug)]
pub enum Func {
    /// `x -> body`, with the variables in scope where it was written.
    Lambda { param: String, body: Expr, scopes: Vec<HashMap<String, Val>> },
    /// A named function or trait method.
    Named(String),
    /// "is a valid value of this type".
    Valid(TypeExpr),
}

/// How an arbitrary element of a list is made: each list's elements are
/// described by where the list came from.
#[derive(Clone, Debug)]
pub enum Gen {
    /// Nothing is known of the elements.
    Unknown,
    /// Valid values of a type (a list that crossed a checked boundary).
    Type(TypeExpr),
    /// One of these values (a list literal).
    Values(Vec<Val>),
    /// An integer in `lo..hi` (`range`).
    Range(T, T),
    /// An element of another list (`rest`, `take`, `drop`, `sort`, ...).
    Sub(usize),
    /// An element of another list that satisfies a function (`filter`).
    Filter(usize, Func),
    /// A function of an element of another list (`map`).
    Map(usize, Func),
    /// An element of the first list when the term holds, else of the second
    /// (`a + b` with a free choice, or two branches merged).
    Either(T, usize, usize),
}

/// What the prover knows about the elements of one list.
#[derive(Clone, Debug)]
struct ListInfo {
    source: Gen,
    /// Facts about every element, each holding when its guard does
    /// (`all(xs, p)` known true, or the list checked against a type).
    preds: Vec<(T, Func)>,
    /// Elements made so far, with the condition under which each exists, so
    /// that facts learnt later reach them too.
    seen: Vec<(T, Val)>,
}

/// One `while` loop, for the termination check.
#[derive(Clone, Debug)]
pub struct LoopInfo {
    /// The loop's condition, as written.
    pub what: String,
    /// Candidate bounds and their obligations ("goes down on every pass").
    pub candidates: Vec<(String, usize)>,
    /// Every path through the body leaves the loop.
    pub always_exits: bool,
    /// Why no bound can be checked, if so.
    pub reason: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Obligation {
    /// What is checked, e.g. "argument `amount` of `withdraw`".
    pub what: String,
    goal: T,
    pc: Vec<T>,
    /// How many facts were known when the obligation was made.
    facts: usize,
    /// A termination check rather than a contract.
    pub termination: bool,
}

/// How many times `check_type` enters the same named type, one inside the
/// other, before it stops (a recursive type's inner levels are unknown).
const MAX_TYPE_NESTING: usize = 2;

pub struct Vc<'a> {
    env: &'a Env,
    decls: Vec<(String, &'static str)>,
    /// Facts about fresh values (each guarded by the path it was made on).
    facts: Vec<T>,
    /// Why a fresh constant is unconstrained (it stands for something not modelled).
    reasons: HashMap<String, String>,
    /// While shaping a value, the reason its constants are unknown.
    reason: Option<String>,
    pc: Vec<T>,
    scopes: Vec<HashMap<String, Val>>,
    pub obligations: Vec<Obligation>,
    /// Inside a contract or refinement: evaluate, but make no obligations.
    spec: usize,
    /// The current path ended (`break` / `continue`).
    dead: bool,
    depth: usize,
    /// The named types whose values `check_type` is checking, outermost first.
    checking: Vec<String>,
    /// Inside an obligation's goal (not a fact to assume): see `check_type`.
    proving: usize,
    /// The validity of a value at a recursive type's cutoff, by type and
    /// value: one unknown for each, so a result assumed valid is the same
    /// atom when it is checked again.
    cutoffs: HashMap<String, T>,
    /// The parameters' values, to rebuild a counterexample from a model.
    pub params: Vec<(String, Val)>,
    /// Results of earlier calls, by callee and arguments: calls are
    /// deterministic, so the same call gives the same result.
    memo: HashMap<String, Val>,
    pub loops: Vec<LoopInfo>,
    /// Each recursive call: the path it is made on and its arguments, to
    /// check candidate measures against (`measure_check`).
    pub rec_sites: Vec<(Vec<T>, Vec<Val>)>,
    self_name: String,
    /// Functions whose bodies contracts may see through (`termination.rs`);
    /// `true` if their termination is only assumed.
    pub transparent: HashMap<String, bool>,
    /// Transparent functions unfolded on the strength of `terminates`.
    pub relied_on: std::collections::BTreeSet<String>,
    /// Early returns (`return e`, `e!`, `e?`): the path and the value.
    returns: Vec<(Vec<T>, Val)>,
    /// Declared types of variables (`let x: T`, `mut p: T`): they hold for
    /// the variable's whole life, so every assignment is checked against
    /// them. Kept by name; a shadowed name keeps all its declarations, which
    /// can only make checks stricter.
    declared: HashMap<String, Vec<TypeExpr>>,
    /// Each `matches(s, re)` made: the string, the pattern, its value, and
    /// whether the pattern was approximated (to relate patterns on one string).
    matched: Vec<(T, String, T, bool)>,
    /// Every list value's elements, by `Val::List::id`.
    lists: Vec<ListInfo>,
    /// Making an element under a quantifier (`every`): it is not a real
    /// element, so facts are not attached to it later.
    binding: usize,
    /// The functions passed to the builtin being called, by argument.
    fn_args: Vec<Option<Func>>,
}

fn and(ts: &[T]) -> T {
    let ts: Vec<&T> = ts.iter().filter(|t| *t != "true").collect();
    match ts.len() {
        0 => "true".into(),
        1 => ts[0].clone(),
        _ => format!("(and {})", ts.iter().map(|t| t.as_str()).collect::<Vec<_>>().join(" ")),
    }
}
fn or(ts: &[T]) -> T {
    match ts.len() {
        0 => "false".into(),
        1 => ts[0].clone(),
        _ => format!("(or {})", ts.join(" ")),
    }
}
fn not(t: &str) -> T {
    match t {
        "true" => "false".into(),
        "false" => "true".into(),
        _ => format!("(not {t})"),
    }
}
fn implies(a: &str, b: &str) -> T {
    if a == "true" { b.to_string() } else if b == "true" { "true".into() } else { format!("(=> {a} {b})") }
}
fn ite(c: &str, a: &str, b: &str) -> T {
    if a == b { a.to_string() } else { format!("(ite {c} {a} {b})") }
}
fn num(n: i64) -> T {
    if n < 0 { format!("(- {})", n.unsigned_abs()) } else { n.to_string() }
}
/// The runtime shape of a value, as far as it decides membership in a union
/// (the interpreter's `shape_fits`): two values with different keys have
/// different shapes. `List*` is any collection (lists, dicts and sets are all
/// `Val::List`). `None` when the shape is not known.
fn val_key(v: &Val) -> Option<String> {
    Some(match v {
        Val::Int(_) => "Int".into(),
        Val::Bool(_) => "Bool".into(),
        Val::Str(_) => "String".into(),
        Val::Unit => "Unit".into(),
        Val::Tuple(xs) => format!("Tuple/{}", xs.len()),
        Val::Rec(n, _) => format!("rec:{n}"),
        Val::Distinct(n, _) => format!("distinct:{n}"),
        Val::Sum { ty, .. } => format!("sum:{ty}"),
        Val::List { .. } => "List*".into(),
        Val::Union { .. } | Val::Absent | Val::Opaque(_) | Val::FnOf { .. } => return None,
    })
}

/// A value as a union: its tag and slots (a plain value is a one-slot union).
fn slots_of(v: &Val) -> Option<(T, Vec<(String, Val)>)> {
    match v {
        Val::Union { tag, slots } => Some((tag.clone(), slots.clone())),
        _ => Some(("0".into(), vec![(val_key(v)?, v.clone())])),
    }
}

/// Can an SMT string hold `s` (every code point at most `MAX_CHAR`)?
fn smt_string(s: &str) -> bool {
    s.chars().all(|c| c as u32 <= MAX_CHAR)
}
const BEYOND: &str = "a string with characters past U+2FFFF, which SMT strings can't hold";

/// An SMT-LIB string literal. Only for strings `smt_string` accepts: past
/// U+2FFFF, `\u{...}` is not an escape (Z3 reads it as its 9 characters).
pub fn str_lit(s: &str) -> T {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\"\""),
            ' '..='~' if c != '\\' => out.push(c),
            _ => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
        }
    }
    out.push('"');
    out
}

impl<'a> Vc<'a> {
    pub fn new(env: &'a Env) -> Self {
        Vc {
            env,
            decls: vec![],
            facts: vec![],
            reasons: HashMap::default(),
            reason: None,
            pc: vec![],
            scopes: vec![HashMap::default()],
            obligations: vec![],
            spec: 0,
            dead: false,
            depth: 0,
            checking: vec![],
            proving: 0,
            cutoffs: HashMap::default(),
            params: vec![],
            memo: HashMap::default(),
            loops: vec![],
            rec_sites: vec![],
            self_name: String::new(),
            transparent: HashMap::default(),
            relied_on: Default::default(),
            returns: vec![],
            declared: HashMap::default(),
            matched: vec![],
            lists: vec![],
            binding: 0,
            fn_args: vec![],
        }
    }

    fn fresh(&mut self, sort: &'static str) -> T {
        let n = format!("v{}", self.decls.len());
        self.decls.push((n.clone(), sort));
        if let Some(r) = &self.reason {
            self.reasons.insert(n.clone(), r.clone());
        }
        n
    }
    fn fresh_why(&mut self, sort: &'static str, why: &str) -> T {
        let old = self.reason.replace(why.to_string());
        let t = self.fresh(sort);
        self.reason = old;
        t
    }
    fn assume(&mut self, t: T) {
        if t != "true" {
            let f = implies(&and(&self.pc), &t);
            self.facts.push(f);
        }
    }

    /// Record an obligation built by `goal` (which itself makes none).
    fn oblige(&mut self, what: String, goal: impl FnOnce(&mut Self) -> T) {
        if self.spec > 0 || self.dead {
            return;
        }
        self.spec += 1;
        self.proving += 1;
        let g = goal(self);
        self.proving -= 1;
        self.spec -= 1;
        if g == "true" {
            return;
        }
        self.obligations.push(Obligation { what, goal: g.clone(), pc: self.pc.clone(), facts: self.facts.len(), termination: false });
        // past a check, execution continues only if it held
        self.pc.push(g);
    }

    /// A termination obligation (never assumed afterwards); its index.
    fn oblige_termination(&mut self, what: String, goal: T) -> usize {
        self.obligations.push(Obligation { what, goal, pc: self.pc.clone(), facts: self.facts.len(), termination: true });
        self.obligations.len() - 1
    }

    // ---- lists: how their elements are made, and what holds for all of them ----

    fn new_list(&mut self, len: T, gen_: Gen) -> Val {
        self.lists.push(ListInfo { source: gen_, preds: vec![], seen: vec![] });
        Val::List { len, id: self.lists.len() - 1 }
    }

    /// The list a value is (an unknown list for a value that isn't one).
    fn list_id(&mut self, v: &Val) -> usize {
        match v {
            Val::List { id, .. } => *id,
            _ => {
                self.lists.push(ListInfo { source: Gen::Unknown, preds: vec![], seen: vec![] });
                self.lists.len() - 1
            }
        }
    }

    /// The type every element of a list is known to have, if one is.
    fn elem_type(&self, id: usize) -> Option<TypeExpr> {
        match &self.lists[id].source {
            Gen::Type(t) => Some(t.clone()),
            Gen::Sub(p) | Gen::Filter(p, _) => self.elem_type(*p),
            _ => None,
        }
    }

    /// Is every element of a list known (unconditionally) to be a valid `t`?
    fn valid_known(&self, id: usize, t: &TypeExpr) -> bool {
        let info = &self.lists[id];
        let same = |t2: &TypeExpr| t2 == t || self.erased(t2) == self.erased(t);
        if info.preds.iter().any(|(g, f)| g == "true" && matches!(f, Func::Valid(t2) if same(t2))) {
            return true;
        }
        match &info.source {
            Gen::Type(t2) => same(t2),
            Gen::Sub(p) | Gen::Filter(p, _) => self.valid_known(*p, t),
            Gen::Either(_, a, b) => self.valid_known(*a, t) && self.valid_known(*b, t),
            // `[]`: no element to be invalid
            Gen::Values(vs) if vs.is_empty() => true,
            _ => false,
        }
    }

    /// The value of a function applied to `x`, as known in a contract.
    fn apply(&mut self, f: &Func, x: Val) -> Val {
        match f {
            Func::Lambda { param, body, scopes } => {
                let saved = std::mem::replace(&mut self.scopes, scopes.clone());
                self.scopes.push([(param.clone(), x)].into_iter().collect());
                let returns = std::mem::take(&mut self.returns);
                let dead = std::mem::replace(&mut self.dead, false);
                self.spec += 1;
                let v = self.eval(body);
                self.spec -= 1;
                self.dead = dead;
                self.returns = returns;
                self.scopes = saved;
                v
            }
            Func::Named(n) => {
                self.spec += 1;
                let v = self.call(n, &[Expr::Unit], vec![x]);
                self.spec -= 1;
                v
            }
            Func::Valid(t) => {
                self.spec += 1;
                let c = self.check_type(t, &x, &[]);
                self.spec -= 1;
                Val::Bool(c)
            }
        }
    }

    /// An element of list `id`, which exists when `exists` holds: what is
    /// known of it is assumed under that condition (an element of an empty
    /// list must not make the path contradictory).
    fn element(&mut self, id: usize, exists: &str) -> Val {
        let n = self.pc.len();
        self.pc.push(exists.to_string());
        let x = self.element_inner(id);
        self.pc.truncate(n);
        x
    }

    fn element_inner(&mut self, id: usize) -> Val {
        let x = match self.lists[id].source.clone() {
            Gen::Unknown => Val::Opaque("the elements of a list are not tracked".into()),
            Gen::Type(t) => self.havoc(&t),
            Gen::Values(vs) if vs.is_empty() => {
                // a list literal `[]` has no element: this one doesn't exist
                self.assume("false".into());
                Val::Opaque("an element of an empty list".into())
            }
            Gen::Values(vs) => {
                let k = self.fresh("Int");
                self.assume(format!("(and (<= 0 {k}) (< {k} {}))", vs.len()));
                let mut v = vs.last().unwrap().clone();
                for i in (0..vs.len() - 1).rev() {
                    v = self.merge(&format!("(= {k} {i})"), &vs[i], &v);
                }
                v
            }
            Gen::Range(lo, hi) => {
                let x = self.fresh("Int");
                self.assume(format!("(and (<= {lo} {x}) (< {x} {hi}))"));
                Val::Int(x)
            }
            Gen::Sub(p) => self.element_inner(p),
            Gen::Filter(p, f) => {
                let x = self.element_inner(p);
                let c = self.apply(&f, x.clone());
                let c = self.as_bool(&c);
                self.assume(c);
                x
            }
            Gen::Map(p, f) => {
                let y = self.element_inner(p);
                self.apply(&f, y)
            }
            // one side is `[]`: the element is from the other one
            Gen::Either(c, a, b) if matches!(&self.lists[a].source, Gen::Values(vs) if vs.is_empty()) => {
                self.assume(not(&c));
                self.element_inner(b)
            }
            Gen::Either(c, a, b) if matches!(&self.lists[b].source, Gen::Values(vs) if vs.is_empty()) => {
                self.assume(c.clone());
                self.element_inner(a)
            }
            Gen::Either(c, a, b) => {
                let n = self.pc.len();
                self.pc.push(c.clone());
                let ea = self.element_inner(a);
                self.pc.truncate(n);
                self.pc.push(not(&c));
                let eb = self.element_inner(b);
                self.pc.truncate(n);
                self.merge(&c, &ea, &eb)
            }
        };
        // what is known of every element of this list
        for (g, f) in self.lists[id].preds.clone() {
            let p = self.apply(&f, x.clone());
            let p = self.as_bool(&p);
            self.assume(implies(&g, &p));
        }
        if self.binding == 0 {
            let exists = and(&self.pc);
            self.lists[id].seen.push((exists, x.clone()));
        }
        x
    }

    /// From now on, `f` holds for every element of list `id` when `guard`
    /// does: for the elements made so far, and for every one made later.
    fn know_all(&mut self, id: usize, guard: T, f: Func) {
        for (exists, x) in self.lists[id].seen.clone() {
            let p = self.apply(&f, x);
            let p = self.as_bool(&p);
            self.facts.push(implies(&and(&[exists, guard.clone()]), &p));
        }
        self.lists[id].preds.push((guard, f));
    }

    /// "Every element of the list satisfies what `prop` says of `x`", as a
    /// formula: for any value made the way the list's elements are (any
    /// value of the generator, an over-approximation of the elements),
    /// `prop` holds. The quantifier sits where the solver only has to find
    /// one counterexample element (a skolem), so it stays decidable. `None`
    /// when `prop` gives up on the element.
    fn every(&mut self, id: usize, prop: impl FnOnce(&mut Self, Val) -> Option<T>) -> Option<T> {
        let (nd, nf, nm) = (self.decls.len(), self.facts.len(), self.matched.len());
        self.binding += 1;
        self.spec += 1;
        let x = self.element(id, "true");
        let p = prop(self, x);
        self.spec -= 1;
        self.binding -= 1;
        let ant = and(&self.facts[nf..].to_vec());
        self.facts.truncate(nf);
        self.matched.truncate(nm);
        let p = p?;
        let bound: Vec<String> = self.decls[nd..].iter().map(|(n, s)| format!("({n} {s})")).collect();
        let body = implies(&ant, &p);
        Some(if bound.is_empty() { body } else { format!("(forall ({}) {body})", bound.join(" ")) })
    }

    /// `all(list, f)`: a Bool that is true when every element satisfies `f`
    /// (proven through `every`), and that, once known, gives `f` to every
    /// element the code looks at.
    fn all_of(&mut self, list: &Val, f: Func) -> T {
        let id = self.list_id(list);
        let len = self.list_len(list);
        let b = self.fresh("Bool");
        self.facts.push(format!("(=> (= {len} 0) {b})"));
        let f2 = f.clone();
        if let Some(q) = self.every(id, |vc, x| {
            let p = vc.apply(&f2, x);
            Some(vc.as_bool(&p))
        }) {
            self.facts.push(implies(&q, &b));
        }
        self.know_all(id, b.clone(), f);
        b
    }

    /// The function an argument expression denotes, if it can be kept.
    fn func_of(&self, e: &Expr) -> Option<Func> {
        match e {
            Expr::Lambda(ps, body) if ps.len() == 1 => Some(Func::Lambda { param: ps[0].name.to_string(), body: (**body).clone(), scopes: self.scopes.clone() }),
            Expr::Var(n) if self.is_fn(e) => Some(Func::Named(n.to_string())),
            _ => None,
        }
    }

    // ---- coercions: an opaque value used as an Int is some Int ----

    /// A union value used as an `Int` (`Bool`, `String`): its member of that
    /// shape when that member is the active one, any value otherwise.
    fn union_as(&mut self, v: &Val, key: &str, sort: &'static str) -> Option<T> {
        let Val::Union { tag, slots } = v else { return None };
        let i = slots.iter().position(|s| s.0 == key && !matches!(s.1, Val::Absent))?;
        let inner = match (&slots[i].1, sort) {
            (Val::Int(t), "Int") | (Val::Bool(t), "Bool") | (Val::Str(t), "String") => t.clone(),
            (Val::List { len, .. }, "Len") => len.clone(),
            _ => return None,
        };
        let other = self.fresh_why(if sort == "Len" { "Int" } else { sort }, "a union value used as one of its members");
        Some(ite(&format!("(= {tag} {i})"), &inner, &other))
    }

    fn as_int(&mut self, v: &Val) -> T {
        if let Some(t) = self.union_as(v, "Int", "Int") {
            return t;
        }
        match v {
            Val::Int(t) => t.clone(),
            Val::Opaque(r) => self.fresh_why("Int", &r.clone()),
            _ => self.fresh_why("Int", "a value of an unexpected shape"),
        }
    }
    fn as_bool(&mut self, v: &Val) -> T {
        if let Some(t) = self.union_as(v, "Bool", "Bool") {
            return t;
        }
        match v {
            Val::Bool(t) => t.clone(),
            Val::Opaque(r) => self.fresh_why("Bool", &r.clone()),
            _ => self.fresh_why("Bool", "a value of an unexpected shape"),
        }
    }
    fn as_str(&mut self, v: &Val) -> T {
        if let Some(t) = self.union_as(v, "String", "String") {
            return t;
        }
        match v {
            Val::Str(t) => t.clone(),
            Val::Opaque(r) => self.fresh_why("String", &r.clone()),
            _ => self.fresh_why("String", "a value of an unexpected shape"),
        }
    }
    fn list_len(&mut self, v: &Val) -> T {
        if let Some(t) = self.union_as(v, "List*", "Len") {
            // `len` of a union with a string member is not this
            if !matches!(v, Val::Union { slots, .. } if slots.iter().any(|s| s.0 == "String")) {
                self.facts.push(format!("(>= {t} 0)"));
                return t;
            }
        }
        match v {
            Val::List { len, .. } => len.clone(),
            Val::Opaque(r) => {
                let l = self.fresh_why("Int", &r.clone());
                self.facts.push(format!("(>= {l} 0)"));
                l
            }
            _ => self.fresh_why("Int", "a value of an unexpected shape"),
        }
    }
    fn unknown_bool(&mut self, why: &str) -> T {
        self.fresh_why("Bool", why)
    }

    // ---- values of a type ----

    /// A value of type `t` with fresh constants, nothing assumed.
    fn shape(&mut self, t: &TypeExpr, depth: usize) -> Val {
        match t {
            TypeExpr::Union(ms) => self.union_shape(ms, depth).unwrap_or_else(|| Val::Opaque("values of this union type are not modelled (members of overlapping or unknown shapes)".into())),
            TypeExpr::Named(n, args) if self.env.variant_parent(n.sym()).is_some() => {
                // a variant as a type: a value of its sum with that tag
                let parent = self.env.variant_parent(n.sym()).unwrap();
                let v = self.shape(&TypeExpr::Named(Symbol::from(parent), args.clone()), depth);
                if let (Val::Sum { tag, ctors, .. }, true) = (&v, true) {
                    if let Some(k) = ctors.iter().position(|c| &c.0 == n) {
                        self.facts.push(format!("(= {tag} {k})"));
                    }
                }
                v
            }
            TypeExpr::Refined(b, _) => self.shape(b, depth),
            TypeExpr::Unit => Val::Unit,
            TypeExpr::Fn(_, ret) => Val::FnOf { ret: (**ret).clone() },
            TypeExpr::Named(n, args) => match n.as_str() {
                "Int" => Val::Int(self.fresh("Int")),
                "Bool" => Val::Bool(self.fresh("Bool")),
                "String" => Val::Str(self.fresh("String")),
                "Float" => Val::Opaque("floats are not modelled yet".into()),
                "List" => {
                    let len = self.fresh("Int");
                    self.facts.push(format!("(>= {len} 0)"));
                    let gen_ = args.first().cloned().map(Gen::Type).unwrap_or(Gen::Unknown);
                    self.new_list(len, gen_)
                }
                "Tuple" => Val::Tuple(args.iter().map(|a| self.shape(a, depth)).collect()),
                // dicts and sets are tracked by their size
                "Dict" | "Set" => {
                    let len = self.fresh("Int");
                    self.facts.push(format!("(>= {len} 0)"));
                    self.new_list(len, Gen::Unknown)
                }
                _ => {
                    let Some(td) = self.env.types.get(&n.sym()).cloned() else {
                        return Val::Opaque(format!("values of the generic type `{n}` are not known"));
                    };
                    let sub: HashMap<Symbol, TypeExpr> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
                    match &td.body {
                        TypeBody::Alias(a) if td.distinct => Val::Distinct(n.to_string(), Box::new(self.shape(&subst_texpr(a, &sub), depth))),
                        TypeBody::Alias(a) => self.shape(&subst_texpr(a, &sub), depth),
                        _ if depth > 3 => Val::Opaque(format!("`{n}` is recursive; only its outer levels are modelled")),
                        TypeBody::Record(_) => {
                            let vs = self.env.variants_of(n, args).unwrap_or_default();
                            let (_, names, tys) = vs.into_iter().next().unwrap_or_default();
                            let names = names.unwrap_or_default();
                            Val::Rec(n.to_string(), names.into_iter().zip(tys.iter()).map(|(f, t)| (f.to_string(), self.shape(t, depth + 1))).collect())
                        }
                        TypeBody::Sum(_) => {
                            let vs = self.env.variants_of(n, args).unwrap_or_default();
                            let tag = self.fresh("Int");
                            self.facts.push(format!("(and (<= 0 {tag}) (< {tag} {}))", vs.len()));
                            let ctors = vs.iter().map(|(c, _, tys)| (c.to_string(), tys.iter().map(|t| self.shape(t, depth + 1)).collect())).collect();
                            Val::Sum { ty: n.to_string(), args: args.clone(), tag, ctors }
                        }
                    }
                }
            },
        }
    }

    // ---- unions ----

    /// The shape key of a type (see `val_key`), ignoring refinements; `None`
    /// for unions, functions, floats, type parameters and traits.
    fn type_key(&self, t: &TypeExpr) -> Option<String> {
        match self.env.expand_alias(t) {
            TypeExpr::Unit => Some("Unit".into()),
            TypeExpr::Named(n, args) => match n.as_str() {
                "Int" | "Bool" | "String" | "Unit" => Some(n.to_string()),
                "List" | "Dict" | "Set" => Some("List*".into()),
                "Tuple" => Some(format!("Tuple/{}", args.len())),
                _ => {
                    if let Some(p) = self.env.variant_parent(n.sym()) {
                        return Some(format!("sum:{p}"));
                    }
                    let td = self.env.types.get(&n.sym())?;
                    match &td.body {
                        TypeBody::Alias(_) if td.distinct => Some(format!("distinct:{n}")),
                        TypeBody::Record(_) => Some(format!("rec:{n}")),
                        TypeBody::Sum(_) => Some(format!("sum:{n}")),
                        TypeBody::Alias(_) => None,
                    }
                }
            },
            _ => None,
        }
    }

    /// The members of a union, looking through unrefined aliases of unions
    /// (`Shape | Int` with `type Shape = Square | Circle`).
    fn flatten_union(&self, t: &TypeExpr, out: &mut Vec<TypeExpr>) {
        match t {
            TypeExpr::Union(ms) => ms.iter().for_each(|m| self.flatten_union(m, out)),
            TypeExpr::Named(n, args) => match self.env.types.get(&n.sym()) {
                Some(td) if !td.distinct && td.refinement.is_none() && matches!(&td.body, TypeBody::Alias(TypeExpr::Union(_))) => {
                    let TypeBody::Alias(a) = &td.body else { unreachable!() };
                    let sub: HashMap<Symbol, TypeExpr> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
                    self.flatten_union(&subst_texpr(a, &sub), out)
                }
                _ => out.push(t.clone()),
            },
            _ => out.push(t.clone()),
        }
    }

    /// A value of a union type: one slot per member shape. `None` when two
    /// members could have the same shape in a way the slots can't tell apart
    /// (`List[Int] | List[String]`), or a member's shape is not known.
    fn union_shape(&mut self, ms: &[TypeExpr], depth: usize) -> Option<Val> {
        let mut members = vec![];
        for m in ms {
            self.flatten_union(m, &mut members);
        }
        let mut groups: Vec<(String, Vec<TypeExpr>)> = vec![];
        for m in members {
            let k = self.type_key(&m)?;
            match groups.iter_mut().find(|g| g.0 == k) {
                Some(g) => g.1.push(m),
                None => groups.push((k, vec![m])),
            }
        }
        let tag = self.fresh("Int");
        self.facts.push(format!("(and (<= 0 {tag}) (< {tag} {}))", groups.len()));
        let mut slots = vec![];
        for (i, (k, ms)) in groups.iter().enumerate() {
            let heads: Vec<TypeExpr> = ms.iter().map(|m| self.env.expand_alias(m)).collect();
            let v = if let Some(sum) = k.strip_prefix("sum:") {
                // the sum itself or some of its variants: a value of the sum,
                // whose constructor is one of those variants
                let mut args: Option<Vec<TypeExpr>> = None;
                let mut ctors: Option<Vec<String>> = Some(vec![]);
                for h in &heads {
                    let TypeExpr::Named(n, a) = h else { return None };
                    if args.get_or_insert_with(|| a.clone()) != a {
                        return None;
                    }
                    if n == sum {
                        ctors = None;
                    } else if let Some(cs) = &mut ctors {
                        cs.push(n.to_string());
                    }
                }
                let v = self.shape(&TypeExpr::Named(Symbol::from(sum), args.unwrap_or_default()), depth);
                if let (Some(cs), Val::Sum { tag: inner, ctors: all, .. }) = (&ctors, &v) {
                    let ks: Vec<T> = cs.iter().filter_map(|c| all.iter().position(|x| &x.0 == c)).map(|j| format!("(= {inner} {j})")).collect();
                    self.facts.push(implies(&format!("(= {tag} {i})"), &or(&ks)));
                }
                v
            } else if ms.len() == 1 {
                self.shape(&ms[0], depth)
            } else {
                match k.as_str() {
                    // `Positive | Negative`: both are integers
                    "Int" | "Bool" | "String" | "Unit" => self.shape(&TypeExpr::Named(Symbol::from(k.as_str()), vec![]), depth),
                    _ => return None,
                }
            };
            slots.push((k.clone(), v));
        }
        Some(Val::Union { tag, slots })
    }

    /// Does `v` have the shape of `t` (the interpreter's `shape_fits`,
    /// refinements ignored)? Exact where the shapes are known, else unknown.
    fn shape_test(&mut self, t: &TypeExpr, v: &Val) -> T {
        match v {
            Val::Opaque(r) => return self.unknown_bool(&r.clone()),
            Val::Absent => return "false".into(),
            Val::Union { tag, slots } => {
                let (tag, slots) = (tag.clone(), slots.clone());
                let cs: Vec<T> = slots
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| !matches!(s.1, Val::Absent))
                    .map(|(i, (_, s))| and(&[format!("(= {tag} {i})"), self.shape_test(t, s)]))
                    .collect();
                return or(&cs);
            }
            _ => {}
        }
        let te = self.env.expand_alias(t);
        match &te {
            TypeExpr::Union(ms) => {
                let cs: Vec<T> = ms.iter().map(|m| self.shape_test(m, v)).collect();
                or(&cs)
            }
            TypeExpr::Named(n, args) => match (n.as_str(), v) {
                ("List", Val::List { len, id }) => {
                    if args.is_empty() || self.elem_type(*id).as_ref() == args.first() {
                        "true".into()
                    } else {
                        let u = self.unknown_bool("the elements of a list are not tracked");
                        format!("(or (= {len} 0) {u})")
                    }
                }
                ("Dict" | "Set", Val::List { .. }) => self.unknown_bool("dicts and sets are not modelled yet"),
                ("Tuple", Val::Tuple(xs)) if xs.len() == args.len() => {
                    let cs: Vec<T> = args.iter().zip(xs).map(|(a, x)| self.shape_test(a, x)).collect();
                    and(&cs)
                }
                _ => match (self.type_key(&te), val_key(v)) {
                    (Some(a), Some(b)) if a != b => "false".into(),
                    (Some(_), Some(_)) => match (self.env.variant_parent(n.sym()), v) {
                        (Some(_), Val::Sum { tag, ctors, .. }) => match ctors.iter().position(|c| &c.0 == n) {
                            Some(k) => format!("(= {tag} {k})"),
                            None => "false".into(),
                        },
                        (Some(_), _) => self.unknown_bool("a value of an unexpected shape"),
                        (None, _) => "true".into(),
                    },
                    _ => self.unknown_bool("a type test that is not modelled"),
                },
            },
            TypeExpr::Unit => match val_key(v) {
                Some(k) => (k == "Unit").to_string(),
                None => self.unknown_bool("a value of an unknown shape"),
            },
            _ => self.unknown_bool("a type test that is not modelled"),
        }
    }

    /// Is `v` a value of `t` (the interpreter's `value_in`: the shape fits
    /// and every refinement holds)? This is what a type pattern and `is` test,
    /// and what makes a value valid for a union.
    fn in_type(&mut self, t: &TypeExpr, v: &Val, extra: &[(String, Val)]) -> T {
        match v {
            Val::Opaque(r) => self.unknown_bool(&r.clone()),
            Val::Absent => "false".into(),
            Val::Union { tag, slots } => {
                let (tag, slots) = (tag.clone(), slots.clone());
                let cs: Vec<T> = slots
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| !matches!(s.1, Val::Absent))
                    .map(|(i, (_, s))| {
                        let c = self.in_type(t, s, extra);
                        and(&[format!("(= {tag} {i})"), c])
                    })
                    .collect();
                or(&cs)
            }
            _ => {
                let s = self.shape_test(t, v);
                if s == "false" {
                    return s;
                }
                let union = matches!(self.env.expand_alias(t), TypeExpr::Union(_));
                if s != "true" && !union && (self.type_key(t).is_none() || val_key(v).is_none()) {
                    // the shape is unknown: the refinements can't be evaluated on it
                    return s;
                }
                let c = self.check_type(t, v, extra);
                and(&[s, c])
            }
        }
    }

    /// Is `t` a name for another type (a non-distinct alias)?
    fn is_alias(&self, t: &TypeExpr) -> bool {
        matches!(t, TypeExpr::Named(n, _) if matches!(self.env.types.get(&n.sym()), Some(td) if !td.distinct && matches!(td.body, TypeBody::Alias(_))))
    }

    /// Does `v` have constructor `c` (`v is Circle`)?
    fn is_ctor(&mut self, v: &Val, c: &str) -> T {
        match v {
            Val::Sum { tag, ctors, .. } => match ctors.iter().position(|k| k.0 == c) {
                Some(k) => format!("(= {tag} {k})"),
                None => "false".into(),
            },
            Val::Rec(n, _) | Val::Distinct(n, _) => (n == c).to_string(),
            Val::Opaque(r) => self.unknown_bool(&r.clone()),
            Val::Union { tag, slots } => {
                let (tag, slots) = (tag.clone(), slots.clone());
                let cs: Vec<T> = slots
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| !matches!(s.1, Val::Absent))
                    .map(|(i, (_, s))| {
                        let x = self.is_ctor(s, c);
                        and(&[format!("(= {tag} {i})"), x])
                    })
                    .collect();
                or(&cs)
            }
            _ => "false".into(),
        }
    }

    /// The shape a pattern needs, when it is a literal, tuple, list or
    /// constructor pattern (others test the value as a whole).
    fn pattern_key(&self, p: &Pattern) -> Option<String> {
        match p {
            Pattern::Int(_) => Some("Int".into()),
            Pattern::Bool(_) => Some("Bool".into()),
            Pattern::Str(_) => Some("String".into()),
            Pattern::Tuple(ps) => Some(format!("Tuple/{}", ps.len())),
            Pattern::List(..) => Some("List*".into()),
            Pattern::Ctor(c, _) => {
                let tn = self.env.ctors.get(&c.sym())?;
                let td = self.env.types.get(tn)?;
                match &td.body {
                    TypeBody::Alias(_) if td.distinct => Some(format!("distinct:{tn}")),
                    TypeBody::Record(_) => Some(format!("rec:{tn}")),
                    TypeBody::Sum(_) => Some(format!("sum:{tn}")),
                    TypeBody::Alias(_) => None,
                }
            }
            _ => None,
        }
    }

    /// The slots of a union value a pattern can match (the others make its
    /// condition `false`, so the arm's guard rules them out).
    fn arm_slots(&mut self, p: &Pattern, slots: &[(String, Val)]) -> Vec<usize> {
        let live = |i: &usize| !matches!(slots[*i].1, Val::Absent);
        match p {
            Pattern::Or(alts) => {
                let mut out: Vec<usize> = vec![];
                for a in alts {
                    for i in self.arm_slots(a, slots) {
                        if !out.contains(&i) {
                            out.push(i);
                        }
                    }
                }
                out.sort();
                out
            }
            Pattern::Ctor(c, CtorPat::None) if !self.env.ctors.contains_key(&c.sym()) && self.env.is_type_name(c) => {
                let t = TypeExpr::Named(c.clone(), vec![]);
                (0..slots.len()).filter(live).filter(|&i| self.shape_test(&t, &slots[i].1) != "false").collect()
            }
            _ => match self.pattern_key(p) {
                Some(k) => (0..slots.len()).filter(live).filter(|&i| slots[i].0 == k).collect(),
                None => (0..slots.len()).filter(live).collect(),
            },
        }
    }

    /// The field `f` of a value.
    fn field(&mut self, v: &Val, f: &str) -> Val {
        match v {
            Val::Rec(_, fs) => fs.iter().find(|x| x.0 == f).map(|x| x.1.clone()).unwrap_or(Val::Opaque(format!("field `{f}`"))),
            Val::Distinct(_, inner) if f == "value" => (**inner).clone(),
            Val::Tuple(xs) => f.parse::<usize>().ok().and_then(|i| xs.get(i).cloned()).unwrap_or(Val::Opaque(format!("field `{f}`"))),
            // a sum's field (a variant used as a type, or a field every variant has):
            // the field of whichever constructor is active
            Val::Sum { ty, args, tag, ctors } => {
                let vs = self.env.variants_of(ty, args).unwrap_or_default();
                let mut found: Vec<(usize, Val)> = vec![];
                for (k, (_, names, _)) in vs.iter().enumerate() {
                    if let Some(i) = names.as_ref().and_then(|ns| ns.iter().position(|n| n == f)) {
                        if let Some(x) = ctors.get(k).and_then(|c| c.1.get(i)) {
                            found.push((k, x.clone()));
                        }
                    }
                }
                let tag = tag.clone();
                match found.pop() {
                    None => Val::Opaque(format!("field `{f}`")),
                    Some((_, last)) => {
                        let mut v = last;
                        while let Some((k, x)) = found.pop() {
                            v = self.merge(&format!("(= {tag} {k})"), &x, &v);
                        }
                        v
                    }
                }
            }
            // a union's field (every member has it, or the value was narrowed
            // to members that have it): the field of the active member; any
            // value for a member that has no such field
            Val::Union { tag, slots } => {
                let (tag, slots) = (tag.clone(), slots.clone());
                let mut v = Val::Opaque(format!("field `{f}` of a union member without it"));
                for (i, (_, s)) in slots.iter().enumerate().rev() {
                    if matches!(s, Val::Absent) {
                        continue;
                    }
                    let x = self.field(s, f);
                    v = self.merge(&format!("(= {tag} {i})"), &x, &v);
                }
                v
            }
            Val::Opaque(r) => Val::Opaque(r.clone()),
            _ => Val::Opaque(format!("field `{f}` of a value of an unexpected shape")),
        }
    }

    /// Merge two values of different shapes into a union.
    fn merge_union(&mut self, c: &str, a: &Val, b: &Val) -> Option<Val> {
        let (ta, sa) = slots_of(a)?;
        let (tb, sb) = slots_of(b)?;
        let mut keys: Vec<String> = sa.iter().map(|s| s.0.clone()).collect();
        for (k, _) in &sb {
            if !keys.contains(k) {
                keys.push(k.clone());
            }
        }
        let find = |ss: &[(String, Val)], k: &str| ss.iter().find(|s| s.0 == k).map(|s| s.1.clone()).unwrap_or(Val::Absent);
        let slots: Vec<(String, Val)> = keys
            .iter()
            .map(|k| {
                let (x, y) = (find(&sa, k), find(&sb, k));
                (k.clone(), self.merge(c, &x, &y))
            })
            .collect();
        // `b`'s tag, renumbered to the merged slots
        let map: Vec<usize> = sb.iter().map(|s| keys.iter().position(|k| k == &s.0).unwrap()).collect();
        let tb = if map.iter().enumerate().all(|(j, &i)| i == j) {
            tb
        } else {
            let mut t = map.last().unwrap().to_string();
            for j in (0..map.len() - 1).rev() {
                t = ite(&format!("(= {tb} {j})"), &map[j].to_string(), &t);
            }
            t
        };
        Some(Val::Union { tag: ite(c, &ta, &tb), slots })
    }

    /// A fresh value of type `t` that satisfies `t`.
    fn havoc(&mut self, t: &TypeExpr) -> Val {
        let v = self.shape(t, 0);
        let c = self.check_type(t, &v, &[]);
        self.assume(c);
        v
    }

    /// A fresh value of the same type as `v`, keeping what types guarantee
    /// (records and sums are always valid; a plain variable may not be).
    fn refresh(&mut self, v: &Val) -> Val {
        match v {
            Val::Int(_) => Val::Int(self.fresh("Int")),
            Val::Bool(_) => Val::Bool(self.fresh("Bool")),
            Val::Str(_) => Val::Str(self.fresh("String")),
            Val::Unit => Val::Unit,
            Val::Tuple(xs) => Val::Tuple(xs.iter().map(|x| self.refresh(x)).collect()),
            Val::Rec(n, _) | Val::Distinct(n, _) => self.havoc(&TypeExpr::Named(Symbol::from(n.as_str()), vec![])),
            Val::Sum { ty, args, .. } => self.havoc(&TypeExpr::Named(Symbol::from(ty.as_str()), args.clone())),
            Val::List { .. } => {
                // any list: what a loop put in it is not known (a declared type is assumed separately)
                let len = self.fresh("Int");
                self.facts.push(format!("(>= {len} 0)"));
                self.new_list(len, Gen::Unknown)
            }
            Val::Union { slots, .. } => {
                let tag = self.fresh("Int");
                let live: Vec<T> = slots.iter().enumerate().filter(|(_, s)| !matches!(s.1, Val::Absent)).map(|(i, _)| format!("(= {tag} {i})")).collect();
                self.facts.push(or(&live));
                let slots = slots.iter().map(|(k, s)| (k.clone(), self.refresh(s))).collect();
                Val::Union { tag, slots }
            }
            Val::Absent => Val::Absent,
            Val::FnOf { .. } => v.clone(),
            Val::Opaque(r) => Val::Opaque(r.clone()),
        }
    }

    /// Does checking a value against `t` involve any refinement?
    fn refined(&self, t: &TypeExpr, depth: usize) -> bool {
        if depth > 8 {
            return true;
        }
        match t {
            TypeExpr::Refined(..) => true,
            TypeExpr::Unit | TypeExpr::Fn(..) => false,
            TypeExpr::Union(ms) => ms.iter().any(|m| self.refined(m, depth + 1)),
            TypeExpr::Named(n, args) if self.env.variant_parent(n.sym()).is_some() => self.refined(&TypeExpr::Named(Symbol::from(self.env.variant_parent(n.sym()).unwrap()), args.clone()), depth + 1),
            TypeExpr::Named(n, args) => {
                if args.iter().any(|a| self.refined(a, depth + 1)) {
                    return true;
                }
                let Some(td) = self.env.types.get(&n.sym()) else { return false };
                if td.refinement.is_some() {
                    return true;
                }
                match &td.body {
                    TypeBody::Alias(a) => self.refined(a, depth + 1),
                    TypeBody::Record(fs) => fs.iter().any(|f| self.refined(&f.ty, depth + 1)),
                    TypeBody::Sum(vs) => vs.iter().any(|v| match &v.payload {
                        Payload::None => false,
                        Payload::Positional(ts) => ts.iter().any(|t| self.refined(t, depth + 1)),
                        Payload::Record(fs) => fs.iter().any(|f| self.refined(&f.ty, depth + 1)),
                    }),
                }
            }
        }
    }

    /// `t` with the type arguments that add no refinement replaced by `_`.
    /// Validity is about refinements only (the checker has the rest), so a
    /// value valid at `Html[M]` for an unknown `M` is valid at `Html[_]`, and
    /// so is one at `Html[Msg]` when `Msg` has no refinement: comparing the
    /// erased types lets a generic callee's result type match its use.
    fn erased(&self, t: &TypeExpr) -> TypeExpr {
        match t {
            TypeExpr::Named(n, args) => TypeExpr::Named(
                n.clone(),
                args.iter().map(|a| if self.refined(a, 0) && !self.valid_by_construction(a) { self.erased(a) } else { TypeExpr::Named("_".into(), vec![]) }).collect(),
            ),
            TypeExpr::Refined(b, p) => TypeExpr::Refined(Box::new(self.erased(b)), p.clone()),
            TypeExpr::Union(ms) => TypeExpr::Union(ms.iter().map(|m| self.erased(m)).collect()),
            _ => t.clone(),
        }
    }

    /// A record or sum type of the program with no refinement of its own and
    /// no type parameters (`Msg`, even with `SetStep { step: Step }`): every
    /// value of it is valid, since making one checks its fields. As a type
    /// argument it adds nothing to check (`Html[Msg]` is `Html[_]`).
    fn valid_by_construction(&self, t: &TypeExpr) -> bool {
        let TypeExpr::Named(n, args) = t else { return false };
        let n = self.env.variant_parent(n.sym()).unwrap_or_else(|| n.clone());
        args.is_empty()
            && self.env.types.get(&n).is_some_and(|td| {
                td.params.is_empty() && td.refinement.is_none() && !td.distinct && matches!(td.body, TypeBody::Record(_) | TypeBody::Sum(_))
            })
    }

    /// The condition for `v` to be a valid value of `t`. `extra` are bindings
    /// visible to inline refinements (a function's parameters, for its result).
    fn check_type(&mut self, t: &TypeExpr, v: &Val, extra: &[(String, Val)]) -> T {
        // a recursive type (a tree whose children are trees) is checked to a
        // fixed depth, like `shape` models it: each level would check every
        // element of its lists again
        if let TypeExpr::Named(n, _) = t {
            if self.env.types.contains_key(&n.sym()) && !matches!(v, Val::Absent) && self.refined(t, 0) {
                if self.checking.iter().filter(|m| *m == n).count() >= MAX_TYPE_NESTING {
                    let key = format!("{:?} {v:?}", self.erased(t));
                    if let Some(b) = self.cutoffs.get(&key) {
                        return b.clone();
                    }
                    let b = self.unknown_bool(&format!("`{n}` is recursive; only its outer levels are checked"));
                    self.cutoffs.insert(key, b.clone());
                    return b;
                }
                self.checking.push(n.to_string());
                let c = self.check_type_at(t, v, extra);
                self.checking.pop();
                return c;
            }
        }
        self.check_type_at(t, v, extra)
    }

    fn check_type_at(&mut self, t: &TypeExpr, v: &Val, extra: &[(String, Val)]) -> T {
        if matches!(v, Val::Absent) || !self.refined(t, 0) {
            return "true".into();
        }
        if let Val::Opaque(r) = v {
            let r = r.clone();
            return self.unknown_bool(&r);
        }
        if matches!(v, Val::Union { .. }) && !matches!(t, TypeExpr::Union(_) | TypeExpr::Refined(..)) && !self.is_alias(t) {
            // a union value against one type (a narrowed value passed on):
            // its active member must be of that type
            return self.in_type(t, v, extra);
        }
        match t {
            // valid for a union: a valid value of one of its members
            TypeExpr::Union(ms) => {
                let cs: Vec<T> = ms.iter().map(|m| self.in_type(m, v, extra)).collect();
                or(&cs)
            }
            TypeExpr::Named(n, args) if self.env.variant_parent(n.sym()).is_some() => {
                let parent = TypeExpr::Named(Symbol::from(self.env.variant_parent(n.sym()).unwrap()), args.clone());
                let c = self.check_type(&parent, v, extra);
                match v {
                    Val::Sum { tag, ctors, .. } => match ctors.iter().position(|c| &c.0 == n) {
                        Some(k) => and(&[format!("(= {tag} {k})"), c]),
                        None => "false".into(),
                    },
                    _ => self.unknown_bool("a value of an unexpected shape"),
                }
            }
            TypeExpr::Refined(b, pred) => {
                let a = self.check_type(b, v, extra);
                let mut binds = extra.to_vec();
                binds.push(("it".into(), v.clone()));
                let p = self.pred(pred, binds);
                and(&[a, p])
            }
            TypeExpr::Unit | TypeExpr::Fn(..) => "true".into(),
            TypeExpr::Named(n, args) => match (n.as_str(), v) {
                ("List", Val::List { len, id }) => match args.first() {
                    None => "true".into(),
                    Some(t) if len == "0" || self.valid_known(*id, t) => "true".into(),
                    // every element valid: proven on an arbitrary element, and known from then on
                    Some(t) => {
                        let t = t.clone();
                        self.all_of(v, Func::Valid(t))
                    }
                },
                ("Tuple", Val::Tuple(xs)) => {
                    let cs: Vec<T> = args.iter().zip(xs).map(|(a, x)| self.check_type(a, x, &[])).collect();
                    and(&cs)
                }
                ("Dict" | "Set", _) => self.unknown_bool("dicts and sets are not modelled yet"),
                _ => {
                    let Some(td) = self.env.types.get(n).cloned() else { return "true".into() };
                    // to prove: a type argument whose values are all valid has
                    // nothing to check where the type holds one (`msg: M` in
                    // `Attr[Msg]`, from a generic `on_click`). Facts to assume
                    // keep them: they say what is known of the value.
                    let args: Vec<TypeExpr> = if self.proving > 0 {
                        args.iter().map(|a| if self.valid_by_construction(a) { TypeExpr::Named("_".into(), vec![]) } else { a.clone() }).collect()
                    } else {
                        args.clone()
                    };
                    let args = &args;
                    let sub: HashMap<Symbol, TypeExpr> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
                    let mut cs = vec![];
                    let it = match (&td.body, v) {
                        (TypeBody::Alias(a), Val::Distinct(_, inner)) if td.distinct => {
                            cs.push(self.check_type(&subst_texpr(a, &sub), inner, &[]));
                            (**inner).clone()
                        }
                        (TypeBody::Alias(a), _) if !td.distinct => {
                            cs.push(self.check_type(&subst_texpr(a, &sub), v, &[]));
                            v.clone()
                        }
                        (TypeBody::Record(_), Val::Rec(_, fs)) => {
                            let vs = self.env.variants_of(n, args).unwrap_or_default();
                            if let Some((_, _, tys)) = vs.first() {
                                for (ft, (_, fv)) in tys.iter().zip(fs) {
                                    cs.push(self.check_type(ft, fv, &[]));
                                }
                            }
                            v.clone()
                        }
                        (TypeBody::Sum(_), Val::Sum { tag, ctors, .. }) => {
                            let vs = self.env.variants_of(n, args).unwrap_or_default();
                            for (k, ((_, _, tys), (_, payload))) in vs.iter().zip(ctors).enumerate() {
                                let fc: Vec<T> = tys.iter().zip(payload).map(|(ft, fv)| self.check_type(ft, fv, &[])).collect();
                                let fc = and(&fc);
                                if fc != "true" {
                                    cs.push(implies(&format!("(= {tag} {k})"), &fc));
                                }
                            }
                            v.clone()
                        }
                        _ => return self.unknown_bool("a value of an unexpected shape"),
                    };
                    if let Some(r) = &td.refinement {
                        cs.push(self.pred(r, vec![("it".into(), it)]));
                    }
                    and(&cs)
                }
            },
        }
    }

    /// Evaluate a predicate (refinement or contract) with only `binds` in scope.
    fn pred(&mut self, e: &Expr, binds: Vec<(String, Val)>) -> T {
        if self.depth > 4 {
            return self.unknown_bool("contracts nest too deeply");
        }
        self.depth += 1;
        let saved = std::mem::replace(&mut self.scopes, vec![binds.into_iter().collect()]);
        let dead = std::mem::replace(&mut self.dead, false);
        let returns = std::mem::take(&mut self.returns);
        self.spec += 1;
        let v = self.eval(e);
        self.spec -= 1;
        self.returns = returns;
        self.dead = dead;
        self.scopes = saved;
        self.depth -= 1;
        self.as_bool(&v)
    }

    /// Leave the function early with `v` on the current path.
    fn early_return(&mut self, v: Val) {
        if !self.dead {
            self.returns.push((self.pc.clone(), v));
        }
        self.dead = true;
    }

    /// The constructor index of `name` in a sum value, if it has one.
    fn ctor_index(v: &Val, name: &str) -> Option<usize> {
        match v {
            Val::Sum { ctors, .. } => ctors.iter().position(|c| c.0 == name),
            _ => None,
        }
    }

    // ---- scopes ----

    fn lookup(&self, x: &str) -> Option<Val> {
        self.scopes.iter().rev().find_map(|s| s.get(x).cloned())
    }
    fn bind(&mut self, x: &str, v: Val) {
        self.scopes.last_mut().unwrap().insert(x.to_string(), v);
    }
    fn set(&mut self, x: &str, v: Val) {
        for s in self.scopes.iter_mut().rev() {
            if let Some(slot) = s.get_mut(x) {
                *slot = v;
                return;
            }
        }
    }

    // ---- merging two paths ----

    fn merge(&mut self, c: &str, a: &Val, b: &Val) -> Val {
        match (a, b) {
            (Val::Absent, x) | (x, Val::Absent) => x.clone(),
            (Val::Int(x), Val::Int(y)) => Val::Int(ite(c, x, y)),
            (Val::Bool(x), Val::Bool(y)) => Val::Bool(ite(c, x, y)),
            (Val::Str(x), Val::Str(y)) => Val::Str(ite(c, x, y)),
            (Val::Int(x), Val::Opaque(r)) => Val::Int(ite(c, x, &self.fresh_why("Int", &r.clone()))),
            (Val::Opaque(r), Val::Int(y)) => Val::Int(ite(c, &self.fresh_why("Int", &r.clone()), y)),
            (Val::Bool(x), Val::Opaque(r)) => Val::Bool(ite(c, x, &self.fresh_why("Bool", &r.clone()))),
            (Val::Opaque(r), Val::Bool(y)) => Val::Bool(ite(c, &self.fresh_why("Bool", &r.clone()), y)),
            (Val::Str(x), Val::Opaque(r)) => Val::Str(ite(c, x, &self.fresh_why("String", &r.clone()))),
            (Val::Opaque(r), Val::Str(y)) => Val::Str(ite(c, &self.fresh_why("String", &r.clone()), y)),
            (Val::Unit, Val::Unit) => Val::Unit,
            (Val::Tuple(xs), Val::Tuple(ys)) if xs.len() == ys.len() => Val::Tuple(xs.iter().zip(ys).map(|(x, y)| self.merge(c, x, y)).collect()),
            (Val::Rec(n, xs), Val::Rec(m, ys)) if n == m => Val::Rec(n.clone(), xs.iter().zip(ys).map(|((f, x), (_, y))| (f.clone(), self.merge(c, x, y))).collect()),
            (Val::Sum { ty, args, tag: t1, ctors: c1 }, Val::Sum { ty: ty2, args: args2, tag: t2, ctors: c2 }) if ty == ty2 => {
                let ctors = c1
                    .iter()
                    .zip(c2)
                    .map(|((n, xs), (_, ys))| (n.clone(), xs.iter().zip(ys).map(|(x, y)| self.merge(c, x, y)).collect()))
                    .collect();
                Val::Sum { ty: ty.clone(), args: if args.is_empty() { args2.clone() } else { args.clone() }, tag: ite(c, t1, t2), ctors }
            }
            (Val::List { len: l1, id: i1 }, Val::List { len: l2, id: i2 }) => {
                if i1 == i2 {
                    Val::List { len: ite(c, l1, l2), id: *i1 }
                } else {
                    let (i1, i2) = (*i1, *i2);
                    self.new_list(ite(c, l1, l2), Gen::Either(c.to_string(), i1, i2))
                }
            }
            (Val::Distinct(n, x), Val::Distinct(m, y)) if n == m => Val::Distinct(n.clone(), Box::new(self.merge(c, x, y))),
            (Val::Opaque(r), _) | (_, Val::Opaque(r)) => Val::Opaque(r.clone()),
            // values of different shapes (`if big { Square {…} } else { Circle {…} }`): a union
            _ => self.merge_union(c, a, b).unwrap_or_else(|| Val::Opaque("values of different shapes were merged".into())),
        }
    }

    /// Run alternative branches (guard, bindings, body) and merge them.
    fn branches(&mut self, arms: Vec<(T, Vec<(String, Val)>, &Expr)>) -> Val {
        self.branches_narrowed(arms, vec![])
    }

    /// `branches`, where in arm `i` the variables `narrow[i]` hold the given
    /// values (equal to theirs under the arm's guard: a union's member).
    fn branches_narrowed(&mut self, arms: Vec<(T, Vec<(String, Val)>, &Expr)>, mut narrow: Vec<Vec<(String, Val)>>) -> Val {
        let base_pc = self.pc.len();
        let saved = self.scopes.clone();
        let mut outs: Vec<(T, Val, Vec<HashMap<String, Val>>)> = vec![];
        let n_arms = arms.len();
        narrow.resize(n_arms, vec![]);
        for ((g, binds, body), narrow) in arms.into_iter().zip(narrow) {
            self.scopes = saved.clone();
            for (x, v) in narrow {
                self.set(&x, v);
            }
            self.dead = false;
            self.pc.push(g.clone());
            self.scopes.push(binds.into_iter().collect());
            let v = self.eval(body);
            self.scopes.pop();
            self.pc.truncate(base_pc);
            if !self.dead {
                outs.push((g, v, self.scopes.clone()));
            }
        }
        self.dead = outs.is_empty();
        let Some((last_guard, mut v, mut scopes)) = outs.pop() else {
            self.scopes = saved;
            return Val::Unit;
        };
        if outs.len() + 1 < n_arms {
            // some branches ended (`break`): only the others continue
            let mut alive: Vec<T> = outs.iter().map(|o| o.0.clone()).collect();
            alive.push(last_guard.clone());
            self.pc.push(or(&alive));
        }
        while let Some((g, v2, s2)) = outs.pop() {
            v = self.merge(&g, &v2, &v);
            for (layer, other) in scopes.iter_mut().zip(&s2) {
                for (name, val) in layer.iter_mut() {
                    if let Some(o) = other.get(name) {
                        *val = self.merge(&g, o, val);
                    }
                }
            }
        }
        self.scopes = scopes;
        v
    }

    // ---- patterns ----

    /// The condition for `v` to match `p`, and the bindings it makes.
    /// Does `v` belong to the type named `c` (a pattern `Positive =>`, `x is Circle`)?
    fn type_test(&mut self, c: &str, v: &Val) -> T {
        self.in_type(&TypeExpr::Named(Symbol::from(c), vec![]), v, &[])
    }

    fn pattern(&mut self, p: &Pattern, v: &Val, binds: &mut Vec<(String, Val)>) -> T {
        if let Val::Union { tag, slots } = v {
            let type_name = matches!(p, Pattern::Ctor(c, CtorPat::None) if !self.env.ctors.contains_key(&c.sym()) && self.env.is_type_name(c));
            if let Some(k) = self.pattern_key(p).filter(|_| !type_name) {
                // a literal, tuple, list or constructor pattern takes the member of its shape
                let (tag, slots) = (tag.clone(), slots.clone());
                return match slots.iter().position(|s| s.0 == k && !matches!(s.1, Val::Absent)) {
                    Some(i) => {
                        let c = self.pattern(p, &slots[i].1, binds);
                        and(&[format!("(= {tag} {i})"), c])
                    }
                    None => {
                        // no member has that shape: it never matches (bind its names anyway)
                        self.pattern(p, &Val::Opaque("a pattern no member of the union matches".into()), binds);
                        "false".into()
                    }
                };
            }
        }
        match p {
            Pattern::For(..) => "false".into(),
            Pattern::Cond(c) => {
                // `where …`: evaluated with `it` and what is in scope; a fault just means no match
                self.scopes.push([("it".to_string(), v.clone())].into_iter().collect());
                self.spec += 1;
                let b = self.eval(c);
                self.spec -= 1;
                self.scopes.pop();
                self.as_bool(&b)
            }
            Pattern::Or(alts) => {
                // the first alternative that matches gives the bindings
                let mut conds = vec![];
                let mut per: Vec<Vec<(String, Val)>> = vec![];
                for a in alts {
                    let mut b = vec![];
                    conds.push(self.pattern(a, v, &mut b));
                    per.push(b);
                }
                if let Some(first) = per.first().cloned() {
                    for (name, _) in first {
                        let mut val: Option<Val> = None;
                        for (c, b) in conds.iter().zip(&per).rev() {
                            let here = b.iter().find(|x| x.0 == name).map(|x| x.1.clone()).unwrap_or(Val::Opaque("a binding of an or-pattern".into()));
                            val = Some(match val {
                                None => here,
                                Some(rest) => self.merge(c, &here, &rest),
                            });
                        }
                        binds.push((name, val.unwrap()));
                    }
                }
                or(&conds)
            }
            Pattern::Ctor(c, CtorPat::None) if !self.env.ctors.contains_key(&c.sym()) && self.env.is_type_name(c) => self.type_test(c, v),
            Pattern::Wild => "true".into(),
            Pattern::Bind(x) => {
                binds.push((x.to_string(), v.clone()));
                "true".into()
            }
            Pattern::Int(n) => format!("(= {} {})", self.as_int(v), num(*n)),
            Pattern::Bool(b) => {
                let t = self.as_bool(v);
                if *b { t } else { not(&t) }
            }
            Pattern::Str(s) if !smt_string(s) => self.unknown_bool(BEYOND),
            Pattern::Str(s) => format!("(= {} {})", self.as_str(v), str_lit(s)),
            Pattern::Tuple(ps) => match v {
                Val::Tuple(xs) if xs.len() == ps.len() => {
                    let cs: Vec<T> = ps.iter().zip(xs).map(|(p, x)| self.pattern(p, x, binds)).collect();
                    and(&cs)
                }
                _ => {
                    let cs: Vec<T> = ps.iter().map(|p| self.pattern(p, &Val::Opaque("a tuple of an unknown shape".into()), binds)).collect();
                    and(&cs)
                }
            },
            Pattern::List(ps, rest) => {
                let len = self.list_len(v);
                let id = self.list_id(v);
                let n = ps.len() as i64;
                let mut cs = vec![if rest.is_some() { format!("(>= {len} {n})") } else { format!("(= {len} {n})") }];
                for (i, p) in ps.iter().enumerate() {
                    // the i-th element exists when the list is long enough
                    let x = self.element(id, &format!("(> {len} {i})"));
                    cs.push(self.pattern(p, &x, binds));
                }
                if let Some(r) = rest {
                    let rest_list = self.new_list(format!("(- {len} {n})"), Gen::Sub(id));
                    binds.push((r.to_string(), rest_list));
                }
                and(&cs)
            }
            Pattern::Ctor(c, cp) => {
                let fields: Vec<Val>;
                let cond: T;
                let names: Option<Vec<String>>;
                match v {
                    Val::Sum { ty, args, tag, ctors } => {
                        let Some(k) = ctors.iter().position(|x| &x.0 == c) else { return self.unknown_bool("a pattern that is not a constructor of the value's type") };
                        cond = format!("(= {tag} {k})");
                        let vs = self.env.variants_of(ty, args).unwrap_or_default();
                        names = vs.get(k).and_then(|x| x.1.as_ref().map(|ns| ns.iter().map(|n| n.to_string()).collect()));
                        fields = ctors[k].1.iter().map(|x| if matches!(x, Val::Absent) { Val::Opaque("an impossible constructor".into()) } else { x.clone() }).collect();
                    }
                    Val::Rec(n, fs) if n == c => {
                        cond = "true".into();
                        names = Some(fs.iter().map(|f| f.0.clone()).collect());
                        fields = fs.iter().map(|f| f.1.clone()).collect();
                    }
                    Val::Distinct(n, inner) if n == c => {
                        cond = "true".into();
                        names = None;
                        fields = vec![(**inner).clone()];
                    }
                    _ => {
                        let r = match v {
                            Val::Opaque(r) => r.clone(),
                            _ => "a value of an unexpected shape".into(),
                        };
                        cond = self.unknown_bool(&r);
                        names = None;
                        fields = vec![];
                    }
                }
                let mut cs = vec![cond];
                let get = |i: usize| fields.get(i).cloned().unwrap_or(Val::Opaque("an unknown field".into()));
                match cp {
                    CtorPat::None => {}
                    CtorPat::Positional(ps) => {
                        for (i, p) in ps.iter().enumerate() {
                            let x = get(i);
                            cs.push(self.pattern(p, &x, binds));
                        }
                    }
                    CtorPat::Record(fps, _) => {
                        for (f, p) in fps {
                            let i = names.as_ref().and_then(|ns| ns.iter().position(|n| n == f));
                            let x = i.map(get).unwrap_or(Val::Opaque("an unknown field".into()));
                            cs.push(self.pattern(p, &x, binds));
                        }
                    }
                }
                and(&cs)
            }
        }
    }

    // ---- expressions ----

    fn eq(&mut self, a: &Val, b: &Val) -> T {
        match (a, b) {
            (Val::Int(x), Val::Int(y)) | (Val::Bool(x), Val::Bool(y)) | (Val::Str(x), Val::Str(y)) => format!("(= {x} {y})"),
            (Val::Int(x), Val::Opaque(_)) | (Val::Opaque(_), Val::Int(x)) => {
                let o = if matches!(a, Val::Opaque(_)) { a } else { b };
                format!("(= {x} {})", self.as_int(o))
            }
            (Val::Str(x), Val::Opaque(_)) | (Val::Opaque(_), Val::Str(x)) => {
                let o = if matches!(a, Val::Opaque(_)) { a } else { b };
                format!("(= {x} {})", self.as_str(o))
            }
            (Val::Bool(x), Val::Opaque(_)) | (Val::Opaque(_), Val::Bool(x)) => {
                let o = if matches!(a, Val::Opaque(_)) { a } else { b };
                format!("(= {x} {})", self.as_bool(o))
            }
            (Val::Unit, Val::Unit) => "true".into(),
            (Val::Tuple(xs), Val::Tuple(ys)) if xs.len() == ys.len() => {
                let cs: Vec<T> = xs.iter().zip(ys).map(|(x, y)| self.eq(x, y)).collect();
                and(&cs)
            }
            (Val::Rec(n, xs), Val::Rec(m, ys)) if n == m => {
                let cs: Vec<T> = xs.iter().zip(ys).map(|((_, x), (_, y))| self.eq(x, y)).collect();
                and(&cs)
            }
            (Val::Distinct(n, x), Val::Distinct(m, y)) if n == m => self.eq(x, y),
            (Val::Sum { ty, tag: t1, ctors: c1, .. }, Val::Sum { ty: ty2, tag: t2, ctors: c2, .. }) if ty == ty2 => {
                let mut cs = vec![format!("(= {t1} {t2})")];
                for (k, ((_, xs), (_, ys))) in c1.iter().zip(c2).enumerate() {
                    let fe: Vec<T> = xs.iter().zip(ys).map(|(x, y)| self.eq(x, y)).collect();
                    let fe = and(&fe);
                    if fe != "true" {
                        cs.push(implies(&format!("(= {t1} {k})"), &fe));
                    }
                }
                and(&cs)
            }
            (Val::List { len: l1, .. }, Val::List { len: l2, .. }) => {
                // equal lists have equal lengths; the elements are not tracked
                let b = self.unknown_bool("the elements of a list are not tracked");
                self.facts.push(format!("(=> {b} (= {l1} {l2}))"));
                b
            }
            (Val::Opaque(r), _) | (_, Val::Opaque(r)) => {
                let r = r.clone();
                self.unknown_bool(&r)
            }
            // union values are equal when they have the same shape and equal values of it
            (Val::Union { .. }, _) | (_, Val::Union { .. }) => {
                let (Some((t1, s1)), Some((t2, s2))) = (slots_of(a), slots_of(b)) else {
                    return self.unknown_bool("comparing values of an unknown shape");
                };
                let mut cs = vec![];
                for (i, (k, x)) in s1.iter().enumerate() {
                    for (j, (k2, y)) in s2.iter().enumerate() {
                        if k == k2 && !matches!(x, Val::Absent) && !matches!(y, Val::Absent) {
                            let e = self.eq(x, y);
                            cs.push(and(&[format!("(= {t1} {i})"), format!("(= {t2} {j})"), e]));
                        }
                    }
                }
                or(&cs)
            }
            _ => self.unknown_bool("comparing values of different shapes"),
        }
    }

    fn arith(&mut self, op: BinOp, a: Val, b: Val) -> Val {
        use BinOp::*;
        let ints = matches!(a, Val::Int(_)) || matches!(b, Val::Int(_));
        if !ints {
            let r = match (&a, &b) {
                (Val::Opaque(r), _) | (_, Val::Opaque(r)) => r.clone(),
                _ => "a value of an unexpected shape".into(),
            };
            return Val::Opaque(r);
        }
        let (x, y) = (self.as_int(&a), self.as_int(&b));
        Val::Int(match op {
            Add => format!("(+ {x} {y})"),
            Sub => format!("(- {x} {y})"),
            Mul => format!("(* {x} {y})"),
            Div | Mod => {
                if y.parse::<i64>().map_or(true, |n| n == 0) {
                    self.oblige("no division by zero".into(), |_| format!("(not (= {y} 0))"));
                }
                format!("({} {x} {y})", if op == Div { "div" } else { "mod" })
            }
            _ => unreachable!(),
        })
    }

    fn compare(&mut self, op: BinOp, a: Val, b: Val) -> Val {
        use BinOp::*;
        let sym = match op {
            Lt => "<",
            Le => "<=",
            Gt => ">",
            Ge => ">=",
            _ => unreachable!(),
        };
        match (&a, &b) {
            (Val::Int(_), _) | (_, Val::Int(_)) => {
                let (x, y) = (self.as_int(&a), self.as_int(&b));
                Val::Bool(format!("({sym} {x} {y})"))
            }
            (Val::Str(_), _) | (_, Val::Str(_)) => {
                let (x, y) = (self.as_str(&a), self.as_str(&b));
                Val::Bool(match op {
                    Lt => format!("(str.< {x} {y})"),
                    Le => format!("(str.<= {x} {y})"),
                    Gt => format!("(str.< {y} {x})"),
                    _ => format!("(str.<= {y} {x})"),
                })
            }
            (Val::Opaque(r), _) | (_, Val::Opaque(r)) => {
                let r = r.clone();
                Val::Bool(self.unknown_bool(&r))
            }
            _ => Val::Bool(self.unknown_bool("comparing values of an unexpected shape")),
        }
    }

    pub fn eval(&mut self, e: &Expr) -> Val {
        if self.dead {
            return Val::Unit;
        }
        match e {
            Expr::Int(n) => Val::Int(num(*n)),
            Expr::BigInt(d) => Val::Int(d.clone()),
            Expr::Float(_) => Val::Opaque("floats are not modelled yet".into()),
            Expr::Str(s) if !smt_string(s) => Val::Opaque(BEYOND.into()),
            Expr::Str(s) => Val::Str(str_lit(s)),
            Expr::Bool(b) => Val::Bool(b.to_string()),
            Expr::Unit => Val::Unit,
            Expr::Tuple(xs) => Val::Tuple(xs.iter().map(|x| self.eval(x)).collect()),
            Expr::Var(x) => match self.lookup(x) {
                Some(v) => v,
                None => self.fn_value(e, vec![]).0,
            },
            Expr::Hole(h) => Val::Opaque(format!("the hole `?{h}`")),
            Expr::Range(..) | Expr::Index(..) | Expr::Comp(..) | Expr::Dict(..) | Expr::Set(..) => Val::Opaque("a range, slice or comprehension not desugared".into()),
            Expr::List(xs) => {
                let vals: Vec<Val> = xs.iter().map(|x| self.eval(x)).collect();
                self.new_list(xs.len().to_string(), Gen::Values(vals))
            }
            Expr::Lambda(..) => self.fn_value(e, vec![]).0,
            Expr::Mock(t) => Val::Opaque(format!("`mock {}`", crate::printer::ty(t))),
            Expr::Mut(x) | Expr::Comptime(x) => self.eval(x),
            Expr::Describe(_) => Val::Opaque("a type's description".into()),
            Expr::Interp(parts) => {
                for part in parts {
                    if let StrPart::Expr(x, _) = part {
                        self.eval(x);
                    }
                }
                Val::Str(self.fresh_why("String", "f-strings (only their parts are checked)"))
            }
            Expr::Return(x) => {
                let v = self.eval(x);
                self.early_return(v);
                Val::Unit
            }
            Expr::Try(x, kind) => {
                let v = self.eval(x);
                let ok_name = if *kind == TryKind::Result { "Ok" } else { "Some" };
                match (&v, Self::ctor_index(&v, ok_name)) {
                    (Val::Sum { tag, ctors, .. }, Some(k)) => {
                        let ok = format!("(= {tag} {k})");
                        let payload = ctors[k].1.first().cloned().unwrap_or(Val::Opaque("the payload of `Ok` / `Some`".into()));
                        // the error (or `None`) leaves the function as it is
                        let n = self.pc.len();
                        self.pc.push(not(&ok));
                        self.early_return(v.clone());
                        self.dead = false;
                        self.pc.truncate(n);
                        self.pc.push(ok);
                        payload
                    }
                    _ => {
                        let n = self.pc.len();
                        self.early_return(Val::Opaque("an early return through `!` / `?`".into()));
                        self.dead = false;
                        self.pc.truncate(n);
                        Val::Opaque("the value under `!` / `?`".into())
                    }
                }
            }
            Expr::Except(..) | Expr::Or(..) => {
                let (x, name, fb) = match e {
                    Expr::Except(x, name, fb) => (x, name, fb),
                    Expr::Or(x, fb) => (x, &None, fb),
                    _ => unreachable!(),
                };
                let v = self.eval(x);
                let k_ok = Self::ctor_index(&v, "Ok").or(Self::ctor_index(&v, "Some"));
                match (&v, k_ok) {
                    (Val::Sum { tag, ctors, .. }, Some(k)) => {
                        let ok = format!("(= {tag} {k})");
                        let payload = ctors[k].1.first().cloned().unwrap_or(Val::Opaque("the payload of `Ok` / `Some`".into()));
                        let mut err_binds = vec![];
                        if let (Some(n), Some(e)) = (name, Self::ctor_index(&v, "Err")) {
                            err_binds.push((n.to_string(), ctors[e].1.first().cloned().unwrap_or(Val::Opaque("the error".into()))));
                        }
                        let ok_var = Expr::Var(" ok".into());
                        self.branches(vec![(ok.clone(), vec![(" ok".into(), payload)], &ok_var), (not(&ok), err_binds, fb)])
                    }
                    _ => {
                        self.scopes.push(name.iter().map(|n| (n.to_string(), Val::Opaque("the error".into()))).collect());
                        self.eval(fb);
                        self.scopes.pop();
                        Val::Opaque("the value under `except`".into())
                    }
                }
            }
            Expr::Field(r, f) => {
                let v = self.eval(r);
                self.field(&v, f)
            }
            Expr::Un(UnOp::Neg, x) => match self.eval(x) {
                Val::Int(t) => Val::Int(format!("(- {t})")),
                other => other,
            },
            Expr::Un(UnOp::Not, x) => {
                let v = self.eval(x);
                Val::Bool(not(&self.as_bool(&v)))
            }
            Expr::Bin(op, l, r) => {
                use BinOp::*;
                match op {
                    And | Or | Implies => {
                        let a = self.eval(l);
                        let a = self.as_bool(&a);
                        let n = self.pc.len();
                        self.pc.push(if *op == Or { not(&a) } else { a.clone() });
                        let b = self.eval(r);
                        let b = self.as_bool(&b);
                        self.pc.truncate(n);
                        Val::Bool(match op {
                            And => and(&[a, b]),
                            Or => or(&[a, b]),
                            _ => implies(&a, &b),
                        })
                    }
                    _ => {
                        let a = self.eval(l);
                        let b = self.eval(r);
                        match op {
                            Eq => Val::Bool(self.eq(&a, &b)),
                            Ne => Val::Bool(not(&self.eq(&a, &b))),
                            Lt | Le | Gt | Ge => self.compare(*op, a, b),
                            Add | Sub | Mul | Div | Mod => {
                                // `+` also joins strings and lists
                                let joins = |v: &Val| matches!(v, Val::Str(_) | Val::List { .. });
                                if *op == Add && (joins(&a) || joins(&b)) {
                                    return self.concat(a, b);
                                }
                                self.arith(*op, a, b)
                            }
                            _ => unreachable!(),
                        }
                    }
                }
            }
            Expr::Is(x, c) if !self.env.ctors.contains_key(&c.sym()) && self.env.is_type_name(c) => {
                let v = self.eval(x);
                Val::Bool(self.type_test(c, &v))
            }
            Expr::Is(x, c) => {
                let v = self.eval(x);
                Val::Bool(self.is_ctor(&v, c))
            }
            Expr::If(c, t, f) => {
                let cv = self.eval(c);
                let cv = self.as_bool(&cv);
                self.branches(vec![(cv.clone(), vec![], t), (not(&cv), vec![], f)])
            }
            Expr::Match(s, name, arms) => {
                let v = self.eval(s);
                let mut prev: Vec<T> = vec![];
                let mut alts = vec![];
                let mut narrow = vec![];
                for a in arms {
                    // a union narrowed to the one member the arm's pattern can match
                    let member = match &v {
                        Val::Union { slots, .. } => {
                            let slots = slots.clone();
                            match self.arm_slots(&a.pat, &slots)[..] {
                                [i] => Some(slots[i].1.clone()),
                                _ => None,
                            }
                        }
                        _ => None,
                    };
                    narrow.push(match (&**s, &member) {
                        (Expr::Var(x), Some(m)) if self.lookup(x).is_some() => vec![(x.to_string(), m.clone())],
                        _ => vec![],
                    });
                    let mut binds: Vec<(String, Val)> = name.iter().map(|n| (n.to_string(), member.clone().unwrap_or(v.clone()))).collect();
                    let n = self.pc.len();
                    self.pc.extend(prev.iter().map(|p| not(p)));
                    let cond = self.pattern(&a.pat, &v, &mut binds);
                    self.pc.truncate(n);
                    let mut g: Vec<T> = prev.iter().map(|p| not(p)).collect();
                    g.push(cond.clone());
                    alts.push((and(&g), binds, &a.body));
                    prev.push(cond);
                }
                self.branches_narrowed(alts, narrow)
            }
            Expr::Block(stmts, last) => {
                self.scopes.push(HashMap::default());
                for s in stmts {
                    self.stmt(s);
                    if self.dead {
                        break;
                    }
                }
                let v = self.eval(last);
                self.scopes.pop();
                v
            }
            Expr::Ctor(c, args) => self.ctor(c, args),
            Expr::Call(f, args) => match &**f {
                Expr::Var(name) if self.lookup(name).is_none() => self.call_exprs(name, args),
                _ => {
                    let callee = self.eval(f);
                    for a in args {
                        self.eval(a);
                    }
                    match callee {
                        // a function value of a known type: some valid value of its result type
                        Val::FnOf { ret } => {
                            let old = self.reason.replace("the result of calling a function value (only its type is known)".into());
                            let res = self.shape(&ret, 0);
                            self.reason = old;
                            self.spec += 1;
                            let c = self.check_type(&ret, &res, &[]);
                            self.spec -= 1;
                            self.assume(c);
                            res
                        }
                        _ => Val::Opaque("calling a function value".into()),
                    }
                }
            },
            Expr::Method(r, m, args) => {
                let mut all = vec![(**r).clone()];
                all.extend(args.iter().cloned());
                self.call_exprs(m, &all)
            }
        }
    }

    fn concat(&mut self, a: Val, b: Val) -> Val {
        match (&a, &b) {
            (Val::List { .. }, _) | (_, Val::List { .. }) => {
                let (x, y) = (self.list_len(&a), self.list_len(&b));
                let (ia, ib) = (self.list_id(&a), self.list_id(&b));
                // an element of `a + b` is an element of one of them
                let choice = self.fresh("Bool");
                self.new_list(format!("(+ {x} {y})"), Gen::Either(choice, ia, ib))
            }
            (Val::Str(_), _) | (_, Val::Str(_)) => {
                let (x, y) = (self.as_str(&a), self.as_str(&b));
                Val::Str(format!("(str.++ {x} {y})"))
            }
            (Val::Opaque(r), _) | (_, Val::Opaque(r)) => Val::Opaque(r.clone()),
            _ => Val::Opaque("concatenating values of an unexpected shape".into()),
        }
    }

    fn ctor(&mut self, c: &str, args: &CtorArgs) -> Val {
        let env = self.env;
        let tn = env.ctors.get(&c.sym()).cloned().unwrap_or_default();
        let Some(td) = env.types.get(&tn) else { return Val::Opaque(format!("unknown constructor `{c}`")) };
        let variants = env.variants_of(&tn, &[]).unwrap_or_default();
        let ordered = |vc: &mut Self, names: &Option<Vec<Symbol>>, fs: &[(Symbol, Expr)]| -> Vec<Val> {
            let vals: HashMap<Symbol, Val> = fs.iter().map(|(n, e)| (*n, vc.eval(e))).collect();
            names.clone().unwrap_or_default().iter().map(|n| vals.get(n).cloned().unwrap_or(Val::Opaque(format!("field `{n}`")))).collect()
        };
        let v = match (&td.body, args) {
            (TypeBody::Alias(_), CtorArgs::Positional(xs)) if td.distinct && xs.len() == 1 => Val::Distinct(tn.to_string(), Box::new(self.eval(&xs[0]))),
            (_, CtorArgs::Update(base, fs)) => {
                let b = self.eval(base);
                match b {
                    Val::Rec(n, mut vals) => {
                        for (f, fe) in fs {
                            let nv = self.eval(fe);
                            if let Some(slot) = vals.iter_mut().find(|(k, _)| k == f) {
                                slot.1 = nv;
                            }
                        }
                        Val::Rec(n, vals)
                    }
                    other => {
                        for (_, fe) in fs {
                            self.eval(fe);
                        }
                        other
                    }
                }
            }
            (TypeBody::Record(_), CtorArgs::Record(fs)) => {
                let names = variants.first().and_then(|v| v.1.clone());
                let vals = ordered(self, &names, fs);
                Val::Rec(tn.to_string(), names.unwrap_or_default().into_iter().map(|n| n.to_string()).zip(vals).collect())
            }
            (TypeBody::Sum(_), _) => {
                let Some(k) = variants.iter().position(|v| v.0 == c) else { return Val::Opaque(format!("unknown constructor `{c}`")) };
                let payload = match args {
                    CtorArgs::None => vec![],
                    CtorArgs::Positional(xs) => xs.iter().map(|x| self.eval(x)).collect(),
                    CtorArgs::Record(fs) => ordered(self, &variants[k].1, fs),
                    CtorArgs::Update(..) => unreachable!(),
                };
                let ctors = variants
                    .iter()
                    .enumerate()
                    .map(|(i, (n, _, tys))| (n.to_string(), if i == k { payload.clone() } else { vec![Val::Absent; tys.len()] }))
                    .collect();
                Val::Sum { ty: tn.to_string(), args: vec![], tag: k.to_string(), ctors }
            }
            _ => Val::Opaque(format!("constructor `{c}`")),
        };
        let t = TypeExpr::Named(Symbol::from(tn.as_str()), vec![]);
        if self.refined(&t, 0) {
            let v2 = v.clone();
            self.oblige(format!("constructing `{c}`"), |vc| vc.check_type(&t, &v2, &[]));
        }
        v
    }

    // ---- calls ----

    /// Is `e` a function value (a lambda, or the name of a function)?
    fn is_fn(&self, e: &Expr) -> bool {
        match e {
            Expr::Lambda(..) => true,
            Expr::Var(x) => self.lookup(x).is_none() && (self.env.fns.contains_key(&x.sym()) || self.env.is_method(x.sym())),
            _ => false,
        }
    }

    /// A function value that will be called with `args` (missing ones are
    /// unknown): its body (for a lambda) or contract (for a named function)
    /// is checked for those arguments. Returns the function as a value and
    /// what one call returns, with the type of results when it is known.
    fn fn_value(&mut self, e: &Expr, args: Vec<Val>) -> (Val, Val, Option<TypeExpr>) {
        let arg = |i: usize| args.get(i).cloned().unwrap_or(Val::Opaque("the arguments of a function value are not known".into()));
        match e {
            Expr::Lambda(ps, body) => {
                // its checks hold on every call; a `return` leaves the lambda only
                self.scopes.push(ps.iter().enumerate().map(|(i, p)| (p.name.to_string(), arg(i))).collect());
                let returns = std::mem::take(&mut self.returns);
                let dead = self.dead;
                let r = self.eval(body);
                self.returns = returns;
                self.dead = dead;
                self.scopes.pop();
                (Val::Opaque("function values are not modelled".into()), r, None)
            }
            Expr::Var(name) => {
                let env = self.env;
                let f = env.fns.get(&name.sym()).or_else(|| env.method_contract(name).map(|(_, d)| d));
                match f {
                    Some(f) => {
                        let n = f.params.len();
                        let r = self.call_contract(name, f, &[], &[], &(0..n).map(arg).collect::<Vec<_>>());
                        let ty = f.type_params.is_empty().then(|| f.ret.clone());
                        (Val::Opaque("function values are not modelled".into()), r, ty)
                    }
                    None => (Val::Opaque(format!("`{name}` used as a value")), Val::Opaque(format!("`{name}`")), None),
                }
            }
            _ => (Val::Opaque("function values are not modelled".into()), Val::Opaque("calling a function value".into()), None),
        }
    }

    /// Evaluate a call's arguments and make the call. A function passed to a
    /// list builtin is checked on one arbitrary element of the list.
    fn call_exprs(&mut self, name: &str, exprs: &[Expr]) -> Val {
        // `Trait::empty(type, ..)`: the type argument only picks the implementation
        let exprs = match self.env.method_contract(name) {
            Some((_, d)) if !crate::resolve::takes_self(d) && exprs.len() == d.params.len() + 1 => &exprs[1..],
            _ => exprs,
        };
        let mut vals: Vec<Option<Val>> = exprs.iter().map(|e| if self.is_fn(e) { None } else { Some(self.eval(e)) }).collect();
        let builtin = !self.env.fns.contains_key(&name.sym()) && !self.env.is_method(name.sym());
        let mut fn_result_ty = None;
        for (i, e) in exprs.iter().enumerate() {
            if vals[i].is_some() {
                continue;
            }
            // the function runs on the elements, if there are any: its checks hold under that
            let mut exists = None;
            let elem = match vals.first() {
                Some(Some(Val::List { len, id })) if builtin && i > 0 => {
                    let (len, id) = (len.clone(), *id);
                    let e = format!("(> {len} 0)");
                    exists = Some(e.clone());
                    Some(self.element(id, &e))
                }
                _ => None,
            };
            let args = match (name, elem) {
                ("fold", Some(x)) => vec![Val::Opaque("the accumulator of `fold`".into()), x],
                (_, Some(x)) => vec![x],
                // passed for a parameter of a function type: it is called with
                // valid values of its parameter types, as any values of them are
                _ => match self.env.fns.get(&Symbol::new(name)).and_then(|f| f.params.get(i)).map(|p| p.ty.clone()) {
                    Some(TypeExpr::Fn(ps, _)) => ps
                        .iter()
                        .map(|t| {
                            let old = self.reason.replace("an argument a function value is called with (only its type is known)".into());
                            let v = self.shape(t, 0);
                            self.reason = old;
                            self.spec += 1;
                            let c = self.check_type(t, &v, &[]);
                            self.spec -= 1;
                            self.assume(c);
                            v
                        })
                        .collect(),
                    _ => vec![],
                },
            };
            let n = self.pc.len();
            if let Some(e) = &exists {
                self.pc.push(e.clone());
            }
            let (fv, _, ty) = self.fn_value(e, args);
            self.pc.truncate(n);
            fn_result_ty = ty;
            vals[i] = Some(fv);
        }
        let vals: Vec<Val> = vals.into_iter().map(|v| v.unwrap()).collect();
        let key = self.call_key(name, exprs, &vals);
        let _ = fn_result_ty;
        let funcs: Vec<Option<Func>> = exprs.iter().map(|e| if self.is_fn(e) { self.func_of(e) } else { None }).collect();
        let saved = std::mem::replace(&mut self.fn_args, funcs);
        let r = self.call(name, exprs, vals);
        self.fn_args = saved;
        if let Some(k) = key {
            match self.memo.get(&k).cloned() {
                Some(old) => {
                    let e = self.eq(&r, &old);
                    self.facts.push(e);
                }
                None => {
                    self.memo.insert(k, r.clone());
                }
            }
        }
        r
    }

    /// What identifies a call: the callee and the arguments' values (for a
    /// lambda, its text and the values of the variables it uses). `None` when
    /// some argument is not known exactly.
    fn call_key(&self, name: &str, exprs: &[Expr], vals: &[Val]) -> Option<String> {
        let mut key = format!("{name}(");
        for (e, v) in exprs.iter().zip(vals) {
            match e {
                Expr::Lambda(ps, body) => {
                    key.push_str(&crate::printer::expr(e, 0));
                    let mut used = std::collections::BTreeSet::new();
                    crate::ast::walk(body, |x| {
                        if let Expr::Var(n) = x {
                            used.insert(n.clone());
                        }
                    });
                    for n in used {
                        if ps.iter().any(|p| p.name == n) {
                            continue;
                        }
                        match self.lookup(&n) {
                            Some(v) => key.push_str(&format!(" {n}={v:?}")),
                            None if self.is_fn(&Expr::Var(n.clone())) => {}
                            None => return None,
                        }
                    }
                }
                Expr::Var(n) if self.is_fn(e) => key.push_str(n),
                _ => key.push_str(&format!("{v:?}")),
            }
            key.push(',');
        }
        (!key.contains("Opaque(")).then_some(key)
    }

    fn call(&mut self, name: &str, arg_exprs: &[Expr], args: Vec<Val>) -> Val {
        let env = self.env;
        if let Some(f) = env.fns.get(&name.sym()) {
            if self.spec > 0 && self.depth < 4 {
                if let Some(&assumed) = self.transparent.get(name) {
                    // see through: in a contract, the call is its body
                    if assumed {
                        self.relied_on.insert(name.to_string());
                    }
                    let binds: HashMap<String, Val> = f.params.iter().map(|p| p.name.to_string()).zip(args.iter().cloned()).collect();
                    self.depth += 1;
                    let saved = std::mem::replace(&mut self.scopes, vec![binds]);
                    let r = self.eval(&f.body);
                    self.scopes = saved;
                    self.depth -= 1;
                    return r;
                }
            }
            let r = self.call_contract(name, f, &[], &[], &args);
            self.write_back(&f.params, arg_exprs, &args);
            return r;
        }
        if let Some((_, decl)) = env.method_contract(name) {
            let r = self.call_contract(name, decl, &[], &[], &args);
            self.write_back(&decl.params, arg_exprs, &args);
            return r;
        }
        if env.is_method(name.sym()) {
            // which implementation runs depends on the receiver's type; a shared name or a type's own method
            if let Some(ps) = env.method_params(name.sym()) {
                self.write_back(&ps, arg_exprs, &args);
            }
            return Val::Opaque(format!("the result of method `{name}` (it depends on the receiver's type)"));
        }
        self.builtin(name, arg_exprs, args)
    }

    /// `mut` parameters may have been changed by the callee.
    fn write_back(&mut self, params: &[Param], arg_exprs: &[Expr], args: &[Val]) {
        for ((p, e), v) in params.iter().zip(arg_exprs).zip(args) {
            if p.mutable {
                // the callee may have changed it: any value of the parameter's
                // type comes back, and is stored with the usual checks
                let nv = self.refresh(v);
                let c = self.check_type(&p.ty, &nv, &[]);
                self.assume(c);
                self.assign(e, nv, "writing back");
            }
        }
    }

    /// A call seen through the callee's contract: check its parameters'
    /// refinements and `requires`, then assume its result type and `ensures`.
    fn call_contract(&mut self, label: &str, f: &FnDecl, extra_req: &[Expr], extra_ens: &[Expr], args: &[Val]) -> Val {
        if self.depth > 4 {
            return Val::Opaque(format!("calls to `{label}` nest too deeply"));
        }
        let binds: Vec<(String, Val)> = f.params.iter().map(|p| p.name.to_string()).zip(args.iter().cloned()).collect();
        if label == self.self_name && self.spec == 0 && !self.dead {
            // a recursive call: remembered, to check measures against
            self.rec_sites.push((self.pc.clone(), args.to_vec()));
        }
        for (p, a) in f.params.iter().zip(args) {
            if self.refined(&p.ty, 0) {
                let (t, a) = (p.ty.clone(), a.clone());
                self.oblige(format!("argument `{}` of `{label}`: {}", p.name, crate::printer::ty(&p.ty)), |vc| vc.check_type(&t, &a, &[]));
            }
        }
        for r in extra_req.iter().chain(&f.requires) {
            let b = binds.clone();
            self.oblige(format!("precondition of `{label}`: requires {}", crate::printer::expr(r, 0)), |vc| vc.pred(r, b));
        }
        let why = format!("the result of `{label}` (only its contract is known)");
        let old = self.reason.replace(why);
        let res = self.shape(&f.ret, 0);
        self.reason = old;
        self.depth += 1;
        self.spec += 1;
        let c = self.check_type(&f.ret, &res, &binds);
        self.assume(c);
        let mut b2 = binds.clone();
        b2.push(("it".into(), res.clone()));
        for en in extra_ens.iter().chain(&f.ensures) {
            let c = self.pred(en, b2.clone());
            self.assume(c);
        }
        self.spec -= 1;
        self.depth -= 1;
        res
    }

    /// `Some(payload)` when `some` holds, else `None` (an `Option[t]`).
    fn option(&mut self, t: &TypeExpr, some: &str, payload: Val) -> Val {
        let vs = self.env.variants_of("Option", &[t.clone()]).unwrap_or_default();
        let (Some(ks), Some(kn)) = (vs.iter().position(|v| v.0 == "Some"), vs.iter().position(|v| v.0 == "None")) else {
            return Val::Opaque("`Option` is not the prelude's".into());
        };
        let ctors = vs.iter().map(|(n, _, _)| (n.to_string(), if n == "Some" { vec![payload.clone()] } else { vec![] })).collect();
        Val::Sum { ty: "Option".into(), args: vec![t.clone()], tag: ite(some, &ks.to_string(), &kn.to_string()), ctors }
    }

    fn builtin(&mut self, name: &str, arg_exprs: &[Expr], a: Vec<Val>) -> Val {
        let arg = |i: usize| a.get(i).cloned().unwrap_or(Val::Opaque("a missing argument".into()));
        let not_modelled = format!("`{name}` is not modelled");
        match (name, a.len()) {
            ("len" | "str_len", 1) => match arg(0) {
                Val::Str(s) => Val::Int(format!("(str.len {s})")),
                v => {
                    let l = self.list_len(&v);
                    if !matches!(v, Val::List { .. }) {
                        self.reasons.entry(l.clone()).or_insert(format!("`len` of a dict or set"));
                    }
                    Val::Int(l)
                }
            },
            ("abs", 1) => match arg(0) {
                Val::Int(x) => Val::Int(format!("(ite (< {x} 0) (- {x}) {x})")),
                Val::Union { .. } => Val::Opaque(not_modelled),
                v => v,
            },
            ("min" | "max", 2) => {
                let (x, y) = (arg(0), arg(1));
                if !matches!(x, Val::Int(_)) && !matches!(y, Val::Int(_)) {
                    return Val::Opaque(not_modelled);
                }
                let (x, y) = (self.as_int(&x), self.as_int(&y));
                Val::Int(format!("(ite ({} {x} {y}) {x} {y})", if name == "min" { "<=" } else { ">=" }))
            }
            ("matches", 2) => {
                let s = self.as_str(&arg(0));
                match arg_exprs.get(1) {
                    Some(Expr::Str(re)) => match regex_model(re) {
                        Some(model) => {
                            let approx = matches!(model, RegexModel::Between(..));
                            let b = match model {
                                RegexModel::Exact(r) => format!("(str.in_re {s} {r})"),
                                RegexModel::Between(under, over) => {
                                    let b = self.fresh_why("Bool", &format!("the regular expression \"{re}\" has a large Unicode class (approximated from both sides)"));
                                    self.facts.push(format!("(=> (str.in_re {s} {under}) {b})"));
                                    self.facts.push(format!("(=> {b} (str.in_re {s} {over}))"));
                                    b
                                }
                            };
                            // what the patterns' languages say about each other, for the same string
                            for (s2, re2, b2, approx2) in self.matched.clone() {
                                if s2 == s && re2 != *re && (approx || approx2) {
                                    let (sub, sup, disjoint) = regex_relations(re, &re2);
                                    if sub {
                                        self.facts.push(implies(&b, &b2));
                                    }
                                    if sup {
                                        self.facts.push(implies(&b2, &b));
                                    }
                                    if disjoint {
                                        self.facts.push(not(&and(&[b.clone(), b2.clone()])));
                                    }
                                }
                            }
                            self.matched.push((s.clone(), re.clone(), b.clone(), approx));
                            Val::Bool(b)
                        }
                        None => Val::Bool(self.unknown_bool(&format!("the regular expression \"{re}\" uses features not modelled yet"))),
                    },
                    _ => Val::Bool(self.unknown_bool("`matches` with a pattern that is not a literal")),
                }
            }
            ("starts_with", 2) => {
                let (s, p) = (self.as_str(&arg(0)), self.as_str(&arg(1)));
                Val::Bool(format!("(str.prefixof {p} {s})"))
            }
            ("contains", 2) if matches!(arg(0), Val::Str(_)) => {
                let (s, p) = (self.as_str(&arg(0)), self.as_str(&arg(1)));
                Val::Bool(format!("(str.contains {s} {p})"))
            }
            ("push", 2) => {
                let l = self.list_len(&arg(0));
                let id = self.list_id(&arg(0));
                // an element is one of the list's, or the new one
                let one = self.new_list("1".into(), Gen::Values(vec![arg(1)]));
                let one = self.list_id(&one);
                let choice = self.fresh("Bool");
                self.new_list(format!("(+ {l} 1)"), Gen::Either(choice, id, one))
            }
            ("take" | "drop", 2) => {
                let l = self.list_len(&arg(0));
                let n = self.as_int(&arg(1));
                let k = format!("(ite (< {n} 0) 0 (ite (< {n} {l}) {n} {l}))");
                let id = self.list_id(&arg(0));
                self.new_list(if name == "take" { k } else { format!("(- {l} {k})") }, Gen::Sub(id))
            }
            ("reverse" | "sort" | "sort_by", _) if !matches!(arg(0), Val::Str(_)) => {
                let l = self.list_len(&arg(0));
                let id = self.list_id(&arg(0));
                self.new_list(l, Gen::Sub(id))
            }
            ("map", 2) => {
                let l = self.list_len(&arg(0));
                let id = self.list_id(&arg(0));
                let gen_ = match self.fn_args.get(1).cloned().flatten() {
                    Some(f) => Gen::Map(id, f),
                    None => Gen::Unknown,
                };
                self.new_list(l, gen_)
            }
            ("enumerate", _) => {
                let l = self.list_len(&arg(0));
                self.new_list(l, Gen::Unknown)
            }
            ("filter", 2) => {
                let l = self.list_len(&arg(0));
                let id = self.list_id(&arg(0));
                let k = self.fresh_why("Int", "`filter` (only the length bound is modelled)");
                self.facts.push(format!("(and (<= 0 {k}) (<= {k} {l}))"));
                let gen_ = match self.fn_args.get(1).cloned().flatten() {
                    Some(f) => Gen::Filter(id, f),
                    None => Gen::Sub(id),
                };
                self.new_list(k, gen_)
            }
            ("find", 2) if matches!(arg(0), Val::List { .. }) && self.fn_args.get(1).cloned().flatten().is_some() => {
                // `Some(x)`: an element of the list that satisfies the function
                let f = self.fn_args[1].clone().unwrap();
                let l = self.list_len(&arg(0));
                let id = self.list_id(&arg(0));
                let found = self.fresh_why("Bool", "`find` (whether an element is found is not modelled)");
                self.facts.push(format!("(=> (= {l} 0) (not {found}))"));
                let x = self.element(id, &found);
                let n = self.pc.len();
                self.pc.push(found.clone());
                let p = self.apply(&f, x.clone());
                let p = self.as_bool(&p);
                self.assume(p);
                self.pc.truncate(n);
                let t = self.elem_type(id).unwrap_or(TypeExpr::Named("_".into(), vec![]));
                self.option(&t, &found, x)
            }
            ("at", 2) if matches!(arg(0), Val::List { .. }) => {
                let l = self.list_len(&arg(0));
                let id = self.list_id(&arg(0));
                let i = self.as_int(&arg(1));
                let inside = format!("(and (<= 0 {i}) (< {i} {l}))");
                self.oblige("index in range".into(), |_| inside.clone());
                self.element(id, &inside)
            }
            ("at", 2) if matches!(arg(0), Val::Str(_)) => {
                let s = self.as_str(&arg(0));
                let i = self.as_int(&arg(1));
                self.oblige("index in range".into(), |_| format!("(and (<= 0 {i}) (< {i} (str.len {s})))"));
                Val::Str(format!("(str.at {s} {i})"))
            }
            ("get", 2) if matches!(arg(0), Val::List { .. }) => {
                let l = self.list_len(&arg(0));
                let id = self.list_id(&arg(0));
                let i = self.as_int(&arg(1));
                let inside = format!("(and (<= 0 {i}) (< {i} {l}))");
                let x = self.element(id, &inside);
                let t = self.elem_type(id).unwrap_or(TypeExpr::Named("_".into(), vec![]));
                self.option(&t, &inside, x)
            }
            ("count", 2) => {
                let l = self.list_len(&arg(0));
                let k = self.fresh_why("Int", "`count` (only the bounds are modelled)");
                self.facts.push(format!("(and (<= 0 {k}) (<= {k} {l}))"));
                Val::Int(k)
            }
            ("zip", 2) => {
                let (x, y) = (self.list_len(&arg(0)), self.list_len(&arg(1)));
                self.new_list(format!("(ite (<= {x} {y}) {x} {y})"), Gen::Unknown)
            }
            ("range", 2) => {
                let (lo, hi) = (self.as_int(&arg(0)), self.as_int(&arg(1)));
                self.new_list(format!("(ite (> {hi} {lo}) (- {hi} {lo}) 0)"), Gen::Range(lo, hi))
            }
            ("set", 1) => {
                let l = self.list_len(&arg(0));
                let id = self.list_id(&arg(0));
                let k = self.fresh_why("Int", "`set` (only its size bounds are modelled)");
                self.facts.push(format!("(and (<= 0 {k}) (<= {k} {l}) (= (= {k} 0) (= {l} 0)))"));
                // a set of a list's elements has only elements of the list
                self.new_list(k, Gen::Sub(id))
            }
            ("to_list", 1) if matches!(arg(0), Val::List { .. }) => {
                let l = self.list_len(&arg(0));
                let id = self.list_id(&arg(0));
                self.new_list(l, Gen::Sub(id))
            }
            ("keys" | "values" | "items" | "to_list", 1) => {
                let l = self.list_len(&arg(0));
                self.new_list(l, Gen::Unknown)
            }
            ("all", 2) if self.fn_args.get(1).cloned().flatten().is_some() => {
                let f = self.fn_args[1].clone().unwrap();
                Val::Bool(self.all_of(&arg(0), f))
            }
            ("all" | "any", 2) => {
                // otherwise only the empty case is modelled
                let l = self.list_len(&arg(0));
                let b = self.fresh_why("Bool", &not_modelled);
                self.facts.push(format!("(=> (= {l} 0) {})", if name == "all" { b.clone() } else { not(&b) }));
                Val::Bool(b)
            }
            ("sum", 1) => {
                // an Int list (a Float list's elements are unknown values)
                let v = arg(0);
                let Val::List { id, .. } = v else { return Val::Opaque(not_modelled) };
                let mut ints = true;
                let q = self.every(id, |vc, x| match x {
                    Val::Int(x) => Some(format!("(>= {x} 0)")),
                    other => {
                        ints = false;
                        let _ = vc;
                        let _ = other;
                        None
                    }
                });
                if !ints {
                    return Val::Opaque(not_modelled);
                }
                let l = self.list_len(&v);
                let total = self.fresh_why("Int", "`sum` (only its sign is modelled)");
                self.facts.push(format!("(=> (= {l} 0) (= {total} 0))"));
                // non-negative elements have a non-negative sum
                if let Some(q) = q {
                    self.facts.push(format!("(=> {q} (>= {total} 0))"));
                }
                Val::Int(total)
            }
            // strings are sequences of code points, as `chars()` counts them
            ("ends_with", 2) => {
                let (s, p) = (self.as_str(&arg(0)), self.as_str(&arg(1)));
                Val::Bool(format!("(str.suffixof {p} {s})"))
            }
            // characters start..end, both clamped to 0..len (`str.substr` takes a length)
            ("slice", 3) if matches!(arg(0), Val::Str(_)) => {
                let s = self.as_str(&arg(0));
                let (a, b) = (self.as_int(&arg(1)), self.as_int(&arg(2)));
                let n = format!("(str.len {s})");
                let clamp = |x: &str| format!("(ite (< {x} 0) 0 (ite (> {x} {n}) {n} {x}))");
                let (a, b) = (clamp(&a), clamp(&b));
                Val::Str(format!("(str.substr {s} {a} (- {b} {a}))"))
            }
            ("get", 2) if matches!(arg(0), Val::Str(_)) => {
                let s = self.as_str(&arg(0));
                let i = self.as_int(&arg(1));
                let string = TypeExpr::Named("String".into(), vec![]);
                self.option(&string, &format!("(and (<= 0 {i}) (< {i} (str.len {s})))"), Val::Str(format!("(str.at {s} {i})")))
            }
            ("chars", 1) => {
                let s = self.as_str(&arg(0));
                self.new_list(format!("(str.len {s})"), Gen::Type(TypeExpr::Named("String".into(), vec![])))
            }
            // the first match, counted in characters; "" is found at 0
            ("index_of", 2) => {
                let (s, p) = (self.as_str(&arg(0)), self.as_str(&arg(1)));
                let i = format!("(str.indexof {s} {p} 0)");
                let int = TypeExpr::Named("Int".into(), vec![]);
                self.option(&int, &format!("(>= {i} 0)"), Val::Int(i))
            }
            // every non-overlapping match, left to right (Rust's `str::replace`);
            // an empty pattern is a runtime fault, so that is checked
            ("replace", 3) => {
                let (s, from, to) = (self.as_str(&arg(0)), self.as_str(&arg(1)), self.as_str(&arg(2)));
                self.oblige("`replace` with a pattern that is not empty".into(), |_| format!("(not (= {from} \"\"))"));
                Val::Str(format!("(str.replace_all {s} {from} {to})"))
            }
            ("to_string", 1) => match arg(0) {
                Val::Str(s) => Val::Str(s),
                Val::Bool(b) => Val::Str(format!("(ite {b} \"true\" \"false\")")),
                // `str.from_int` is "" for negative numbers
                Val::Int(n) => Val::Str(format!("(ite (>= {n} 0) (str.from_int {n}) (str.++ \"-\" (str.from_int (- {n}))))")),
                _ => Val::Str(self.fresh_why("String", &not_modelled)),
            },
            // only the forms whose result is certain: digits, `-` and digits, and
            // strings without an ASCII digit (never a number). Whitespace, `+`
            // and `_` (which the parser accepts) leave the result unknown.
            ("parse_int", 1) => {
                let s = self.as_str(&arg(0));
                let digits = "(re.+ (re.range \"0\" \"9\"))";
                let pos = format!("(str.in_re {s} {digits})");
                let rest = format!("(str.substr {s} 1 (- (str.len {s}) 1))");
                let neg = format!("(and (str.prefixof \"-\" {s}) (str.in_re {rest} {digits}))");
                let none = format!("(not (str.in_re {s} (re.++ re.all (re.range \"0\" \"9\") re.all)))");
                // true by the definition of `str.to_int` (-1 unless all digits), which Z3 does not find alone
                for x in [&s, &rest] {
                    self.facts.push(format!("(= (>= (str.to_int {x}) 0) (str.in_re {x} {digits}))"));
                }
                let n = self.fresh_why("Int", "`parse_int` of a string with whitespace, `+` or `_`");
                let payload = format!("(ite {pos} (str.to_int {s}) (ite {neg} (- (str.to_int {rest})) {n}))");
                let some = self.fresh_why("Bool", "`parse_int` of a string with whitespace, `+` or `_`");
                let int = TypeExpr::Named("Int".into(), vec![]);
                self.option(&int, &format!("(or {pos} {neg} (and (not {none}) {some}))"), Val::Int(payload))
            }
            // ASCII digits only, and not empty
            ("is_digit", 1) => {
                let s = self.as_str(&arg(0));
                Val::Bool(format!("(str.in_re {s} (re.+ (re.range \"0\" \"9\")))"))
            }
            ("trim", 1) => {
                let s = self.as_str(&arg(0));
                let r = self.fresh_why("String", "`trim` (only its length bound is modelled)");
                self.facts.push(format!("(<= (str.len {r}) (str.len {s}))"));
                Val::Str(r)
            }
            _ => {
                // anything else: a fresh value of the builtin's result type, when it has one
                // (when every version of it returns the same type, free of type variables)
                let rets: Vec<TypeExpr> = crate::types::BUILTINS
                    .iter()
                    .filter(|b| b.name == name)
                    .filter_map(|b| match crate::types::parse_type_str(b.sig) {
                        TypeExpr::Fn(_, ret) if !b.params.split(',').any(|v| !v.is_empty() && crate::printer::ty(&ret).split(|c: char| !c.is_alphanumeric()).any(|w| w == v)) => Some(*ret),
                        _ => None,
                    })
                    .collect();
                let n = crate::types::BUILTINS.iter().filter(|b| b.name == name).count();
                match rets.first() {
                    Some(ret) if rets.len() == n && rets.iter().all(|r| r == ret) => {
                        let old = self.reason.replace(not_modelled);
                        let v = self.shape(ret, 0);
                        self.reason = old;
                        v
                    }
                    _ => Val::Opaque(not_modelled),
                }
            }
        }
    }

    // ---- statements ----

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Comment(_) | Stmt::ComptimeFor(..) => {}
            Stmt::Expr(e) => {
                self.eval(e);
            }
            Stmt::Let(_, ty, e) | Stmt::Var(_, ty, e) => {
                let v = self.eval(e);
                if let Some(t) = ty {
                    if self.refined(t, 0) {
                        let (t2, v2) = (t.clone(), v.clone());
                        self.oblige(format!("`let` annotated `{}`", crate::printer::ty(t)), |vc| vc.check_type(&t2, &v2, &[]));
                    }
                    // past the check, the elements are valid (on this path)
                    if let (Val::List { id, .. }, TypeExpr::Named(n, args)) = (&v, self.env.expand_alias(t)) {
                        if let (true, Some(et)) = (n == "List", args.first()) {
                            let guard = and(&self.pc);
                            self.know_all(*id, guard, Func::Valid(et.clone()));
                        }
                    }
                }
                if let (Some(t), Stmt::Var(x, ..) | Stmt::Let(Pattern::Bind(x), ..)) = (ty, s) {
                    self.declared.entry(x.to_string()).or_default().push(t.clone());
                }
                match s {
                    Stmt::Var(x, ..) => self.bind(x, v),
                    Stmt::Let(p, ..) => {
                        let mut binds = vec![];
                        let c = self.pattern(p, &v, &mut binds);
                        self.pc.push(c);
                        for (x, b) in binds {
                            self.bind(&x, b);
                        }
                    }
                    _ => {}
                }
            }
            Stmt::Assign(place, e) => {
                let v = self.eval(e);
                self.assign(place, v, "assigning");
            }
            Stmt::Assert(e) => {
                let v = self.eval(e);
                let b = self.as_bool(&v);
                self.oblige(format!("assert {}", crate::printer::expr(e, 0)), |_| b);
            }
            Stmt::Break | Stmt::Continue => self.dead = true,
            Stmt::While(c, body) => {
                self.havoc_loop(body, Some(c));
                let cv = self.eval(c);
                let cv = self.as_bool(&cv);
                let saved = self.scopes.clone();
                let n = self.pc.len();
                self.pc.push(cv.clone());
                // candidate bounds, from the condition's comparisons, before the pass
                let has_continue = {
                    let mut found = false;
                    crate::ast::walk(body, |x| {
                        if let Expr::Block(ss, _) = x {
                            found |= ss.iter().any(|s| matches!(s, Stmt::Continue));
                        }
                    });
                    found
                };
                let measures = bounds(c);
                let before: Vec<(String, T)> = measures
                    .iter()
                    .map(|m| {
                        self.spec += 1;
                        let v = self.eval(m);
                        self.spec -= 1;
                        (crate::printer::expr(m, 0), self.as_int(&v))
                    })
                    .collect();
                self.eval(body);
                let mut info = LoopInfo { what: format!("while {}", crate::printer::expr(c, 0)), candidates: vec![], always_exits: self.dead, reason: None };
                if has_continue {
                    info.reason = Some(format!("the loop `{}` uses `continue`", info.what));
                } else if !self.dead {
                    // after the pass: either the loop is over, or the bound went down
                    self.spec += 1;
                    let again = self.eval(c);
                    self.spec -= 1;
                    let again = self.as_bool(&again);
                    for (m, (shown, b)) in measures.iter().zip(before) {
                        self.spec += 1;
                        let v = self.eval(m);
                        self.spec -= 1;
                        let a = self.as_int(&v);
                        let k = self.oblige_termination(format!("loop bound `{shown}`"), format!("(or (not {again}) (and (>= {b} 0) (< {a} {b})))"));
                        info.candidates.push((shown, k));
                    }
                }
                self.loops.push(info);
                self.pc.truncate(n);
                self.dead = false;
                self.scopes = saved;
                self.pc.push(not(&cv));
            }
            Stmt::For(p, xs, body) => {
                let xv = self.eval(xs);
                self.havoc_loop(body, None);
                let saved = self.scopes.clone();
                let n = self.pc.len();
                let x = match &xv {
                    Val::List { len, id } => {
                        let (len, id) = (len.clone(), *id);
                        self.pc.push(format!("(> {len} 0)"));
                        self.element(id, "true")
                    }
                    _ => Val::Opaque("the elements of a list are not tracked".into()),
                };
                self.scopes.push(HashMap::default());
                let mut binds = vec![];
                let c = self.pattern(p, &x, &mut binds);
                self.pc.push(c);
                for (x, b) in binds {
                    self.bind(&x, b);
                }
                self.eval(body);
                self.pc.truncate(n);
                self.dead = false;
                self.scopes = saved;
            }
            Stmt::Loop(body) => {
                self.havoc_loop(body, None);
                let saved = self.scopes.clone();
                let n = self.pc.len();
                self.eval(body);
                self.pc.truncate(n);
                self.dead = false;
                self.scopes = saved;
            }
        }
    }

    /// Store `v` into `place` (`x`, `x.f.g`, possibly marked `mut`), with the
    /// checks the interpreter makes: the variable's declared type, and the
    /// refinements of the record whose field changed.
    fn assign(&mut self, place: &Expr, v: Val, verb: &str) {
        let place = match place {
            Expr::Mut(x) => &**x,
            p => p,
        };
        let mut path = vec![];
        let mut root = place;
        while let Expr::Field(r, f) = root {
            path.push(f.to_string());
            root = r;
        }
        path.reverse();
        let Expr::Var(x) = root else { return };
        let Some(base) = self.lookup(x) else { return };
        let new = if path.is_empty() { v } else { set_path(base, &path, v) };
        // the variable's declared type holds after every assignment
        for t in self.declared.get(x.as_str()).cloned().unwrap_or_default() {
            if self.refined(&t, 0) {
                let n2 = new.clone();
                self.oblige(format!("{verb} `{}`: {}", crate::printer::expr(place, 0), crate::printer::ty(&t)), |vc| vc.check_type(&t, &n2, &[]));
            }
        }
        if !path.is_empty() {
            let tn = match &new {
                Val::Rec(n, _) | Val::Distinct(n, _) => Some(n.clone()),
                Val::Sum { ty, .. } => Some(ty.clone()),
                Val::Opaque(r) => {
                    // whatever the interpreter checks here can't be proven
                    let r = r.clone();
                    self.oblige(format!("{verb} `{}`", crate::printer::expr(place, 0)), |vc| vc.unknown_bool(&r));
                    None
                }
                _ => None,
            };
            if let Some(tn) = tn {
                let t = TypeExpr::Named(Symbol::from(tn), vec![]);
                if self.refined(&t, 0) {
                    let n2 = new.clone();
                    self.oblige(format!("{verb} `{}`", crate::printer::expr(place, 0)), |vc| vc.check_type(&t, &n2, &[]));
                }
            }
        }
        self.set(x, new);
    }

    /// Before a loop: every variable the loop may change takes any value.
    fn havoc_loop(&mut self, body: &Expr, cond: Option<&Expr>) {
        let env = self.env;
        let mut_params = |n: &str| env.mut_params(n);
        let names: HashSet<String> = std::iter::once(body).chain(cond).flat_map(|e| changed_vars(e, &mut_params)).collect();
        for x in names {
            if let Some(v) = self.lookup(&x) {
                let nv = self.refresh(&v);
                // whatever the loop does, a declared type still holds
                for t in self.declared.get(&x).cloned().unwrap_or_default() {
                    let c = self.check_type(&t, &nv, &[]);
                    self.assume(c);
                }
                self.set(&x, nv);
            }
        }
    }

    /// An integer expression over the parameters, evaluated with `binds`.
    fn int_of(&mut self, e: &Expr, binds: Vec<(String, Val)>) -> T {
        let saved = std::mem::replace(&mut self.scopes, vec![binds.into_iter().collect()]);
        self.spec += 1;
        let v = self.eval(e);
        self.spec -= 1;
        self.scopes = saved;
        self.as_int(&v)
    }

    /// The obligation that, at recursive call `site`, the measure goes down:
    /// the measures `ms` compared in order (lexicographically), each never
    /// negative where it decides. Returns the obligation's index.
    pub fn measure_check(&mut self, site: usize, ms: &[Expr]) -> usize {
        let (pc, args) = self.rec_sites[site].clone();
        let before_binds = self.params.clone();
        let after_binds: Vec<(String, Val)> = self.params.iter().map(|p| p.0.clone()).zip(args).collect();
        let mut alts = vec![];
        let mut equal_so_far = vec![];
        for m in ms {
            let b = self.int_of(m, before_binds.clone());
            let a = self.int_of(m, after_binds.clone());
            let mut here = equal_so_far.clone();
            here.push(format!("(>= {b} 0)"));
            here.push(format!("(< {a} {b})"));
            alts.push(and(&here));
            equal_so_far.push(format!("(= {a} {b})"));
        }
        let shown = ms.iter().map(|m| crate::printer::expr(m, 0)).collect::<Vec<_>>().join(", ");
        self.obligations.push(Obligation { what: format!("measure ({shown}) goes down"), goal: or(&alts), pc, facts: self.facts.len(), termination: true });
        self.obligations.len() - 1
    }

    /// The obligations of a function: its body under its parameters'
    /// refinements and `requires`, checked against every contract it meets.
    pub fn function(&mut self, f: &FnDecl, label: &str) {
        self.self_name = label.to_string();
        for p in &f.params {
            let v = self.havoc(&p.ty);
            if p.mutable {
                self.declared.entry(p.name.to_string()).or_default().push(p.ty.clone());
            }
            self.params.push((p.name.to_string(), v.clone()));
            self.bind(&p.name, v);
        }
        let binds = self.params.clone();
        for r in &f.requires {
            let c = self.pred(r, binds.clone());
            self.pc.push(c);
        }
        let res = self.eval(&f.body);
        // the result is checked where the body ends, and at every early return
        let mut exits = vec![];
        if !self.dead {
            exits.push((self.pc.clone(), res, ""));
        }
        for (pc, v) in std::mem::take(&mut self.returns) {
            exits.push((pc, v, " (early return)"));
        }
        for (pc, res, how) in exits {
            self.pc = pc;
            self.dead = false;
            if self.refined(&f.ret, 0) {
                let (t, r, b) = (f.ret.clone(), res.clone(), binds.clone());
                self.oblige(format!("result of `{label}`{how}: {}", crate::printer::ty(&f.ret)), |vc| vc.check_type(&t, &r, &b));
            }
            let mut b2 = binds.clone();
            b2.push(("it".into(), res));
            for en in &f.ensures {
                let b = b2.clone();
                let n = self.pc.len();
                self.oblige(format!("ensures {}{how}", crate::printer::expr(en, 0)), |vc| vc.pred(en, b));
                self.pc.truncate(n);
            }
        }
    }
}

/// Candidate bounds of a loop: for each comparison in its condition (`a < b`,
/// `a <= b`, ...), how far it is from becoming false.
fn bounds(c: &Expr) -> Vec<Expr> {
    let sub = |a: &Expr, b: &Expr| Expr::Bin(BinOp::Sub, Box::new(a.clone()), Box::new(b.clone()));
    let plus1 = |e: Expr| Expr::Bin(BinOp::Add, Box::new(e), Box::new(Expr::Int(1)));
    match c {
        Expr::Bin(BinOp::And, l, r) => {
            let mut v = bounds(l);
            v.extend(bounds(r));
            v
        }
        Expr::Bin(BinOp::Lt, a, b) => vec![sub(b, a)],
        Expr::Bin(BinOp::Le, a, b) => vec![plus1(sub(b, a))],
        Expr::Bin(BinOp::Gt, a, b) => vec![sub(a, b)],
        Expr::Bin(BinOp::Ge, a, b) => vec![plus1(sub(a, b))],
        Expr::Bin(BinOp::Ne, a, b) => vec![sub(a, b), sub(b, a)],
        _ => vec![],
    }
}

/// Does checking a value against `t` involve any refinement?
pub fn type_is_refined(env: &Env, t: &TypeExpr) -> bool {
    Vc::new(env).refined(t, 0)
}

fn set_path(base: Val, path: &[String], v: Val) -> Val {
    let Some((f, rest)) = path.split_first() else { return v };
    match base {
        Val::Rec(n, mut fs) => {
            if let Some(slot) = fs.iter_mut().find(|x| &x.0 == f) {
                let old = std::mem::replace(&mut slot.1, Val::Unit);
                slot.1 = set_path(old, rest, v);
            }
            Val::Rec(n, fs)
        }
        // a field of a sum or union value: which member's field changed is
        // not tracked, so the whole value is unknown
        _ => Val::Opaque("a value whose field was assigned".into()),
    }
}

/// Variables a loop may change: assigned ones, and those passed to `mut` parameters.
// ---- regular expressions (smt/regexes.rs) ----

mod regexes;
pub use regexes::*;

// ---- the solver ----

#[derive(Debug, Clone)]
pub enum Outcome {
    Proven,
    /// The solver found values that break the obligation (not yet confirmed
    /// by running them): parameter name -> value.
    Refuted(Vec<(String, Value)>, String),
    Unknown(String),
}

/// Which SMT solver runs the queries: they are plain SMT-LIB, only the
/// command line differs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SolverKind {
    Z3,
    Cvc5,
}

#[derive(Clone, Debug)]
pub struct Solver {
    pub bin: String,
    /// What `--version` printed (part of the verification cache key).
    pub version: String,
    pub kind: SolverKind,
}

/// The solver: `STRATA_SOLVER` (a path or a command on the `PATH`), else
/// `z3`; `None` if it can't be run. A `--version` that names cvc5 means
/// cvc5's flags; anything else is run like Z3.
pub fn solver() -> Option<Solver> {
    let bin = std::env::var("STRATA_SOLVER").unwrap_or_else(|_| "z3".into());
    let out = Command::new(&bin).arg("--version").output().ok()?;
    let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let kind = if version.to_lowercase().contains("cvc5") { SolverKind::Cvc5 } else { SolverKind::Z3 };
    out.status.success().then_some(Solver { bin, version, kind })
}

impl Solver {
    /// The command line: a deterministic resource limit per query (so
    /// results can be cached), and a time limit only as a backstop.
    fn args(&self) -> Vec<String> {
        match self.kind {
            SolverKind::Z3 => vec!["-in".into(), "-T:20".into(), format!("rlimit={Z3_RLIMIT}")],
            SolverKind::Cvc5 => vec!["--lang".into(), "smt2".into(), "--produce-models".into(), format!("--rlimit-per={CVC5_RLIMIT}"), "--tlimit-per=20000".into()],
        }
    }
}

/// Resource limits per obligation: deterministic, unlike a timeout. The
/// units differ; both are a few seconds of hard nonlinear arithmetic.
const Z3_RLIMIT: u64 = 20_000_000;
const CVC5_RLIMIT: u64 = 5_000_000;

impl Vc<'_> {
    /// The free constants an obligation depends on (its cone of influence).
    fn cone(&self, ob: &Obligation, seeds: &[String]) -> (Vec<T>, HashSet<String>) {
        let consts = |t: &str| -> Vec<String> { t.split(|c: char| !c.is_alphanumeric() && c != '_').filter(|w| w.starts_with('v') && w[1..].chars().all(|c| c.is_ascii_digit()) && w.len() > 1).map(String::from).collect() };
        let facts = &self.facts[..ob.facts];
        let mut seen: HashSet<String> = consts(&ob.goal).into_iter().chain(ob.pc.iter().flat_map(|p| consts(p))).chain(seeds.iter().cloned()).collect();
        let fact_consts: Vec<Vec<String>> = facts.iter().map(|f| consts(f)).collect();
        let mut used = vec![false; facts.len()];
        loop {
            let mut changed = false;
            for (i, fc) in fact_consts.iter().enumerate() {
                if !used[i] && fc.iter().any(|c| seen.contains(c)) {
                    used[i] = true;
                    changed = true;
                    seen.extend(fc.iter().cloned());
                }
            }
            if !changed {
                break;
            }
        }
        let kept = facts.iter().zip(&used).filter(|(_, u)| **u).map(|(f, _)| f.clone()).collect();
        (kept, seen)
    }

    fn script(&self, ob: &Obligation, leaves: &[String]) -> String {
        // the parameters are in the cone too, so a model satisfies their types
        let (facts, consts) = self.cone(ob, leaves);
        let mut s = String::from("(set-option :produce-models true)\n(set-logic ALL)\n");
        for (n, sort) in &self.decls {
            if consts.contains(n) || leaves.contains(n) {
                s.push_str(&format!("(declare-const {n} {sort})\n"));
            }
        }
        for f in facts {
            s.push_str(&format!("(assert {f})\n"));
        }
        for p in &ob.pc {
            s.push_str(&format!("(assert {p})\n"));
        }
        s.push_str(&format!("(assert (not {}))\n(check-sat)\n", ob.goal));
        if !leaves.is_empty() {
            s.push_str(&format!("(get-value ({}))\n", leaves.join(" ")));
        }
        s
    }

    /// Why an obligation could not be proven: the unmodelled things it depends on.
    fn why(&self, ob: &Obligation) -> Vec<String> {
        let (_, consts) = self.cone(ob, &[]);
        let mut rs: Vec<String> = consts.iter().filter_map(|c| self.reasons.get(c).cloned()).collect();
        rs.sort();
        rs.dedup();
        rs
    }

    /// The SMT-LIB text of an obligation (for `--smt`).
    pub fn smtlib(&self, i: usize) -> String {
        self.script(&self.obligations[i], &[])
    }

    pub fn solve(&self, solver: &Solver, i: usize) -> Outcome {
        let ob = &self.obligations[i];
        let mut leaves = vec![];
        for (_, v) in &self.params {
            leaves_of(v, &mut leaves);
        }
        let script = self.script(ob, &leaves);
        let run = || -> Option<String> {
            let mut child = Command::new(&solver.bin).args(solver.args()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
            child.stdin.take()?.write_all(script.as_bytes()).ok()?;
            let out = child.wait_with_output().ok()?;
            Some(String::from_utf8_lossy(&out.stdout).to_string())
        };
        let Some(out) = run() else { return Outcome::Unknown("the solver could not be run".into()) };
        let first = out.lines().next().unwrap_or("").trim();
        let why = self.why(ob);
        let because = if why.is_empty() { String::new() } else { format!(" (depends on {})", why.join("; ")) };
        match first {
            "unsat" => Outcome::Proven,
            "sat" => {
                let model = parse_values(&out[first.len()..]);
                let args: Option<Vec<(String, Value)>> = self.params.iter().map(|(n, v)| to_value(self.env, v, &model).map(|x| (n.clone(), x))).collect();
                let shown = |args: &[(String, Value)]| args.iter().map(|(n, v)| format!("{n} = {}", crate::interp::to_json(v))).collect::<Vec<_>>().join(", ");
                match args {
                    Some(a) => {
                        let s = shown(&a);
                        Outcome::Refuted(a, format!("the solver found a possible counterexample: {s}{because}"))
                    }
                    None => Outcome::Unknown(format!("the solver found a possible counterexample it could not express{because}")),
                }
            }
            _ => Outcome::Unknown(format!("the solver gave up{because}")),
        }
    }
}

fn leaves_of(v: &Val, out: &mut Vec<String>) {
    match v {
        Val::Int(t) | Val::Bool(t) | Val::Str(t) => out.push(t.clone()),
        Val::Tuple(xs) => xs.iter().for_each(|x| leaves_of(x, out)),
        Val::Rec(_, fs) => fs.iter().for_each(|f| leaves_of(&f.1, out)),
        Val::Sum { tag, ctors, .. } => {
            out.push(tag.clone());
            ctors.iter().flat_map(|c| &c.1).for_each(|x| leaves_of(x, out));
        }
        Val::Union { tag, slots } => {
            out.push(tag.clone());
            slots.iter().for_each(|s| leaves_of(&s.1, out));
        }
        Val::List { len, .. } => out.push(len.clone()),
        Val::Distinct(_, x) => leaves_of(x, out),
        Val::Unit | Val::Absent | Val::Opaque(_) | Val::FnOf { .. } => {}
    }
}

/// A parameter's value in a model, as an interpreter value (when it can be
/// rebuilt: lists only when empty, nothing opaque).
fn to_value(env: &Env, v: &Val, m: &HashMap<String, String>) -> Option<Value> {
    Some(match v {
        Val::Int(t) => crate::interp::int_value(parse_big(m.get(t)?)?),
        Val::Bool(t) => Value::Bool(m.get(t)? == "true"),
        Val::Str(t) => Value::Str(parse_str(m.get(t)?)?),
        Val::Unit => Value::Unit,
        Val::Tuple(xs) => Value::Tuple(xs.iter().map(|x| to_value(env, x, m)).collect::<Option<_>>()?),
        Val::Rec(n, fs) => Value::Record(n.clone(), fs.iter().map(|(f, x)| to_value(env, x, m).map(|y| (f.clone(), y))).collect::<Option<_>>()?),
        Val::Distinct(n, x) => Value::Distinct(n.clone(), Box::new(to_value(env, x, m)?)),
        Val::List { len, .. } => {
            if parse_int(m.get(len)?)? != 0 {
                return None;
            }
            Value::List(vec![])
        }
        Val::Sum { ty, args, tag, ctors } => {
            let k = parse_int(m.get(tag)?)? as usize;
            let (c, payload) = ctors.get(k)?;
            let vs = env.variants_of(ty, args)?;
            let fields: Vec<Value> = payload.iter().map(|x| to_value(env, x, m)).collect::<Option<_>>()?;
            let f = match &vs.get(k)?.1 {
                _ if fields.is_empty() => Fields::None,
                Some(names) => Fields::rec(names.iter().map(|n| n.to_string()).zip(fields).collect()),
                None => Fields::pos(fields),
            };
            Value::Variant(c.clone(), f)
        }
        // a union value is its active member's value
        Val::Union { tag, slots } => to_value(env, &slots.get(parse_int(m.get(tag)?)? as usize)?.1, m)?,
        // a function can't be shown as a counterexample
        Val::Absent | Val::Opaque(_) | Val::FnOf { .. } => return None,
    })
}

/// A model's integer, of any size: `42` or `(- 42)`.
fn parse_big(s: &str) -> Option<num_bigint::BigInt> {
    let s = s.trim();
    if let Some(inner) = s.strip_prefix("(-").and_then(|x| x.strip_suffix(')')) {
        return inner.trim().parse::<num_bigint::BigInt>().ok().map(|n| -n);
    }
    s.parse().ok()
}

fn parse_int(s: &str) -> Option<i64> {
    let s = s.trim();
    if let Some(inner) = s.strip_prefix("(-").and_then(|x| x.strip_suffix(')')) {
        return inner.trim().parse::<i64>().ok().map(|n| -n);
    }
    s.parse().ok()
}

fn parse_str(s: &str) -> Option<String> {
    let inner = s.strip_prefix('"')?.strip_suffix('"')?.replace("\"\"", "\"");
    let mut out = String::new();
    let mut rest = inner.as_str();
    while let Some(p) = rest.find("\\u{") {
        out.push_str(&rest[..p]);
        let close = rest[p..].find('}')? + p;
        out.push(char::from_u32(u32::from_str_radix(&rest[p + 3..close], 16).ok()?)?);
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// Parse `((v1 5) (v2 "a") (v3 (- 2)))` into name -> value text.
fn parse_values(s: &str) -> HashMap<String, String> {
    let mut out = HashMap::default();
    let cs: Vec<char> = s.chars().collect();
    let mut i = cs.iter().position(|&c| c == '(').map(|p| p + 1).unwrap_or(cs.len());
    while i < cs.len() {
        while i < cs.len() && cs[i] != '(' && cs[i] != ')' {
            i += 1;
        }
        if i >= cs.len() || cs[i] == ')' {
            break;
        }
        i += 1;
        let start = i;
        while i < cs.len() && !cs[i].is_whitespace() {
            i += 1;
        }
        let name: String = cs[start..i].iter().collect();
        while i < cs.len() && cs[i].is_whitespace() {
            i += 1;
        }
        // the value: a string, a parenthesised term, or an atom
        let vstart = i;
        let mut depth = 0;
        let mut in_str = false;
        while i < cs.len() {
            let c = cs[i];
            if in_str {
                if c == '"' {
                    if cs.get(i + 1) == Some(&'"') {
                        i += 1;
                    } else {
                        in_str = false;
                    }
                }
            } else if c == '"' {
                in_str = true;
            } else if c == '(' {
                depth += 1;
            } else if c == ')' {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            i += 1;
        }
        out.insert(name, cs[vstart..i].iter().collect::<String>().trim().to_string());
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regexes() {
        // a literal string is one literal; the anchors the interpreter adds are no-ops
        assert_eq!(regex_to_smt("ab").unwrap(), "(str.to_re \"ab\")");
        assert_eq!(regex_to_smt("^ab$").unwrap(), "(str.to_re \"ab\")");
        assert_eq!(regex_to_smt("a|^b$").unwrap(), "(re.union (str.to_re \"a\") (str.to_re \"b\"))");
        assert!(regex_to_smt("ord_[a-z0-9]{6}").unwrap().contains("((_ re.loop 6 6) (re.union (re.range \"0\" \"9\") (re.range \"a\" \"z\")))"));
        assert!(regex_to_smt("[A-Za-z0-9_-]{1,64}").is_some());
        assert_eq!(regex_to_smt("a(b|c)*d?").unwrap(), "(re.++ (str.to_re \"a\") (re.* (re.range \"b\" \"c\")) (re.opt (str.to_re \"d\")))");
        assert_eq!(regex_to_smt("a{2,}").unwrap(), "(re.++ ((_ re.loop 2 2) (str.to_re \"a\")) (re.* (str.to_re \"a\")))");
        // Unicode classes: \d is every decimal digit, not just 0-9
        let has = |re: &str, c: char| in_class(&regex_to_smt(re).unwrap(), c);
        assert!(has("\\d", '7') && has("\\d", '\u{663}') && has("\\d", '\u{ff19}') && !has("\\d", 'a') && !has("\\d", '\u{b2}'));
        assert!(has("\\w", '_') && has("\\w", '\u{e9}') && has("\\w", '\u{4e2d}') && has("\\w", '9') && !has("\\w", '-') && !has("\\w", ' '));
        for c in ['\t', '\n', '\r', ' ', '\u{85}', '\u{a0}', '\u{2028}', '\u{3000}'] {
            assert!(has("\\s", c), "{c:?}");
        }
        assert!(!has("\\s", 'a') && !has("\\s", '\u{200b}'));
        assert!(has("\\p{L}", 'A') && has("\\p{L}", '\u{3b1}') && has("\\p{L}", '\u{4e2d}') && !has("\\p{L}", '1') && !has("\\p{L}", '_'));
        // (?i) is Unicode case folding: k also matches K and the Kelvin sign
        for c in ['k', 'K', '\u{212a}'] {
            assert!(has("(?i)k", c), "{c:?}");
        }
        assert!(!has("(?i)k", 'j'));
        // each class agrees with the interpreter's regex on the characters SMT strings have
        for re in ["\\d", "\\w", "\\s", "\\p{L}", "(?i)k", "(?i)[a-zß]", "[^a]", "."] {
            let ranges = parsed_ranges(&regex_to_smt(re).unwrap());
            let r = regex::Regex::new(&format!("^(?:{re})$")).unwrap();
            for cp in (0..=MAX_CHAR).step_by(7).chain(0..0x3000) {
                if let Some(c) = char::from_u32(cp) {
                    let ours = ranges.iter().any(|(lo, hi)| *lo <= c && c <= *hi);
                    assert_eq!(ours, r.is_match(&c.to_string()), "{re} on U+{cp:X}");
                }
            }
        }
        // large classes are approximated from both sides: under <= exact <= over, on every character
        assert!(matches!(regex_model("\\d{3}"), Some(RegexModel::Between(..))) && matches!(regex_model("[a-z_]+"), Some(RegexModel::Exact(_))));
        for re in ["\\w", "\\p{L}", "\\d", "(?i)\\w"] {
            let Some(RegexModel::Between(u, o)) = regex_model(re) else { panic!("{re} should be approximated") };
            let (e, u, o) = (parsed_ranges(&regex_to_smt(re).unwrap()), parsed_ranges(&u), parsed_ranges(&o));
            assert!(u.len() <= MAX_RANGES && o.len() <= MAX_RANGES, "{re}");
            let inside = |rs: &[(char, char)], c: char| rs.iter().any(|(lo, hi)| *lo <= c && c <= *hi);
            for c in (0..=MAX_CHAR).filter_map(char::from_u32) {
                assert!(!inside(&u, c) || inside(&e, c), "{re}: under has U+{:X}", c as u32);
                assert!(!inside(&e, c) || inside(&o, c), "{re}: over misses U+{:X}", c as u32);
            }
        }
        let Some(RegexModel::Between(u, _)) = regex_model("\\w") else { unreachable!() };
        assert!(['a', 'z', 'A', 'Z', '0', '9', '_', '\u{e9}'].iter().all(|c| in_class(&u, *c)), "the ASCII part of \\w is kept exactly: {u}");
        // `.` is any character but \n, up to the last one SMT strings have
        let dot = regex_to_smt(".").unwrap();
        assert_eq!(dot, "(re.union (re.range \"\\u{0}\" \"\\u{9}\") (re.range \"\\u{b}\" \"\\u{2ffff}\"))");
        // what is not certain stays unknown
        for re in ["\\bword\\b", "(?m)^a$", "a^b", "a$b", "(^a)*", "\u{10FFFF}", "(?-u:\\xFF)", "(", "a{3,2}"] {
            assert!(regex_to_smt(re).is_none(), "{re}");
        }
    }

    /// The ranges of a translated character class (a union of ranges and single characters).
    fn parsed_ranges(t: &str) -> Vec<(char, char)> {
        let lits: Vec<char> = t.split('"').skip(1).step_by(2).map(|x| parse_str(&format!("\"{x}\"")).unwrap().chars().next().unwrap()).collect();
        let mut i = 0;
        let mut out = vec![];
        for part in t.split('(').skip(1) {
            if part.starts_with("re.range ") {
                out.push((lits[i], lits[i + 1]));
                i += 2;
            } else if part.starts_with("str.to_re ") {
                out.push((lits[i], lits[i]));
                i += 1;
            }
        }
        out
    }
    fn in_class(t: &str, c: char) -> bool {
        parsed_ranges(t).iter().any(|(lo, hi)| *lo <= c && c <= *hi)
    }

    #[test]
    fn values() {
        let m = parse_values("((v1 5)\n (v2 \"a\"\"b\")\n (v3 (- 2)))");
        assert_eq!(parse_int(&m["v1"]), Some(5));
        assert_eq!(parse_str(&m["v2"]).as_deref(), Some("a\"b"));
        assert_eq!(parse_int(&m["v3"]), Some(-2));
        assert_eq!(parse_str("\"\\u{e9}\"").as_deref(), Some("é"));
        // cvc5 prints the model on one line, and a backslash as \u{5c}
        let m = parse_values("((v0 (- 6)) (v1 \"a\\u{e9}\\u{0}\\u{1f600}\\u{a}\"\"7\\u{5c}\\u{2ffff}\") (v2 false) (v3 \"ba\"))");
        assert_eq!(parse_int(&m["v0"]), Some(-6));
        assert_eq!(parse_str(&m["v1"]).as_deref(), Some("aé\u{0}😀\n\"7\\\u{2ffff}"));
        assert_eq!(m["v2"], "false");
        assert_eq!(parse_str(&m["v3"]).as_deref(), Some("ba"));
    }
}
