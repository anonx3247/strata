//! Ranges, slices and comprehensions are kept in the tree as written, so they
//! render back the same. Once a program is resolved they become calls to
//! builtins, and nothing that checks, runs or verifies code sees them:
//!
//! - `a..b` is `range(a, b)`, `a..=b` is `range(a, b + 1)`;
//! - `xs[i]` is `at(xs, i)`, which fails when `i` is out of range;
//! - `xs[a..b]` is `slice(xs, a, b)`, with `0` for a missing start and
//!   `len(xs)` for a missing end;
//! - `[e for x in xs where c]` is `map(filter(xs, x -> c), x -> e)`, and each
//!   further `for` nests in a `fold` that joins the lists;
//! - `Dict { k: v }` is `dict([(k, v)])`, `Set { x }` is `set([x])`, and their
//!   comprehensions are the same around a list comprehension. Type arguments
//!   (`Dict[String, Int] {}`) become an annotated `let`;
//! - a record constructor that leaves out a field with a default
//!   (`times: Int = 1`) gets the default: `Options { names }` is
//!   `Options { names, times: 1 }`.

use crate::ast::*;
use crate::types::Diag;
use crate::fx::HashMap;

pub fn program(p: &mut Program, errors: &mut Vec<Diag>) {
    for d in &mut p.decls {
        let mut s = Sugar { decl: d.name(), errors: Vec::new(), fresh: 0 };
        walk_decl_mut(&mut s, &mut d.kind);
        errors.extend(s.errors);
    }
    defaults(p);
    inline_refinements(p);
}

/// The prefix of the name given to a refinement written inside a type
/// argument (`List[Int where it > 0]`): `$where_0`, an alias like `Positive`,
/// so the type argument names it when the code runs.
pub const WHERE: &str = "$where_";

/// Each refinement written as a type argument becomes an alias of its own
/// (the same text, the same alias).
fn inline_refinements(p: &mut Program) {
    struct Named {
        made: Vec<(String, Symbol, TypeExpr, Expr)>,
    }
    impl Named {
        fn name(&mut self, b: &TypeExpr, pred: &Expr) -> Symbol {
            let text = crate::printer::ty(&TypeExpr::Refined(Box::new(b.clone()), Box::new(pred.clone())));
            if let Some((_, n, _, _)) = self.made.iter().find(|m| m.0 == text) {
                return *n;
            }
            let n = Symbol::new(&format!("{WHERE}{}", self.made.len()));
            self.made.push((text, n, b.clone(), pred.clone()));
            n
        }
    }
    impl VisitMut for Named {
        fn ty(&mut self, t: &mut TypeExpr) {
            walk_ty_mut(self, t);
            if let TypeExpr::Named(_, args) = t {
                for a in args.iter_mut() {
                    if let TypeExpr::Refined(b, pred) = a {
                        let n = self.name(b, pred);
                        *a = TypeExpr::Named(n, vec![]);
                    }
                }
            }
        }
    }
    let mut v = Named { made: vec![] };
    for d in &mut p.decls {
        walk_decl_mut(&mut v, &mut d.kind);
    }
    for (_, n, b, pred) in v.made {
        p.decls.push(Decl {
            comments: vec![],
            doc: vec![],
            kind: DeclKind::Type(TypeDecl { distinct: false, name: n, params: vec![], body: TypeBody::Alias(b), refinement: Some(pred) }),
            module: Symbol::new(""),
            public: true,
        });
    }
}

/// Fill in the fields a record constructor leaves out that have defaults.
fn defaults(p: &mut Program) {
    let mut of: HashMap<Symbol, Vec<(Symbol, Expr)>> = HashMap::default();
    let mut add = |name: Symbol, fs: &[Field]| {
        let ds: Vec<(Symbol, Expr)> = fs.iter().filter_map(|f| Some((f.name, f.default.clone()?))).collect();
        if !ds.is_empty() {
            of.insert(name, ds);
        }
    };
    for d in &p.decls {
        if let DeclKind::Type(t) = &d.kind {
            match &t.body {
                TypeBody::Record(fs) => add(t.name, fs),
                TypeBody::Sum(vs) => vs.iter().for_each(|v| {
                    if let Payload::Record(fs) = &v.payload {
                        add(v.name, fs)
                    }
                }),
                TypeBody::Alias(_) => {}
            }
        }
    }
    if of.is_empty() {
        return;
    }
    // outermost first, so a default that is itself a constructor is filled in too
    for d in &mut p.decls {
        decl_exprs_mut(&mut d.kind, |e| {
            if let Expr::Ctor(c, CtorArgs::Record(fs)) = e {
                for (name, default) in of.get(&c.sym()).into_iter().flatten() {
                    if !fs.iter().any(|(f, _)| f == name) {
                        fs.push((name.clone(), default.clone()));
                    }
                }
            }
        });
    }
}

struct Sugar {
    decl: String,
    errors: Vec<Diag>,
    fresh: usize,
}

impl VisitMut for Sugar {
    // innermost first, so what is rebuilt holds no sugar
    fn expr(&mut self, e: &mut Expr) {
        match e {
            // a slice's range stays a range until the slice is rewritten
            Expr::Index(xs, r) if matches!(**r, Expr::Range(..)) => {
                self.expr(xs);
                walk_expr_mut(self, r)
            }
            _ => walk_expr_mut(self, e),
        }
        if matches!(e, Expr::Range(..) | Expr::Index(..) | Expr::Comp(..) | Expr::Dict(..) | Expr::Set(..)) {
            let sugar = std::mem::replace(e, Expr::Unit);
            *e = self.rewrite(sugar);
        }
    }
}

fn call(f: &str, args: Vec<Expr>) -> Expr {
    Expr::Call(Box::new(Expr::Var(f.into())), args)
}

fn plus_one(e: Expr) -> Expr {
    Expr::Bin(BinOp::Add, Box::new(e), Box::new(Expr::Int(1)))
}

impl Sugar {
    fn name(&mut self, what: &str) -> Symbol {
        self.fresh += 1;
        Symbol::from(format!("${what}{}", self.fresh))
    }

    fn rewrite(&mut self, e: Expr) -> Expr {
        match e {
            Expr::Range(Some(lo), Some(hi), inclusive) => call("range", vec![*lo, if inclusive { plus_one(*hi) } else { *hi }]),
            Expr::Range(..) => {
                let msg = "a range needs both ends (`a..b`), except in a slice: `xs[a..]`, `xs[..b]`".to_string();
                self.errors.push(Diag { decl: self.decl.clone(), msg });
                Expr::Unit
            }
            Expr::Index(xs, i) if !matches!(*i, Expr::Range(..)) => call("at", vec![*xs, *i]),
            Expr::Index(xs, r) => {
                let Expr::Range(lo, hi, inclusive) = *r else { unreachable!() };
                let lo = lo.map_or(Expr::Int(0), |x| *x);
                match hi {
                    Some(hi) => call("slice", vec![*xs, lo, if inclusive { plus_one(*hi) } else { *hi }]),
                    // the end is `len(xs)`: name `xs` unless it is a variable or a field already
                    None if place_path(&xs).is_some() => call("slice", vec![(*xs).clone(), lo, call("len", vec![*xs])]),
                    None => {
                        let v = self.name("sliced");
                        let body = call("slice", vec![Expr::Var(v.clone()), lo, call("len", vec![Expr::Var(v.clone())])]);
                        Expr::Block(vec![Stmt::Let(Pattern::Bind(v), None, *xs)], Box::new(body))
                    }
                }
            }
            Expr::Comp(kind, elem, gens) => {
                let list = self.comp(*elem, gens);
                match kind {
                    CompKind::List => list,
                    CompKind::Set => call("set", vec![list]),
                    CompKind::Dict => call("dict", vec![list]),
                }
            }
            Expr::Dict(tys, kvs) => {
                let pairs = kvs.into_iter().map(|(k, v)| Expr::Tuple(vec![k, v])).collect();
                self.typed("Dict", tys, call("dict", vec![Expr::List(pairs)]))
            }
            Expr::Set(tys, xs) => self.typed("Set", tys, call("set", vec![Expr::List(xs)])),
            e => e,
        }
    }

    /// `e`, as a `name[tys]` when type arguments are written.
    fn typed(&mut self, name: &str, tys: Vec<TypeExpr>, e: Expr) -> Expr {
        if tys.is_empty() {
            return e;
        }
        let v = self.name("lit");
        let t = TypeExpr::Named(name.into(), tys);
        Expr::Block(vec![Stmt::Let(Pattern::Bind(v.clone()), Some(t), e)], Box::new(Expr::Var(v)))
    }

    fn comp(&mut self, elem: Expr, mut gens: Vec<CompFor>) -> Expr {
        let g = gens.remove(0);
        let xs = match g.cond {
            Some(c) => {
                let keep = self.lambda(vec![], &g.pat, c);
                call("filter", vec![g.iter, keep])
            }
            None => g.iter,
        };
        if gens.is_empty() {
            let each = self.lambda(vec![], &g.pat, elem);
            return call("map", vec![xs, each]);
        }
        // `for x in xs for y in ys`: the lists made for each `x`, joined
        let acc = self.name("acc");
        let inner = self.comp(elem, gens);
        let join = Expr::Bin(BinOp::Add, Box::new(Expr::Var(acc.clone())), Box::new(inner));
        let step = self.lambda(vec![LamParam { name: acc, ty: None }], &g.pat, join);
        call("fold", vec![xs, Expr::List(vec![]), step])
    }

    /// A lambda taking `params` and then an element matched against `pat`.
    fn lambda(&mut self, mut params: Vec<LamParam>, pat: &Pattern, body: Expr) -> Expr {
        let body = match pat {
            Pattern::Bind(x) => {
                params.push(LamParam { name: x.clone(), ty: None });
                body
            }
            _ => {
                let v = self.name("item");
                params.push(LamParam { name: v.clone(), ty: None });
                Expr::Block(vec![Stmt::Let(pat.clone(), None, Expr::Var(v))], Box::new(body))
            }
        };
        Expr::Lambda(params, Box::new(body))
    }
}
