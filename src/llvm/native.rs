//! Reading a value compiled code made, from its memory: the layouts of
//! `runtime.c` and `llvm.rs` (a string `{len, bytes}`, a list `{len, buf,
//! off}` over a buffer `{cap, used, data}`, a record, variant or tuple a
//! block of 8-byte slots after a tag, a union's other members in a box
//! `{tag, value}`, a big integer a pointer kept outside the small range),
//! guided by the value's `Shape`. Compile-time code's results come back so;
//! the FFI will pass values the same way.

use super::{Shape, Shapes};
use crate::interp::{Fields, Value};
use num_bigint::{BigInt, Sign};

type Res<X> = Result<X, String>;

/// The slots of a block (or a buffer) at `p`.
unsafe fn slot(p: i64, i: usize) -> i64 {
    unsafe { *(p as *const i64).add(i) }
}

const SMALL: i64 = 1 << 62;

/// An `Int`'s value: small, or a big integer's digits.
unsafe fn int(raw: i64) -> Value {
    if (-SMALL..SMALL).contains(&raw) {
        return Value::Int(raw);
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
    crate::interp::int_value(BigInt::from_slice(s, digits))
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
pub unsafe fn read(s: &Shape, shapes: &Shapes, raw: i64) -> Res<Value> {
    unsafe {
        Ok(match s {
            Shape::Int => int(raw),
            Shape::Float => Value::Float(f64::from_bits(raw as u64)),
            Shape::Bool => Value::Bool(raw & 1 != 0),
            Shape::Unit => Value::Unit,
            Shape::Str => Value::Str(string(raw)),
            Shape::List(e) => Value::List(elements(raw).into_iter().map(|x| read(e, shapes, x)).collect::<Res<_>>()?),
            Shape::Set(e) => Value::Set(elements(raw).into_iter().map(|x| read(e, shapes, x)).collect::<Res<_>>()?),
            Shape::Dict(k, v) => Value::Dict(
                elements(raw)
                    .into_iter()
                    .map(|pair| Ok((read(k, shapes, slot(pair, 1))?, read(v, shapes, slot(pair, 2))?)))
                    .collect::<Res<_>>()?,
            ),
            Shape::Tuple(ts) => Value::Tuple(ts.iter().enumerate().map(|(i, t)| read(t, shapes, slot(raw, i + 1))).collect::<Res<_>>()?),
            Shape::Distinct(n, inner) => Value::Distinct(n.clone(), Box::new(read(inner, shapes, raw)?)),
            Shape::Named(k) => {
                let tag = slot(raw, 0);
                let cs = shapes.named.get(k).ok_or(format!("no shape for `{k}`"))?;
                let c = cs.iter().find(|c| c.tag == tag).ok_or(format!("a `{k}` with tag {tag}"))?;
                let mut vals = vec![];
                for (i, (_, f)) in c.fields.iter().enumerate() {
                    vals.push(read(f, shapes, slot(raw, i + 1))?);
                }
                let named: Vec<(String, Value)> = c.fields.iter().zip(vals.iter()).filter_map(|((n, _), v)| n.clone().map(|n| (n, v.clone()))).collect();
                if c.record {
                    Value::Record(c.name.clone(), named)
                } else if c.fields.is_empty() {
                    Value::Variant(c.name.clone(), Fields::None)
                } else if c.fields.iter().all(|(n, _)| n.is_some()) {
                    Value::Variant(c.name.clone(), Fields::rec(named))
                } else {
                    Value::Variant(c.name.clone(), Fields::pos(vals))
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
// What crosses into compiled code from outside (`Codebase`'s snapshot; an
// app's messages, a server's requests) comes as JSON, as `interp::to_json`
// writes values: read against the shape of the type it should be (`decode`),
// then laid out in memory (`write`). Values from outside are complete: a
// missing field is an error, not its default. Refinements are not checked
// here: the compiled code they are handed to checks them.

use serde_json::Value as Json;

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

/// The value of `e` when it is data: literals, lists and tuples of them,
/// constructors given them (a field's default, filled in for JSON that
/// leaves the field out). `None` when something would have to run.
pub fn constant(env: &crate::types::Env, e: &crate::ast::Expr) -> Option<Value> {
    use crate::ast::{CtorArgs, Expr, UnOp};
    Some(match e {
        Expr::Int(n) => Value::Int(*n),
        Expr::BigInt(b) => crate::interp::int_value(b.parse().ok()?),
        Expr::Float(x) => Value::Float(*x),
        Expr::Bool(b) => Value::Bool(*b),
        Expr::Str(t) => Value::Str(t.clone()),
        Expr::Unit => Value::Unit,
        Expr::Un(UnOp::Neg, x) => match constant(env, x)? {
            Value::Int(n) => crate::interp::int_value(-BigInt::from(n)),
            Value::Big(n) => crate::interp::int_value(-n),
            Value::Float(f) => Value::Float(-f),
            _ => return None,
        },
        Expr::List(xs) => Value::List(xs.iter().map(|x| constant(env, x)).collect::<Option<_>>()?),
        Expr::Tuple(xs) => Value::Tuple(xs.iter().map(|x| constant(env, x)).collect::<Option<_>>()?),
        Expr::Ctor(c, args) => {
            let tname = *env.ctors.get(c)?;
            let td = env.types.get(&tname)?;
            match args {
                CtorArgs::None => Value::Variant(c.to_string(), Fields::None),
                CtorArgs::Positional(es) => Value::Variant(c.to_string(), Fields::pos(es.iter().map(|x| constant(env, x)).collect::<Option<_>>()?)),
                CtorArgs::Record(fs) => {
                    let order = env.variants_of(&tname, &[]).and_then(|vs| vs.into_iter().find(|v| &v.0 == c)).and_then(|v| v.1)?;
                    let mut vals = vec![];
                    for f in &order {
                        let (_, fe) = fs.iter().find(|(n, _)| n == f)?;
                        vals.push((f.to_string(), constant(env, fe)?));
                    }
                    if matches!(td.body, crate::ast::TypeBody::Record(_)) { Value::Record(c.to_string(), vals) } else { Value::Variant(c.to_string(), Fields::rec(vals)) }
                }
                CtorArgs::Update(..) => return None,
            }
        }
        _ => return None,
    })
}

/// A constructor's name as JSON writes it: without its module.
fn short(c: &str) -> &str {
    c.rsplit('.').next().unwrap_or(c)
}

/// The value of shape `s` that the JSON `j` writes (as `interp::to_json`
/// does); `path` says where, for the error.
pub fn decode(s: &Shape, shapes: &Shapes, j: &Json, path: &str) -> Res<Value> {
    let bad = |what: &str| Err(format!("{path}: expected {what}, got {j}"));
    Ok(match s {
        Shape::Unit => match j {
            Json::Null => Value::Unit,
            _ => return bad("null"),
        },
        // a larger integer may come as a string of digits
        Shape::Int => match j {
            Json::Number(n) if n.is_i64() || n.is_u64() => crate::interp::int_value(n.to_string().parse().unwrap()),
            Json::String(t) => match t.parse::<BigInt>() {
                Ok(n) => crate::interp::int_value(n),
                Err(_) => return bad("an integer"),
            },
            _ => return bad("an integer"),
        },
        Shape::Float => match j.as_f64() {
            Some(f) => Value::Float(f),
            None => return bad("a number"),
        },
        Shape::Bool => match j.as_bool() {
            Some(b) => Value::Bool(b),
            None => return bad("a boolean"),
        },
        Shape::Str => match j.as_str() {
            Some(t) => Value::Str(t.to_string()),
            None => return bad("a string"),
        },
        Shape::List(e) | Shape::Set(e) => {
            let Some(xs) = j.as_array() else { return bad("an array") };
            let vs = xs.iter().enumerate().map(|(i, x)| decode(e, shapes, x, &format!("{path}[{i}]"))).collect::<Res<Vec<_>>>()?;
            if matches!(s, Shape::Set(_)) { crate::interp::make_set(vs) } else { Value::List(vs) }
        }
        Shape::Dict(k, v) => {
            let mut pairs = vec![];
            if let Some(o) = j.as_object() {
                for (key, x) in o {
                    pairs.push((decode(k, shapes, &Json::String(key.clone()), &format!("{path} key {key:?}"))?, decode(v, shapes, x, &format!("{path}[{key:?}]"))?));
                }
            } else if let Some(xs) = j.as_array() {
                for (i, x) in xs.iter().enumerate() {
                    let Some([kj, vj]) = x.as_array().and_then(|a| <&[Json; 2]>::try_from(a.as_slice()).ok()) else {
                        return Err(format!("{path}[{i}]: expected a [key, value] pair"));
                    };
                    pairs.push((decode(k, shapes, kj, &format!("{path}[{i}].key"))?, decode(v, shapes, vj, &format!("{path}[{i}].value"))?));
                }
            } else {
                return bad("an object or an array of [key, value] pairs");
            }
            crate::interp::make_dict(pairs)
        }
        Shape::Tuple(ts) => match j.as_array().filter(|a| a.len() == ts.len()) {
            Some(xs) => Value::Tuple(ts.iter().zip(xs).enumerate().map(|(i, (t, x))| decode(t, shapes, x, &format!("{path}.{i}"))).collect::<Res<_>>()?),
            None => return bad(&format!("an array of {} values", ts.len())),
        },
        Shape::Distinct(n, inner) => Value::Distinct(n.clone(), Box::new(decode(inner, shapes, j, path)?)),
        // the first member the JSON is a value of
        Shape::Union(ms) => match ms.iter().find_map(|(_, m)| decode(m, shapes, j, path).ok()) {
            Some(v) => v,
            None => return bad("a value of one of the union's types"),
        },
        Shape::Named(k) => {
            let cs = shapes.named.get(k).ok_or(format!("no shape for `{k}`"))?;
            let record = cs.len() == 1 && cs[0].record;
            // a record is its fields; a variant `"C"`, or `{"C": payload}`
            let (name, payload) = if record {
                (cs[0].name.as_str(), j)
            } else if let Some(t) = j.as_str() {
                (t, &Json::Null)
            } else if let Some((c, x)) = j.as_object().filter(|o| o.len() == 1).and_then(|o| o.iter().next()) {
                (c.as_str(), x)
            } else {
                return bad(&format!("one of {}", cs.iter().map(|c| short(&c.name)).collect::<Vec<_>>().join(" | ")));
            };
            let Some(c) = cs.iter().find(|c| c.name == name || short(&c.name) == name) else {
                // a variant as a type (`e: TooBig`) has one constructor
                if let [only] = cs.as_slice() {
                    return bad(&format!("a `{}`", short(&only.name)));
                }
                return Err(format!("{path}: `{name}` is not a constructor of `{}`", short(k)));
            };
            let p = if record { path.to_string() } else { format!("{path}.{}", short(&c.name)) };
            let named = !c.fields.is_empty() && c.fields.iter().all(|(n, _)| n.is_some());
            if named {
                let Some(obj) = payload.as_object() else { return Err(format!("{p}: expected an object")) };
                let mut fs = vec![];
                for (i, (n, f)) in c.fields.iter().enumerate() {
                    let n = n.clone().unwrap_or_default();
                    let v = match (obj.get(&n), c.defaults.get(i).cloned().flatten()) {
                        (Some(x), _) => decode(f, shapes, x, &format!("{p}.{n}"))?,
                        // left out: its default (checked with the rest, by the compiled code)
                        (None, Some(Ok(d))) => decode(f, shapes, &d, &format!("{p}.{n}"))?,
                        (None, Some(Err(why))) => return Err(format!("{p}: missing field `{n}`: {why}, so give it")),
                        (None, None) => return Err(format!("{p}: missing field `{n}`")),
                    };
                    fs.push((n.clone(), v));
                }
                if let Some(extra) = obj.keys().find(|key| !c.fields.iter().any(|(n, _)| n.as_deref() == Some(key.as_str()))) {
                    return Err(format!("{p}: unknown field `{extra}`"));
                }
                if c.record { Value::Record(c.name.clone(), fs) } else { Value::Variant(c.name.clone(), Fields::rec(fs)) }
            } else if c.fields.is_empty() {
                Value::Variant(c.name.clone(), Fields::None)
            } else {
                let items: Vec<Json> = if c.fields.len() == 1 { vec![payload.clone()] } else { payload.as_array().cloned().unwrap_or_default() };
                if items.len() != c.fields.len() {
                    return Err(format!("{p}: expected {} values", c.fields.len()));
                }
                Value::Variant(c.name.clone(), Fields::pos(c.fields.iter().zip(&items).enumerate().map(|(i, ((_, f), x))| decode(f, shapes, x, &format!("{p}.{i}"))).collect::<Res<_>>()?))
            }
        }
    })
}

/// Whether `v` is a value of shape `s` (to find a union's member).
fn fits(s: &Shape, shapes: &Shapes, v: &Value) -> bool {
    match (s, v) {
        (Shape::Union(ms), _) => ms.iter().any(|(_, m)| fits(m, shapes, v)),
        (Shape::Int, Value::Int(_) | Value::Big(_)) | (Shape::Float, Value::Float(_)) | (Shape::Bool, Value::Bool(_)) | (Shape::Unit, Value::Unit) | (Shape::Str, Value::Str(_)) => true,
        (Shape::List(e), Value::List(xs)) | (Shape::Set(e), Value::Set(xs)) => xs.iter().all(|x| fits(e, shapes, x)),
        (Shape::Dict(k, e), Value::Dict(kvs)) => kvs.iter().all(|(a, b)| fits(k, shapes, a) && fits(e, shapes, b)),
        (Shape::Tuple(ts), Value::Tuple(xs)) => ts.len() == xs.len() && ts.iter().zip(xs).all(|(t, x)| fits(t, shapes, x)),
        (Shape::Distinct(n, _), Value::Distinct(m, _)) => n == m,
        (Shape::Named(k), Value::Record(c, _) | Value::Variant(c, _)) => shapes.named.get(k).is_some_and(|cs| cs.iter().any(|x| &x.name == c)),
        _ => false,
    }
}

/// Lay `v`, a value of shape `s`, out in compiled code's memory, as `read`
/// reads it: the slot that holds it, its objects held once.
pub fn write(s: &Shape, shapes: &Shapes, v: &Value) -> Res<i64> {
    let wrong = || Err(format!("{} is not a value of shape {s:?}", crate::interp::show(v)));
    let all = |e: &Shape, xs: &[Value]| xs.iter().map(|x| write(e, shapes, x)).collect::<Res<Vec<_>>>();
    Ok(match (s, v) {
        (Shape::Int, Value::Int(n)) => new_int(&BigInt::from(*n)),
        (Shape::Int, Value::Big(n)) => new_int(n),
        (Shape::Float, Value::Float(f)) => f.to_bits() as i64,
        (Shape::Bool, Value::Bool(b)) => *b as i64,
        (Shape::Unit, Value::Unit) => 0,
        (Shape::Str, Value::Str(t)) => new_string(t),
        (Shape::List(e), Value::List(xs)) | (Shape::Set(e), Value::Set(xs)) => new_list(&all(e, xs)?),
        // a sorted list of (key, value) pairs
        (Shape::Dict(k, e), Value::Dict(kvs)) => {
            let pairs = kvs.iter().map(|(a, b)| Ok(new_block(0, &[write(k, shapes, a)?, write(e, shapes, b)?]))).collect::<Res<Vec<_>>>()?;
            new_list(&pairs)
        }
        (Shape::Tuple(ts), Value::Tuple(xs)) if ts.len() == xs.len() => new_block(0, &ts.iter().zip(xs).map(|(t, x)| write(t, shapes, x)).collect::<Res<Vec<_>>>()?),
        (Shape::Distinct(_, inner), Value::Distinct(_, x)) => write(inner, shapes, x)?,
        // the member it is a value of: in a box with that member's tag, or as it is
        (Shape::Union(ms), _) => match ms.iter().find(|(_, m)| fits(m, shapes, v)) {
            Some((Some(tag), m)) => new_block(*tag, &[write(m, shapes, v)?]),
            Some((None, m)) => write(m, shapes, v)?,
            None => return wrong(),
        },
        (Shape::Named(k), Value::Record(name, _) | Value::Variant(name, _)) => {
            let cs = shapes.named.get(k).ok_or(format!("no shape for `{k}`"))?;
            let Some(c) = cs.iter().find(|c| &c.name == name) else { return wrong() };
            let given: Vec<(Option<&str>, &Value)> = match v {
                Value::Record(_, fs) => fs.iter().map(|(n, x)| (Some(n.as_str()), x)).collect(),
                Value::Variant(_, Fields::Rec(fs)) => fs.iter().map(|(n, x)| (Some(n.as_str()), x)).collect(),
                Value::Variant(_, Fields::Pos(xs)) => xs.iter().map(|x| (None, x)).collect(),
                _ => vec![],
            };
            if given.len() != c.fields.len() {
                return wrong();
            }
            // fields by name when they have one, else in order
            let mut slots = vec![];
            for (i, (n, f)) in c.fields.iter().enumerate() {
                let x = match n {
                    Some(n) => given.iter().find(|(g, _)| *g == Some(n.as_str())).map(|(_, x)| *x).ok_or(format!("{name} has no field `{n}`"))?,
                    None => given[i].1,
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

    /// JSON → a value → compiled memory → a value → JSON gives back the
    /// JSON, for every kind of shape.
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
        let j: Json = serde_json::from_str(
            r#"{"id": 7, "tags": ["b", "a"], "scores": {"x": 1, "y": -2}, "keyed": [[3, true], [1, false]],
                "pos": [4, true], "either": "text", "many": [1, {"Circle": {"r": 1.5}}, ["s"], "Dot"],
                "big": "123456789012345678901234567890", "small": -4611686018427387904, "nothing": null,
                "maybe": {"Some": {"Pair": [2, "two"]}}, "figures": [{"Square": 2.0}, "Dot"]}"#,
        )
        .unwrap();
        let v = decode(&roots[0], &shapes, &j, "x").unwrap();
        let raw = write(&roots[0], &shapes, &v).unwrap();
        let back = unsafe { read(&roots[0], &shapes, raw) }.unwrap();
        assert_eq!(back, v);
        // (sets and dicts come back sorted, as compiled code keeps them)
        let want: Json = serde_json::from_str(
            r#"{"id": 7, "tags": ["a", "b"], "scores": {"x": 1, "y": -2}, "keyed": [[1, false], [3, true]],
                "pos": [4, true], "either": "text", "many": [1, {"Circle": {"r": 1.5}}, ["s"], "Dot"],
                "big": "123456789012345678901234567890", "small": -4611686018427387904, "nothing": null,
                "maybe": {"Some": {"Pair": [2, "two"]}}, "figures": [{"Square": 2.0}, "Dot"]}"#,
        )
        .unwrap();
        assert_eq!(crate::interp::to_json(&back), want);
    }

    /// What doesn't fit is an error that says where.
    #[test]
    fn decoding_says_where() {
        let prog = crate::parser::parse_program("type P = { name: String, at: List[Int] }\n").unwrap();
        let (roots, shapes) = crate::llvm::shapes_of(&prog, &[crate::ast::TypeExpr::Named("P".into(), vec![])]).unwrap();
        let err = |j: &str| decode(&roots[0], &shapes, &serde_json::from_str(j).unwrap(), "p").unwrap_err();
        assert_eq!(err(r#"{"name": "a", "at": [1, "x"]}"#), r#"p.at[1]: expected an integer, got "x""#);
        assert_eq!(err(r#"{"name": "a"}"#), "p: missing field `at`");
        assert_eq!(err(r#"{"name": "a", "at": [], "z": 1}"#), "p: unknown field `z`");
    }
}
