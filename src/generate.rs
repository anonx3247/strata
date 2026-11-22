//! Value generation from types. Every type (including refined ones) gets a
//! generator for free; it powers `mock` handlers and `verify` fuzzing.

use crate::ast::*;
use crate::interp::{Fields, Interp, Value};
use crate::types::ints::{self, Ints};
use crate::types::{subst_texpr, Env};
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use crate::fx::HashMap;

pub struct Gen {
    rng: StdRng,
}

/// What a type's refinements say about the values to generate: the
/// integers they allow, the lengths, and a regex the value must match.
/// Anything not understood is still enforced by rejection sampling.
#[derive(Clone, Debug)]
struct Hints {
    ints: Ints,
    lens: Ints,
    regex: Option<String>,
}

impl Default for Hints {
    fn default() -> Self {
        Hints { ints: vec![(ints::MIN, ints::MAX)], lens: vec![(0, ints::MAX)], regex: None }
    }
}

impl Hints {
    /// These hints, narrowed by a refinement predicate.
    fn and(&self, pred: &Expr) -> Hints {
        Hints { ints: ints::and(&self.ints, &ints::hull(pred)), lens: ints::and(&self.lens, &ints::len_hull(pred)), regex: regex_of(pred).or_else(|| self.regex.clone()) }
    }
}

/// The regex of a `matches(it, "...")` among a predicate's `&&`s.
fn regex_of(e: &Expr) -> Option<String> {
    let is_it = |e: &Expr| matches!(e, Expr::Var(v) if v == "it");
    match e {
        Expr::Bin(BinOp::And, l, r) => regex_of(l).or_else(|| regex_of(r)),
        Expr::Call(f, a) if matches!(&**f, Expr::Var(n) if n == "matches") && a.len() == 2 && is_it(&a[0]) => match &a[1] {
            Expr::Str(re) => Some(re.clone()),
            _ => None,
        },
        Expr::Method(r, m, a) if m == "matches" && is_it(r) && a.len() == 1 => match &a[0] {
            Expr::Str(re) => Some(re.clone()),
            _ => None,
        },
        _ => None,
    }
}

impl Gen {
    pub fn new(seed: u64) -> Self {
        Gen { rng: StdRng::seed_from_u64(seed) }
    }

    pub fn value(&mut self, env: &Env, t: &TypeExpr, it: &mut Interp) -> Result<Value, String> {
        self.produce(env, t, it, 0, &Hints::default())
    }

    /// One of the intervals of `set`, open ends brought within `below` /
    /// `above` of the finite one (or around 0): `(lo, hi)`.
    fn interval(&mut self, set: &Ints, below: i128, above: i128) -> (i64, i64) {
        let Some(&(a, b)) = (!set.is_empty()).then(|| &set[self.rng.random_range(0..set.len())]) else {
            return (0, 0); // nothing allowed: the check rejects whatever comes
        };
        let (lo, hi) = match (a == ints::MIN, b == ints::MAX) {
            (true, true) => (-below, below),
            (true, false) => (b - below, b),
            (false, true) => (a, a + above),
            (false, false) => (a, b),
        };
        (lo.max(ints::MIN) as i64, hi.min(ints::MAX).max(lo) as i64)
    }

    fn int(&mut self, h: &Hints) -> i64 {
        // with only a lower bound (`Natural`), stay near it: edge cases live there
        let (lo, hi) = self.interval(&h.ints, 1000, 100);
        // Boundary values find most bugs: pick them often.
        match self.rng.random_range(0..10) {
            0 => lo,
            1 => hi,
            2 if lo <= 0 && 0 <= hi => 0,
            _ => self.rng.random_range(lo..=hi),
        }
    }

    /// A length the hints allow, at most `spread` past an open lower bound.
    fn len(&mut self, h: &Hints, spread: i128) -> usize {
        let (lo, hi) = self.interval(&h.lens, 0, spread);
        self.rng.random_range(lo.max(0)..=hi.max(0)) as usize
    }

    fn string(&mut self, h: &Hints) -> Result<String, String> {
        if let Some(re) = &h.regex {
            let mut parser = regex_syntax::ParserBuilder::new().unicode(false).build();
            let hir = parser.parse(re).map_err(|e| format!("bad regex {re:?}: {e}"))?;
            let g = rand_regex::Regex::with_hir(hir, 8).map_err(|e| e.to_string())?;
            return Ok(self.rng.sample::<String, _>(&g));
        }
        let n = self.len(h, 8);
        const CH: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789 _-";
        Ok((0..n).map(|_| CH[self.rng.random_range(0..CH.len())] as char).collect())
    }

    fn produce(&mut self, env: &Env, t: &TypeExpr, it: &mut Interp, depth: usize, h: &Hints) -> Result<Value, String> {
        // a hidden type argument (`$Type[Money]`): the type's name
        if let TypeExpr::Named(n, args) = t {
            if n == crate::types::TYPE_ARG {
                // (whole: `List[Int]`, so an impl's type parameters are known)
                return Ok(Value::Str(args.first().map(crate::printer::type_text).unwrap_or_default()));
            }
        }
        if let TypeExpr::Refined(b, pred) = t {
            let h2 = h.and(pred);
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
                let whole = self.int(h);
                let frac: f64 = self.rng.random_range(0.0..1.0);
                // stay inside integer bounds when there are any
                let f = if ints::contains(&h.ints, whole as i128 + 1, whole as i128 + 1) { whole as f64 + frac } else { whole as f64 };
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
                // deep down, the shortest list allowed, so recursion ends
                let len = if depth > 3 { h.lens.first().map_or(0, |&(lo, _)| lo.max(0) as usize) } else { self.len(h, 4) };
                let el = args.first().cloned().unwrap_or(TypeExpr::Unit);
                return Ok(Value::List((0..len).map(|_| self.produce(env, &el, it, depth + 1, &Hints::default())).collect::<Result<_, _>>()?));
            }
            _ => {}
        }
        // a trait as a type: generate a mock implementation
        if env.traits.contains_key(&n.sym()) {
            return Ok(Value::Mock(t.clone()));
        }
        // a variant's type: generate its sum, keeping only that variant
        let (n, only) = match env.variant_parent(n.sym()) {
            Some(p) => (&p.clone(), Some(n.clone())),
            None => (n, None),
        };
        let Some(td) = env.types.get(n).cloned() else {
            return Err(format!("cannot generate a value of unknown type `{n}` (generic parameters are not supported)"));
        };
        let h2 = td.refinement.as_ref().map_or_else(Hints::default, |r| Hints::default().and(r));
        for _attempt in 0..300 {
            let v = match &td.body {
                TypeBody::Alias(a) if td.distinct => {
                    let sub: HashMap<Symbol, TypeExpr> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
                    Value::Distinct(n.to_string(), Box::new(self.produce(env, &subst_texpr(a, &sub), it, depth, &h2)?))
                }
                TypeBody::Alias(a) => {
                    let sub: HashMap<Symbol, TypeExpr> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
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
                        (Some(ns), true) => Value::Record(c.to_string(), ns.iter().map(|n| n.to_string()).zip(vals).collect()),
                        (Some(ns), false) => Value::Variant(c.to_string(), Fields::rec(ns.iter().map(|n| n.to_string()).zip(vals).collect())),
                        (None, _) if vals.is_empty() => Value::Variant(c.to_string(), Fields::None),
                        (None, _) => Value::Variant(c.to_string(), Fields::pos(vals)),
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
