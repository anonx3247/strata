// Nested and mutual datatypes: a list of the type inside it (`List[Expr]`)
// is a datatype of its own (`List_Expr`) defined with it, and functions on
// the two recurse into each other. `Expr` is a slice of the compiler's own
// tree (compiler/ast.strata).

type BinOp = | Add | Sub | Mul

type UnOp = | Neg | Not

type Expr =
  | IntLit(Int)
  | BoolLit(Bool)
  | Var(String)
  | ListExpr(List[Expr])
  | Call { callee: Expr, args: List[Expr] }
  | Binary { op: BinOp, left: Expr, right: Expr }
  | Unary { op: UnOp, operand: Expr }
  | IfExpr { cond: Expr, then: Expr, otherwise: Expr }

/// The nodes of `e`.
fn nodes(e: Expr) -> Int {
  match e {
    IntLit(_) => 1,
    BoolLit(_) => 1,
    Var(_) => 1,
    ListExpr(xs) => 1 + nodes_all(xs),
    Call { callee, args } => 1 + nodes(callee) + nodes_all(args),
    Binary { left, right, .. } => 1 + nodes(left) + nodes(right),
    Unary { operand, .. } => 1 + nodes(operand),
    IfExpr { cond, then, otherwise } => 1 + nodes(cond) + nodes(then) + nodes(otherwise),
  }
}

fn nodes_all(es: List[Expr]) -> Int {
  match es {
    [] => 0,
    [x, ..rest] => nodes(x) + nodes_all(rest),
  }
}

lemma nodes_pos(e: Expr) ensures nodes(e) >= 1 {
  ?proof
}

lemma nodes_all_nonneg(es: List[Expr]) ensures nodes_all(es) >= 0 {
  ?proof
}

lemma nodes_not(e: Expr) ensures nodes(Unary { op: Not, operand: e }) == nodes(e) + 1 {
  ?proof
}

lemma nodes_call_one(f: Expr, x: Expr) ensures nodes(Call { callee: f, args: [x] }) == 1 + nodes(f) + nodes(x) {
  ?proof
}
