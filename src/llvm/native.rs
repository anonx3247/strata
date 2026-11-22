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
