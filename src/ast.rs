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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpSig {
    #[serde(default)]
    pub doc: Vec<String>,
    pub name: String,
    pub params: Vec<Param>,
    pub ret: TypeExpr,
    /// Contract every handler of this operation must honour.
    #[serde(default)]
    pub requires: Vec<Expr>,
    #[serde(default)]
    pub ensures: Vec<Expr>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectDecl {
    pub name: String,
    #[serde(default)]
    pub params: Vec<String>,
    pub ops: Vec<OpSig>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FnDecl {
    pub name: String,
    pub type_params: Vec<String>,
    pub params: Vec<Param>,
    pub ret: TypeExpr,
    /// Effects this function may perform, e.g. `Db` or `Queue[P, E]`.
    pub effects: Vec<TypeExpr>,
    pub requires: Vec<Expr>,
    /// Postconditions; `it` is the return value.
    pub ensures: Vec<Expr>,
    pub body: Expr,
}

impl FnDecl {
    pub fn effect_names(&self) -> Vec<String> {
        self.effects.iter().map(|e| match e {
            TypeExpr::Named(n, _) => n.clone(),
            other => crate::printer::ty(other),
        }).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestDecl {
    pub name: String,
    pub body: Expr,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DeclKind {
    Type(TypeDecl),
    Effect(EffectDecl),
    Fn(FnDecl),
    Test(TestDecl),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decl {
    /// `///` doc comment lines, kept in the tree so text round-trips.
    pub doc: Vec<String>,
    pub kind: DeclKind,
}

impl Decl {
    pub fn name(&self) -> String {
        match &self.kind {
            DeclKind::Type(t) => t.name.clone(),
            DeclKind::Effect(e) => e.name.clone(),
            DeclKind::Fn(f) => f.name.clone(),
            DeclKind::Test(t) => format!("test \"{}\"", t.name),
        }
    }
    pub fn kind_str(&self) -> &'static str {
        match &self.kind {
            DeclKind::Type(_) => "type",
            DeclKind::Effect(_) => "effect",
            DeclKind::Fn(_) => "fn",
            DeclKind::Test(_) => "test",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Concat,
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
            Concat => "++",
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
    pub pat: Pattern,
    pub body: Expr,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Stmt {
    Let(Pattern, Option<TypeExpr>, Expr),
    Expr(Expr),
    Assert(Expr),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HandlerKind {
    /// `with Db { load(id) => ..., save(o) => () }`
    Clauses(Vec<HandlerClause>),
    /// `with mock Db` — every op returns a generated value of its return type
    MockAuto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandlerClause {
    pub op: String,
    pub params: Vec<String>,
    pub body: Expr,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Handler {
    pub effect: String,
    pub kind: HandlerKind,
    /// Stateful handler: `with Queue(q = init) { ... }`. Clauses see the state
    /// as `q` and answer with `Resume { value, state }`.
    #[serde(default)]
    pub state: Option<(String, Expr)>,
    /// `return(r) => expr`: transforms the handled computation's result
    /// (with the final state in scope).
    #[serde(default)]
    pub on_return: Option<(String, Expr)>,
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
    /// `Db.save(o)` — perform an effect operation
    Perform(String, String, Vec<Expr>),
    /// `e.f(args)` — method-call sugar for `f(e, args)`
    Method(Box<Expr>, String, Vec<Expr>),
    Field(Box<Expr>, String),
    Lambda(Vec<String>, Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    Un(UnOp, Box<Expr>),
    /// `e is Shipped`
    Is(Box<Expr>, String),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    Match(Box<Expr>, Vec<Arm>),
    Block(Vec<Stmt>, Box<Expr>),
    Handle(Box<Expr>, Vec<Handler>),
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
