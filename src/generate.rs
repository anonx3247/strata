//! Value generation from types. Every type (including refined ones) gets a
//! generator for free; it powers `mock` handlers and `verify` fuzzing.

use crate::ast::*;
use crate::interp::{Fields, Interp, Value};
use crate::types::{subst_texpr, Env};
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use std::collections::HashMap;

pub struct Gen {
    rng: StdRng,
}

#[derive(Default, Clone, Debug)]
struct Hints {
    lo: Option<i64>,
    hi: Option<i64>,
    len_lo: Option<i64>,
    len_hi: Option<i64>,
    regex: Option<String>,
}

fn is_it(e: &Expr) -> bool {
    matches!(e, Expr::Var(v) if v == "it")
}

/// Extract simple bounds from a refinement predicate to steer generation.
/// Anything not understood is still enforced by rejection sampling.
fn hints(e: &Expr, h: &mut Hints) {
    use BinOp::*;
    match e {
        Expr::Bin(And, l, r) => {
            hints(l, h);
            hints(r, h);
        }
        Expr::Bin(op, l, r) => {
            let (subject, n, op) = match (&**l, &**r) {
                (s, Expr::Int(n)) => (s, *n, *op),
                (Expr::Int(n), s) => (
                    s,
                    *n,
                    match op {
                        Lt => Gt,
                        Le => Ge,
                        Gt => Lt,
                        Ge => Le,
                        o => *o,
                    },
                ),
                _ => return,
            };
            let is_len = |s: &Expr| match s {
                Expr::Call(f, a) => matches!(&**f, Expr::Var(n) if n == "len") && a.len() == 1 && is_it(&a[0]),
                Expr::Method(r, m, a) => m == "len" && a.is_empty() && is_it(r),
                _ => false,
            };
            let (lo, hi) = if is_it(subject) {
                (&mut h.lo, &mut h.hi)
            } else if is_len(subject) {
                (&mut h.len_lo, &mut h.len_hi)
            } else {
                return;
            };
            match op {
                Ge => *lo = Some(lo.map_or(n, |x: i64| x.max(n))),
                Gt => *lo = Some(lo.map_or(n + 1, |x: i64| x.max(n + 1))),
                Le => *hi = Some(hi.map_or(n, |x: i64| x.min(n))),
                Lt => *hi = Some(hi.map_or(n - 1, |x: i64| x.min(n - 1))),
                Eq => {
                    *lo = Some(n);
                    *hi = Some(n)
                }
                _ => {}
            }
        }
        Expr::Call(f, a) if matches!(&**f, Expr::Var(n) if n == "matches") && a.len() == 2 && is_it(&a[0]) => {
            if let Expr::Str(re) = &a[1] {
                h.regex = Some(re.clone());
            }
        }
        Expr::Method(r, m, a) if m == "matches" && is_it(r) && a.len() == 1 => {
            if let Expr::Str(re) = &a[0] {
                h.regex = Some(re.clone());
            }
        }
        _ => {}
    }
}

impl Gen {
    pub fn new(seed: u64) -> Self {
        Gen { rng: StdRng::seed_from_u64(seed) }
    }

    pub fn value(&mut self, env: &Env, t: &TypeExpr, it: &mut Interp) -> Result<Value, String> {
        self.produce(env, t, it, 0, &Hints::default())
    }

    fn int(&mut self, h: &Hints) -> i64 {
        let lo = h.lo.unwrap_or_else(|| h.hi.map_or(-1000, |x| x - 1000));
        // with only a lower bound (`Natural`), stay near it: edge cases live there
        let hi = h.hi.unwrap_or(if h.lo.is_some() { lo + 100 } else { lo.max(-1000) + 2000 }).max(lo);
        // Boundary values find most bugs: pick them often.
        match self.rng.random_range(0..10) {
            0 => lo,
            1 => hi,
            2 if lo <= 0 && 0 <= hi => 0,
            _ => self.rng.random_range(lo..=hi),
        }
    }

    fn string(&mut self, h: &Hints) -> Result<String, String> {
        if let Some(re) = &h.regex {
            let mut parser = regex_syntax::ParserBuilder::new().unicode(false).build();
            let hir = parser.parse(re).map_err(|e| format!("bad regex {re:?}: {e}"))?;
            let g = rand_regex::Regex::with_hir(hir, 8).map_err(|e| e.to_string())?;
            return Ok(self.rng.sample::<String, _>(&g));
        }
        let lo = h.len_lo.unwrap_or(0).max(0) as usize;
        let hi = h.len_hi.map_or(lo + 8, |x| x.max(lo as i64) as usize);
        let n = self.rng.random_range(lo..=hi);
        const CH: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789 _-";
        Ok((0..n).map(|_| CH[self.rng.random_range(0..CH.len())] as char).collect())
    }

    fn produce(&mut self, env: &Env, t: &TypeExpr, it: &mut Interp, depth: usize, h: &Hints) -> Result<Value, String> {
        // a hidden type argument (`$Type[Money]`): the type's name
        if let TypeExpr::Named(n, args) = t {
            if n == crate::types::TYPE_ARG {
                return Ok(Value::Str(args.first().map(crate::types::texpr_head).unwrap_or_default()));
            }
        }
        if let TypeExpr::Refined(b, pred) = t {
            let mut h2 = h.clone();
            hints(pred, &mut h2);
            for _ in 0..300 {
                let v = self.produce(env, b, it, depth, &h2)?;
                if let Ok(None) = it.check_type(t, &v) {
                    return Ok(v);
                }
            }
            return Err(format!("could not generate a value satisfying `{}` after 300 attempts", crate::printer::ty(t)));
        }
        if let TypeExpr::Union(ms) = t {
            let m = &ms[self.rng.random_range(0..ms.len())];
            return self.produce(env, m, it, depth, h);
        }
        let TypeExpr::Named(n, args) = t else {
            return match t {
                TypeExpr::Unit => Ok(Value::Unit),
                _ => Err("cannot generate functions".into()),
            };
        };
        match n.as_str() {
            "Int" => return Ok(Value::Int(self.int(h))),
            "Float" => {
                let whole = self.int(h) as f64;
                let frac: f64 = self.rng.random_range(0.0..1.0);
                // stay inside integer bounds when there are any
                let f = if h.hi.is_some_and(|hi| whole + frac > hi as f64) { whole } else { whole + frac };
                return Ok(Value::Float(f));
            }
            "Tuple" => {
                return Ok(Value::Tuple(args.iter().map(|t| self.produce(env, t, it, depth + 1, &Hints::default())).collect::<Result<_, _>>()?));
            }
            "Set" | "Dict" => {
                let len = if depth > 3 { 0 } else { self.rng.random_range(0..=4) };
                let mut items = Vec::new();
                for _ in 0..len {
                    let k = self.produce(env, &args[0], it, depth + 1, &Hints::default())?;
                    if n == "Set" {
                        items.push((k, Value::Unit));
                    } else {
                        items.push((k, self.produce(env, &args[1], it, depth + 1, &Hints::default())?));
                    }
                }
                return Ok(if n == "Set" {
                    crate::interp::make_set(items.into_iter().map(|(k, _)| k).collect())
                } else {
                    crate::interp::make_dict(items)
                });
            }
            "Bool" => return Ok(Value::Bool(self.rng.random_bool(0.5))),
            "String" => return Ok(Value::Str(self.string(h)?)),
            "Unit" => return Ok(Value::Unit),
            "List" => {
                let lo = h.len_lo.unwrap_or(0).max(0) as usize;
                let hi = h.len_hi.map_or(lo + 4, |x| x.max(lo as i64) as usize);
                let max = if depth > 3 { lo } else { hi };
                let len = self.rng.random_range(lo..=max.max(lo));
                let el = args.first().cloned().unwrap_or(TypeExpr::Unit);
                return Ok(Value::List((0..len).map(|_| self.produce(env, &el, it, depth + 1, &Hints::default())).collect::<Result<_, _>>()?));
            }
            _ => {}
        }
        // a trait as a type: generate a mock implementation
        if env.traits.contains_key(n) {
            return Ok(Value::Mock(t.clone()));
        }
        // a variant's type: generate its sum, keeping only that variant
        let (n, only) = match env.variant_parent(n) {
            Some(p) => (&p.clone(), Some(n.clone())),
            None => (n, None),
        };
        let Some(td) = env.types.get(n).cloned() else {
            return Err(format!("cannot generate a value of unknown type `{n}` (generic parameters are not supported)"));
        };
        let mut h2 = Hints::default();
        if let Some(r) = &td.refinement {
            hints(r, &mut h2);
        }
        for _attempt in 0..300 {
            let v = match &td.body {
                TypeBody::Alias(a) if td.distinct => {
                    let sub: HashMap<String, TypeExpr> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
                    Value::Distinct(n.clone(), Box::new(self.produce(env, &subst_texpr(a, &sub), it, depth, &h2)?))
                }
                TypeBody::Alias(a) => {
                    let sub: HashMap<String, TypeExpr> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
                    self.produce(env, &subst_texpr(a, &sub), it, depth, &h2)?
                }
                TypeBody::Record(_) | TypeBody::Sum(_) => {
                    let mut variants = env.variants_of(n, args).unwrap();
                    if let Some(o) = &only {
                        variants.retain(|v| &v.0 == o);
                    }
                    // Past a certain depth prefer the variant with the fewest fields so recursion ends.
                    let idx = if depth > 3 {
                        (0..variants.len()).min_by_key(|i| variants[*i].2.len()).unwrap()
                    } else {
                        self.rng.random_range(0..variants.len())
                    };
                    let (c, names, ftys) = &variants[idx];
                    let vals = ftys.iter().map(|ft| self.produce(env, ft, it, depth + 1, &Hints::default())).collect::<Result<Vec<_>, _>>()?;
                    match (names, matches!(td.body, TypeBody::Record(_))) {
                        (Some(ns), true) => Value::Record(c.clone(), ns.iter().cloned().zip(vals).collect()),
                        (Some(ns), false) => Value::Variant(c.clone(), Fields::Rec(ns.iter().cloned().zip(vals).collect())),
                        (None, _) if vals.is_empty() => Value::Variant(c.clone(), Fields::None),
                        (None, _) => Value::Variant(c.clone(), Fields::Pos(vals)),
                    }
                }
            };
            if let Ok(None) = it.check_type(t, &v) {
                return Ok(v);
            }
        }
        Err(format!("could not generate a value satisfying `{}` after 300 attempts", crate::printer::ty(t)))
    }
}
