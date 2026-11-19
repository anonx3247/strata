//! The Strata syntax tree. This is the canonical representation of code:
//! the codebase stores these nodes (as JSON, content-addressed), and text is
//! only ever a rendering of them.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TypeExpr {
    /// `Int`, `Order`, `List[T]`, `T`
    Named(String, Vec<TypeExpr>),
    /// `(A, B) -> C`
    Fn(Vec<TypeExpr>, Box<TypeExpr>),
    Unit,
    /// Inline refinement: `Int where 5 <= it && it <= 13`. Structural: any
    /// value of the base type is accepted where the predicate holds.
    Refined(Box<TypeExpr>, Box<Expr>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    /// `//` comment lines just above it, as written ("" is a blank line between groups).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<String>,
    pub name: String,
    pub ty: TypeExpr,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Payload {
    None,
    Positional(Vec<TypeExpr>),
    Record(Vec<Field>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Variant {
    /// `//` comment lines just above it, as written ("" is a blank line between groups).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<String>,
    pub name: String,
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
    pub name: String,
    pub params: Vec<String>,
    pub body: TypeBody,
    /// Refinement predicate over `it`.
    pub refinement: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub ty: TypeExpr,
    /// `mut x: T` / `mut self`: the function may change it, and the caller's
    /// variable is updated when the call returns.
    #[serde(default)]
    pub mutable: bool,
}

/// `T: Ord + Hash` on a type parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bound {
    pub param: String,
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
    pub name: String,
    pub type_params: Vec<String>,
    #[serde(default)]
    pub bounds: Vec<Bound>,
    pub params: Vec<Param>,
    pub ret: TypeExpr,
    pub requires: Vec<Expr>,
    /// Postconditions; `it` is the return value.
    pub ensures: Vec<Expr>,
    pub body: Expr,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestDecl {
    pub name: String,
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
    pub name: String,
    pub params: Vec<Param>,
    pub body: Expr,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraitDecl {
    pub name: String,
    pub params: Vec<String>,
    /// Supertraits: implementing this trait requires implementing these.
    pub supers: Vec<TypeExpr>,
    pub methods: Vec<TraitMethod>,
    pub laws: Vec<Law>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImplDecl {
    pub type_params: Vec<String>,
    pub bounds: Vec<Bound>,
    pub trait_name: String,
    pub trait_args: Vec<TypeExpr>,
    pub for_type: TypeExpr,
    pub methods: Vec<FnDecl>,
}

impl ImplDecl {
    pub fn name(&self) -> String {
        let tr = crate::printer::ty(&TypeExpr::Named(self.trait_name.clone(), self.trait_args.clone()));
        format!("impl {tr} for {}", crate::printer::ty(&self.for_type))
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
    pub module: String,
    /// `pub`: part of the module's contract, visible to other modules.
    #[serde(default)]
    pub public: bool,
}

/// `use orders.{Order, ship}` / `use orders.*`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UseDecl {
    pub path: String,
    /// `None` means everything public (`.*`).
    pub items: Option<Vec<String>>,
}

impl Decl {
    /// The definition's identity in the codebase: `module.name`.
    pub fn qname(&self) -> String {
        if self.module.is_empty() { self.name() } else { format!("{}.{}", self.module, self.name()) }
    }

    pub fn name(&self) -> String {
        match &self.kind {
            DeclKind::Type(t) => t.name.clone(),
            DeclKind::Fn(f) => f.name.clone(),
            DeclKind::Test(t) => format!("test \"{}\"", t.name),
            DeclKind::Trait(t) => t.name.clone(),
            DeclKind::Impl(i) => i.name(),
            DeclKind::Use(u) => format!("use {}", u.path),
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
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BinOp {
    /// Numbers, and joining strings or lists (`"a" + "b"`, `[1] + [2]`).
    /// Older trees spelled list/string joining `Concat` (`++`).
    #[serde(alias = "Concat")]
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
    Bind(String),
    Int(i64),
    Str(String),
    Bool(bool),
    /// `None`, `Some(p)`, `Pending { id, lines: l, .. }`
    Ctor(String, CtorPat),
    /// `[]`, `[a, b]`, `[x, ..rest]`
    List(Vec<Pattern>, Option<String>),
    /// `(a, _)`
    Tuple(Vec<Pattern>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CtorPat {
    None,
    Positional(Vec<Pattern>),
    /// field patterns, and whether `..` was present
    Record(Vec<(String, Pattern)>, bool),
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
    Let(Pattern, Option<TypeExpr>, Expr),
    /// `let mut x = e`
    Var(String, Option<TypeExpr>, Expr),
    /// `x = e`, `x.field = e`
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Int(i64),
    Float(f64),
    /// `(a, b)`
    Tuple(Vec<Expr>),
    Str(String),
    Bool(bool),
    Unit,
    Var(String),
    /// Typed hole: `?name`. Checks against anything, fails at runtime.
    Hole(String),
    List(Vec<Expr>),
    /// `Some(x)`, `None`, `Line { sku: s, qty: 1 }`
    Ctor(String, CtorArgs),
    Call(Box<Expr>, Vec<Expr>),
    /// `e.f(args)` — method-call sugar for `f(e, args)`
    Method(Box<Expr>, String, Vec<Expr>),
    Field(Box<Expr>, String),
    /// `x -> body`, `(a, b) -> body`, `(s: String) -> body`
    Lambda(Vec<LamParam>, Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    Un(UnOp, Box<Expr>),
    /// `e is Shipped`
    Is(Box<Expr>, String),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    Match(Box<Expr>, Vec<Arm>),
    Block(Vec<Stmt>, Box<Expr>),
    /// `mock Db`: an implementation of a trait whose methods return generated
    /// values that satisfy the trait's contracts.
    Mock(TypeExpr),
}

/// A lambda parameter, with an optional type: `x` or `x: Int`.
/// Stored as a plain string when it has no type, as lambdas always were.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "LamParamRepr", into = "LamParamRepr")]
pub struct LamParam {
    pub name: String,
    pub ty: Option<TypeExpr>,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum LamParamRepr {
    Name(String),
    Typed { name: String, ty: TypeExpr },
}

impl From<LamParamRepr> for LamParam {
    fn from(r: LamParamRepr) -> Self {
        match r {
            LamParamRepr::Name(name) => LamParam { name, ty: None },
            LamParamRepr::Typed { name, ty } => LamParam { name, ty: Some(ty) },
        }
    }
}

impl From<LamParam> for LamParamRepr {
    fn from(p: LamParam) -> Self {
        match p.ty {
            None => LamParamRepr::Name(p.name),
            Some(ty) => LamParamRepr::Typed { name: p.name, ty },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CtorArgs {
    None,
    Positional(Vec<Expr>),
    Record(Vec<(String, Expr)>),
    /// `Entry { ..e, status: Done }`: copy of `e` with some fields replaced
    Update(Box<Expr>, Vec<(String, Expr)>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Program {
    pub decls: Vec<Decl>,
}
