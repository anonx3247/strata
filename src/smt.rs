//! SMT verification: a function's contracts become proof obligations that an
//! SMT solver (Z3) proves or refutes.
//!
//! The body is executed symbolically. Values are SMT terms (`Int`, `Bool`,
//! `String`), records and sums are kept as structures of terms (a sum is a tag
//! plus the payload of every constructor), whose structured parts are made
//! when the code first reads them (`Val::Lazy`), and lists are tracked by position
//! where they can be (a checked list's element is a function of its position;
//! slices, `+` and literals translate positions) and otherwise by their
//! length only. A branch splits the path condition and the two results are
//! merged with `ite`. A loop forgets what it changes: those variables become
//! fresh values that keep only what their types guarantee, and the loop's
//! invariants, inferred (`infer_invariants`). After the loop, the state is that one with a `while`'s
//! condition false, or the state at one of its `break`s.
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
    /// A part of a value (a field of a record, the payload of a constructor)
    /// not looked at yet: `Vc::cells[id]`, made the first time it is read
    /// (`Vc::force`), so that the cost follows what the code reads rather
    /// than the size of the type.
    Lazy(usize),
}

/// How a lazy part is made when it is first read.
#[derive(Clone, Debug)]
enum Make {
    /// A fresh value of the cell's type.
    Shape,
    /// The part `of` (of a list's element template) at position `i` inside `outer` (see `place`).
    View { of: usize, i: T, outer: Vec<T> },
    /// Part `a` when `c` holds, else part `b` (two branches merged).
    Merge { c: T, a: usize, b: usize },
}

/// A lazy part of a value (`Val::Lazy`).
#[derive(Clone, Debug)]
struct Cell {
    ty: TypeExpr,
    make: Make,
    /// "The part is a valid `ty`": `true` when `ty` has no refinement, else
    /// an unknown whose definition is assumed when the part is made. A check
    /// of the whole value is this term for the part, so checking or assuming
    /// the validity of a value never makes its parts.
    valid: T,
    /// The context it was made in: fresh unknowns are functions of these
    /// positions, and have this reason.
    indexing: Option<Vec<String>>,
    reason: Option<String>,
    /// The scope (`Vc::regions`) it belongs to; 0 for the function's.
    region: usize,
    /// Made with placeholders (a part of a list's element template): what is
    /// known of it is kept in `facts`, placed for each position, rather than
    /// assumed.
    placeholder: bool,
    value: Option<Val>,
    facts: Vec<T>,
}

/// A scope whose facts and unknowns are taken apart from the rest when it
/// ends (a quantified fact, a list's element template): where they start.
#[derive(Clone, Debug)]
struct Region {
    id: usize,
    nd: usize,
    nf: usize,
}

/// The prover's size budget: at most this many lazy parts per function;
/// past it, parts are `Opaque` (any value).
const MAX_CELLS: usize = 200_000;

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
    /// The negation of a function to Bool (`any(xs, f)` is `!all(xs, !f)`).
    Not(Box<Func>),
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
    /// (two branches merged): the same position in either.
    Either(T, usize, usize),
    /// The elements of another list from a position on (`drop`, `slice`,
    /// `take`, a `..rest` pattern): element `i` is the other's `off + i`.
    Shift(usize, T),
    /// Another list backwards (`reverse`), of this length.
    Rev(usize, T),
    /// The first list, then the second (`a + b`, `push`), the first of this
    /// length.
    Concat(usize, usize, T),
}

/// The element of a list of a checked type at a placeholder position
/// (`IDX`): its unknowns are functions of the position, so equal positions
/// hold equal elements. What is known of it holds for every element.
#[derive(Clone, Debug)]
struct Template {
    val: Val,
    facts: Vec<T>,
}

/// A candidate loop invariant: a Bool expression over the variables in
/// scope, `$entry_x` for `x` before the loop, and `it` for `it` (an
/// `ensures`, of the variable the function returns).
#[derive(Clone, Debug)]
struct Candidate {
    e: Expr,
    it: Option<String>,
}

impl Candidate {
    fn shown(&self) -> String {
        let s = rename_entry(&crate::printer::expr(&self.e, 0));
        match &self.it {
            Some(x) => format!("{s} (it = {x})"),
            None => s,
        }
    }
}

/// `$entry_x` written `x₀` (its value on entry).
fn rename_entry(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(k) = rest.find("$entry_") {
        out.push_str(&rest[..k]);
        let after = &rest[k + 7..];
        let n = after.chars().take_while(|c| c.is_alphanumeric() || *c == '_').map(|c| c.len_utf8()).sum::<usize>();
        out.push_str(&after[..n]);
        out.push('₀');
        rest = &after[n..];
    }
    out.push_str(rest);
    out
}

/// At most this many candidate invariants per loop.
const MAX_CANDIDATES: usize = 40;

/// The most elements a list made by the solver as an input has, and how many
/// levels of lists and sums inside an input are made.
pub const INPUT_LEN: usize = 6;
const INPUT_DEPTH: usize = 3;

/// Stands for the position in a `Template`'s terms; `@@o0`, `@@o1`, … for
/// the positions of the lists it is inside of.
const IDX: &str = "@@i";

/// `t` with `IDX` replaced by `own` and `@@oK` by `outer[K]`, all at once
/// (a replacement may itself hold a placeholder).
fn place(t: &str, own: &str, outer: &[T]) -> T {
    if !t.contains("@@") {
        return t.to_string();
    }
    let mut out = String::with_capacity(t.len());
    let mut rest = t;
    while let Some(k) = rest.find("@@") {
        out.push_str(&rest[..k]);
        let after = &rest[k + 2..];
        if let Some(a) = after.strip_prefix('i') {
            out.push_str(own);
            rest = a;
        } else if let Some(a) = after.strip_prefix('o') {
            let n = a.chars().take_while(|c| c.is_ascii_digit()).count();
            let idx: usize = a[..n].parse().unwrap_or(0);
            out.push_str(outer.get(idx).map(String::as_str).unwrap_or("0"));
            rest = &a[n..];
        } else {
            out.push_str("@@");
            rest = after;
        }
    }
    out.push_str(rest);
    out
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
    /// For a list of a checked type: its element at any position.
    template: Option<Template>,
    /// The positions this list is inside of (an element's list in a list of
    /// records is made once per element): its elements' unknowns are
    /// functions of these and of their own position.
    outer: Vec<T>,
    /// The list whose template this one shares (a copy made per element).
    tpl_from: Option<usize>,
    /// Elements taken by position, by the position's term, with what was
    /// assumed of them (assumed again on another path).
    at: HashMap<String, (Val, Vec<T>)>,
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
    /// For each loop being run, innermost last: the path and the variables
    /// at each `break`, merged into the state after the loop.
    breaks: Vec<Vec<(Vec<T>, Vec<HashMap<String, Val>>)>>,
    /// The same at each `continue`: where a pass ends early.
    continues: Vec<Vec<(Vec<T>, Vec<HashMap<String, Val>>)>>,
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
    /// Fresh unknowns are functions of these positions: the placeholders
    /// while making a `Template`, a quantified fact's variable while making it.
    indexing: Option<Vec<String>>,
    /// Names of the variables of quantified facts made so far.
    qvars: usize,
    /// Unknowns bound by a quantifier (`every`): not free in what contains it.
    bound: HashSet<String>,
    /// The functions passed to the builtin being called, by argument.
    fn_args: Vec<Option<Func>>,
    /// The constant standing for each bit operation met (`& x y`), so one met
    /// twice is one value.
    bit_terms: HashMap<String, T>,
    /// The lazy parts of values (`Val::Lazy`).
    cells: Vec<Cell>,
    /// For inputs made by the solver (`inputs`): each list's first elements.
    input_elems: HashMap<usize, Vec<Val>>,
    /// The solver, to infer loop invariants while running the body (none:
    /// loops keep only their variables' declared types).
    pub solver: Option<Solver>,
    /// Inside a trial pass of a loop (`infer_invariants`): inner loops infer nothing.
    trial: usize,
    /// The function's `ensures`, each with the variable its body returns
    /// (`it`), and its `requires`: candidate invariants.
    hints: Vec<(Expr, String)>,
    requires_hints: Vec<Expr>,
    /// The invariants inferred, as shown in the report.
    pub invariants: Vec<String>,
    /// The scopes open, innermost last (see `Region`).
    regions: Vec<Region>,
    next_region: usize,
    /// For two parts compared before either was made: the unknown that
    /// stands for their equality, and its scope. Its definition (the
    /// comparison of the made parts) is assumed when one of them is made.
    eq_atoms: HashMap<(usize, usize), (T, usize)>,
    /// By part: the parts it was compared with, the equality's unknown and scope.
    pending: HashMap<usize, Vec<(usize, T, usize)>>,
    /// Comparing parts made from list element templates, nested this deep.
    eq_depth: usize,
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
/// The sort of a function from a position to a value of `sort`.
fn fn_sort(sort: &str, arity: usize) -> Option<&'static str> {
    Some(match (sort, arity) {
        ("Int", 1) => "(Int) Int",
        ("Bool", 1) => "(Int) Bool",
        ("String", 1) => "(Int) String",
        ("Int", 2) => "(Int Int) Int",
        ("Bool", 2) => "(Int Int) Bool",
        ("String", 2) => "(Int Int) String",
        ("Int", 3) => "(Int Int Int) Int",
        ("Bool", 3) => "(Int Int Int) Bool",
        ("String", 3) => "(Int Int Int) String",
        _ => return None,
    })
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
        Val::Union { .. } | Val::Absent | Val::Opaque(_) | Val::FnOf { .. } | Val::Lazy(_) => return None,
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
            breaks: vec![],
            continues: vec![],
            declared: HashMap::default(),
            matched: vec![],
            lists: vec![],
            binding: 0,
            indexing: None,
            qvars: 0,
            bound: HashSet::default(),
            fn_args: vec![],
            bit_terms: HashMap::default(),
            cells: vec![],
            input_elems: HashMap::default(),
            solver: None,
            trial: 0,
            hints: vec![],
            requires_hints: vec![],
            invariants: vec![],
            regions: vec![],
            next_region: 1,
            eq_atoms: HashMap::default(),
            pending: HashMap::default(),
            eq_depth: 0,
        }
    }

    fn fresh(&mut self, sort: &'static str) -> T {
        let n = format!("v{}", self.decls.len());
        let indexed = self.indexing.as_ref().and_then(|args| fn_sort(sort, args.len()));
        self.decls.push((n.clone(), indexed.unwrap_or(sort)));
        if let Some(r) = &self.reason {
            self.reasons.insert(n.clone(), r.clone());
        }
        match (indexed, &self.indexing) {
            (Some(_), Some(args)) => format!("({n} {})", args.join(" ")),
            _ => n,
        }
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
        // (the prelude's names as written: `self.len()`, not `self.FiniteIterator::std.prelude.len()`)
        let what = crate::types::shown_name(&what);
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
        self.lists.push(ListInfo { source: gen_, preds: vec![], seen: vec![], template: None, at: HashMap::default(), outer: self.indexing.clone().unwrap_or_default(), tpl_from: None });
        Val::List { len, id: self.lists.len() - 1 }
    }

    /// The list a value is (an unknown list for a value that isn't one).
    fn list_id(&mut self, v: &Val) -> usize {
        match v {
            Val::List { id, .. } => *id,
            _ => {
                self.lists.push(ListInfo { source: Gen::Unknown, preds: vec![], seen: vec![], template: None, at: HashMap::default(), outer: vec![], tpl_from: None });
                self.lists.len() - 1
            }
        }
    }

    /// The type every element of a list is known to have, if one is.
    fn elem_type(&self, id: usize) -> Option<TypeExpr> {
        match &self.lists[id].source {
            Gen::Type(t) => Some(t.clone()),
            Gen::Sub(p) | Gen::Filter(p, _) | Gen::Shift(p, _) | Gen::Rev(p, _) => self.elem_type(*p),
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
            Gen::Sub(p) | Gen::Filter(p, _) | Gen::Shift(p, _) | Gen::Rev(p, _) => self.valid_known(*p, t),
            Gen::Either(_, a, b) | Gen::Concat(a, b, _) => self.valid_known(*a, t) && self.valid_known(*b, t),
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
            Func::Not(f) => {
                let v = self.apply(f, x);
                Val::Bool(not(&self.as_bool(&v)))
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
            Gen::Sub(p) | Gen::Shift(p, _) | Gen::Rev(p, _) => self.element_inner(p),
            // an element of `a + b` is one of `a`'s or one of `b`'s
            Gen::Concat(a, b, _) => {
                let c = self.fresh("Bool");
                let n = self.pc.len();
                self.pc.push(c.clone());
                let ea = self.element_inner(a);
                self.pc.truncate(n);
                self.pc.push(not(&c));
                let eb = self.element_inner(b);
                self.pc.truncate(n);
                self.merge(&c, &ea, &eb)
            }
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

    /// The element at position `i` of list `id`, when the list's positions
    /// are known (`None` for `filter`, `sort`, a list nothing is known of).
    /// It exists when `exists` holds, and what is known of it is assumed
    /// there.
    fn element_at(&mut self, id: usize, i: &str, exists: &str) -> Option<Val> {
        let n = self.pc.len();
        self.pc.push(exists.to_string());
        let x = self.element_at_inner(id, i);
        self.pc.truncate(n);
        x
    }

    /// Can elements of list `id` be taken by position?
    fn positional(&self, id: usize) -> bool {
        match &self.lists[id].source {
            Gen::Type(_) | Gen::Values(_) | Gen::Range(..) => true,
            Gen::Shift(p, _) | Gen::Rev(p, _) | Gen::Map(p, _) => self.positional(*p),
            Gen::Either(_, a, b) | Gen::Concat(a, b, _) => self.positional(*a) && self.positional(*b),
            Gen::Unknown | Gen::Sub(_) | Gen::Filter(..) => false,
        }
    }

    fn element_at_inner(&mut self, id: usize, i: &str) -> Option<Val> {
        if !self.positional(id) {
            return None;
        }
        let x = match self.lists[id].source.clone() {
            Gen::Type(_) => self.instance(id, i)?,
            Gen::Values(vs) if vs.is_empty() => {
                self.assume("false".into());
                Val::Opaque("an element of an empty list".into())
            }
            Gen::Values(vs) => {
                let mut v = vs.last().unwrap().clone();
                for k in (0..vs.len() - 1).rev() {
                    v = self.merge(&format!("(= {i} {k})"), &vs[k], &v);
                }
                v
            }
            Gen::Range(lo, _) => Val::Int(format!("(+ {lo} {i})")),
            Gen::Shift(p, off) => self.element_at_inner(p, &format!("(+ {off} {i})"))?,
            Gen::Rev(p, l) => self.element_at_inner(p, &format!("(- {l} 1 {i})"))?,
            Gen::Map(p, f) => {
                let y = self.element_at_inner(p, i)?;
                self.apply(&f, y)
            }
            Gen::Concat(a, b, la) => {
                let c = format!("(< {i} {la})");
                self.positions_either(&c, (a, i.to_string()), (b, format!("(- {i} {la})")))?
            }
            Gen::Either(c, a, b) => self.positions_either(&c, (a, i.to_string()), (b, i.to_string()))?,
            Gen::Unknown | Gen::Sub(_) | Gen::Filter(..) => return None,
        };
        for (g, f) in self.lists[id].preds.clone() {
            let p = self.apply(&f, x.clone());
            let p = self.as_bool(&p);
            self.assume(implies(&g, &p));
        }
        if self.binding == 0 {
            let exists = and(&self.pc);
            self.lists[id].seen.push((exists, x.clone()));
        }
        Some(x)
    }

    /// The element at position `ia` of list `a` when `c` holds, else at `ib` of `b`.
    fn positions_either(&mut self, c: &str, (a, ia): (usize, String), (b, ib): (usize, String)) -> Option<Val> {
        // one side is `[]`: the element is the other's
        let empty = |vc: &Self, l: usize| matches!(&vc.lists[l].source, Gen::Values(vs) if vs.is_empty());
        if empty(self, a) {
            self.assume(not(c));
            return self.element_at_inner(b, &ib);
        }
        if empty(self, b) {
            self.assume(c.to_string());
            return self.element_at_inner(a, &ia);
        }
        let n = self.pc.len();
        self.pc.push(c.to_string());
        let ea = self.element_at_inner(a, &ia);
        self.pc.truncate(n);
        self.pc.push(not(c));
        let eb = self.element_at_inner(b, &ib);
        self.pc.truncate(n);
        Some(self.merge(c, &ea?, &eb?))
    }

    /// The element at position `i` of a list of a checked type.
    fn instance(&mut self, id: usize, i: &str) -> Option<Val> {
        if let Some((v, fs)) = self.lists[id].at.get(i).cloned() {
            for f in fs {
                self.assume(f);
            }
            return Some(v);
        }
        let tpl = self.template(id)?;
        let outer = self.lists[id].outer.clone();
        let v = self.subst_val(&tpl.val, i, &outer);
        let fs: Vec<T> = tpl.facts.iter().map(|f| place(f, i, &outer)).collect();
        for f in &fs {
            self.assume(f.clone());
        }
        self.lists[id].at.insert(i.to_string(), (v.clone(), fs));
        Some(v)
    }

    /// The element of list `id` (of a checked type) at position `IDX`, inside
    /// the positions `@@o0`, … (made once, for the list and its copies).
    fn template(&mut self, id: usize) -> Option<Template> {
        let root = self.lists[id].tpl_from.unwrap_or(id);
        if let Some(t) = &self.lists[root].template {
            return Some(t.clone());
        }
        let Gen::Type(t) = self.lists[root].source.clone() else { return None };
        let n = self.lists[root].outer.len();
        let mut args: Vec<String> = (0..n).map(|k| format!("@@o{k}")).collect();
        args.push(IDX.to_string());
        if fn_sort("Int", args.len()).is_none() {
            return None;
        }
        let scope = self.enter();
        let pc = std::mem::take(&mut self.pc);
        let indexing = self.indexing.replace(args);
        self.spec += 1;
        let v = self.shape(&t);
        let valid = self.check_type(&t, &v, &[]);
        self.spec -= 1;
        self.indexing = indexing;
        self.pc = pc;
        let (_, nf) = self.leave(scope);
        let mut facts: Vec<T> = self.facts.drain(nf..).collect();
        facts.push(valid);
        let tpl = Template { val: v, facts };
        self.lists[root].template = Some(tpl.clone());
        Some(tpl)
    }

    /// `v` at position `i` inside `outer` (see `place`); each list in it is
    /// a new list, sharing the template of the one it copies.
    fn subst_val(&mut self, v: &Val, i: &str, outer: &[T]) -> Val {
        let t = |x: &T| place(x, i, outer);
        match v {
            Val::Int(x) => Val::Int(t(x)),
            Val::Bool(x) => Val::Bool(t(x)),
            Val::Str(x) => Val::Str(t(x)),
            Val::Tuple(xs) => Val::Tuple(xs.iter().map(|x| self.subst_val(x, i, outer)).collect()),
            Val::Rec(n, fs) => Val::Rec(n.clone(), fs.iter().map(|(f, x)| (f.clone(), self.subst_val(x, i, outer))).collect()),
            Val::Sum { ty, args, tag, ctors } => Val::Sum {
                ty: ty.clone(),
                args: args.clone(),
                tag: t(tag),
                ctors: ctors.iter().map(|(c, xs)| (c.clone(), xs.iter().map(|x| self.subst_val(x, i, outer)).collect())).collect(),
            },
            Val::Union { tag, slots } => Val::Union { tag: t(tag), slots: slots.iter().map(|(k, x)| (k.clone(), self.subst_val(x, i, outer))).collect() },
            Val::Distinct(n, x) => Val::Distinct(n.clone(), Box::new(self.subst_val(x, i, outer))),
            Val::List { len, id } => {
                let mut info = self.lists[*id].clone();
                info.preds = info.preds.iter().map(|(g, f)| (t(g), self.subst_func(f, i, outer))).collect();
                info.seen = info.seen.iter().map(|(g, x)| (t(g), self.subst_val(x, i, outer))).collect();
                info.tpl_from = Some(info.tpl_from.unwrap_or(*id));
                info.template = None;
                info.at = HashMap::default();
                info.outer = info.outer.iter().map(|o| t(o)).collect();
                info.source = match info.source {
                    Gen::Range(a, b) => Gen::Range(t(&a), t(&b)),
                    Gen::Values(vs) => Gen::Values(vs.iter().map(|x| self.subst_val(x, i, outer)).collect()),
                    // a checked type: the lists of a template are only made by `shape`
                    g => g,
                };
                self.lists.push(info);
                Val::List { len: t(len), id: self.lists.len() - 1 }
            }
            // a part with placeholders: the same part at this position, made when read
            Val::Lazy(id) if self.cells[*id].placeholder => {
                let c = &self.cells[*id];
                let valid = t(&c.valid);
                let placeholder = i.contains("@@") || outer.iter().any(|o| o.contains("@@"));
                let cell = Cell { ty: c.ty.clone(), make: Make::View { of: *id, i: i.to_string(), outer: outer.to_vec() }, valid, indexing: self.indexing.clone(), reason: c.reason.clone(), region: 0, placeholder, value: None, facts: vec![] };
                self.new_cell(cell)
            }
            Val::Unit | Val::Absent | Val::Opaque(_) | Val::FnOf { .. } | Val::Lazy(_) => v.clone(),
        }
    }

    fn subst_func(&mut self, f: &Func, i: &str, outer: &[T]) -> Func {
        match f {
            Func::Lambda { param, body, scopes } => Func::Lambda {
                param: param.clone(),
                body: body.clone(),
                scopes: scopes.iter().map(|s| s.iter().map(|(k, x)| (k.clone(), self.subst_val(x, i, outer))).collect()).collect(),
            },
            Func::Not(f) => Func::Not(Box::new(self.subst_func(f, i, outer))),
            f => f.clone(),
        }
    }

    /// `f` of the element at every position of list `id` (of length `len`),
    /// as one quantified fact, so that it reaches the elements the code
    /// takes by position (`xs[k]`). `None` when `f` of an element made
    /// unknowns of its own (a call's result, a nested `all`): they would be
    /// the same for every position.
    fn forall_positions(&mut self, id: usize, len: &str, f: &Func) -> Option<T> {
        if !self.positional(id) {
            return None;
        }
        self.qvars += 1;
        let j = format!("q{}", self.qvars);
        let scope = self.enter();
        let nm = self.matched.len();
        let inside = format!("(and (<= 0 {j}) (< {j} {len}))");
        let pc = std::mem::take(&mut self.pc);
        // what is learnt in here is about position `j`: it stays inside
        let preds: Vec<usize> = self.lists.iter().map(|l| l.preds.len()).collect();
        let (memo, cutoffs) = (self.memo.clone(), self.cutoffs.clone());
        let indexing = self.indexing.replace(vec![j.clone()]);
        self.binding += 1;
        self.spec += 1;
        let p = self.element_at(id, &j, &inside).map(|x| {
            let p = self.apply(f, x);
            self.as_bool(&p)
        });
        self.spec -= 1;
        self.binding -= 1;
        self.indexing = indexing;
        self.pc = pc;
        let (nd, nf) = self.leave(scope);
        (self.memo, self.cutoffs) = (memo, cutoffs);
        for (l, n) in self.lists.iter_mut().zip(preds) {
            l.preds.truncate(n);
        }
        let known: Vec<T> = self.facts.drain(nf..).collect();
        self.matched.truncate(nm);
        let p = p?;
        // (an unknown of a sort that can't be a function of `j` would be one for every position)
        if self.decls[nd..].iter().any(|(n, s)| !s.starts_with('(') && !self.bound.contains(n)) {
            return None;
        }
        // what is known of the elements, and of the unknowns made for them, holds at every position
        let mut body = known;
        body.push(p);
        Some(format!("(forall (({j} Int)) (=> {inside} {}))", and(&body)))
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
    fn every(&mut self, id: usize, len: &str, prop: impl FnOnce(&mut Self, Val) -> Option<T>) -> Option<T> {
        let scope = self.enter();
        let nm = self.matched.len();
        let indexing = self.indexing.take();
        self.binding += 1;
        self.spec += 1;
        // by position when the list has them, so that facts about positions reach it
        let x = match self.positional(id) {
            true => {
                let k = self.fresh("Int");
                let inside = format!("(and (<= 0 {k}) (< {k} {len}))");
                self.assume(inside.clone());
                self.element_at(id, &k, &inside).unwrap_or(Val::Opaque("an element".into()))
            }
            false => self.element(id, "true"),
        };
        let p = prop(self, x);
        self.spec -= 1;
        self.binding -= 1;
        self.indexing = indexing;
        let (nd, nf) = self.leave(scope);
        let ant = and(&self.facts[nf..].to_vec());
        self.facts.truncate(nf);
        self.matched.truncate(nm);
        let p = p?;
        // (functions of a position belong to a list's template, not to this element)
        let bound: Vec<String> = self.decls[nd..].iter().filter(|(_, s)| !s.starts_with('(')).map(|(n, s)| format!("({n} {s})")).collect();
        let names: Vec<String> = self.decls[nd..].iter().filter(|(_, s)| !s.starts_with('(')).map(|(n, _)| n.clone()).collect();
        self.bound.extend(names);
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
        if let Some(q) = self.every(id, &len, |vc, x| {
            let p = vc.apply(&f2, x);
            Some(vc.as_bool(&p))
        }) {
            self.facts.push(implies(&q, &b));
        }
        if let Some(q) = self.forall_positions(id, &len, &f) {
            self.facts.push(implies(&b, &q));
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
            Val::Lazy(_) => {
                let v = self.force(v.clone());
                self.as_int(&v)
            }
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
            Val::Lazy(_) => {
                let v = self.force(v.clone());
                self.as_bool(&v)
            }
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
            Val::Lazy(_) => {
                let v = self.force(v.clone());
                self.as_str(&v)
            }
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
            Val::Lazy(_) => {
                let v = self.force(v.clone());
                self.list_len(&v)
            }
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

    /// A value of type `t` with fresh constants, nothing assumed. Its outer
    /// level is made now; the fields of a record and the payloads of a sum
    /// that are themselves structured are lazy parts (`Val::Lazy`), made
    /// when the code reads them.
    fn shape(&mut self, t: &TypeExpr) -> Val {
        match t {
            TypeExpr::Union(ms) => self.union_shape(ms).unwrap_or_else(|| Val::Opaque("values of this union type are not modelled (members of overlapping or unknown shapes)".into())),
            TypeExpr::Named(n, args) if self.env.variant_parent(n.sym()).is_some() => {
                // a variant as a type: a value of its sum with that tag
                let parent = self.env.variant_parent(n.sym()).unwrap();
                let v = self.shape(&TypeExpr::Named(Symbol::from(parent), args.clone()));
                if let (Val::Sum { tag, ctors, .. }, true) = (&v, true) {
                    if let Some(k) = ctors.iter().position(|c| &c.0 == n) {
                        self.facts.push(format!("(= {tag} {k})"));
                    }
                }
                v
            }
            TypeExpr::Refined(b, _) => self.shape(b),
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
                "Tuple" => Val::Tuple(args.iter().map(|a| self.shape(a)).collect()),
                // a task's handle stands for its result
                "Task" if args.len() == 1 => self.shape(&args[0]),
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
                        TypeBody::Alias(a) if td.distinct => Val::Distinct(n.to_string(), Box::new(self.shape(&subst_texpr(a, &sub)))),
                        TypeBody::Alias(a) => self.shape(&subst_texpr(a, &sub)),
                        TypeBody::Record(_) => {
                            let vs = self.env.variants_of(n, args).unwrap_or_default();
                            let (_, names, tys) = vs.into_iter().next().unwrap_or_default();
                            let names = names.unwrap_or_default();
                            Val::Rec(n.to_string(), names.into_iter().zip(tys.iter()).map(|(f, t)| (f.to_string(), self.part(t))).collect())
                        }
                        TypeBody::Sum(_) => {
                            let vs = self.env.variants_of(n, args).unwrap_or_default();
                            let tag = self.fresh("Int");
                            self.facts.push(format!("(and (<= 0 {tag}) (< {tag} {}))", vs.len()));
                            let ctors = vs.iter().map(|(c, _, tys)| (c.to_string(), tys.iter().map(|t| self.part(t)).collect())).collect();
                            Val::Sum { ty: n.to_string(), args: args.clone(), tag, ctors }
                        }
                    }
                }
            },
        }
    }

    // ---- lazy parts ----

    /// A part of a value, of type `t`: made now when it is a plain value, else
    /// lazily (`Val::Lazy`).
    fn part(&mut self, t: &TypeExpr) -> Val {
        if !self.structured(t) {
            return self.shape(t);
        }
        if self.cells.len() >= MAX_CELLS {
            return Val::Opaque("a part of a value past the prover's size budget (only the parts read first are modelled)".into());
        }
        let valid = if self.refined(t, 0) {
            let old = self.reason.take();
            let v = self.fresh("Bool");
            self.reason = old;
            v
        } else {
            "true".into()
        };
        let placeholder = self.indexing.as_ref().is_some_and(|ix| ix.iter().any(|a| a.contains("@@")));
        self.new_cell(Cell { ty: t.clone(), make: Make::Shape, valid, indexing: self.indexing.clone(), reason: self.reason.clone(), region: 0, placeholder, value: None, facts: vec![] })
    }

    fn new_cell(&mut self, mut c: Cell) -> Val {
        c.region = self.regions.last().map_or(0, |r| r.id);
        self.cells.push(c);
        Val::Lazy(self.cells.len() - 1)
    }

    /// Is a value of `t` more than one term or list (a record, sum, tuple,
    /// union): worth making only when read?
    fn structured(&self, t: &TypeExpr) -> bool {
        match self.env.expand_alias(t) {
            TypeExpr::Union(_) => true,
            TypeExpr::Unit | TypeExpr::Fn(..) | TypeExpr::Refined(..) => false,
            TypeExpr::Named(n, _) => match n.as_str() {
                "Int" | "Bool" | "String" | "Float" | "List" | "Dict" | "Set" => false,
                "Tuple" => true,
                _ => self.env.variant_parent(n.sym()).is_some() || self.env.types.contains_key(&n.sym()),
            },
        }
    }

    /// `v`, made if it is a lazy part.
    fn force(&mut self, v: Val) -> Val {
        match v {
            Val::Lazy(id) => self.force_cell(id),
            v => v,
        }
    }

    /// Make lazy part `id` (once): in the context it was made in, with what
    /// its type guarantees defined (its `valid`), its facts placed where its
    /// scope's are, and the comparisons waiting on it settled.
    fn force_cell(&mut self, id: usize) -> Val {
        if let Some(v) = &self.cells[id].value {
            return v.clone();
        }
        let (home, placeholder) = (self.cells[id].region, self.cells[id].placeholder);
        // a part of an outer scope made inside an inner one (a quantified
        // fact): it is made as if there, its facts and unknowns before the
        // inner scopes'
        let keep = match home {
            // (a part with placeholders keeps its facts: no scope to place them in)
            _ if placeholder => 0,
            0 => 0,
            _ => match self.regions.iter().position(|r| r.id == home) {
                Some(p) => p + 1,
                None => return Val::Opaque("a part of a value made in a scope that has ended".into()),
            },
        };
        let inner = if placeholder { vec![] } else { self.regions.split_off(keep) };
        let (tail_d, tail_f) = match inner.first() {
            Some(r) => (self.decls.split_off(r.nd), self.facts.split_off(r.nf)),
            None => (vec![], vec![]),
        };
        let (at_d, at_f) = (self.decls.len(), self.facts.len());
        let pc = std::mem::take(&mut self.pc);
        let indexing = std::mem::replace(&mut self.indexing, self.cells[id].indexing.clone());
        let reason = std::mem::replace(&mut self.reason, self.cells[id].reason.clone());
        let checking = std::mem::take(&mut self.checking);
        let proving = std::mem::replace(&mut self.proving, 0);
        let binding = std::mem::replace(&mut self.binding, 0);
        let dead = std::mem::replace(&mut self.dead, false);
        self.spec += 1;
        let scope = placeholder.then(|| self.enter());
        let v = self.materialize(id);
        self.cells[id].value = Some(v.clone());
        match scope {
            Some(r) => {
                let (_, nf) = self.leave(r);
                self.cells[id].facts = self.facts.split_off(nf);
            }
            None => self.settle(id),
        }
        self.spec -= 1;
        self.dead = dead;
        self.binding = binding;
        self.proving = proving;
        self.checking = checking;
        self.reason = reason;
        self.indexing = indexing;
        self.pc = pc;
        if !inner.is_empty() {
            let (ad, af) = (self.decls.len() - at_d, self.facts.len() - at_f);
            self.decls.extend(tail_d);
            self.facts.extend(tail_f);
            self.regions.extend(inner.into_iter().map(|r| Region { id: r.id, nd: r.nd + ad, nf: r.nf + af }));
            // comparisons made in the inner scopes
            self.settle(id);
        }
        v
    }

    fn materialize(&mut self, id: usize) -> Val {
        match self.cells[id].make.clone() {
            Make::Shape => {
                let t = self.cells[id].ty.clone();
                let v = self.shape(&t);
                let valid = self.cells[id].valid.clone();
                if valid != "true" {
                    let c = self.check_type(&t, &v, &[]);
                    self.facts.push(format!("(= {valid} {c})"));
                }
                v
            }
            Make::View { of, i, outer } => {
                let base = self.force_cell(of);
                let v = self.subst_val(&base, &i, &outer);
                let fs: Vec<T> = self.cells[of].facts.iter().map(|f| place(f, &i, &outer)).collect();
                self.facts.extend(fs);
                v
            }
            Make::Merge { c, a, b } => {
                let x = self.force_cell(a);
                let y = self.force_cell(b);
                self.merge(&c, &x, &y)
            }
        }
    }

    /// The comparisons of part `id` (now made) with parts it was compared
    /// with before either was made: each one's unknown is defined by
    /// comparing the made parts (one level; their parts compare the same way).
    fn settle(&mut self, id: usize) {
        let Some(list) = self.pending.remove(&id) else { return };
        let mut later = vec![];
        for (y, e, r) in list {
            if r != 0 && !self.regions.iter().any(|g| g.id == r) {
                later.push((y, e, r));
                continue;
            }
            if let Some(l) = self.pending.get_mut(&y) {
                l.retain(|p| !(p.0 == id && p.1 == e));
            }
            let a = self.cells[id].value.clone().unwrap_or(Val::Opaque("a part".into()));
            let b = self.force_cell(y);
            let q = self.eq(&a, &b);
            self.facts.push(format!("(= {e} {q})"));
        }
        if !later.is_empty() {
            self.pending.entry(id).or_default().extend(later);
        }
    }

    /// Open a scope (see `Region`).
    fn enter(&mut self) -> usize {
        let id = self.next_region;
        self.next_region += 1;
        self.regions.push(Region { id, nd: self.decls.len(), nf: self.facts.len() });
        id
    }

    /// Close scope `id`: where its unknowns and facts start. Comparisons
    /// made in it that are still waiting are forgotten.
    fn leave(&mut self, id: usize) -> (usize, usize) {
        let r = self.regions.pop().expect("a scope is open");
        debug_assert_eq!(r.id, id);
        if !self.pending.is_empty() {
            self.pending.retain(|_, l| {
                l.retain(|p| p.2 != id);
                !l.is_empty()
            });
            self.eq_atoms.retain(|_, a| a.1 != id);
        }
        (r.nd, r.nf)
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
    fn union_shape(&mut self, ms: &[TypeExpr]) -> Option<Val> {
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
                let v = self.shape(&TypeExpr::Named(Symbol::from(sum), args.unwrap_or_default()));
                if let (Some(cs), Val::Sum { tag: inner, ctors: all, .. }) = (&ctors, &v) {
                    let ks: Vec<T> = cs.iter().filter_map(|c| all.iter().position(|x| &x.0 == c)).map(|j| format!("(= {inner} {j})")).collect();
                    self.facts.push(implies(&format!("(= {tag} {i})"), &or(&ks)));
                }
                v
            } else if ms.len() == 1 {
                self.shape(&ms[0])
            } else {
                match k.as_str() {
                    // `Positive | Negative`: both are integers
                    "Int" | "Bool" | "String" | "Unit" => self.shape(&TypeExpr::Named(Symbol::from(k.as_str()), vec![])),
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
            Val::Lazy(_) => {
                let v = self.force(v.clone());
                return self.shape_test(t, &v);
            }
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
            Val::Lazy(_) => {
                let v = self.force(v.clone());
                self.in_type(t, &v, extra)
            }
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
            Val::Lazy(_) => {
                let v = self.force(v.clone());
                self.is_ctor(&v, c)
            }
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
            Pattern::Ctor(..) if self.env.type_pattern(p).is_some() => {
                let t = self.env.type_pattern(p).unwrap();
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
            Val::Lazy(_) => {
                let v = self.force(v.clone());
                self.field(&v, f)
            }
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
        let v = self.shape(t);
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
            Val::Lazy(id) => {
                let t = self.cells[*id].ty.clone();
                self.havoc(&t)
            }
        }
    }

    /// Does checking a value against `t` involve any refinement? Each type
    /// it can reach (its arguments, the fields and payloads of its body, the
    /// sum of a variant) is looked at once, however deep it is (a syntax tree
    /// nests a dozen types) and however often it is met again (within itself,
    /// as `Array(List[JsonValue])` in `JsonValue`, or along another path).
    fn refined(&self, t: &TypeExpr, _depth: usize) -> bool {
        let mut seen: Vec<TypeExpr> = vec![];
        let mut todo = vec![t.clone()];
        while let Some(t) = todo.pop() {
            if seen.contains(&t) {
                continue;
            }
            seen.push(t.clone());
            match &t {
                TypeExpr::Refined(..) => return true,
                TypeExpr::Unit | TypeExpr::Fn(..) => {}
                TypeExpr::Union(ms) => todo.extend(ms.iter().cloned()),
                TypeExpr::Named(n, args) => {
                    if let Some(parent) = self.env.variant_parent(n.sym()) {
                        todo.push(TypeExpr::Named(Symbol::from(parent), args.clone()));
                        continue;
                    }
                    todo.extend(args.iter().cloned());
                    let Some(td) = self.env.types.get(&n.sym()) else { continue };
                    if td.refinement.is_some() {
                        return true;
                    }
                    match &td.body {
                        TypeBody::Alias(a) => todo.push(a.clone()),
                        TypeBody::Record(fs) => todo.extend(fs.iter().map(|f| f.ty.clone())),
                        TypeBody::Sum(vs) => {
                            for v in vs {
                                match &v.payload {
                                    Payload::Type(_) => unreachable!("a union member defined elsewhere is desugared away"),
                                    Payload::None => {}
                                    Payload::Positional(ts) => todo.extend(ts.iter().cloned()),
                                    Payload::Record(fs) => todo.extend(fs.iter().map(|f| f.ty.clone())),
                                }
                            }
                        }
                    }
                }
            }
        }
        false
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
        if let Val::Lazy(id) = v {
            // a part not made yet is valid for its own type when its `valid` holds
            if !self.refined(t, 0) {
                return "true".into();
            }
            let ty = self.cells[*id].ty.clone();
            if &ty == t || self.erased(&ty) == self.erased(t) {
                return self.cells[*id].valid.clone();
            }
            let v = self.force(v.clone());
            return self.check_type(t, &v, extra);
        }
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
            (Val::Lazy(x), Val::Lazy(y)) if x == y => a.clone(),
            // two parts, one of them not made yet: a part that is either, made
            // when read (making it here could go on forever: a tree merged
            // with its own subtree, `if c { t } else { t.left }`, merges each
            // subtree with the one below it)
            (Val::Lazy(x), Val::Lazy(y)) if (self.cells[*x].value.is_none() || self.cells[*y].value.is_none()) && self.cells.len() < MAX_CELLS => {
                let (x, y) = (*x, *y);
                let (vx, vy) = (self.cells[x].valid.clone(), self.cells[y].valid.clone());
                let placeholder = self.cells[x].placeholder || self.cells[y].placeholder || c.contains("@@");
                let cell = Cell { ty: self.cells[x].ty.clone(), make: Make::Merge { c: c.to_string(), a: x, b: y }, valid: ite(c, &vx, &vy), indexing: self.indexing.clone(), reason: self.reason.clone(), region: 0, placeholder, value: None, facts: vec![] };
                self.new_cell(cell)
            }
            (Val::Opaque(r), Val::Lazy(_)) | (Val::Lazy(_), Val::Opaque(r)) => Val::Opaque(r.clone()),
            (Val::Lazy(_), _) | (_, Val::Lazy(_)) => {
                let (x, y) = (self.force(a.clone()), self.force(b.clone()));
                self.merge(c, &x, &y)
            }
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
        if let (Val::Lazy(_), false) = (v, matches!(p, Pattern::Bind(_) | Pattern::Wild)) {
            let v = self.force(v.clone());
            return self.pattern(p, &v, binds);
        }
        if let Val::Union { tag, slots } = v {
            let type_name = self.env.type_pattern(p).is_some();
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
            Pattern::Ctor(..) if self.env.type_pattern(p).is_some() => {
                let t = self.env.type_pattern(p).unwrap();
                self.in_type(&t, v, &[])
            }
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
                    let exists = format!("(> {len} {i})");
                    let x = match self.element_at(id, &i.to_string(), &exists) {
                        Some(x) => x,
                        None => self.element(id, &exists),
                    };
                    cs.push(self.pattern(p, &x, binds));
                }
                if let Some(r) = rest {
                    let rest_list = self.new_list(format!("(- {len} {n})"), Gen::Shift(id, n.to_string()));
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
                    CtorPat::None | CtorPat::Type(_) => {}
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
            (Val::Lazy(x), Val::Lazy(y)) if x == y => "true".into(),
            (Val::Lazy(x), Val::Lazy(y)) if (self.cells[*x].value.is_none() || self.cells[*y].value.is_none()) && !self.cells[*x].placeholder && !self.cells[*y].placeholder && self.indexing.is_none() => {
                // a part not made yet: their equality is an unknown, defined when
                // both are (as for merging, making it here could go on forever)
                let key = (*x.min(y), *x.max(y));
                // (one made in a scope that is not open here can't be used here)
                if let Some((e, r)) = self.eq_atoms.get(&key) {
                    if *r == 0 || self.regions.iter().any(|g| g.id == *r) {
                        return e.clone();
                    }
                }
                // (no reason: it is defined as soon as the parts are read)
                let old = self.reason.take();
                let e = self.fresh("Bool");
                self.reason = old;
                let r = self.regions.last().map_or(0, |g| g.id);
                self.eq_atoms.insert(key, (e.clone(), r));
                self.pending.entry(*x).or_default().push((*y, e.clone(), r));
                self.pending.entry(*y).or_default().push((*x, e.clone(), r));
                e
            }
            (Val::Lazy(x), Val::Lazy(y)) if self.eq_depth > 3 && (self.indexing.is_some() || self.cells[*x].placeholder || self.cells[*y].placeholder) => self.unknown_bool("comparing parts of values nested deeply"),
            (Val::Opaque(r), Val::Lazy(_)) | (Val::Lazy(_), Val::Opaque(r)) => {
                let r = r.clone();
                self.unknown_bool(&r)
            }
            (Val::Lazy(_), _) | (_, Val::Lazy(_)) => {
                let (x, y) = (self.force(a.clone()), self.force(b.clone()));
                self.eq_depth += 1;
                let e = self.eq(&x, &y);
                self.eq_depth -= 1;
                e
            }
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
            // float arithmetic faults on a result that isn't finite (and an
            // unknown value may be an Int divided by zero): not known to hold
            self.oblige(format!("`{}` gives a finite number", op.symbol()), |vc| vc.unknown_bool(&r));
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

    /// `&`, `|`, `^`, `<<`, `>>`: exact where SMT-LIB's integers can say it
    /// (a shift by a literal is `*` or `div` by a power of two, `x & (2^k - 1)`
    /// is `x mod 2^k`, two literals are computed), else a fresh integer with
    /// what holds of non-negative operands (`0 <= x & y <= x`,
    /// `max(x, y) <= x | y <= x + y`, `0 <= x ^ y <= x + y`).
    fn bits(&mut self, op: BinOp, a: Val, b: Val) -> Val {
        use BinOp::*;
        if !matches!(a, Val::Int(_)) && !matches!(b, Val::Int(_)) {
            let r = match (&a, &b) {
                (Val::Opaque(r), _) | (_, Val::Opaque(r)) => r.clone(),
                _ => "a value of an unexpected shape".into(),
            };
            return Val::Opaque(r);
        }
        let (x, y) = (self.as_int(&a), self.as_int(&b));
        let lit = |t: &str| t.parse::<i64>().ok();
        let pow2 = |k: i64| format!("{}", num_bigint_pow2(k));
        if matches!(op, Shl | Shr) {
            if lit(&y).is_none_or(|k| k < 0) {
                self.oblige(format!("`{}` by a non-negative number of bits", op.symbol()), |_| format!("(>= {y} 0)"));
            }
            if let Some(k) = lit(&y).filter(|k| (0..=4096).contains(k)) {
                return Val::Int(if op == Shl { format!("(* {x} {})", pow2(k)) } else { format!("(div {x} {})", pow2(k)) });
            }
        } else {
            if let (Some(m), Some(n)) = (lit(&x), lit(&y)) {
                return Val::Int(num(match op {
                    BitAnd => m & n,
                    BitOr => m | n,
                    _ => m ^ n,
                }));
            }
            // a mask of the low bits
            let mask = |t: &str| lit(t).filter(|m| *m > 0 && (m & (m + 1)) == 0).map(|m| (m + 1).trailing_zeros() as i64);
            if op == BitAnd {
                if let Some(k) = mask(&y) {
                    return Val::Int(format!("(mod {x} {})", pow2(k)));
                }
                if let Some(k) = mask(&x) {
                    return Val::Int(format!("(mod {y} {})", pow2(k)));
                }
            }
        }
        // the same operation on the same operands is the same value (a refinement
        // assumed of `x` and checked again of it are about one number)
        // (not under a quantifier, whose constants are bound by it)
        let key = format!("{} {x} {y}", op.symbol());
        if self.binding == 0 {
            if let Some(r) = self.bit_terms.get(&key) {
                return Val::Int(r.clone());
            }
        }
        let r = self.fresh_why("Int", &format!("`{}` is only bounded, not modelled exactly", op.symbol()));
        if self.binding == 0 {
            self.bit_terms.insert(key, r.clone());
        }
        let nonneg = format!("(and (>= {x} 0) (>= {y} 0))");
        let fact = match op {
            BitAnd => format!("(and (=> (>= {x} 0) (and (<= 0 {r}) (<= {r} {x}))) (=> (>= {y} 0) (and (<= 0 {r}) (<= {r} {y}))))"),
            BitOr => format!("(=> {nonneg} (and (>= {r} {x}) (>= {r} {y}) (<= {r} (+ {x} {y}))))"),
            BitXor => format!("(=> {nonneg} (and (>= {r} 0) (<= {r} (+ {x} {y}))))"),
            Shl => format!("(=> (>= {x} 0) (>= {r} {x}))"),
            _ => format!("(=> (>= {x} 0) (and (<= 0 {r}) (<= {r} {x})))"),
        };
        self.facts.push(fact);
        Val::Int(r)
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

    /// The value of `e` (made, if it is a part of another value).
    pub fn eval(&mut self, e: &Expr) -> Val {
        let v = self.eval_expr(e);
        self.force(v)
    }

    fn eval_expr(&mut self, e: &Expr) -> Val {
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
            // which arm runs depends on timing
            Expr::Select(_) => Val::Opaque("which arm of a `select` runs depends on timing".into()),
            Expr::Race(_) => Val::Opaque("which arm of a `race` wins depends on timing".into()),
            // a task's result is its expression's value: `await spawn e` is `e`
            Expr::Mut(x) | Expr::Comptime(x) | Expr::Scope(x) | Expr::Spawn(x) | Expr::Await(x) => self.eval(x),
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
                            BitAnd | BitOr | BitXor | Shl | Shr => self.bits(*op, a, b),
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
                // a call given its receiver's whole type: the call
                Expr::Var(name) if name == crate::types::SELF_TYPE && args.len() == 2 => self.eval(&args[1]),
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
                            let res = self.shape(&ret);
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
                self.new_list(format!("(+ {x} {y})"), Gen::Concat(ia, ib, x))
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
                // an argument not given is any value of the parameter's type, when it is known
                let vals: Vec<(String, Val)> = ps
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let v = match (args.get(i), &p.ty) {
                            (Some(v), _) => v.clone(),
                            (None, Some(t)) => {
                                let old = self.reason.replace("the arguments of a function value are not known".into());
                                let v = self.shape(t);
                                self.reason = old;
                                v
                            }
                            (None, None) => arg(i),
                        };
                        (p.name.to_string(), v)
                    })
                    .collect();
                self.scopes.push(vals.into_iter().collect());
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
                            let v = self.shape(t);
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

    /// `byte_len(s)`: the SMT function `utf8_len`, with what is known of it.
    fn utf8_len(&mut self, s: &str) -> T {
        let (n, chars) = (format!("(utf8_len {s})"), format!("(str.len {s})"));
        self.facts.push(format!("(and (<= {chars} {n}) (<= {n} (* 4 {chars})) (=> {} (= {n} {chars})))", ascii(s)));
        n
    }

    fn call(&mut self, name: &str, arg_exprs: &[Expr], args: Vec<Val>) -> Val {
        // `len(s.code_points())` is the SMT string's length (its code points), and
        // `len(s.bytes())` its `byte_len`: modelled on `s` itself
        if crate::names::short(crate::types::split_trait_call(name).map_or(name, |(_, m)| m)) == "len" {
            if let Some((unit, s)) = arg_exprs.first().and_then(string_unit) {
                let v = self.eval(s);
                return match unit {
                    "code_points" => Val::Int(format!("(str.len {})", self.as_str(&v))),
                    _ => self.builtin("byte_len", std::slice::from_ref(s), vec![v]),
                };
            }
        }
        // `to_string` through `Display` (every type has it): the built-in's
        // model, unless the program implements `Display` for a built-in type
        if (name == "Display::to_string" || name == "to_string") && !self.env.impls.iter().any(|i| i.trait_name == "Display" && matches!(crate::types::texpr_head(&i.for_type).as_str(), "Int" | "Bool" | "String")) {
            return self.builtin("to_string", arg_exprs, args);
        }
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
        let why = format!("the result of `{}` (only its contract is known)", crate::types::shown_name(&label));
        let old = self.reason.replace(why);
        let res = self.shape(&f.ret);
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
        self.builtin_faults(name, &a);
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
                // the list's elements, then the new one
                let one = self.new_list("1".into(), Gen::Values(vec![arg(1)]));
                let one = self.list_id(&one);
                self.new_list(format!("(+ {l} 1)"), Gen::Concat(id, one, l))
            }
            ("take" | "drop", 2) => {
                let l = self.list_len(&arg(0));
                let n = self.as_int(&arg(1));
                let k = format!("(ite (< {n} 0) 0 (ite (< {n} {l}) {n} {l}))");
                let id = self.list_id(&arg(0));
                if name == "take" {
                    self.new_list(k, Gen::Shift(id, "0".into()))
                } else {
                    self.new_list(format!("(- {l} {k})"), Gen::Shift(id, k))
                }
            }
            // elements start..end of a list, both clamped to 0..len
            ("slice", 3) if matches!(arg(0), Val::List { .. }) => {
                let l = self.list_len(&arg(0));
                let (a, b) = (self.as_int(&arg(1)), self.as_int(&arg(2)));
                let clamp = |x: &str| format!("(ite (< {x} 0) 0 (ite (> {x} {l}) {l} {x}))");
                let (a, b) = (clamp(&a), clamp(&b));
                let id = self.list_id(&arg(0));
                self.new_list(format!("(ite (< {a} {b}) (- {b} {a}) 0)"), Gen::Shift(id, a))
            }
            ("reverse", _) if !matches!(arg(0), Val::Str(_)) => {
                let l = self.list_len(&arg(0));
                let id = self.list_id(&arg(0));
                self.new_list(l.clone(), Gen::Rev(id, l))
            }
            ("sort" | "sort_by", _) if !matches!(arg(0), Val::Str(_)) => {
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
                match self.element_at(id, &i, &inside) {
                    Some(x) => x,
                    None => self.element(id, &inside),
                }
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
                let x = match self.element_at(id, &i, &inside) {
                    Some(x) => x,
                    None => self.element(id, &inside),
                };
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
            // some element satisfies `f`: not every element fails it
            ("any", 2) if self.fn_args.get(1).cloned().flatten().is_some() => {
                let f = self.fn_args[1].clone().unwrap();
                Val::Bool(not(&self.all_of(&arg(0), Func::Not(Box::new(f)))))
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
                let Val::List { id, len } = v.clone() else { return Val::Opaque(not_modelled) };
                let mut ints = true;
                let q = self.every(id, &len, |vc, x| match x {
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
            // Positions in strings are byte offsets; SMT strings are code points.
            // On an ASCII string the two are the same, and that is modelled exactly;
            // on others only bounds are known.
            ("byte_len", 1) => {
                let s = self.as_str(&arg(0));
                Val::Int(self.utf8_len(&s))
            }
            // bytes a..b, both clamped to 0..byte_len: on an ASCII string the code
            // points a..b (`str.substr` takes a length); on any, at most b - a
            // bytes (an offset inside a character moves back up to 3 bytes)
            ("byte_slice", 3) => {
                let s = self.as_str(&arg(0));
                let (a, b) = (self.as_int(&arg(1)), self.as_int(&arg(2)));
                let n = self.utf8_len(&s);
                let clamp = |x: &str| format!("(ite (< {x} 0) 0 (ite (> {x} {n}) {n} {x}))");
                let (a, b) = (clamp(&a), clamp(&b));
                let other = self.fresh_why("String", "`byte_slice` of a string that may not be ASCII");
                let r = format!("(ite {} (str.substr {s} {a} (- {b} {a})) {other})", ascii(&s));
                let rn = self.utf8_len(&r);
                let ascii_s = ascii(&s);
                // a >= b: nothing (moving back keeps the order)
                self.facts.push(format!("(=> (>= {a} {b}) (= {r} \"\"))"));
                self.facts.push(format!("(and (>= {rn} 0) (<= {rn} {b}) (<= {rn} (+ (- {b} {a}) 3)) (=> {ascii_s} (= {rn} (ite (< {a} {b}) (- {b} {a}) 0))))"));
                Val::Str(r)
            }
            ("chars", 1) => {
                let s = self.as_str(&arg(0));
                // each element is one character
                self.new_list(format!("(str.len {s})"), Gen::Type(crate::types::parse_type_str("String where len(it) == 1")))
            }
            // the first match's byte offset (its code-point offset on an ASCII string,
            // never less elsewhere); "" is found at 0
            ("index_of", 2) => {
                let (s, p) = (self.as_str(&arg(0)), self.as_str(&arg(1)));
                let i = format!("(str.indexof {s} {p} 0)");
                let b = self.fresh_why("Int", "`index_of` in a string that may not be ASCII");
                let (sn, pn) = (self.utf8_len(&s), self.utf8_len(&p));
                // a match ends inside the string
                self.facts.push(format!("(and (=> {} (= {b} {i})) (=> (= {i} 0) (= {b} 0)) (=> (>= {i} 0) (and (>= {b} {i}) (<= (+ {b} {pn}) {sn}))))", ascii(&s)));
                let int = TypeExpr::Named("Int".into(), vec![]);
                self.option(&int, &format!("(>= {i} 0)"), Val::Int(b))
            }
            // every non-overlapping match, left to right (Rust's `str::replace`); an
            // empty pattern is found around every character (SMT's replace_all leaves
            // the string alone there: not modelled)
            ("replace", 3) => {
                let (s, from, to) = (self.as_str(&arg(0)), self.as_str(&arg(1)), self.as_str(&arg(2)));
                let around = self.fresh_why("String", "`replace` with an empty pattern");
                Val::Str(format!("(ite (= {from} \"\") {around} (str.replace_all {s} {from} {to}))"))
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
            // (within what SMT strings hold; `builtin_faults` checks the code point)
            ("chr", 1) if matches!(arg(0), Val::Int(_)) => {
                let n = self.as_int(&arg(0));
                let c = self.fresh_why("String", "`chr` of a code point past U+2FFFF");
                self.facts.push(format!("(= (str.len {c}) 1)"));
                self.facts.push(format!("(=> (<= {n} {MAX_CHAR}) (= {c} (str.from_code {n})))"));
                Val::Str(c)
            }
            // (one character: `builtin_faults` checks it)
            ("ord", 1) => {
                let s = self.as_str(&arg(0));
                Val::Int(format!("(str.to_code {s})"))
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
                        let v = self.shape(ret);
                        self.reason = old;
                        v
                    }
                    _ => Val::Opaque(not_modelled),
                }
            }
        }
    }

    /// The checks of a builtin that faults on some arguments (as compiled
    /// code does): exact where the arguments are modelled, unknown (never
    /// proven) where they are floats.
    fn builtin_faults(&mut self, name: &str, a: &[Val]) {
        let floats = "floats are not modelled yet";
        match (name, a) {
            ("chr", [Val::Int(n)]) => self.oblige("`chr` of a code point that is a character".into(), |_| {
                format!("(and (<= 0 {n}) (<= {n} 1114111) (not (and (<= 55296 {n}) (<= {n} 57343))))")
            }),
            ("chr", [_]) => self.oblige("`chr` of a code point that is a character".into(), |vc| vc.unknown_bool("`chr` of a value not modelled")),
            ("ord", [Val::Str(s)]) => self.oblige("`ord` of a single character".into(), |_| format!("(= (str.len {s}) 1)")),
            // (`split` and `repeat` are total: an empty separator splits at every
            // character, a count below 1 gives ""; out of memory isn't modelled)
            ("ord", _) => self.oblige(format!("`{name}` with arguments it accepts"), |vc| vc.unknown_bool(&format!("`{name}` of a value not modelled"))),
            // exactly the Ints that round to a finite Float: below 2^1024 - 2^970,
            // halfway between the largest Float and 2^1024 (from there, up)
            ("float", [Val::Int(n)]) => self.oblige("`float` of an Int a Float can hold".into(), |_| {
                use num_bigint::BigInt;
                let big = (BigInt::from(1) << 1024u32) - (BigInt::from(1) << 970u32);
                format!("(and (< {n} {big}) (< (- {big}) {n}))")
            }),
            ("sqrt", [_]) => self.oblige("`sqrt` of a number that is not negative".into(), |vc| vc.unknown_bool(floats)),
            ("floor" | "ceil" | "round", [_]) => self.oblige(format!("`{name}` of a finite number"), |vc| vc.unknown_bool(floats)),
            _ => {}
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
            Stmt::Break => {
                if !self.dead {
                    if let Some(frame) = self.breaks.last_mut() {
                        frame.push((self.pc.clone(), self.scopes.clone()));
                    }
                }
                self.dead = true;
            }
            Stmt::Continue => {
                if !self.dead {
                    if let Some(frame) = self.continues.last_mut() {
                        frame.push((self.pc.clone(), self.scopes.clone()));
                    }
                }
                self.dead = true;
            }
            // (checked on entry and after every pass: `written_on_entry`, `written_kept`)
            Stmt::Invariant(_) => {}
            Stmt::While(c, body) => {
                let head = format!("while {}", crate::printer::expr(c, 0));
                let written: Vec<Expr> = crate::types::loop_invariants(body).cloned().collect();
                self.written_on_entry(&written, &head);
                let entry = self.entry_values(body, c);
                let invariants = self.infer_invariants(c, body, &entry, &written);
                self.havoc_loop(body, Some(c));
                self.assume_written(&written);
                // what every pass keeps holds at the head; what the declared
                // types already say there is left out (it says nothing more)
                let terms: Vec<Option<T>> = invariants.iter().map(|(_, cand)| self.candidate_value(cand, &entry)).collect();
                let mut implied = vec![true; terms.len()];
                if let Some(solver) = self.solver.clone() {
                    let pc = self.pc.clone();
                    self.drop_falsifiable(&solver, &pc, &terms, &mut implied);
                }
                for (((shown, _), t), implied) in invariants.iter().zip(terms).zip(implied) {
                    let Some(t) = t.filter(|_| !implied) else { continue };
                    self.assume(t);
                    self.invariants.push(format!("invariant of `{head}`: {shown} (inferred)"));
                }
                self.breaks.push(vec![]);
                self.continues.push(vec![]);
                let cv = self.eval(c);
                let cv = self.as_bool(&cv);
                let saved = self.scopes.clone();
                let n = self.pc.len();
                self.pc.push(cv.clone());
                // candidate bounds, from the condition's comparisons, before the pass
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
                let ends = self.pass_ends(saved.len());
                self.written_kept(&written, &head, &ends);
                let mut info = LoopInfo { what: head.clone(), candidates: vec![], always_exits: ends.is_empty(), reason: None };
                if !ends.is_empty() {
                    // after every pass (the body's end, each `continue`): either the loop is over, or the bound went down
                    let (pc, scopes) = (std::mem::take(&mut self.pc), self.scopes.clone());
                    let mut goals = vec![vec![]; measures.len()];
                    for (p, s, _) in &ends {
                        (self.pc, self.scopes) = (p.clone(), s.clone());
                        self.spec += 1;
                        let again = self.eval(c);
                        let again = self.as_bool(&again);
                        for (k, (m, (_, b))) in measures.iter().zip(&before).enumerate() {
                            let v = self.eval(m);
                            let a = self.as_int(&v);
                            goals[k].push(implies(&and(&p[n + 1..]), &format!("(or (not {again}) (and (>= {b} 0) (< {a} {b})))")));
                        }
                        self.spec -= 1;
                    }
                    (self.pc, self.scopes) = (pc, scopes);
                    self.pc.truncate(n + 1);
                    for ((shown, _), g) in before.into_iter().zip(goals) {
                        let k = self.oblige_termination(format!("loop bound `{shown}`"), and(&g));
                        info.candidates.push((shown, k));
                    }
                }
                self.loops.push(info);
                self.pc.truncate(n);
                self.dead = false;
                self.scopes = saved;
                // left when the condition is false, or at a `break`
                let breaks = self.breaks.pop().unwrap_or_default();
                self.leave_loop(n, Some(not(&cv)), breaks);
            }
            Stmt::For(p, xs, body) => {
                let xv = self.eval(xs);
                let head = format!("for {} in {}", crate::printer::pat(p), crate::printer::expr(xs, 0));
                let written: Vec<Expr> = crate::types::loop_invariants(body).cloned().collect();
                self.written_on_entry(&written, &head);
                self.havoc_loop(body, None);
                self.assume_written(&written);
                self.breaks.push(vec![]);
                self.continues.push(vec![]);
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
                let ends = self.pass_ends(saved.len());
                self.written_kept(&written, &head, &ends);
                self.pc.truncate(n);
                self.dead = false;
                self.scopes = saved;
                // after the last element the loop's variables are any values
                // of their types (and invariants), which covers the state at
                // a `break` too, unless invariants were assumed: then afresh
                let breaks = self.breaks.pop().unwrap_or_default();
                if !breaks.is_empty() && !written.is_empty() {
                    self.havoc_loop(body, None);
                }
            }
            Stmt::Loop(body) => {
                let head = "loop".to_string();
                let written: Vec<Expr> = crate::types::loop_invariants(body).cloned().collect();
                self.written_on_entry(&written, &head);
                self.havoc_loop(body, None);
                self.assume_written(&written);
                self.breaks.push(vec![]);
                self.continues.push(vec![]);
                let saved = self.scopes.clone();
                let n = self.pc.len();
                self.eval(body);
                let ends = self.pass_ends(saved.len());
                self.written_kept(&written, &head, &ends);
                self.pc.truncate(n);
                self.dead = false;
                self.scopes = saved;
                // left only at a `break`
                let breaks = self.breaks.pop().unwrap_or_default();
                self.leave_loop(n, None, breaks);
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
        let new = if path.is_empty() { v } else { self.set_path(base, &path, v) };
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

    /// `base` with the field at `path` set to `v`.
    fn set_path(&mut self, base: Val, path: &[String], v: Val) -> Val {
        let Some((f, rest)) = path.split_first() else { return v };
        match self.force(base) {
            Val::Rec(n, mut fs) => {
                if let Some(slot) = fs.iter_mut().find(|x| &x.0 == f) {
                    let old = std::mem::replace(&mut slot.1, Val::Unit);
                    slot.1 = self.set_path(old, rest, v);
                }
                Val::Rec(n, fs)
            }
            // a field of a sum or union value: which member's field changed is
            // not tracked, so the whole value is unknown
            _ => Val::Opaque("a value whose field was assigned".into()),
        }
    }

    /// Where a pass of the innermost loop ended: its body's end (unless no
    /// path reaches it) and each `continue` (`true`), with their paths and
    /// their variables down to `depth` scopes (the loop's own).
    fn pass_ends(&mut self, depth: usize) -> Vec<(Vec<T>, Vec<HashMap<String, Val>>, bool)> {
        let mut ends: Vec<_> = self.continues.pop().unwrap_or_default().into_iter().map(|(p, s)| (p, s, true)).collect();
        if !self.dead {
            ends.insert(0, (self.pc.clone(), self.scopes.clone(), false));
        }
        for (_, s, _) in &mut ends {
            s.truncate(depth);
        }
        ends
    }

    /// A loop's written invariants hold before it.
    fn written_on_entry(&mut self, written: &[Expr], head: &str) {
        for x in written {
            let v = self.eval(x);
            let b = self.as_bool(&v);
            self.oblige(format!("invariant `{}` on entry to `{head}`", crate::printer::expr(x, 0)), |_| b);
        }
    }

    /// At the loop's head: its written invariants hold (they are checked
    /// on entry and after every pass).
    fn assume_written(&mut self, written: &[Expr]) {
        for x in written {
            self.spec += 1;
            let v = self.eval(x);
            self.spec -= 1;
            let b = self.as_bool(&v);
            self.assume(b);
        }
    }

    /// A loop's written invariants hold where each pass ends.
    fn written_kept(&mut self, written: &[Expr], head: &str, ends: &[(Vec<T>, Vec<HashMap<String, Val>>, bool)]) {
        if written.is_empty() {
            return;
        }
        let (pc, scopes, dead) = (std::mem::take(&mut self.pc), self.scopes.clone(), self.dead);
        for (p, s, at_continue) in ends {
            (self.pc, self.scopes, self.dead) = (p.clone(), s.clone(), false);
            for x in written {
                let v = self.eval(x);
                let b = self.as_bool(&v);
                let at = if *at_continue { "at a `continue` in" } else { "after a pass of" };
                self.oblige(format!("invariant `{}` {at} `{head}`", crate::printer::expr(x, 0)), |_| b);
            }
        }
        (self.pc, self.scopes, self.dead) = (pc, scopes, dead);
    }

    /// After a loop: the state is the loop head's when it is left normally
    /// (under `normal`, the condition being false), or the state at one of its
    /// `break`s. `n` is the length of the path before the loop, which every
    /// `break`'s path extends; `self.scopes` is the loop head's state.
    fn leave_loop(&mut self, n: usize, normal: Option<T>, breaks: Vec<(Vec<T>, Vec<HashMap<String, Val>>)>) {
        let depth = self.scopes.len();
        let mut exits: Vec<(T, Vec<HashMap<String, Val>>)> =
            breaks.into_iter().map(|(pc, mut s)| (and(pc.get(n..).unwrap_or_default()), { s.truncate(depth); s })).collect();
        let mut alive: Vec<T> = exits.iter().map(|e| e.0.clone()).collect();
        alive.extend(normal.clone());
        if alive.is_empty() {
            // never left: what follows is unreachable
            self.dead = true;
            return;
        }
        self.pc.push(or(&alive));
        // the paths are disjoint: a pass that breaks began with the condition true
        let mut scopes = match normal {
            Some(_) => std::mem::take(&mut self.scopes),
            None => exits.pop().unwrap().1,
        };
        for (g, other) in exits.into_iter().rev() {
            for (layer, o) in scopes.iter_mut().zip(&other) {
                for (name, val) in layer.iter_mut() {
                    if let Some(ov) = o.get(name) {
                        *val = self.merge(&g, ov, val);
                    }
                }
            }
        }
        self.scopes = scopes;
    }

    /// The variables a loop changes, with their values before it: bound as
    /// `$entry_x` in candidates (`x >= $entry_x`).
    fn entry_values(&mut self, body: &Expr, c: &Expr) -> Vec<(String, String, Val)> {
        let env = self.env;
        let mut_params = |n: &str| env.mut_params(n);
        let mut names: Vec<String> = std::iter::once(body).chain([c]).flat_map(|e| changed_vars(e, &mut_params)).collect::<HashSet<String>>().into_iter().collect();
        names.sort();
        names.into_iter().filter_map(|x| self.lookup(&x).map(|v| (x.clone(), format!("$entry_{x}"), v))).collect()
    }

    /// Facts every pass of `while c { body }` keeps, from a list of
    /// candidates (Houdini): those that can be false on entry are dropped,
    /// then, from any state where the others and the condition hold, one
    /// pass runs and those that can be false after it are dropped, until a
    /// pass drops nothing. What remains holds on entry and after every pass,
    /// so at the head of the loop, every time. Shown, with the candidate.
    fn infer_invariants(&mut self, c: &Expr, body: &Expr, entry: &[(String, String, Val)], written: &[Expr]) -> Vec<(String, Candidate)> {
        let Some(solver) = self.solver.clone() else { return vec![] };
        // (inner loops of a trial infer nothing)
        if self.trial > 0 || self.spec > 0 || self.dead {
            return vec![];
        }
        let cands = self.invariant_candidates(c, entry);
        if cands.is_empty() {
            return vec![];
        }
        // on entry
        let at_entry: Vec<Option<T>> = cands.iter().map(|k| self.candidate_value(k, entry)).collect();
        let mut alive: Vec<bool> = at_entry.iter().map(|t| t.is_some()).collect();
        let pc = self.pc.clone();
        self.drop_falsifiable(&solver, &pc, &at_entry, &mut alive);
        // after a pass, from the head
        for _ in 0..=cands.len() {
            if !alive.iter().any(|a| *a) {
                break;
            }
            let (scopes, n_pc, dead, n_ob, n_loops, n_rec) = (self.scopes.clone(), self.pc.len(), self.dead, self.obligations.len(), self.loops.len(), self.rec_sites.len());
            let returns = std::mem::take(&mut self.returns);
            self.trial += 1;
            self.spec += 1;
            self.havoc_loop(body, Some(c));
            // (the written invariants are proven on their own, with these: together they hold)
            for x in written {
                let v = self.eval(x);
                let b = self.as_bool(&v);
                self.pc.push(b);
            }
            for (k, cand) in cands.iter().enumerate() {
                if alive[k] {
                    if let Some(t) = self.candidate_value(cand, entry) {
                        self.pc.push(t);
                    }
                }
            }
            let cv = self.eval(c);
            let cv = self.as_bool(&cv);
            self.pc.push(cv);
            let base = self.pc.len();
            let depth = self.scopes.len();
            self.breaks.push(vec![]);
            self.continues.push(vec![]);
            self.eval(body);
            self.breaks.pop();
            // kept where every pass ends (none: every path left the loop, nothing to keep)
            let ends = self.pass_ends(depth);
            let mut pc_after = std::mem::take(&mut self.pc);
            pc_after.truncate(base);
            let mut after: Vec<Option<T>> = cands.iter().map(|_| Some("true".to_string())).collect();
            for (p, s, _) in &ends {
                (self.pc, self.scopes) = (p.clone(), s.clone());
                for (k, cand) in cands.iter().enumerate() {
                    let v = if alive[k] { self.candidate_value(cand, entry) } else { None };
                    after[k] = match (after[k].take(), v) {
                        (Some(acc), Some(v)) => Some(and(&[acc, implies(&and(&p[base..]), &v)])),
                        _ => None,
                    };
                }
            }
            self.pc = pc_after.clone();
            self.spec -= 1;
            self.trial -= 1;
            self.returns = returns;
            self.scopes = scopes;
            self.pc.truncate(n_pc);
            self.dead = dead;
            self.obligations.truncate(n_ob);
            self.loops.truncate(n_loops);
            self.rec_sites.truncate(n_rec);
            if !self.drop_falsifiable(&solver, &pc_after, &after, &mut alive) {
                break;
            }
        }
        cands.into_iter().zip(alive).filter(|(_, a)| *a).map(|(k, _)| (k.shown(), k)).collect()
    }

    /// Drop (in `alive`) each term that can be false under `pc`; a term that
    /// is `None` is dropped. Whether any was dropped.
    fn drop_falsifiable(&self, solver: &Solver, pc: &[T], terms: &[Option<T>], alive: &mut [bool]) -> bool {
        let mut dropped = false;
        for (a, t) in alive.iter_mut().zip(terms) {
            if *a && t.is_none() {
                *a = false;
                dropped = true;
            }
        }
        loop {
            let live: Vec<usize> = (0..terms.len()).filter(|&k| alive[k]).collect();
            if live.is_empty() {
                return dropped;
            }
            let goal = and(&live.iter().map(|&k| terms[k].clone().unwrap_or_default()).collect::<Vec<_>>());
            let ob = Obligation { what: "invariant".into(), goal, pc: pc.to_vec(), facts: self.facts.len(), termination: false };
            let script = self.script(&ob, &[]);
            let Some(head) = script.find("(check-sat)") else { return dropped };
            let mut defs = String::new();
            for (j, &k) in live.iter().enumerate() {
                defs.push_str(&format!("(declare-const inv{j} Bool)\n(assert (= inv{j} {}))\n", terms[k].as_deref().unwrap_or("true")));
            }
            let names: Vec<String> = (0..live.len()).map(|j| format!("inv{j}")).collect();
            let q = format!("{}{defs}(check-sat)\n(get-value ({}))\n", &script[..head], names.join(" "));
            let out = run_solver(solver, &q, &std::sync::atomic::AtomicBool::new(false)).ok().flatten().unwrap_or_default();
            let first = out.lines().next().unwrap_or("").trim();
            match first {
                "unsat" => return dropped,
                "sat" => {
                    let vals = parse_values(&out[first.len()..]);
                    let mut any = false;
                    for (j, &k) in live.iter().enumerate() {
                        if vals.get(&format!("inv{j}")).map(|v| v.trim()) != Some("true") {
                            alive[k] = false;
                            any = true;
                        }
                    }
                    if !any {
                        // (a model that falsifies none: give up on them all)
                        live.iter().for_each(|&k| alive[k] = false);
                    }
                    dropped = true;
                }
                // the solver gave up on them together: each on its own, kept if proven
                _ => {
                    for &k in &live {
                        let ob = Obligation { what: "invariant".into(), goal: terms[k].clone().unwrap_or_default(), pc: pc.to_vec(), facts: self.facts.len(), termination: false };
                        let q = self.script(&ob, &[]);
                        let out = run_solver(solver, &q, &std::sync::atomic::AtomicBool::new(false)).ok().flatten().unwrap_or_default();
                        if out.lines().next().map(str::trim) != Some("unsat") {
                            alive[k] = false;
                        }
                    }
                    return true;
                }
            }
        }
    }

    /// The candidates for `while c`'s invariants: the condition's comparisons
    /// made non-strict (what holds after the last pass too), the `ensures`
    /// of the variable returned, the `requires`, and each integer the loop
    /// changes against its value on entry, 0, the integer parameters and the
    /// other integers it changes. At most `MAX_CANDIDATES`.
    fn invariant_candidates(&mut self, c: &Expr, entry: &[(String, String, Val)]) -> Vec<Candidate> {
        let var = |n: &str| Expr::Var(Symbol::new(n));
        let bin = |op: BinOp, a: Expr, b: Expr| Expr::Bin(op, Box::new(a), Box::new(b));
        let plain = |e: Expr| Candidate { e, it: None };
        let mut out: Vec<Candidate> = vec![];
        fn atoms(e: &Expr, out: &mut Vec<Expr>) {
            match e {
                Expr::Bin(BinOp::And, a, b) => {
                    atoms(a, out);
                    atoms(b, out);
                }
                Expr::Bin(BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Ne, _, _) => out.push(e.clone()),
                _ => {}
            }
        }
        let mut cs = vec![];
        atoms(c, &mut cs);
        for a in cs {
            let Expr::Bin(op, l, r) = a else { continue };
            match op {
                BinOp::Lt | BinOp::Le => out.push(plain(bin(BinOp::Le, *l, *r))),
                BinOp::Gt | BinOp::Ge => out.push(plain(bin(BinOp::Ge, *l, *r))),
                BinOp::Ne => {
                    out.push(plain(bin(BinOp::Le, (*l).clone(), (*r).clone())));
                    out.push(plain(bin(BinOp::Ge, *l, *r)));
                }
                _ => {}
            }
        }
        let changed: Vec<&String> = entry.iter().map(|(x, _, _)| x).collect();
        for (e, x) in self.hints.clone() {
            if changed.contains(&&x) {
                out.push(Candidate { e, it: Some(x) });
            }
        }
        for r in self.requires_hints.clone() {
            out.push(plain(r));
        }
        let ints: Vec<&(String, String, Val)> = entry.iter().filter(|(_, _, v)| matches!(v, Val::Int(_))).collect();
        let params: Vec<String> = self.params.iter().filter(|(n, v)| matches!(v, Val::Int(_)) && !changed.contains(&n)).map(|(n, _)| n.clone()).collect();
        for (x, ex, _) in &ints {
            out.push(plain(bin(BinOp::Ge, var(x), var(ex))));
            out.push(plain(bin(BinOp::Le, var(x), var(ex))));
            out.push(plain(bin(BinOp::Ge, var(x), Expr::Int(0))));
            let others = params.iter().cloned().chain(ints.iter().map(|(y, _, _)| y.clone()).filter(|y| y != x));
            for y in others {
                out.push(plain(bin(BinOp::Le, var(x), var(&y))));
                out.push(plain(bin(BinOp::Ge, var(x), var(&y))));
            }
        }
        let mut seen = HashSet::default();
        out.retain(|k| seen.insert(k.shown()));
        out.truncate(MAX_CANDIDATES);
        out
    }

    /// A candidate's value in the current state (the loop's variables as
    /// they are now, `$entry_x` as before the loop). `None` when it isn't
    /// a known Bool.
    fn candidate_value(&mut self, k: &Candidate, entry: &[(String, String, Val)]) -> Option<T> {
        let mut layer: HashMap<String, Val> = entry.iter().map(|(_, ex, v)| (ex.clone(), v.clone())).collect();
        if let Some(x) = &k.it {
            layer.insert("it".into(), self.lookup(x)?);
        }
        self.scopes.push(layer);
        self.spec += 1;
        let v = self.eval(&k.e);
        self.spec -= 1;
        self.scopes.pop();
        match v {
            Val::Bool(t) => Some(t),
            _ => None,
        }
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
    /// Values of `f`'s parameters that meet their types and its `requires`,
    /// made by the solver: up to `n` of them, each different from the ones
    /// before. Lists have at most `INPUT_LEN` elements. For the refinements
    /// random values rarely meet (`all(1..len(it), i -> it[i - 1] <= it[i])`,
    /// a record whose fields depend on each other).
    pub fn inputs(&mut self, f: &FnDecl, solver: &Solver, n: usize, seed: u64) -> Vec<Vec<Expr>> {
        self.self_name = f.name.to_string();
        for p in &f.params {
            let v = self.havoc(&p.ty);
            self.params.push((p.name.to_string(), v.clone()));
            self.bind(&p.name, v);
        }
        let binds = self.params.clone();
        for r in &f.requires {
            let c = self.pred(r, binds.clone());
            self.pc.push(c);
        }
        // every part a value is shown with: its lazy parts, its lists' first elements
        let mut lens = vec![];
        let mut leaves: Vec<(T, &'static str)> = vec![];
        for (_, v) in self.params.clone() {
            self.open_input(&v, 0, &mut lens, &mut leaves);
        }
        let mut seen: HashSet<String> = HashSet::default();
        leaves.retain(|(t, _)| seen.insert(t.clone()));
        let ob = Obligation { what: "inputs".into(), goal: "false".into(), pc: self.pc.clone(), facts: self.facts.len(), termination: false };
        // a constant for each leaf (the model is read by name)
        let names: Vec<String> = (0..leaves.len()).map(|k| format!("input{k}")).collect();
        let seeds: Vec<String> = leaves.iter().flat_map(|(t, _)| t.split(|c: char| !c.is_alphanumeric() && c != '_').filter(|w| w.starts_with('v') && w.len() > 1 && w[1..].chars().all(|c| c.is_ascii_digit())).map(String::from)).collect();
        let script = self.script(&ob, &seeds);
        let Some(head) = script.find("(check-sat)") else { return vec![] };
        let mut defs = String::new();
        for ((t, sort), a) in leaves.iter().zip(&names) {
            defs.push_str(&format!("(declare-const {a} {sort})\n(assert (= {a} {t}))\n"));
        }
        for l in &lens {
            defs.push_str(&format!("(assert (<= {l} {INPUT_LEN}))\n"));
        }
        let mut found: Vec<Vec<Expr>> = vec![];
        let mut blocked = String::new();
        let never = std::sync::atomic::AtomicBool::new(false);
        for k in 0..n {
            let s = seed.wrapping_add(k as u64) % 1_000_000;
            let q = format!("(set-option :smt.random_seed {s})\n(set-option :sat.random_seed {s})\n{}{defs}{blocked}(check-sat)\n(get-value ({}))\n", &script[..head], names.join(" "));
            let Ok(Some(out)) = run_solver(solver, &q, &never) else { break };
            let first = out.lines().next().unwrap_or("").trim();
            if first != "sat" {
                break;
            }
            let by_name = parse_values(&out[first.len()..]);
            let mut model: HashMap<String, String> = HashMap::default();
            for ((t, _), a) in leaves.iter().zip(&names) {
                if let Some(v) = by_name.get(a) {
                    model.insert(t.clone(), v.clone());
                }
            }
            // the next model differs from this one
            let same: Vec<String> = names.iter().filter_map(|a| by_name.get(a).map(|v| format!("(= {a} {v})"))).collect();
            if same.is_empty() {
                break;
            }
            blocked.push_str(&format!("(assert (not (and {})))\n", same.join(" ")));
            // (a method's hidden type argument is no value: the run leaves it out)
            let args: Option<Vec<Expr>> = self
                .params
                .iter()
                .zip(&f.params)
                .map(|((_, v), p)| match &p.ty {
                    TypeExpr::Named(n, _) if n == crate::types::TYPE_ARG => Some(Expr::Unit),
                    _ => to_literal_in(self.env, &self.cells, &self.input_elems, v, &model),
                })
                .collect();
            if let Some(a) = args {
                found.push(a);
            }
        }
        found
    }

    /// Make the parts of `v` an input is shown with: lazy parts, and each
    /// list's first `INPUT_LEN` elements (kept in `input_elems`), down to
    /// `INPUT_DEPTH` levels; collect the lists' lengths and every leaf.
    fn open_input(&mut self, v: &Val, depth: usize, lens: &mut Vec<T>, leaves: &mut Vec<(T, &'static str)>) {
        if depth > INPUT_DEPTH {
            return;
        }
        let v = self.force(v.clone());
        match &v {
            Val::Int(t) => leaves.push((t.clone(), "Int")),
            Val::Bool(t) => leaves.push((t.clone(), "Bool")),
            Val::Str(t) => leaves.push((t.clone(), "String")),
            Val::Tuple(xs) => xs.iter().for_each(|x| self.open_input(x, depth, lens, leaves)),
            Val::Rec(_, fs) => fs.iter().for_each(|(_, x)| self.open_input(x, depth, lens, leaves)),
            Val::Distinct(_, x) => self.open_input(x, depth, lens, leaves),
            Val::Sum { tag, ctors, .. } => {
                leaves.push((tag.clone(), "Int"));
                ctors.iter().flat_map(|c| &c.1).for_each(|x| self.open_input(x, depth + 1, lens, leaves));
            }
            Val::Union { tag, slots } => {
                leaves.push((tag.clone(), "Int"));
                slots.iter().for_each(|(_, x)| self.open_input(x, depth, lens, leaves));
            }
            Val::List { len, id } => {
                leaves.push((len.clone(), "Int"));
                lens.push(len.clone());
                if self.input_elems.contains_key(id) || !self.positional(*id) {
                    return;
                }
                let xs: Vec<Val> = (0..INPUT_LEN).filter_map(|k| self.element_at(*id, &k.to_string(), &format!("(> {len} {k})"))).collect();
                if xs.len() < INPUT_LEN {
                    return;
                }
                let xs: Vec<Val> = xs.into_iter().map(|x| self.force(x)).collect();
                self.input_elems.insert(*id, xs.clone());
                for x in &xs {
                    self.open_input(x, depth + 1, lens, leaves);
                }
            }
            Val::Lazy(_) | Val::Unit | Val::Absent | Val::Opaque(_) | Val::FnOf { .. } => {}
        }
    }

    pub fn env(&self) -> &'a Env {
        self.env
    }

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
        // what a loop may need to keep: the `ensures`, of the variable returned
        if let Expr::Block(_, tail) = &f.body {
            if let Expr::Var(x) = &**tail {
                self.hints = f.ensures.iter().map(|e| (e.clone(), x.to_string())).collect();
            }
        }
        self.requires_hints = f.requires.clone();
        let res = self.eval(&f.body);
        // the result is checked where the body ends, and at every early return
        let mut exits = vec![];
        if !self.dead {
            exits.push((self.pc.clone(), res, ""));
        }
        for (pc, v) in std::mem::take(&mut self.returns) {
            exits.push((pc, v, " (early return)"));
        }
        // a prelude trait's method by its short name (`FiniteIterator::len`)
        let label = crate::types::shown_name(label);
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
    Refuted(Vec<(String, Expr)>, String),
    Unknown(String),
    /// Stopped before it answered (`verify`'s fuzzer found a failure first).
    Stopped,
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

/// Run `script` on the solver: its answer, `Ok(None)` if it can't be run,
/// `Err(())` once `stop` is set (the process is killed).
fn run_solver(solver: &Solver, script: &str, stop: &std::sync::atomic::AtomicBool) -> Result<Option<String>, ()> {
    use std::sync::atomic::Ordering::Relaxed;
    let Some(_slot) = solver_slot_until(stop) else { return Err(()) };
    let Ok(mut child) = Command::new(&solver.bin).args(solver.args()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else { return Ok(None) };
    let Some(mut stdin) = child.stdin.take() else { return Ok(None) };
    if stdin.write_all(script.as_bytes()).is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Ok(None);
    }
    drop(stdin);
    // read the answer on another thread, so a long one can't fill the pipe while we wait
    let Some(mut stdout) = child.stdout.take() else { return Ok(None) };
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        let _ = std::io::Read::read_to_string(&mut stdout, &mut out);
        out
    });
    loop {
        if stop.load(Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            // (the reader ends when the pipe closes: not waited for, a child of the solver may hold it)
            drop(reader);
            return Err(());
        }
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(2)),
            Err(_) => return Ok(None),
        }
    }
    Ok(reader.join().ok())
}

/// Solver processes running at once, at most one per core: `verify` checks
/// functions, and each function's obligations, in parallel.
static RUNNING: (std::sync::Mutex<usize>, std::sync::Condvar) = (std::sync::Mutex::new(0), std::sync::Condvar::new());

/// A place to run a solver process, held until dropped.
pub struct SolverSlot;

/// A place to run a solver process, or `None` once `stop` is set.
pub fn solver_slot_until(stop: &std::sync::atomic::AtomicBool) -> Option<SolverSlot> {
    let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
    let (lock, cv) = &RUNNING;
    let mut n = lock.lock().unwrap_or_else(|e| e.into_inner());
    while *n >= cores {
        if stop.load(std::sync::atomic::Ordering::Relaxed) {
            return None;
        }
        n = cv.wait_timeout(n, std::time::Duration::from_millis(20)).unwrap_or_else(|e| e.into_inner()).0;
    }
    if stop.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    *n += 1;
    Some(SolverSlot)
}

impl Drop for SolverSlot {
    fn drop(&mut self) {
        let (lock, cv) = &RUNNING;
        *lock.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
        cv.notify_one();
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
        // `utf8_len`: a string's length in bytes (`byte_len`), known where it is used
        let mut s = String::from("(set-option :produce-models true)\n(set-logic ALL)\n(declare-fun utf8_len (String) Int)\n");
        for (n, sort) in &self.decls {
            if consts.contains(n) || leaves.contains(n) {
                if sort.starts_with('(') {
                    s.push_str(&format!("(declare-fun {n} {sort})\n"));
                } else {
                    s.push_str(&format!("(declare-const {n} {sort})\n"));
                }
            }
        }
        for f in facts {
            s.push_str(&format!("(assert {f})\n"));
        }
        for p in &ob.pc {
            s.push_str(&format!("(assert {p})\n"));
        }
        s.push_str(&format!("(assert (not {}))\n", ob.goal));
        // no string is longer than memory: without this bound Z3 tries to
        // build a string of 10^308 characters to refute `len(s) < 10^308`
        for t in str_len_terms(&s) {
            s.push_str(&format!("(assert (< {t} 18446744073709551616))\n"));
        }
        s.push_str("(check-sat)\n");
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
        self.solve_until(solver, i, &std::sync::atomic::AtomicBool::new(false))
    }

    /// `solve`, giving up (`Stopped`) as soon as `stop` is set: the solver
    /// process is killed.
    pub fn solve_until(&self, solver: &Solver, i: usize, stop: &std::sync::atomic::AtomicBool) -> Outcome {
        let ob = &self.obligations[i];
        let mut leaves = vec![];
        for (_, v) in &self.params {
            leaves_of(&self.cells, v, &mut leaves);
        }
        let script = self.script(ob, &leaves);
        let out = match run_solver(solver, &script, stop) {
            Err(()) => return Outcome::Stopped,
            Ok(None) => return Outcome::Unknown("the solver could not be run".into()),
            Ok(Some(out)) => out,
        };
        let first = out.lines().next().unwrap_or("").trim();
        let why = self.why(ob);
        let because = if why.is_empty() { String::new() } else { format!(" (depends on {})", why.join("; ")) };
        match first {
            "unsat" => Outcome::Proven,
            "sat" => {
                let model = parse_values(&out[first.len()..]);
                let args: Option<Vec<(String, Expr)>> = self.params.iter().map(|(n, v)| to_literal(self.env, &self.cells, v, &model).map(|x| (n.clone(), x))).collect();
                let shown = |args: &[(String, Expr)]| args.iter().map(|(n, v)| format!("{n} = {}", crate::printer::literal(v))).collect::<Vec<_>>().join(", ");
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

/// The `(str.len x)` terms of a script whose `x` is free of quantified
/// variables (a constant, or a function of constants).
fn str_len_terms(script: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut rest = script;
    while let Some(k) = rest.find("(str.len ") {
        let t = &rest[k..];
        let mut depth = 0;
        let mut end = t.len();
        for (i, c) in t.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        let term = &t[..end];
        // (a term under a quantifier names its bound variables: left out)
        let free = !term.contains('"') && term.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '.').all(|w| w.is_empty() || w == "str.len" || (w.starts_with('v') && w[1..].chars().all(|c| c.is_ascii_digit())) || w.chars().all(|c| c.is_ascii_digit()));
        if free && !out.iter().any(|o| o == term) {
            out.push(term.to_string());
        }
        rest = &t[9..];
    }
    out
}

/// The unknowns a model must give a value for, to rebuild `v` (the parts made).
fn leaves_of(cells: &[Cell], v: &Val, out: &mut Vec<String>) {
    match v {
        Val::Int(t) | Val::Bool(t) | Val::Str(t) => out.push(t.clone()),
        Val::Tuple(xs) => xs.iter().for_each(|x| leaves_of(cells, x, out)),
        Val::Rec(_, fs) => fs.iter().for_each(|f| leaves_of(cells, &f.1, out)),
        Val::Sum { tag, ctors, .. } => {
            out.push(tag.clone());
            ctors.iter().flat_map(|c| &c.1).for_each(|x| leaves_of(cells, x, out));
        }
        Val::Union { tag, slots } => {
            out.push(tag.clone());
            slots.iter().for_each(|s| leaves_of(cells, &s.1, out));
        }
        Val::List { len, .. } => out.push(len.clone()),
        Val::Distinct(_, x) => leaves_of(cells, x, out),
        Val::Lazy(id) => {
            if let Some(x) = &cells[*id].value {
                leaves_of(cells, x, out)
            }
        }
        Val::Unit | Val::Absent | Val::Opaque(_) | Val::FnOf { .. } => {}
    }
}

/// Some value of type `t`, as code (the simplest one there is): a part of
/// a counterexample that was never read can be anything. Replaying the
/// counterexample checks it (an invalid one is discarded there).
fn some_literal(env: &Env, t: &TypeExpr, depth: usize) -> Option<Expr> {
    if depth > 8 {
        return None;
    }
    Some(match env.expand_alias(t) {
        TypeExpr::Unit => Expr::Unit,
        TypeExpr::Union(ms) => return ms.iter().find_map(|m| some_literal(env, m, depth + 1)),
        TypeExpr::Named(n, args) => match n.as_str() {
            "Int" => Expr::Int(0),
            "Bool" => Expr::Bool(false),
            "String" => Expr::Str(String::new()),
            "List" => Expr::List(vec![]),
            "Tuple" => Expr::Tuple(args.iter().map(|a| some_literal(env, a, depth + 1)).collect::<Option<_>>()?),
            _ => {
                let td = env.types.get(&n.sym())?;
                if td.distinct {
                    let TypeBody::Alias(a) = &td.body else { return None };
                    let sub: HashMap<Symbol, TypeExpr> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
                    return Some(Expr::Ctor(n, CtorArgs::Positional(vec![some_literal(env, &subst_texpr(a, &sub), depth + 1)?])));
                }
                // the constructor with the fewest fields that can be made
                let mut vs = env.variants_of(&n, &args)?;
                vs.sort_by_key(|v| v.2.len());
                vs.into_iter().find_map(|(c, names, tys)| {
                    let fields: Vec<Expr> = tys.iter().map(|t| some_literal(env, t, depth + 1)).collect::<Option<_>>()?;
                    Some(Expr::Ctor(
                        c,
                        match names {
                            _ if fields.is_empty() => CtorArgs::None,
                            Some(ns) => CtorArgs::Record(ns.into_iter().zip(fields).collect()),
                            None => CtorArgs::Positional(fields),
                        },
                    ))
                })?
            }
        },
        _ => return None,
    })
}

/// A parameter's value in a model, as the code that writes it (when it can
/// be rebuilt: lists only when empty, nothing opaque).
fn to_literal(env: &Env, cells: &[Cell], v: &Val, m: &HashMap<String, String>) -> Option<Expr> {
    to_literal_in(env, cells, &HashMap::default(), v, m)
}

/// `to_literal`, with the first elements of lists (`Vc::input_elems`): a
/// list is shown when the model's length is within them.
fn to_literal_in(env: &Env, cells: &[Cell], elems: &HashMap<usize, Vec<Val>>, v: &Val, m: &HashMap<String, String>) -> Option<Expr> {
    let to_literal = |env: &Env, v: &Val, m: &HashMap<String, String>| to_literal_in(env, cells, elems, v, m);
    Some(match v {
        Val::Lazy(id) => match &cells[*id].value {
            Some(x) => to_literal(env, x, m)?,
            None => some_literal(env, &cells[*id].ty, 0)?,
        },
        Val::Int(t) => crate::llvm::native::int_expr(parse_big(m.get(t)?)?),
        Val::Bool(t) => Expr::Bool(m.get(t)? == "true"),
        Val::Str(t) => Expr::Str(parse_str(m.get(t)?)?),
        Val::Unit => Expr::Unit,
        Val::Tuple(xs) => Expr::Tuple(xs.iter().map(|x| to_literal(env, x, m)).collect::<Option<_>>()?),
        Val::Rec(n, fs) => Expr::Ctor(Symbol::new(n), CtorArgs::Record(fs.iter().map(|(f, x)| to_literal(env, x, m).map(|y| (Symbol::new(f), y))).collect::<Option<_>>()?)),
        Val::Distinct(n, x) => Expr::Ctor(Symbol::new(n), CtorArgs::Positional(vec![to_literal(env, x, m)?])),
        Val::List { len, id } => {
            let n = parse_int(m.get(len)?)?;
            if n == 0 {
                return Some(Expr::List(vec![]));
            }
            let xs = elems.get(id).filter(|xs| (n as usize) <= xs.len())?;
            Expr::List(xs[..n as usize].iter().map(|x| to_literal(env, x, m)).collect::<Option<_>>()?)
        }
        Val::Sum { ty, args, tag, ctors } => {
            let k = parse_int(m.get(tag)?)? as usize;
            let (c, payload) = ctors.get(k)?;
            let vs = env.variants_of(ty, args)?;
            let fields: Vec<Expr> = payload.iter().map(|x| to_literal(env, x, m)).collect::<Option<_>>()?;
            let f = match &vs.get(k)?.1 {
                _ if fields.is_empty() => CtorArgs::None,
                Some(names) => CtorArgs::Record(names.iter().copied().zip(fields).collect()),
                None => CtorArgs::Positional(fields),
            };
            Expr::Ctor(Symbol::new(c), f)
        }
        // a union value is its active member's value
        Val::Union { tag, slots } => to_literal(env, &slots.get(parse_int(m.get(tag)?)? as usize)?.1, m)?,
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

    /// `std.regex.relations`, as the prover calls it: `(a <= b, b <= a, disjoint)`.
    #[test]
    fn regex_relations_from_std() {
        // compile-time code runs as the CLI runs it, on a large stack
        std::thread::Builder::new().stack_size(1 << 30).spawn(relations_from_std).unwrap().join().unwrap();
    }

    fn relations_from_std() {
        for (a, b, want) in [
            ("[a-c]+", "[a-z]+", (true, false, false)),
            ("[0-9]+", "[a-z]+", (false, false, true)),
            ("a|b", "b|a", (true, true, false)),
            ("\\d{3}", "\\d+", (true, false, false)),
            ("\\d{3}", "\\d{2}", (false, false, true)),
            ("\\w+", "[a-z]+", (false, true, false)),
            ("\\p{L}+", "[0-9]*", (false, false, true)),
            // undecided: a pattern a DFA can't match, or no regex at all
            ("a\\bb", "ab", (false, false, false)),
            ("(", "a", (false, false, false)),
        ] {
            assert_eq!(regex_relations(a, b), want, "{a} vs {b}");
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

/// 2^k, in decimal (k up to a few thousand: SMT-LIB integers are unbounded).
fn num_bigint_pow2(k: i64) -> String {
    // decimal digits, least significant first
    let mut d: Vec<u8> = vec![1];
    for _ in 0..k {
        let mut carry = 0;
        for x in d.iter_mut() {
            let v = *x * 2 + carry;
            *x = v % 10;
            carry = v / 10;
        }
        if carry > 0 {
            d.push(carry);
        }
    }
    d.iter().rev().map(|x| (b'0' + x) as char).collect()
}

/// Whether SMT string `s` is ASCII (its code points are its bytes).
fn ascii(s: &str) -> String {
    format!("(str.in_re {s} (re.* (re.range \"\\u{{0}}\" \"\\u{{7f}}\")))")
}

/// `s.code_points()` / `s.bytes()` (as a method call, through its trait or
/// not): the unit and the string.
fn string_unit(e: &Expr) -> Option<(&'static str, &Expr)> {
    let (name, recv) = match e {
        Expr::Method(r, m, args) if args.is_empty() => (m.as_str(), &**r),
        Expr::Call(f, args) if args.len() == 1 => match &**f {
            Expr::Var(n) => (n.as_str(), &args[0]),
            _ => return None,
        },
        _ => return None,
    };
    match crate::names::short(crate::types::split_trait_call(name).map_or(name, |(_, m)| m)) {
        "code_points" => Some(("code_points", recv)),
        "bytes" => Some(("bytes", recv)),
        _ => None,
    }
}
