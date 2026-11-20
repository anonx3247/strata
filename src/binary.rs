//! Strata's binary format: a whole program (and optionally its package's
//! `strata.toml`) as one file. Written by hand, with no serialization crate,
//! so that Strata itself can read and write it once it is bootstrapped. The
//! layout is specified in `docs/binary-format.md`; keep the two in step.
//!
//! In short: a header (`STRATA` + format version), a string table (every name,
//! comment and literal string, each stored once), then the tree. Integers are
//! LEB128 (signed ones zigzag-encoded first), a string is its index in the
//! table, a list is its length then its items, an option is `0` or `1` then
//! the value, an enum is a one-byte tag then its fields, a struct is its fields
//! in order. The encoding of a tree is unique, so a file is canonical by
//! construction.

use crate::ast::*;
use std::collections::HashMap;

pub const MAGIC: &[u8; 6] = b"STRATA";
/// Bumped on every change to the layout; older files are rejected (re-pack
/// them from source: there is no backwards compatibility).
pub const VERSION: u64 = 1;
/// Deepest nesting of expressions, patterns and types a file may have.
const MAX_DEPTH: usize = 10_000;

/// What a binary file holds.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Image {
    /// The package's `strata.toml`, as written; `None` for a script.
    pub manifest: Option<String>,
    pub program: Program,
}

/// Does `bytes` look like a binary Strata file (rather than source text)?
pub fn is_binary(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

pub fn encode(img: &Image) -> Vec<u8> {
    let mut w = Writer::default();
    img.manifest.enc(&mut w);
    img.program.decls.enc(&mut w);
    let mut out = MAGIC.to_vec();
    uint(&mut out, VERSION);
    uint(&mut out, w.strings.len() as u64);
    for s in &w.strings {
        uint(&mut out, s.len() as u64);
        out.extend_from_slice(s.as_bytes());
    }
    out.extend_from_slice(&w.body);
    out
}

pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    let mut r = Reader { bytes, pos: 0, strings: Vec::new(), depth: 0 };
    if !is_binary(bytes) {
        return Err("not a binary Strata file (it doesn't start with `STRATA`)".into());
    }
    r.pos = MAGIC.len();
    let v = r.uint()?;
    if v != VERSION {
        return Err(format!("binary format version {v}, but this strata reads version {VERSION}: re-pack it from source"));
    }
    let n = r.len()?;
    for _ in 0..n {
        let len = r.len()?;
        let s = std::str::from_utf8(r.take(len)?).map(str::to_string);
        let s = s.map_err(|_| r.err("a string that isn't UTF-8"))?;
        r.strings.push(s);
    }
    let manifest = Option::<String>::dec(&mut r)?;
    let decls = Vec::<Decl>::dec(&mut r)?;
    if r.pos != bytes.len() {
        return Err(r.err(&format!("{} bytes left over after the program", bytes.len() - r.pos)));
    }
    Ok(Image { manifest, program: Program { decls } })
}

// ── primitives ──────────────────────────────────────────────────────────────

fn uint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

#[derive(Default)]
struct Writer {
    body: Vec<u8>,
    strings: Vec<String>,
    index: HashMap<String, u64>,
}

impl Writer {
    fn uint(&mut self, v: u64) {
        uint(&mut self.body, v)
    }
    fn byte(&mut self, b: u8) {
        self.body.push(b)
    }
    fn str(&mut self, s: &str) {
        let i = match self.index.get(s) {
            Some(&i) => i,
            None => {
                let i = self.strings.len() as u64;
                self.strings.push(s.to_string());
                self.index.insert(s.to_string(), i);
                i
            }
        };
        self.uint(i)
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
    strings: Vec<String>,
    depth: usize,
}

impl Reader<'_> {
    fn err(&self, what: &str) -> String {
        format!("corrupt binary Strata file at byte {}: {what}", self.pos)
    }
    fn byte(&mut self) -> Result<u8, String> {
        let b = *self.bytes.get(self.pos).ok_or_else(|| self.err("unexpected end of file"))?;
        self.pos += 1;
        Ok(b)
    }
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        if self.bytes.len() - self.pos < n {
            return Err(self.err("unexpected end of file"));
        }
        self.pos += n;
        Ok(&self.bytes[self.pos - n..self.pos])
    }
    fn uint(&mut self) -> Result<u64, String> {
        let mut v: u64 = 0;
        for shift in (0..64).step_by(7) {
            let b = self.byte()?;
            let bits = (b & 0x7f) as u64;
            if shift == 63 && bits > 1 {
                return Err(self.err("an integer too large for 64 bits"));
            }
            v |= bits << shift;
            if b & 0x80 == 0 {
                // the shortest form only, so each value has one encoding
                if b == 0 && shift > 0 {
                    return Err(self.err("an integer with a redundant trailing byte"));
                }
                return Ok(v);
            }
        }
        Err(self.err("an integer too large for 64 bits"))
    }
    /// A count of items or bytes still to come: it can't exceed what is left.
    fn len(&mut self) -> Result<usize, String> {
        let n = self.uint()?;
        if n > (self.bytes.len() - self.pos) as u64 {
            return Err(self.err(&format!("a length of {n}, past the end of the file")));
        }
        Ok(n as usize)
    }
    fn tag(&mut self, of: &str, max: u8) -> Result<u8, String> {
        let t = self.byte()?;
        if t > max {
            self.pos -= 1;
            return Err(self.err(&format!("unknown {of} tag {t}")));
        }
        Ok(t)
    }
    fn nest<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, String>) -> Result<T, String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.err("nesting too deep"));
        }
        let v = f(self);
        self.depth -= 1;
        v
    }
}

/// A value with a binary encoding.
trait Bin: Sized {
    fn enc(&self, w: &mut Writer);
    fn dec(r: &mut Reader) -> Result<Self, String>;
}

impl Bin for u64 {
    fn enc(&self, w: &mut Writer) {
        w.uint(*self)
    }
    fn dec(r: &mut Reader) -> Result<Self, String> {
        r.uint()
    }
}

impl Bin for usize {
    fn enc(&self, w: &mut Writer) {
        w.uint(*self as u64)
    }
    fn dec(r: &mut Reader) -> Result<Self, String> {
        let v = r.uint()?;
        usize::try_from(v).map_err(|_| r.err("a number too large"))
    }
}

/// Zigzag: 0, -1, 1, -2, … become 0, 1, 2, 3, …, so small negatives stay short.
impl Bin for i64 {
    fn enc(&self, w: &mut Writer) {
        w.uint(((*self << 1) ^ (*self >> 63)) as u64)
    }
    fn dec(r: &mut Reader) -> Result<Self, String> {
        let u = r.uint()?;
        Ok(((u >> 1) as i64) ^ -((u & 1) as i64))
    }
}

/// The IEEE 754 bits, little-endian.
impl Bin for f64 {
    fn enc(&self, w: &mut Writer) {
        w.body.extend_from_slice(&self.to_bits().to_le_bytes())
    }
    fn dec(r: &mut Reader) -> Result<Self, String> {
        let b: [u8; 8] = r.take(8)?.try_into().unwrap();
        Ok(f64::from_bits(u64::from_le_bytes(b)))
    }
}

impl Bin for bool {
    fn enc(&self, w: &mut Writer) {
        w.byte(*self as u8)
    }
    fn dec(r: &mut Reader) -> Result<Self, String> {
        Ok(r.tag("bool", 1)? == 1)
    }
}

/// A Unicode scalar value, as an unsigned integer.
impl Bin for char {
    fn enc(&self, w: &mut Writer) {
        w.uint(*self as u64)
    }
    fn dec(r: &mut Reader) -> Result<Self, String> {
        let v = r.uint()?;
        u32::try_from(v).ok().and_then(char::from_u32).ok_or_else(|| r.err(&format!("{v} is not a character")))
    }
}

impl Bin for String {
    fn enc(&self, w: &mut Writer) {
        w.str(self)
    }
    fn dec(r: &mut Reader) -> Result<Self, String> {
        let i = r.uint()?;
        r.strings.get(i as usize).cloned().ok_or_else(|| r.err(&format!("string {i}, not in the table")))
    }
}

impl<T: Bin> Bin for Vec<T> {
    fn enc(&self, w: &mut Writer) {
        w.uint(self.len() as u64);
        self.iter().for_each(|x| x.enc(w))
    }
    fn dec(r: &mut Reader) -> Result<Self, String> {
        // every item takes at least one byte, so `len` bounds the allocation
        let n = r.len()?;
        (0..n).map(|_| T::dec(r)).collect()
    }
}

impl<T: Bin> Bin for Option<T> {
    fn enc(&self, w: &mut Writer) {
        match self {
            None => w.byte(0),
            Some(x) => {
                w.byte(1);
                x.enc(w)
            }
        }
    }
    fn dec(r: &mut Reader) -> Result<Self, String> {
        Ok(match r.tag("option", 1)? {
            0 => None,
            _ => Some(T::dec(r)?),
        })
    }
}

impl<T: Bin> Bin for Box<T> {
    fn enc(&self, w: &mut Writer) {
        (**self).enc(w)
    }
    fn dec(r: &mut Reader) -> Result<Self, String> {
        T::dec(r).map(Box::new)
    }
}

impl<A: Bin, B: Bin> Bin for (A, B) {
    fn enc(&self, w: &mut Writer) {
        self.0.enc(w);
        self.1.enc(w)
    }
    fn dec(r: &mut Reader) -> Result<Self, String> {
        Ok((A::dec(r)?, B::dec(r)?))
    }
}

/// A struct: its fields in order.
macro_rules! record {
    ($t:ident { $($f:ident),* }) => {
        impl Bin for $t {
            fn enc(&self, w: &mut Writer) {
                $(self.$f.enc(w);)*
            }
            fn dec(r: &mut Reader) -> Result<Self, String> {
                Ok($t { $($f: Bin::dec(r)?),* })
            }
        }
    };
}

/// An enum whose variants carry nothing: one tag byte, in the order listed
/// (the order is the format, not the order in `ast.rs`).
macro_rules! tags {
    ($t:ident { $($v:ident = $n:literal),* }) => {
        impl Bin for $t {
            fn enc(&self, w: &mut Writer) {
                w.byte(match self { $($t::$v => $n),* })
            }
            fn dec(r: &mut Reader) -> Result<Self, String> {
                let max = [$($n),*].into_iter().max().unwrap();
                match r.tag(stringify!($t), max)? {
                    $($n => Ok($t::$v),)*
                    _ => unreachable!(),
                }
            }
        }
    };
}

/// A variant with fields: its tag, then each field.
macro_rules! put {
    ($w:ident, $tag:literal, $($x:expr),+) => {{
        $w.byte($tag);
        $($x.enc($w);)+
    }};
}

// ── the tree ────────────────────────────────────────────────────────────────

record!(Decl { comments, doc, module, public, kind });
record!(TypeDecl { distinct, name, params, body, refinement });
record!(Field { comments, doc, name, ty });
record!(Variant { comments, name, payload });
record!(Param { name, ty, mutable });
record!(Bound { param, traits });
record!(FnDecl { doc, comments, name, type_params, bounds, params, ret, requires, ensures, terminates, body });
record!(TestDecl { name, body });
record!(TraitMethod { comments, doc, decl, default });
record!(Law { comments, doc, name, params, body });
record!(TraitDecl { name, params, supers, methods, laws });
record!(ImplDecl { type_params, bounds, trait_name, trait_args, for_type, methods });
record!(UseDecl { path, items });
record!(Arm { comments, pat, body });
record!(LamParam { name, ty });
record!(FmtSpec { fill, align, plus, zero, width, precision });

tags!(BinOp { Add = 0, Sub = 1, Mul = 2, Div = 3, Mod = 4, Eq = 5, Ne = 6, Lt = 7, Le = 8, Gt = 9, Ge = 10, And = 11, Or = 12, Implies = 13 });
tags!(UnOp { Neg = 0, Not = 1 });
tags!(TryKind { Result = 0, Option = 1 });

impl Bin for DeclKind {
    fn enc(&self, w: &mut Writer) {
        match self {
            DeclKind::Type(x) => put!(w, 0, x),
            DeclKind::Fn(x) => put!(w, 1, x),
            DeclKind::Test(x) => put!(w, 2, x),
            DeclKind::Trait(x) => put!(w, 3, x),
            DeclKind::Impl(x) => put!(w, 4, x),
            DeclKind::Use(x) => put!(w, 5, x),
        }
    }
    fn dec(r: &mut Reader) -> Result<Self, String> {
        Ok(match r.tag("declaration", 5)? {
            0 => DeclKind::Type(Bin::dec(r)?),
            1 => DeclKind::Fn(Bin::dec(r)?),
            2 => DeclKind::Test(Bin::dec(r)?),
            3 => DeclKind::Trait(Bin::dec(r)?),
            4 => DeclKind::Impl(Bin::dec(r)?),
            _ => DeclKind::Use(Bin::dec(r)?),
        })
    }
}

impl Bin for TypeBody {
    fn enc(&self, w: &mut Writer) {
        match self {
            TypeBody::Alias(t) => put!(w, 0, t),
            TypeBody::Record(fs) => put!(w, 1, fs),
            TypeBody::Sum(vs) => put!(w, 2, vs),
        }
    }
    fn dec(r: &mut Reader) -> Result<Self, String> {
        Ok(match r.tag("type body", 2)? {
            0 => TypeBody::Alias(Bin::dec(r)?),
            1 => TypeBody::Record(Bin::dec(r)?),
            _ => TypeBody::Sum(Bin::dec(r)?),
        })
    }
}

impl Bin for Payload {
    fn enc(&self, w: &mut Writer) {
        match self {
            Payload::None => w.byte(0),
            Payload::Positional(ts) => put!(w, 1, ts),
            Payload::Record(fs) => put!(w, 2, fs),
        }
    }
    fn dec(r: &mut Reader) -> Result<Self, String> {
        Ok(match r.tag("variant payload", 2)? {
            0 => Payload::None,
            1 => Payload::Positional(Bin::dec(r)?),
            _ => Payload::Record(Bin::dec(r)?),
        })
    }
}

impl Bin for TypeExpr {
    fn enc(&self, w: &mut Writer) {
        match self {
            TypeExpr::Named(n, args) => put!(w, 0, n, args),
            TypeExpr::Fn(ps, ret) => put!(w, 1, ps, ret),
            TypeExpr::Unit => w.byte(2),
            TypeExpr::Refined(t, e) => put!(w, 3, t, e),
            TypeExpr::Union(ts) => put!(w, 4, ts),
        }
    }
    fn dec(r: &mut Reader) -> Result<Self, String> {
        r.nest(|r| {
            Ok(match r.tag("type", 4)? {
                0 => TypeExpr::Named(Bin::dec(r)?, Bin::dec(r)?),
                1 => TypeExpr::Fn(Bin::dec(r)?, Bin::dec(r)?),
                2 => TypeExpr::Unit,
                3 => TypeExpr::Refined(Bin::dec(r)?, Bin::dec(r)?),
                _ => TypeExpr::Union(Bin::dec(r)?),
            })
        })
    }
}

impl Bin for Pattern {
    fn enc(&self, w: &mut Writer) {
        match self {
            Pattern::Wild => w.byte(0),
            Pattern::Bind(n) => put!(w, 1, n),
            Pattern::Int(i) => put!(w, 2, i),
            Pattern::Str(s) => put!(w, 3, s),
            Pattern::Bool(b) => put!(w, 4, b),
            Pattern::Ctor(n, c) => put!(w, 5, n, c),
            Pattern::List(ps, rest) => put!(w, 6, ps, rest),
            Pattern::Tuple(ps) => put!(w, 7, ps),
            Pattern::Cond(e) => put!(w, 8, e),
            Pattern::Or(ps) => put!(w, 9, ps),
        }
    }
    fn dec(r: &mut Reader) -> Result<Self, String> {
        r.nest(|r| {
            Ok(match r.tag("pattern", 9)? {
                0 => Pattern::Wild,
                1 => Pattern::Bind(Bin::dec(r)?),
                2 => Pattern::Int(Bin::dec(r)?),
                3 => Pattern::Str(Bin::dec(r)?),
                4 => Pattern::Bool(Bin::dec(r)?),
                5 => Pattern::Ctor(Bin::dec(r)?, Bin::dec(r)?),
                6 => Pattern::List(Bin::dec(r)?, Bin::dec(r)?),
                7 => Pattern::Tuple(Bin::dec(r)?),
                8 => Pattern::Cond(Bin::dec(r)?),
                _ => Pattern::Or(Bin::dec(r)?),
            })
        })
    }
}

impl Bin for CtorPat {
    fn enc(&self, w: &mut Writer) {
        match self {
            CtorPat::None => w.byte(0),
            CtorPat::Positional(ps) => put!(w, 1, ps),
            CtorPat::Record(fs, rest) => put!(w, 2, fs, rest),
        }
    }
    fn dec(r: &mut Reader) -> Result<Self, String> {
        Ok(match r.tag("constructor pattern", 2)? {
            0 => CtorPat::None,
            1 => CtorPat::Positional(Bin::dec(r)?),
            _ => CtorPat::Record(Bin::dec(r)?, Bin::dec(r)?),
        })
    }
}

impl Bin for Stmt {
    fn enc(&self, w: &mut Writer) {
        match self {
            Stmt::Let(p, t, e) => put!(w, 0, p, t, e),
            Stmt::Var(n, t, e) => put!(w, 1, n, t, e),
            Stmt::Assign(a, b) => put!(w, 2, a, b),
            Stmt::While(c, b) => put!(w, 3, c, b),
            Stmt::For(p, xs, b) => put!(w, 4, p, xs, b),
            Stmt::Loop(b) => put!(w, 5, b),
            Stmt::Break => w.byte(6),
            Stmt::Continue => w.byte(7),
            Stmt::Expr(e) => put!(w, 8, e),
            Stmt::Assert(e) => put!(w, 9, e),
            Stmt::Comment(ls) => put!(w, 10, ls),
        }
    }
    fn dec(r: &mut Reader) -> Result<Self, String> {
        Ok(match r.tag("statement", 10)? {
            0 => Stmt::Let(Bin::dec(r)?, Bin::dec(r)?, Bin::dec(r)?),
            1 => Stmt::Var(Bin::dec(r)?, Bin::dec(r)?, Bin::dec(r)?),
            2 => Stmt::Assign(Bin::dec(r)?, Bin::dec(r)?),
            3 => Stmt::While(Bin::dec(r)?, Bin::dec(r)?),
            4 => Stmt::For(Bin::dec(r)?, Bin::dec(r)?, Bin::dec(r)?),
            5 => Stmt::Loop(Bin::dec(r)?),
            6 => Stmt::Break,
            7 => Stmt::Continue,
            8 => Stmt::Expr(Bin::dec(r)?),
            9 => Stmt::Assert(Bin::dec(r)?),
            _ => Stmt::Comment(Bin::dec(r)?),
        })
    }
}

impl Bin for Expr {
    fn enc(&self, w: &mut Writer) {
        match self {
            Expr::Int(i) => put!(w, 0, i),
            Expr::BigInt(digits) => put!(w, 1, digits),
            Expr::Float(f) => put!(w, 2, f),
            Expr::Tuple(xs) => put!(w, 3, xs),
            Expr::Str(s) => put!(w, 4, s),
            Expr::Bool(b) => put!(w, 5, b),
            Expr::Unit => w.byte(6),
            Expr::Var(n) => put!(w, 7, n),
            Expr::Hole(n) => put!(w, 8, n),
            Expr::List(xs) => put!(w, 9, xs),
            Expr::Ctor(n, a) => put!(w, 10, n, a),
            Expr::Call(f, xs) => put!(w, 11, f, xs),
            Expr::Method(x, m, xs) => put!(w, 12, x, m, xs),
            Expr::Field(x, f) => put!(w, 13, x, f),
            Expr::Lambda(ps, b) => put!(w, 14, ps, b),
            Expr::Bin(op, a, b) => put!(w, 15, op, a, b),
            Expr::Un(op, x) => put!(w, 16, op, x),
            Expr::Is(x, v) => put!(w, 17, x, v),
            Expr::If(c, t, e) => put!(w, 18, c, t, e),
            Expr::Match(x, name, arms) => put!(w, 19, x, name, arms),
            Expr::Mut(x) => put!(w, 20, x),
            Expr::Block(ss, last) => put!(w, 21, ss, last),
            Expr::Mock(t) => put!(w, 22, t),
            Expr::Return(x) => put!(w, 23, x),
            Expr::Try(x, k) => put!(w, 24, x, k),
            Expr::Except(x, err, fb) => put!(w, 25, x, err, fb),
            Expr::Interp(parts) => put!(w, 26, parts),
        }
    }
    fn dec(r: &mut Reader) -> Result<Self, String> {
        r.nest(|r| {
            Ok(match r.tag("expression", 26)? {
                0 => Expr::Int(Bin::dec(r)?),
                1 => Expr::BigInt(Bin::dec(r)?),
                2 => Expr::Float(Bin::dec(r)?),
                3 => Expr::Tuple(Bin::dec(r)?),
                4 => Expr::Str(Bin::dec(r)?),
                5 => Expr::Bool(Bin::dec(r)?),
                6 => Expr::Unit,
                7 => Expr::Var(Bin::dec(r)?),
                8 => Expr::Hole(Bin::dec(r)?),
                9 => Expr::List(Bin::dec(r)?),
                10 => Expr::Ctor(Bin::dec(r)?, Bin::dec(r)?),
                11 => Expr::Call(Bin::dec(r)?, Bin::dec(r)?),
                12 => Expr::Method(Bin::dec(r)?, Bin::dec(r)?, Bin::dec(r)?),
                13 => Expr::Field(Bin::dec(r)?, Bin::dec(r)?),
                14 => Expr::Lambda(Bin::dec(r)?, Bin::dec(r)?),
                15 => Expr::Bin(Bin::dec(r)?, Bin::dec(r)?, Bin::dec(r)?),
                16 => Expr::Un(Bin::dec(r)?, Bin::dec(r)?),
                17 => Expr::Is(Bin::dec(r)?, Bin::dec(r)?),
                18 => Expr::If(Bin::dec(r)?, Bin::dec(r)?, Bin::dec(r)?),
                19 => Expr::Match(Bin::dec(r)?, Bin::dec(r)?, Bin::dec(r)?),
                20 => Expr::Mut(Bin::dec(r)?),
                21 => Expr::Block(Bin::dec(r)?, Bin::dec(r)?),
                22 => Expr::Mock(Bin::dec(r)?),
                23 => Expr::Return(Bin::dec(r)?),
                24 => Expr::Try(Bin::dec(r)?, Bin::dec(r)?),
                25 => Expr::Except(Bin::dec(r)?, Bin::dec(r)?, Bin::dec(r)?),
                _ => Expr::Interp(Bin::dec(r)?),
            })
        })
    }
}

impl Bin for StrPart {
    fn enc(&self, w: &mut Writer) {
        match self {
            StrPart::Lit(s) => put!(w, 0, s),
            StrPart::Expr(e, spec) => put!(w, 1, e, spec),
        }
    }
    fn dec(r: &mut Reader) -> Result<Self, String> {
        Ok(match r.tag("f-string part", 1)? {
            0 => StrPart::Lit(Bin::dec(r)?),
            _ => StrPart::Expr(Bin::dec(r)?, Bin::dec(r)?),
        })
    }
}

impl Bin for CtorArgs {
    fn enc(&self, w: &mut Writer) {
        match self {
            CtorArgs::None => w.byte(0),
            CtorArgs::Positional(xs) => put!(w, 1, xs),
            CtorArgs::Record(fs) => put!(w, 2, fs),
            CtorArgs::Update(base, fs) => put!(w, 3, base, fs),
        }
    }
    fn dec(r: &mut Reader) -> Result<Self, String> {
        Ok(match r.tag("constructor arguments", 3)? {
            0 => CtorArgs::None,
            1 => CtorArgs::Positional(Bin::dec(r)?),
            2 => CtorArgs::Record(Bin::dec(r)?),
            _ => CtorArgs::Update(Bin::dec(r)?, Bin::dec(r)?),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(img: &Image) {
        let bytes = encode(img);
        assert_eq!(&decode(&bytes).unwrap(), img);
        // canonical: encoding the decoded tree gives the same bytes
        assert_eq!(encode(&decode(&bytes).unwrap()), bytes);
    }

    #[test]
    fn integers() {
        for v in [0, 1, -1, 63, -64, 64, 1 << 40, i64::MAX, i64::MIN] {
            let mut w = Writer::default();
            v.enc(&mut w);
            let mut r = Reader { bytes: &w.body, pos: 0, strings: vec![], depth: 0 };
            assert_eq!(i64::dec(&mut r).unwrap(), v);
            assert_eq!(r.pos, w.body.len());
        }
        let mut out = Vec::new();
        uint(&mut out, 300);
        assert_eq!(out, [0xac, 0x02]);
    }

    #[test]
    fn every_example_round_trips() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut paths: Vec<_> = std::fs::read_dir(root.join("examples")).unwrap().map(|e| e.unwrap().path()).collect();
        paths.push(root.join("std"));
        for path in paths {
            let program = crate::package::program(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let manifest = std::fs::read_to_string(path.join(crate::package::FILE)).ok();
            roundtrip(&Image { manifest, program });
        }
    }

    #[test]
    fn every_node_round_trips() {
        let src = r#"
/// doc
// comment
pub distinct type Id = Int where it > 0
type R[T] = { /// f
  a: T, b: (Int, String) -> () }
type S = | A | B(Int, Float) | C { x: Int | String, y: Int where it < 5 }
trait Tr[P]: Eq { fn m(self: Self, mut q: P) -> Int
  law l(a: Self): a == a }
impl[T: Eq + Tr[Int]] Tr[Int] for R[T] { fn m(self: Self, mut q: Int) -> Int { 0 } }
fn f(x: Int) -> Int requires x > 0 ensures it >= 0 terminates {
  let (a, _) = (1, 2.5)
  let mut v: Int = -3
  v = v + 99999999999999999999999
  while v < 0 { v = v + 1 }
  for [h, ..t] in [[1]] { break }
  loop { continue }
  assert !true ==> false
  // c
  let g = (y: Int) -> y % 2
  let z = match S.A as s { A | B(_, _) => 1, C { x: 3, .. } => 2, B("s", true) => 4, where it is C => 3, _ => ?hole }
  let o = Some(v)? + Ok(1)! + (Err(2) except e { e })
  let r = R { a: 1, b: g } ; let r2 = R { ..r, a: 2 }
  print(f"{x:>08.2} and {r.a}")
  mock Tr
  if "s".len() == 1 { return 1 } else { f(mut v) }
}
test "t" { assert f(1) == 1 }
use a.b.{c, d}
use e.*
"#;
        // not every line has to check: only parse, then round-trip the tree
        let program = match crate::parser::parse_module_file(src, "m") {
            Ok(p) => p,
            Err(e) => panic!("{e}"),
        };
        roundtrip(&Image { manifest: None, program });
    }

    #[test]
    fn bad_input_is_an_error_not_a_crash() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let program = crate::package::program(&root.join("examples/orders.strata")).unwrap();
        let bytes = encode(&Image { manifest: None, program });
        for n in 0..bytes.len() {
            assert!(decode(&bytes[..n]).is_err(), "a file cut at {n} bytes decoded");
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(decode(&extra).is_err());
        let mut newer = bytes.clone();
        newer[MAGIC.len()] = VERSION as u8 + 1;
        assert!(decode(&newer).unwrap_err().contains("version"));
        // flipping any byte gives an error or another tree, never a panic
        for i in 0..bytes.len() {
            let mut b = bytes.clone();
            b[i] ^= 0xff;
            let _ = decode(&b);
        }
        // deep nesting is refused, before it runs out of stack: a test whose
        // body is `mut mut … ()`
        let mut deep = MAGIC.to_vec();
        deep.extend([VERSION as u8, 1, 0]); // one string, ""
        deep.extend([0, 1]); // no manifest; one declaration:
        deep.extend([0, 0, 0, 0, 2, 0]); // no comments or doc, module "", private, test ""
        deep.extend(std::iter::repeat_n(20, 2 * MAX_DEPTH));
        deep.push(6);
        let deep = std::thread::Builder::new().stack_size(1 << 30).spawn(move || decode(&deep)).unwrap().join().unwrap();
        assert!(deep.unwrap_err().contains("nesting too deep"));
    }
}
