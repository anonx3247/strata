//! The Strata syntax tree: the canonical representation of code. Every
//! analysis and rewrite works on these types, through the walkers at the end.

use serde::{Deserialize, Serialize};
pub use crate::symbol::{Sym, Symbol};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TypeExpr {
    /// `Int`, `Order`, `List[T]`, `T`
    Named(Symbol, Vec<TypeExpr>),
    /// `(A, B) -> C`
    Fn(Vec<TypeExpr>, Box<TypeExpr>),
    Unit,
    /// Inline refinement: `Int where 5 <= it && it <= 13`. Structural: any
    /// value of the base type is accepted where the predicate holds.
    Refined(Box<TypeExpr>, Box<Expr>),
    /// `Square | Circle`: any value of any member (a set union, so
    /// `Int | Positive` is just `Int`).
    Union(Vec<TypeExpr>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    /// `//` comment lines just above it, as written ("" is a blank line between groups).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<String>,
    /// `///` doc comment lines (e.g. a command-line option's help).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub doc: Vec<String>,
    pub name: Symbol,
    pub ty: TypeExpr,
    /// `times: Int = 1`: the value a constructor, JSON input or the command
    /// line gives the field when it is left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Payload {
    None,
    Positional(Vec<TypeExpr>),
    Record(Vec<Field>),
    /// A member of a union that is a type defined elsewhere (`Circle` in
    /// `type Shape = Circle | Square { side: Float }`): the variant's name
    /// is that type's.
    Type(TypeExpr),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Variant {
    /// `//` comment lines just above it, as written ("" is a blank line between groups).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<String>,
    pub name: Symbol,
    pub payload: Payload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TypeBody {
    /// `type Port = Int where ...` — a (possibly refined) alias
    Alias(TypeExpr),
    /// `type Line = { sku: String, qty: Quantity }`
    Record(Vec<Field>),
    /// `type Order = | Pending {..} | Shipped {..}`
    Sum(Vec<Variant>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypeDecl {
    /// `distinct type UserId = String`: a new nominal type, built with
    /// `UserId(s)` and unwrapped with `.value`; never mixed up with its base.
    #[serde(default)]
    pub distinct: bool,
    pub name: Symbol,
    pub params: Vec<Symbol>,
    pub body: TypeBody,
    /// Refinement predicate over `it`.
    pub refinement: Option<Expr>,
    /// `mock MemFiles = { ... }`: a fake for tests. Used only in tests and
    /// other mocks; its implementations may leave methods out (generated).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mock: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Param {
    pub name: Symbol,
    pub ty: TypeExpr,
    /// `mut x: T` / `mut self`: the function may change it, and the caller's
    /// variable is updated when the call returns.
    #[serde(default)]
    pub mutable: bool,
    /// `comptime f: Field`: known when the program is compiled; the function
    /// is compiled once per value given (so `f.type` is a type in each copy).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub comptime: bool,
}

/// `T: Ord + Hash` on a type parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bound {
    pub param: Symbol,
    /// Trait references, e.g. `Worker[P, E]`.
    pub traits: Vec<TypeExpr>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FnDecl {
    /// Doc and comments of a method inside an `impl` (a top-level function
    /// keeps them on its `Decl`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub doc: Vec<String>,
    /// `//` comment lines just above it, as written ("" is a blank line between groups).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<String>,
    pub name: Symbol,
    pub type_params: Vec<Symbol>,
    #[serde(default)]
    pub bounds: Vec<Bound>,
    pub params: Vec<Param>,
    pub ret: TypeExpr,
    pub requires: Vec<Expr>,
    /// Postconditions; `it` is the return value.
    pub ensures: Vec<Expr>,
    /// `terminates`: the author asserts the function always finishes, where
    /// `verify` can't show it. Reported as assumed, never as proven.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub terminates: bool,
    pub body: Expr,
    /// `comptime fn`: runs only while the program is compiled.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub comptime: bool,
    /// `derived fn` in a derivable trait: its body is specialized for each
    /// type the trait is derived for.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub derived: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestDecl {
    pub name: String,
    /// The capabilities it asks for, as `main` does (`test "opens" (sqlite: Sqlite) { ... }`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<Param>,
    pub body: Expr,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraitMethod {
    /// `//` comment lines just above it, as written ("" is a blank line between groups).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<String>,
    pub doc: Vec<String>,
    /// Signature (and default body when `default` is true). The first
    /// parameter is always `self: Self`.
    pub decl: FnDecl,
    pub default: bool,
}

/// `law associative(a: Self, b: Self, c: Self): ...`, a property every
/// implementation must satisfy. Checked by `strata verify`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Law {
    /// `//` comment lines just above it, as written ("" is a blank line between groups).
    pub comments: Vec<String>,
    /// `///` doc comment lines.
    pub doc: Vec<String>,
    pub name: Symbol,
    pub params: Vec<Param>,
    pub body: Expr,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraitDecl {
    pub name: Symbol,
    pub params: Vec<Symbol>,
    /// Supertraits: implementing this trait requires implementing these.
    pub supers: Vec<TypeExpr>,
    pub methods: Vec<TraitMethod>,
    pub laws: Vec<Law>,
    /// `derivable(t: Record) trait ... requires ...`: the trait can be
    /// derived (`derive Trait for Type`) for types of that kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derivable: Option<Derivable>,
    /// `capability trait`: an interface to the outside world (`Console`,
    /// `Files`, `Database`). Anything may implement one (fakes for tests do).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub capability: bool,
}

/// What a derivable trait says about the types it can be derived for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Derivable {
    /// The kind, a type of `std.typing` (`Record`, `Sum`, `Choice`).
    pub kind: TypeExpr,
    /// Conditions on the type, checked when it is derived.
    pub requires: Vec<Expr>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImplDecl {
    pub type_params: Vec<Symbol>,
    pub bounds: Vec<Bound>,
    /// Empty for methods defined on the type itself (`impl Item { ... }`).
    pub trait_name: Symbol,
    pub trait_args: Vec<TypeExpr>,
    pub for_type: TypeExpr,
    pub methods: Vec<FnDecl>,
    /// `derive Trait for Type { ... }`: the trait's `derived` methods are
    /// made for the type; `methods` are the ones written by hand.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub derived: bool,
}

impl FnDecl {
    /// The function without its body (`()`) or notes: what checking calls to
    /// it needs.
    pub fn signature(&self) -> FnDecl {
        FnDecl {
            doc: vec![],
            comments: vec![],
            name: self.name,
            type_params: self.type_params.clone(),
            bounds: self.bounds.clone(),
            params: self.params.clone(),
            ret: self.ret.clone(),
            requires: self.requires.clone(),
            ensures: self.ensures.clone(),
            terminates: self.terminates,
            body: Expr::Unit,
            comptime: self.comptime,
            derived: self.derived,
        }
    }
}

impl ImplDecl {
    /// The block with its methods' signatures only.
    pub fn signatures(&self) -> ImplDecl {
        ImplDecl {
            type_params: self.type_params.clone(),
            bounds: self.bounds.clone(),
            trait_name: self.trait_name,
            trait_args: self.trait_args.clone(),
            for_type: self.for_type.clone(),
            methods: self.methods.iter().map(FnDecl::signature).collect(),
            derived: self.derived,
        }
    }
}

impl TraitDecl {
    /// The trait with its methods' signatures only (laws kept).
    pub fn signatures(&self) -> TraitDecl {
        TraitDecl {
            name: self.name,
            params: self.params.clone(),
            supers: self.supers.clone(),
            methods: self.methods.iter().map(|m| TraitMethod { comments: vec![], doc: vec![], decl: m.decl.signature(), default: m.default }).collect(),
            laws: self.laws.clone(),
            derivable: self.derivable.clone(),
            capability: self.capability,
        }
    }
}

impl ImplDecl {
    /// `impl Item { ... }`: methods of the type itself, not of a trait.
    pub fn is_inherent(&self) -> bool {
        self.trait_name.is_empty()
    }

    pub fn name(&self) -> String {
        if self.is_inherent() {
            return format!("impl {}", crate::printer::ty(&self.for_type));
        }
        let tr = crate::printer::ty(&TypeExpr::Named(self.trait_name.clone(), self.trait_args.clone()));
        let kw = if self.derived { "derive" } else { "impl" };
        format!("{kw} {tr} for {}", crate::printer::ty(&self.for_type))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DeclKind {
    Type(TypeDecl),
    Fn(FnDecl),
    Test(TestDecl),
    Trait(TraitDecl),
    Impl(ImplDecl),
    Use(UseDecl),
    /// `extern c fn ...` / `extern c type ...`: a definition made in another
    /// language, called through its ABI (docs/language.md, "Foreign functions").
    Extern(ExternDecl),
    /// `capability Sqlite { fn open(path: String) -> ... }`: functions that
    /// reach the outside world, holding no data. Only the runtime makes one,
    /// for an entry point (`main`, a test); its functions are called on it.
    Capability(CapabilityDecl),
}

/// A capability: its name and its functions (written without `self`: the
/// value they are called on is the permission, and holds nothing).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilityDecl {
    pub name: Symbol,
    pub methods: Vec<FnDecl>,
}

/// A foreign definition: its language and what it is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExternDecl {
    pub lang: Lang,
    pub item: ExternItem,
}

/// The language of an `extern` (only C so far).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lang {
    C,
}

impl Lang {
    pub fn name(self) -> &'static str {
        match self {
            Lang::C => "c",
        }
    }
    pub fn from_name(s: &str) -> Option<Lang> {
        match s {
            "c" => Some(Lang::C),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExternItem {
    /// `extern c fn strlen(s: String) -> U64`: a signature with contracts and
    /// no body (`body` is `()`); the symbol is the function's name.
    Fn(FnDecl),
    /// `extern c type Sqlite3`: an opaque handle (a pointer), only passed back.
    Type(Symbol),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decl {
    /// `//` comment lines just above it, as written ("" is a blank line between groups).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<String>,
    /// `///` doc comment lines, kept in the tree so text round-trips.
    pub doc: Vec<String>,
    pub kind: DeclKind,
    /// Dotted module path (`shop.orders`); empty for the root module.
    #[serde(default)]
    pub module: Symbol,
    /// `pub`: part of the module's contract, visible to other modules.
    #[serde(default)]
    pub public: bool,
}

/// `use orders.{Order, ship}` / `use orders.*`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UseDecl {
    pub path: Symbol,
    /// `None` means everything public (`.*`).
    pub items: Option<Vec<Symbol>>,
}

impl Decl {
    /// The definition's identity in the codebase: `module.name`.
    pub fn qname(&self) -> String {
        if self.module.is_empty() { self.name() } else { format!("{}.{}", self.module, self.name()) }
    }

    pub fn name(&self) -> String {
        match &self.kind {
            DeclKind::Type(t) => t.name.to_string(),
            DeclKind::Fn(f) => f.name.to_string(),
            DeclKind::Test(t) => format!("test \"{}\"", t.name),
            DeclKind::Trait(t) => t.name.to_string(),
            DeclKind::Impl(i) => i.name(),
            DeclKind::Use(u) => format!("use {}", u.path),
            DeclKind::Extern(e) => match &e.item {
                ExternItem::Fn(f) => f.name.to_string(),
                ExternItem::Type(t) => t.to_string(),
            },
            DeclKind::Capability(c) => c.name.to_string(),
        }
    }
    pub fn kind_str(&self) -> &'static str {
        match &self.kind {
            DeclKind::Type(_) => "type",
            DeclKind::Fn(_) => "fn",
            DeclKind::Test(_) => "test",
            DeclKind::Trait(_) => "trait",
            DeclKind::Impl(_) => "impl",
            DeclKind::Use(_) => "use",
            DeclKind::Extern(_) => "extern",
            DeclKind::Capability(_) => "capability",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BinOp {
    /// Numbers, and joining strings or lists (`"a" + "b"`, `[1] + [2]`).
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Implies,
    /// `&`, `|`, `^` on integers, bit by bit (as two's complement, infinitely
    /// sign-extended: `-1 & x` is `x`), and `<<`, `>>`: times or floor-divided
    /// by a power of two (the shift can't be negative).
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

impl BinOp {
    pub fn symbol(self) -> &'static str {
        use BinOp::*;
        match self {
            Add => "+",
            Sub => "-",
            Mul => "*",
            Div => "/",
            Mod => "%",
            Eq => "==",
            Ne => "!=",
            Lt => "<",
            Le => "<=",
            Gt => ">",
            Ge => ">=",
            And => "&&",
            Or => "||",
            Implies => "==>",
            BitAnd => "&",
            BitOr => "|",
            BitXor => "^",
            Shl => "<<",
            Shr => ">>",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Pattern {
    Wild,
    Bind(Symbol),
    Int(i64),
    Str(String),
    Bool(bool),
    /// `None`, `Some(p)`, `Pending { id, lines: l, .. }`
    Ctor(Symbol, CtorPat),
    /// `[]`, `[a, b]`, `[x, ..rest]`
    List(Vec<Pattern>, Option<Symbol>),
    /// `(a, _)`
    Tuple(Vec<Pattern>),
    /// `where 3 <= it && it <= 5`: the values for which the condition holds.
    Cond(Box<Expr>),
    /// `Square | Circle`, `0 | 1`: any of the alternatives (they bind the same names).
    Or(Vec<Pattern>),
    /// `v => body for v in $Self.variants`, as a match arm: one arm per
    /// element of a list known while compiling, the element known in each
    /// (the arm's pattern, the loop's pattern, the list).
    For(Box<Pattern>, Box<Pattern>, Box<Expr>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CtorPat {
    None,
    Positional(Vec<Pattern>),
    /// field patterns, and whether `..` was present
    Record(Vec<(Symbol, Pattern)>, bool),
    /// a type's arguments: `List[Int]` (a type pattern), `Option[x]` (in `match comptime t`)
    Type(Vec<Pattern>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Arm {
    /// `//` comment lines just above it, as written ("" is a blank line between groups).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<String>,
    pub pat: Pattern,
    pub body: Expr,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Stmt {
    /// `p := e`, `let p: T = e`
    Let(Pattern, Option<TypeExpr>, Expr),
    /// `mut x := e`, `mut x: T = e`
    Var(Symbol, Option<TypeExpr>, Expr),
    /// `x = e`, `x.field = e`; `x += e` is `x = x + e`
    Assign(Expr, Expr),
    /// `while cond { ... }`
    While(Expr, Expr),
    /// `for pattern in xs { ... }`
    For(Pattern, Expr, Expr),
    /// `loop { ... }`: repeat until `break`
    Loop(Expr),
    Break,
    Continue,
    Expr(Expr),
    Assert(Expr),
    /// `//` comment lines before the next statement; no effect when run.
    Comment(Vec<String>),
    /// `comptime for v in $Self.variants { ... }`: the loop runs while the
    /// program is compiled; its body is written out once per element, with
    /// the element known in each copy.
    ComptimeFor(Pattern, Expr, Expr),
    /// `invariant e`, written between a loop's header and its body
    /// (`while c invariant e { ... }`) and kept as the first statements of
    /// the body: true on entry and after every pass and `continue`.
    Invariant(Expr),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Int(i64),
    /// An integer literal too large for 64 bits (decimal digits): `Int` is a
    /// big integer when it needs to be.
    BigInt(String),
    Float(f64),
    /// `(a, b)`
    Tuple(Vec<Expr>),
    Str(String),
    Bool(bool),
    Unit,
    Var(Symbol),
    /// Typed hole: `?name`. Checks against anything, fails at runtime.
    Hole(Symbol),
    List(Vec<Expr>),
    /// `Some(x)`, `None`, `Line { sku: s, qty: 1 }`
    Ctor(Symbol, CtorArgs),
    Call(Box<Expr>, Vec<Expr>),
    /// `e.f(args)` — method-call sugar for `f(e, args)`
    Method(Box<Expr>, Symbol, Vec<Expr>),
    Field(Box<Expr>, Symbol),
    /// `x -> body`, `(a, b) -> body`, `(s: String) -> body`
    Lambda(Vec<LamParam>, Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    Un(UnOp, Box<Expr>),
    /// `e is Shipped`
    Is(Box<Expr>, Symbol),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `match e { … }`, or `match e as x { … }` to name the value; a matched
    /// variable (or the `as` name) is narrowed to each arm's pattern.
    Match(Box<Expr>, Option<Symbol>, Vec<Arm>),
    /// `mut x` as an argument: passed to a `mut` parameter, which changes it.
    Mut(Box<Expr>),
    Block(Vec<Stmt>, Box<Expr>),
    /// `mock Db`: an implementation of a trait whose methods return generated
    /// values that satisfy the trait's contracts.
    Mock(TypeExpr),
    /// `return e` (`return` alone returns `()`): leave the enclosing function
    /// or lambda early.
    Return(Box<Expr>),
    /// `e!`: the `Ok` value of a `Result`, or return its `Err` from the function.
    /// `e?`: the `Some` value of an `Option`, or return `None` from the function.
    Try(Box<Expr>, TryKind),
    /// `e except { fallback }`, `e except err { fallback }`, `e !! fallback`:
    /// a `Result`'s `Ok` value, or else the fallback, which can use the error
    /// under `err`.
    Except(Box<Expr>, Option<Symbol>, Box<Expr>),
    /// `e or { fallback }`, `e ?? fallback`: an `Option`'s `Some` value, or
    /// else the fallback.
    Or(Box<Expr>, Box<Expr>),
    /// `f"{name} has {len(xs)} items"`: each part shown as by `to_string`.
    Interp(Vec<StrPart>),
    /// `comptime e`: evaluated while the program is compiled; the value is
    /// put in the program.
    Comptime(Box<Expr>),
    /// `a..b` (`b` excluded), `a..=b` (`b` included): the integers between,
    /// as a `List[Int]`. An end may be left out only in a slice (`xs[2..]`).
    Range(Option<Box<Expr>>, Option<Box<Expr>>, bool),
    /// `xs[i]`: one element of a list or string (`i` must be in range).
    /// `xs[a..b]`, `s[..3]`, `xs[2..]`: part of it, clamped to its length
    /// (the index is a `Range`, which may leave out its ends here).
    Index(Box<Expr>, Box<Expr>),
    /// `[x * x for x in xs where x > 0]`, `[(a, b) for a in xs for b in ys]`:
    /// a list built from each element of one or more lists; `Set { … for … }`,
    /// `Dict { k: v for … }` (the element is then the tuple `(k, v)`).
    Comp(CompKind, Box<Expr>, Vec<CompFor>),
    /// `Dict { "a": 1, "b": 2 }`, `Dict[String, Int] {}`: the type arguments
    /// (none, or the key and value types) and the entries.
    Dict(Vec<TypeExpr>, Vec<(Expr, Expr)>),
    /// `Set { 1, 2 }`, `Set[Int] {}`.
    Set(Vec<TypeExpr>, Vec<Expr>),
    /// `scope { … }`: a block that ends only once every task spawned in it
    /// has finished, awaited or not.
    Scope(Box<Expr>),
    /// `spawn e`: `e` run in a new task of the enclosing `scope`; a handle,
    /// `Task[T]`.
    Spawn(Box<Expr>),
    /// `await h`: a task's result; of a tuple or list of handles, a tuple or
    /// list of results.
    Await(Box<Expr>),
    /// `select { rx.recv() as x => …, tx.send(v) => …, await h as r => … }`:
    /// waits until one of the operations can happen, does that one only, and
    /// runs its arm.
    Select(Vec<SelectArm>),
}

/// One arm of a `select`: an operation that may wait (`rx.recv()`,
/// `tx.send(v)`, `await h`), the name its result gets (`as x`), and the code
/// run when it is the one that happens.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelectArm {
    /// `//` comment lines just above it, as written ("" is a blank line between groups).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<String>,
    pub op: Expr,
    pub name: Option<Symbol>,
    pub body: Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum CompKind {
    List,
    Set,
    Dict,
}

/// One `for p in xs where cond` of a comprehension.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompFor {
    pub pat: Pattern,
    pub iter: Expr,
    pub cond: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StrPart {
    Lit(String),
    /// `{e}` or `{e:spec}`.
    Expr(Expr, Option<FmtSpec>),
}

/// How an f-string part is shown: `[[fill]align][+][0][width][.precision]`,
/// a subset of Python's format specs. `{price:.2}`, `{name:<12}`, `{n:05}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FmtSpec {
    pub fill: char,
    /// `<` left, `>` right, `^` centre; `None` is the default for the type
    /// (numbers right, everything else left).
    pub align: Option<char>,
    /// `+`: always show the sign of a number.
    pub plus: bool,
    /// `0`: pad a number with zeros after its sign.
    pub zero: bool,
    pub width: usize,
    /// `.N`: decimal places of a `Float`, or the most characters of a `String`.
    pub precision: Option<usize>,
}

impl FmtSpec {
    pub fn parse(s: &str) -> Result<FmtSpec, String> {
        let cs: Vec<char> = s.chars().collect();
        let mut f = FmtSpec { fill: ' ', align: None, plus: false, zero: false, width: 0, precision: None };
        let mut i = 0;
        let is_align = |c: char| matches!(c, '<' | '>' | '^');
        if cs.len() >= 2 && is_align(cs[1]) {
            if matches!(cs[0], '{' | '}') {
                return Err(format!("`{}` can't be a fill character", cs[0]));
            }
            (f.fill, f.align, i) = (cs[0], Some(cs[1]), 2);
        } else if cs.first().is_some_and(|&c| is_align(c)) {
            (f.align, i) = (Some(cs[0]), 1);
        }
        if cs.get(i) == Some(&'+') {
            (f.plus, i) = (true, i + 1);
        }
        if cs.get(i) == Some(&'0') {
            (f.zero, i) = (true, i + 1);
        }
        let digits = |i: &mut usize| {
            let st = *i;
            while cs.get(*i).is_some_and(|c| c.is_ascii_digit()) {
                *i += 1;
            }
            cs[st..*i].iter().collect::<String>()
        };
        let w = digits(&mut i);
        if !w.is_empty() {
            f.width = w.parse().map_err(|_| format!("width `{w}` is too large"))?;
        }
        if cs.get(i) == Some(&'.') {
            i += 1;
            let p = digits(&mut i);
            if p.is_empty() {
                return Err("expected digits after `.` in a format spec".into());
            }
            f.precision = Some(p.parse().map_err(|_| format!("precision `{p}` is too large"))?);
        }
        if i != cs.len() {
            return Err(format!("bad format spec `{s}`: expected `[[fill]align][+][0][width][.precision]`, e.g. `.2`, `>8`, `05`"));
        }
        if s.is_empty() {
            return Err("empty format spec after `:`".into());
        }
        Ok(f)
    }
}

impl std::fmt::Display for FmtSpec {
    fn fmt(&self, o: &mut std::fmt::Formatter) -> std::fmt::Result {
        if let Some(a) = self.align {
            if self.fill != ' ' {
                write!(o, "{}", self.fill)?;
            }
            write!(o, "{a}")?;
        }
        if self.plus {
            write!(o, "+")?;
        }
        if self.zero {
            write!(o, "0")?;
        }
        if self.width > 0 {
            write!(o, "{}", self.width)?;
        }
        if let Some(p) = self.precision {
            write!(o, ".{p}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TryKind {
    /// `!` on a `Result`
    Result,
    /// `?` on an `Option`
    Option,
}

/// A lambda parameter, with an optional type: `x` or `x: Int`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LamParam {
    pub name: Symbol,
    pub ty: Option<TypeExpr>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CtorArgs {
    None,
    Positional(Vec<Expr>),
    Record(Vec<(Symbol, Expr)>),
    /// `Entry { ..e, status: Done }`: copy of `e` with some fields replaced
    Update(Box<Expr>, Vec<(Symbol, Expr)>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Program {
    pub decls: Vec<Decl>,
}

// ---- walking the tree ----
//
// `Visit` (read) and `VisitMut` (change in place) reach every node: a
// definition's expressions, statements, patterns and types, in the order they
// are written. Each method's default visits the node's children; override
// `expr`, `pattern` or `ty` to act on a node (and call `walk_*` to go on into
// its children, or not, to skip them). The helpers below cover the usual
// case of a closure over expressions.

macro_rules! visitor {
    ($Visit:ident, $expr:ident, $stmt:ident, $pattern:ident, $ty:ident, $func:ident, $decl:ident $(, $m:ident)?) => {
        pub trait $Visit {
            fn expr(&mut self, e: &$($m)? Expr) {
                $expr(self, e)
            }
            fn pattern(&mut self, p: &$($m)? Pattern) {
                $pattern(self, p)
            }
            fn ty(&mut self, t: &$($m)? TypeExpr) {
                $ty(self, t)
            }
        }

        /// Visit the children of an expression.
        pub fn $expr<V: $Visit + ?Sized>(v: &mut V, e: &$($m)? Expr) {
            match e {
                Expr::Int(_) | Expr::BigInt(_) | Expr::Float(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Unit | Expr::Var(_) | Expr::Hole(_) => {}
                Expr::Tuple(xs) | Expr::List(xs) => xs.into_iter().for_each(|x| v.expr(x)),
                Expr::Ctor(_, args) => match args {
                    CtorArgs::None => {}
                    CtorArgs::Positional(xs) => xs.into_iter().for_each(|x| v.expr(x)),
                    CtorArgs::Record(fs) => fs.into_iter().for_each(|(_, x)| v.expr(x)),
                    CtorArgs::Update(b, fs) => {
                        v.expr(b);
                        fs.into_iter().for_each(|(_, x)| v.expr(x))
                    }
                },
                Expr::Call(f, xs) | Expr::Method(f, _, xs) => {
                    v.expr(f);
                    xs.into_iter().for_each(|x| v.expr(x))
                }
                Expr::Field(r, _) | Expr::Is(r, _) | Expr::Un(_, r) | Expr::Return(r) | Expr::Mut(r) | Expr::Try(r, _) | Expr::Comptime(r) | Expr::Scope(r) | Expr::Spawn(r) | Expr::Await(r) => v.expr(r),
                Expr::Lambda(ps, body) => {
                    for p in ps {
                        if let Some(t) = &$($m)? p.ty {
                            v.ty(t)
                        }
                    }
                    v.expr(body)
                }
                Expr::Bin(_, l, r) | Expr::Except(l, _, r) | Expr::Or(l, r) => {
                    v.expr(l);
                    v.expr(r)
                }
                Expr::If(c, t, f) => {
                    v.expr(c);
                    v.expr(t);
                    v.expr(f)
                }
                Expr::Match(s, _, arms) => {
                    v.expr(s);
                    for a in arms {
                        v.pattern(&$($m)? a.pat);
                        v.expr(&$($m)? a.body)
                    }
                }
                Expr::Select(arms) => {
                    for a in arms {
                        v.expr(&$($m)? a.op);
                        v.expr(&$($m)? a.body)
                    }
                }
                Expr::Block(stmts, last) => {
                    stmts.into_iter().for_each(|s| $stmt(v, s));
                    v.expr(last)
                }
                Expr::Mock(t) => v.ty(t),
                Expr::Interp(parts) => {
                    for p in parts {
                        if let StrPart::Expr(x, _) = p {
                            v.expr(x)
                        }
                    }
                }
                Expr::Range(lo, hi, _) => {
                    if let Some(lo) = lo {
                        v.expr(lo)
                    }
                    if let Some(hi) = hi {
                        v.expr(hi)
                    }
                }
                Expr::Index(x, r) => {
                    v.expr(x);
                    v.expr(r)
                }
                Expr::Dict(tys, kvs) => {
                    tys.into_iter().for_each(|t| v.ty(t));
                    for (k, x) in kvs {
                        v.expr(k);
                        v.expr(x)
                    }
                }
                Expr::Set(tys, xs) => {
                    tys.into_iter().for_each(|t| v.ty(t));
                    xs.into_iter().for_each(|x| v.expr(x))
                }
                Expr::Comp(_, elem, gens) => {
                    for g in gens {
                        v.expr(&$($m)? g.iter);
                        v.pattern(&$($m)? g.pat);
                        if let Some(c) = &$($m)? g.cond {
                            v.expr(c)
                        }
                    }
                    v.expr(elem)
                }
            }
        }

        /// Visit the children of a statement.
        pub fn $stmt<V: $Visit + ?Sized>(v: &mut V, s: &$($m)? Stmt) {
            match s {
                Stmt::Comment(_) | Stmt::Break | Stmt::Continue => {}
                Stmt::Let(p, t, x) => {
                    v.pattern(p);
                    if let Some(t) = t {
                        v.ty(t)
                    }
                    v.expr(x)
                }
                Stmt::Var(_, t, x) => {
                    if let Some(t) = t {
                        v.ty(t)
                    }
                    v.expr(x)
                }
                Stmt::For(p, a, b) | Stmt::ComptimeFor(p, a, b) => {
                    v.pattern(p);
                    v.expr(a);
                    v.expr(b)
                }
                Stmt::Assign(a, b) | Stmt::While(a, b) => {
                    v.expr(a);
                    v.expr(b)
                }
                Stmt::Loop(x) | Stmt::Expr(x) | Stmt::Assert(x) | Stmt::Invariant(x) => v.expr(x),
            }
        }

        /// Visit the children of a pattern.
        pub fn $pattern<V: $Visit + ?Sized>(v: &mut V, p: &$($m)? Pattern) {
            match p {
                Pattern::Wild | Pattern::Bind(_) | Pattern::Int(_) | Pattern::Str(_) | Pattern::Bool(_) => {}
                Pattern::Ctor(_, CtorPat::None) => {}
                Pattern::Ctor(_, CtorPat::Positional(ps) | CtorPat::Type(ps)) | Pattern::List(ps, _) | Pattern::Tuple(ps) | Pattern::Or(ps) => ps.into_iter().for_each(|p| v.pattern(p)),
                Pattern::For(p, x, xs) => {
                    v.pattern(p);
                    v.pattern(x);
                    v.expr(xs)
                }
                Pattern::Ctor(_, CtorPat::Record(fs, _)) => fs.into_iter().for_each(|(_, p)| v.pattern(p)),
                Pattern::Cond(e) => v.expr(e),
            }
        }

        /// Visit the children of a type.
        pub fn $ty<V: $Visit + ?Sized>(v: &mut V, t: &$($m)? TypeExpr) {
            match t {
                TypeExpr::Unit => {}
                TypeExpr::Named(_, ts) | TypeExpr::Union(ts) => ts.into_iter().for_each(|t| v.ty(t)),
                TypeExpr::Fn(ps, r) => {
                    ps.into_iter().for_each(|t| v.ty(t));
                    v.ty(r)
                }
                TypeExpr::Refined(b, e) => {
                    v.ty(b);
                    v.expr(e)
                }
            }
        }

        #[allow(dead_code)] // the read-only twin may not be used yet
        /// Visit a function: its bounds, parameter and return types, contract and body.
        pub fn $func<V: $Visit + ?Sized>(v: &mut V, f: &$($m)? FnDecl) {
            for b in &$($m)? f.bounds {
                (&$($m)? b.traits).into_iter().for_each(|t| v.ty(t))
            }
            for p in &$($m)? f.params {
                v.ty(&$($m)? p.ty)
            }
            v.ty(&$($m)? f.ret);
            (&$($m)? f.requires).into_iter().for_each(|e| v.expr(e));
            (&$($m)? f.ensures).into_iter().for_each(|e| v.expr(e));
            v.expr(&$($m)? f.body)
        }

        #[allow(dead_code)] // the read-only twin may not be used yet
        /// Visit everything in a definition.
        pub fn $decl<V: $Visit + ?Sized>(v: &mut V, d: &$($m)? DeclKind) {
            let fields = |v: &mut V, fs: &$($m)? Vec<Field>| {
                for f in fs {
                    v.ty(&$($m)? f.ty);
                    if let Some(x) = &$($m)? f.default {
                        v.expr(x)
                    }
                }
            };
            match d {
                DeclKind::Type(t) => {
                    match &$($m)? t.body {
                        TypeBody::Alias(x) => v.ty(x),
                        TypeBody::Record(fs) => fields(v, fs),
                        TypeBody::Sum(vs) => {
                            for var in vs {
                                match &$($m)? var.payload {
                                    Payload::None => {}
                                    Payload::Positional(ts) => ts.into_iter().for_each(|t| v.ty(t)),
                                    Payload::Record(fs) => fields(v, fs),
                                    Payload::Type(t) => v.ty(t),
                                }
                            }
                        }
                    }
                    if let Some(r) = &$($m)? t.refinement {
                        v.expr(r)
                    }
                }
                DeclKind::Fn(f) => $func(v, f),
                DeclKind::Test(t) => {
                    for p in &$($m)? t.params {
                        v.ty(&$($m)? p.ty)
                    }
                    v.expr(&$($m)? t.body)
                }
                DeclKind::Trait(t) => {
                    (&$($m)? t.supers).into_iter().for_each(|t| v.ty(t));
                    for m in &$($m)? t.methods {
                        $func(v, &$($m)? m.decl)
                    }
                    for l in &$($m)? t.laws {
                        for p in &$($m)? l.params {
                            v.ty(&$($m)? p.ty)
                        }
                        v.expr(&$($m)? l.body)
                    }
                }
                DeclKind::Impl(i) => {
                    for b in &$($m)? i.bounds {
                        (&$($m)? b.traits).into_iter().for_each(|t| v.ty(t))
                    }
                    (&$($m)? i.trait_args).into_iter().for_each(|t| v.ty(t));
                    v.ty(&$($m)? i.for_type);
                    for m in &$($m)? i.methods {
                        $func(v, m)
                    }
                }
                DeclKind::Use(_) => {}
                DeclKind::Extern(e) => match &$($m)? e.item {
                    ExternItem::Fn(f) => $func(v, f),
                    ExternItem::Type(_) => {}
                },
                DeclKind::Capability(c) => {
                    for m in &$($m)? c.methods {
                        $func(v, m)
                    }
                }
            }
        }
    };
}

visitor!(Visit, walk_expr, walk_stmt, walk_pattern, walk_ty, walk_fn, walk_decl);
visitor!(VisitMut, walk_expr_mut, walk_stmt_mut, walk_pattern_mut, walk_ty_mut, walk_fn_mut, walk_decl_mut, mut);

/// A closure over expressions, outermost first, that doesn't go into
/// patterns or types (the refinements written in them).
struct Exprs<F>(F);

impl<F: FnMut(&Expr)> Visit for Exprs<F> {
    fn expr(&mut self, e: &Expr) {
        (self.0)(e);
        walk_expr(self, e)
    }
    fn pattern(&mut self, _: &Pattern) {}
    fn ty(&mut self, _: &TypeExpr) {}
}

/// The same, allowing changes; `post`: innermost first.
struct ExprsMut<F> {
    f: F,
    post: bool,
}

impl<F: FnMut(&mut Expr)> VisitMut for ExprsMut<F> {
    fn expr(&mut self, e: &mut Expr) {
        if !self.post {
            (self.f)(e);
        }
        walk_expr_mut(self, e);
        if self.post {
            (self.f)(e);
        }
    }
    fn pattern(&mut self, _: &mut Pattern) {}
    fn ty(&mut self, _: &mut TypeExpr) {}
}

/// `ExprsMut`, innermost first, going into types and patterns too: the
/// refinements written in them (`zip: String where len(it.code_points()) == 5`)
/// and their `where` conditions.
struct ExprsMutTyped<F>(F);

impl<F: FnMut(&mut Expr)> VisitMut for ExprsMutTyped<F> {
    fn expr(&mut self, e: &mut Expr) {
        walk_expr_mut(self, e);
        (self.0)(e);
    }
    // (a pattern's `where` conditions run too)
    fn pattern(&mut self, p: &mut Pattern) {
        walk_pattern_mut(self, p)
    }
    fn ty(&mut self, t: &mut TypeExpr) {
        walk_ty_mut(self, t)
    }
}

/// Every expression of a declaration, innermost first, those in its types'
/// refinements and its patterns' conditions too.
pub fn decl_exprs_mut_post_typed(d: &mut DeclKind, f: impl FnMut(&mut Expr)) {
    walk_decl_mut(&mut ExprsMutTyped(f), d)
}

/// Visit `e` and every expression inside it, outermost first (not those in
/// patterns or types).
pub fn walk(e: &Expr, f: impl FnMut(&Expr)) {
    Exprs(f).expr(e)
}

/// Every expression of a definition (bodies, contracts, a type's refinement;
/// not those in patterns or written types), outermost first, allowing changes.
pub fn decl_exprs_mut(d: &mut DeclKind, f: impl FnMut(&mut Expr)) {
    walk_decl_mut(&mut ExprsMut { f, post: false }, d)
}

/// `decl_exprs_mut`, and also the refinements in written types
/// (`s: String where matches(it, "…")`), outermost first.
pub fn decl_all_exprs_mut(d: &mut DeclKind, f: impl FnMut(&mut Expr)) {
    struct All<F>(F);
    impl<F: FnMut(&mut Expr)> VisitMut for All<F> {
        fn expr(&mut self, e: &mut Expr) {
            (self.0)(e);
            walk_expr_mut(self, e);
        }
        fn pattern(&mut self, _: &mut Pattern) {}
        fn ty(&mut self, t: &mut TypeExpr) {
            walk_ty_mut(self, t);
        }
    }
    walk_decl_mut(&mut All(f), d)
}

/// The same, innermost first: a node is changed after everything inside it.
pub fn decl_exprs_mut_post(d: &mut DeclKind, f: impl FnMut(&mut Expr)) {
    walk_decl_mut(&mut ExprsMut { f, post: true }, d)
}

/// Whether anything in `e` is `pred` (not looking into patterns or types).
pub fn any_expr(e: &Expr, mut pred: impl FnMut(&Expr) -> bool) -> bool {
    let mut found = false;
    walk(e, |x| found = found || pred(x));
    found
}

/// The written path of a place: `x`, `o.status` (`mut o.status` as an argument).
pub fn place_path(e: &Expr) -> Option<String> {
    match e {
        Expr::Mut(x) => place_path(x),
        Expr::Var(x) => Some(x.to_string()),
        Expr::Field(r, f) => Some(format!("{}.{f}", place_path(r)?)),
        _ => None,
    }
}

/// The places (`x`, `o.status`) that `e` may change: those assigned, and
/// those passed to a `mut` parameter (a method's receiver too: `q.claim(2)`
/// changes `q` when `claim` takes `mut self`). `mut_params(callee)` says which
/// of a callee's parameters are `mut`. A closure can't change the enclosing
/// function's variables, so lambdas are not looked into.
pub fn changed_places(e: &Expr, mut_params: &dyn Fn(&str) -> Vec<bool>) -> Vec<String> {
    struct Changes<'a> {
        mut_params: &'a dyn Fn(&str) -> Vec<bool>,
        out: Vec<String>,
    }
    impl Changes<'_> {
        fn passed<'e>(&mut self, callee: &str, args: impl Iterator<Item = &'e Expr>) {
            for (m, a) in (self.mut_params)(callee).into_iter().zip(args) {
                if m {
                    self.out.extend(place_path(a));
                }
            }
        }
    }
    impl Visit for Changes<'_> {
        fn expr(&mut self, e: &Expr) {
            match e {
                Expr::Lambda(..) => return,
                Expr::Call(f, args) => {
                    if let Expr::Var(n) = &**f {
                        self.passed(n, args.iter())
                    }
                }
                Expr::Method(r, m, args) => self.passed(m, std::iter::once(&**r).chain(args)),
                Expr::Block(stmts, _) => {
                    for s in stmts {
                        if let Stmt::Assign(p, _) = s {
                            self.out.extend(place_path(p))
                        }
                    }
                }
                _ => {}
            }
            walk_expr(self, e)
        }
        fn pattern(&mut self, _: &Pattern) {}
        fn ty(&mut self, _: &TypeExpr) {}
    }
    let mut c = Changes { mut_params, out: Vec::new() };
    c.expr(e);
    c.out
}

/// The variables `e` may change (the roots of `changed_places`).
pub fn changed_vars(e: &Expr, mut_params: &dyn Fn(&str) -> Vec<bool>) -> crate::fx::HashSet<String> {
    changed_places(e, mut_params).into_iter().map(|p| p.split('.').next().unwrap_or_default().to_string()).collect()
}
