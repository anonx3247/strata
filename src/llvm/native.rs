//! Reading a value compiled code made, from its memory: the layouts of
//! `runtime.c` and `llvm.rs` (a string `{len, bytes}`, a list `{len, buf,
//! off}` over a buffer `{cap, used, data}`, a record, variant or tuple a
//! block of 8-byte slots after a tag, a union's other members in a box
//! `{tag, value}`, a big integer a pointer kept outside the small range),
//! guided by the value's `Shape`, as a literal: the code that writes the
//! value (`P { x: 1 }`, `Some("a")`, `Set { 1, 2 }`). Compile-time code's
//! results come back so; the FFI will pass values the same way.

use super::{Shape, Shapes};
use crate::ast::{CtorArgs, Expr, Symbol};
use num_bigint::{BigInt, Sign};

type Res<X> = Result<X, String>;

/// The slots of a block (or a buffer) at `p`.
unsafe fn slot(p: i64, i: usize) -> i64 {
    unsafe { *(p as *const i64).add(i) }
}

const SMALL: i64 = 1 << 62;

/// An integer as a literal: `Int` when it fits in 64 bits, else its digits
/// (so equal numbers are equal literals).
pub fn int_expr(n: BigInt) -> Expr {
    match i64::try_from(&n) {
        Ok(k) => Expr::Int(k),
        Err(_) => Expr::BigInt(n.to_string()),
    }
}

/// An `Int`'s value: small, or a big integer's digits.
unsafe fn int(raw: i64) -> Expr {
    if (-SMALL..SMALL).contains(&raw) {
        return Expr::Int(raw);
    }
    // the inverse of runtime.c's `int_of`: the block's address, over 8
    let k = if raw > 0 { (raw as u64).wrapping_sub(SMALL as u64) } else { ((-(raw + 1)) as u64).wrapping_sub(SMALL as u64) };
    let b = (k << 3) as i64;
    let (sign, n) = unsafe { (slot(b, 0), slot(b, 1)) };
    let digits = unsafe { std::slice::from_raw_parts((b as *const i64).add(2) as *const u32, n as usize) };
    let s = match sign {
        s if s < 0 => Sign::Minus,
        0 => Sign::NoSign,
        _ => Sign::Plus,
    };
    int_expr(BigInt::from_slice(s, digits))
}

unsafe fn string(p: i64) -> String {
    unsafe {
        let len = slot(p, 0) as usize;
        let bytes = std::slice::from_raw_parts((p as *const u8).add(8), len);
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// A list's elements, as raw slots.
unsafe fn elements(p: i64) -> Vec<i64> {
    unsafe {
        let (len, buf, off) = (slot(p, 0), slot(p, 1), slot(p, 2));
        (0..len).map(|i| slot(buf, (2 + off + i) as usize)).collect()
    }
}

/// The value of shape `s` whose slot is `raw`.
///
/// # Safety
/// `raw` must be a live value of that shape, made by code compiled with the
/// `Shapes` given.
pub unsafe fn read(s: &Shape, shapes: &Shapes, raw: i64) -> Res<Expr> {
    // a capability is a null pointer (the runtime's): shown by its type, `Terminal`
    let pointer = matches!(s, Shape::Str | Shape::List(_) | Shape::Set(_) | Shape::Dict(..) | Shape::Tuple(_) | Shape::Named(_) | Shape::Union(_));
    if pointer && raw == 0 {
        return match s {
            Shape::Named(k) => Ok(Expr::Ctor(Symbol::new(k), CtorArgs::None)),
            _ => Err("a null pointer".into()),
        };
    }
    unsafe {
        Ok(match s {
            Shape::Int => int(raw),
            Shape::Float => Expr::Float(f64::from_bits(raw as u64)),
            Shape::Bool => Expr::Bool(raw & 1 != 0),
            Shape::Unit => Expr::Unit,
            Shape::Str => Expr::Str(string(raw)),
            Shape::List(e) => Expr::List(elements(raw).into_iter().map(|x| read(e, shapes, x)).collect::<Res<_>>()?),
            Shape::Set(e) => Expr::Set(vec![], elements(raw).into_iter().map(|x| read(e, shapes, x)).collect::<Res<_>>()?),
            Shape::Dict(k, v) => Expr::Dict(
                vec![],
                elements(raw)
                    .into_iter()
                    .map(|pair| Ok((read(k, shapes, slot(pair, 1))?, read(v, shapes, slot(pair, 2))?)))
                    .collect::<Res<_>>()?,
            ),
            Shape::Tuple(ts) => Expr::Tuple(ts.iter().enumerate().map(|(i, t)| read(t, shapes, slot(raw, i + 1))).collect::<Res<_>>()?),
            Shape::Distinct(n, inner) => Expr::Ctor(Symbol::new(n), CtorArgs::Positional(vec![read(inner, shapes, raw)?])),
            Shape::Named(k) => {
                let tag = slot(raw, 0);
                let cs = shapes.named.get(k).ok_or(format!("no shape for `{k}`"))?;
                let c = cs.iter().find(|c| c.tag == tag).ok_or(format!("a `{k}` with tag {tag}"))?;
                let mut vals = vec![];
                for (i, (_, f)) in c.fields.iter().enumerate() {
                    vals.push(read(f, shapes, slot(raw, i + 1))?);
                }
                let name = Symbol::new(&c.name);
                if c.fields.is_empty() && !c.record {
                    Expr::Ctor(name, CtorArgs::None)
                } else if c.record || c.fields.iter().all(|(n, _)| n.is_some()) {
                    // in declaration order
                    Expr::Ctor(name, CtorArgs::Record(c.fields.iter().zip(vals).map(|((n, _), v)| (Symbol::new(n.as_deref().unwrap_or("")), v)).collect()))
                } else {
                    Expr::Ctor(name, CtorArgs::Positional(vals))
                }
            }
            Shape::Union(ms) => {
                let tag = slot(raw, 0);
                // a boxed member: its box's tag, the value in the slot after it
                if let Some((_, m)) = ms.iter().find(|(t, _)| *t == Some(tag)) {
                    return read(m, shapes, slot(raw, 1));
                }
                // a record's, variant's or tuple's block, as it is
                let fits = |m: &Shape| match m {
                    Shape::Named(k) => shapes.named.get(k).is_some_and(|cs| cs.iter().any(|c| c.tag == tag)),
                    Shape::Tuple(_) => tag == 0,
                    _ => false,
                };
                let (_, m) = ms.iter().find(|(t, m)| t.is_none() && fits(m)).ok_or(format!("a union value with tag {tag}"))?;
                read(m, shapes, raw)?
            }
        })
    }
}

// ---- values the host makes, for compiled code ----
//
// What strata hands to compiled code (`Codebase`'s snapshot; a server's
// requests) is a literal: an `Expr` of data, as `read` gives back, laid out
// in memory by the shape of its type (`write`). Refinements are not checked
// here: the compiled code it is handed to checks them (`Entry::Call`).

unsafe extern "C" {
    pub(super) fn strata_alloc(n: i64) -> *mut i64;
}

/// A new string in compiled code's memory, held once.
pub fn new_string(s: &str) -> i64 {
    unsafe {
        let p = strata_alloc(8 + s.len() as i64);
        *p = s.len() as i64;
        std::ptr::copy_nonoverlapping(s.as_ptr(), p.add(1) as *mut u8, s.len());
        p as i64
    }
}

/// A block of slots after a tag (a record, variant, tuple or union box), held once.
fn new_block(tag: i64, slots: &[i64]) -> i64 {
    unsafe {
        let p = strata_alloc(8 * (slots.len() as i64 + 1));
        *p = tag;
        for (i, s) in slots.iter().enumerate() {
            *p.add(i + 1) = *s;
        }
        p as i64
    }
}

/// A list of the slots, held once (each element is handed over).
fn new_list(slots: &[i64]) -> i64 {
    unsafe {
        let cap = slots.len().max(4);
        let buf = strata_alloc(8 * (cap as i64 + 2));
        *buf = cap as i64;
        *buf.add(1) = slots.len() as i64;
        for (i, s) in slots.iter().enumerate() {
            *buf.add(i + 2) = *s;
        }
        let l = strata_alloc(24);
        *l = slots.len() as i64;
        *l.add(1) = buf as i64;
        *l.add(2) = 0;
        l as i64
    }
}

/// An `Int`'s slot: the number itself when small, else a big integer's
/// object (the inverse of `int`, as runtime.c's `int_of` makes them).
fn new_int(n: &BigInt) -> i64 {
    if let Ok(small) = i64::try_from(n) {
        if (-SMALL..SMALL).contains(&small) {
            return small;
        }
    }
    let (sign, digits) = n.to_u32_digits();
    unsafe {
        let b = strata_alloc(16 + 4 * digits.len() as i64);
        *b = match sign {
            Sign::Minus => -1,
            Sign::NoSign => 0,
            Sign::Plus => 1,
        };
        *b.add(1) = digits.len() as i64;
        std::ptr::copy_nonoverlapping(digits.as_ptr(), b.add(2) as *mut u32, digits.len());
        let k = (b as u64) >> 3;
        let up = (1u64 << 62).wrapping_add(k) as i64;
        if sign == Sign::Minus { -up - 1 } else { up }
    }
}

/// A constructor's name without its module.
fn short(c: &str) -> &str {
    c.rsplit('.').next().unwrap_or(c)
}

/// The constructor of shape `k` named `name` (qualified or not).
fn ctor<'a>(shapes: &'a Shapes, k: &str, name: &str) -> Option<&'a super::CtorShape> {
    shapes.named.get(k)?.iter().find(|c| c.name == name || short(&c.name) == short(name))
}

/// Whether the literal `e` is a value of shape `s` (to find a union's member).
fn fits(s: &Shape, shapes: &Shapes, e: &Expr) -> bool {
    match (s, e) {
        (Shape::Union(ms), _) => ms.iter().any(|(_, m)| fits(m, shapes, e)),
        (Shape::Int, Expr::Int(_) | Expr::BigInt(_)) | (Shape::Float, Expr::Float(_)) | (Shape::Bool, Expr::Bool(_)) | (Shape::Unit, Expr::Unit) | (Shape::Str, Expr::Str(_)) => true,
        (Shape::List(t), Expr::List(xs)) | (Shape::Set(t), Expr::Set(_, xs)) => xs.iter().all(|x| fits(t, shapes, x)),
        (Shape::Dict(k, v), Expr::Dict(_, kvs)) => kvs.iter().all(|(a, b)| fits(k, shapes, a) && fits(v, shapes, b)),
        (Shape::Tuple(ts), Expr::Tuple(xs)) => ts.len() == xs.len() && ts.iter().zip(xs).all(|(t, x)| fits(t, shapes, x)),
        (Shape::Distinct(n, _), Expr::Ctor(c, _)) => short(n) == short(c.as_str()),
        (Shape::Named(k), Expr::Ctor(c, _)) => ctor(shapes, k, c.as_str()).is_some(),
        _ => false,
    }
}

/// Compiled code's order on the elements of a set or the keys of a dict
/// (llvm.rs's `cmp` helper): numbers by value, strings by their bytes,
/// tuples and lists element by element. `None` for a value it orders by its
/// JSON, which the host doesn't write.
fn order(a: &Expr, b: &Expr) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    let big = |e: &Expr| match e {
        Expr::Int(n) => Some(BigInt::from(*n)),
        Expr::BigInt(d) => d.parse().ok(),
        _ => None,
    };
    Some(match (a, b) {
        (Expr::Int(_) | Expr::BigInt(_), _) => big(a)?.cmp(&big(b)?),
        (Expr::Float(x), Expr::Float(y)) => x.total_cmp(y),
        (Expr::Bool(x), Expr::Bool(y)) => x.cmp(y),
        (Expr::Str(x), Expr::Str(y)) => x.as_bytes().cmp(y.as_bytes()),
        (Expr::Unit, Expr::Unit) => Ordering::Equal,
        (Expr::Tuple(xs), Expr::Tuple(ys)) | (Expr::List(xs), Expr::List(ys)) => {
            for (x, y) in xs.iter().zip(ys) {
                match order(x, y)? {
                    Ordering::Equal => {}
                    o => return Some(o),
                }
            }
            xs.len().cmp(&ys.len())
        }
        _ => return None,
    })
}

/// `xs` sorted in compiled code's order, without repeats (by the first key).
fn sorted<'a, T>(mut xs: Vec<T>, key: impl Fn(&T) -> &'a Expr) -> Res<Vec<T>> {
    let mut unordered = None;
    xs.sort_by(|a, b| {
        order(key(a), key(b)).unwrap_or_else(|| {
            unordered = Some(crate::printer::literal(key(a)));
            std::cmp::Ordering::Equal
        })
    });
    if let Some(v) = unordered {
        return Err(format!("the host can't order {v} in a set or as a key (compiled code orders it by its JSON)"));
    }
    xs.dedup_by(|a, b| order(key(a), key(b)) == Some(std::cmp::Ordering::Equal));
    Ok(xs)
}

/// Lay the literal `e`, a value of shape `s`, out in compiled code's
/// memory, as `read` reads it: the slot that holds it, its objects held once.
pub fn write(s: &Shape, shapes: &Shapes, e: &Expr) -> Res<i64> {
    let wrong = || Err(format!("{} is not a value of shape {s:?}", crate::printer::literal(e)));
    let all = |t: &Shape, xs: &[&Expr]| xs.iter().map(|x| write(t, shapes, x)).collect::<Res<Vec<_>>>();
    Ok(match (s, e) {
        (Shape::Int, Expr::Int(n)) => new_int(&BigInt::from(*n)),
        (Shape::Int, Expr::BigInt(d)) => new_int(&d.parse().map_err(|_| format!("`{d}` is not an integer"))?),
        (Shape::Float, Expr::Float(f)) => f.to_bits() as i64,
        (Shape::Bool, Expr::Bool(b)) => *b as i64,
        (Shape::Unit, Expr::Unit) => 0,
        (Shape::Str, Expr::Str(t)) => new_string(t),
        (Shape::List(t), Expr::List(xs)) => new_list(&all(t, &xs.iter().collect::<Vec<_>>())?),
        (Shape::Set(t), Expr::Set(_, xs)) => new_list(&all(t, &sorted(xs.iter().collect(), |x| *x)?)?),
        // a sorted list of (key, value) pairs
        (Shape::Dict(k, v), Expr::Dict(_, kvs)) => {
            let kvs = sorted(kvs.iter().collect(), |kv| &kv.0)?;
            let pairs = kvs.iter().map(|(a, b)| Ok(new_block(0, &[write(k, shapes, a)?, write(v, shapes, b)?]))).collect::<Res<Vec<_>>>()?;
            new_list(&pairs)
        }
        (Shape::Tuple(ts), Expr::Tuple(xs)) if ts.len() == xs.len() => new_block(0, &ts.iter().zip(xs).map(|(t, x)| write(t, shapes, x)).collect::<Res<Vec<_>>>()?),
        (Shape::Distinct(_, inner), Expr::Ctor(_, CtorArgs::Positional(xs))) if xs.len() == 1 => write(inner, shapes, &xs[0])?,
        // the member it is a value of: in a box with that member's tag, or as it is
        (Shape::Union(ms), _) => match ms.iter().find(|(_, m)| fits(m, shapes, e)) {
            Some((Some(tag), m)) => new_block(*tag, &[write(m, shapes, e)?]),
            Some((None, m)) => write(m, shapes, e)?,
            None => return wrong(),
        },
        (Shape::Named(k), Expr::Ctor(name, args)) => {
            let Some(c) = ctor(shapes, k, name.as_str()) else { return wrong() };
            let given: Vec<(Option<&str>, &Expr)> = match args {
                CtorArgs::None => vec![],
                CtorArgs::Record(fs) => fs.iter().map(|(n, x)| (Some(n.as_str()), x)).collect(),
                CtorArgs::Positional(xs) => xs.iter().map(|x| (None, x)).collect(),
                CtorArgs::Update(..) => return wrong(),
            };
            if given.len() != c.fields.len() {
                return Err(format!("`{}` has {} fields, given {}", short(&c.name), c.fields.len(), given.len()));
            }
            // fields by name when they have one, else in order
            let mut slots = vec![];
            for (i, (n, f)) in c.fields.iter().enumerate() {
                let x = match (n, given[i].0) {
                    (Some(n), Some(_)) => given.iter().find(|(g, _)| *g == Some(n.as_str())).map(|(_, x)| *x).ok_or(format!("`{}` has no field `{n}` given", short(&c.name)))?,
                    _ => given[i].1,
                };
                slots.push(write(f, shapes, x)?);
            }
            new_block(c.tag, &slots)
        }
        _ => return wrong(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A literal → compiled memory → a literal gives it back (sets and
    /// dicts sorted, as compiled code keeps them), for every kind of shape.
    #[test]
    fn values_cross_both_ways() {
        let src = r#"
distinct type Id = Int
type Figure =
  | Circle { r: Float }
  | Square(Float)
  | Pair(Int, String)
  | Dot
type Everything = {
  id: Id,
  tags: Set[String],
  scores: Dict[String, Int],
  keyed: Dict[Int, Bool],
  pos: (Int, Bool),
  either: Int | String,
  many: List[Int | Figure | List[String]],
  big: Int,
  small: Int,
  nothing: (),
  maybe: Option[Figure],
  figures: List[Figure],
}
"#;
        let prog = crate::parser::parse_program(src).unwrap();
        let t = crate::ast::TypeExpr::Named("Everything".into(), vec![]);
        let (roots, shapes) = crate::llvm::shapes_of(&prog, &[t]).unwrap();
        let lit = |s: &str| crate::parser::parse_expr(s).unwrap();
        let given = lit(r#"Everything { id: Id(7), tags: Set { "b", "a", "b" }, scores: Dict { "y": -2, "x": 1 }, keyed: Dict { 3: true, 1: false },
            pos: (4, true), either: "text", many: [1, Circle { r: 1.5 }, ["s"], Dot],
            big: 123456789012345678901234567890, small: -4611686018427387904, nothing: (),
            maybe: Some(Pair(2, "two")), figures: [Square(2.0), Dot] }"#);
        let raw = write(&roots[0], &shapes, &given).unwrap();
        let back = unsafe { read(&roots[0], &shapes, raw) }.unwrap();
        assert_eq!(
            crate::printer::literal(&back),
            crate::printer::literal(&lit(r#"Everything { id: Id(7), tags: Set { "a", "b" }, scores: Dict { "x": 1, "y": -2 }, keyed: Dict { 1: false, 3: true },
            pos: (4, true), either: "text", many: [1, Circle { r: 1.5 }, ["s"], Dot],
            big: 123456789012345678901234567890, small: -4611686018427387904, nothing: (),
            maybe: Some(Pair(2, "two")), figures: [Square(2.0), Dot] }"#))
        );
    }

    /// What doesn't fit is an error that says what.
    #[test]
    fn writing_says_what_does_not_fit() {
        let prog = crate::parser::parse_program("type P = { name: String, at: List[Int] }\n").unwrap();
        let (roots, shapes) = crate::llvm::shapes_of(&prog, &[crate::ast::TypeExpr::Named("P".into(), vec![])]).unwrap();
        let err = |s: &str| write(&roots[0], &shapes, &crate::parser::parse_expr(s).unwrap()).unwrap_err();
        assert!(err(r#"P { name: "a", at: [1, "x"] }"#).starts_with(r#""x" is not a value of shape Int"#), "{}", err(r#"P { name: "a", at: [1, "x"] }"#));
        assert_eq!(err(r#"P { name: "a" }"#), "`P` has 2 fields, given 1");
        assert_eq!(err(r#"P { name: "a", z: [] }"#), "`P` has no field `at` given");
    }
}
