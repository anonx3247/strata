//! Strata's binary format: the syntax tree, not text, written by hand with no
//! serialization crate so that Strata itself can read and write it once it is
//! bootstrapped. The layout is specified in `docs/binary-format.md`; keep the
//! two in step.
//!
//! A **module file** is a header, then its definitions. The header (a table of
//! contents) gives each definition's name, kind, visibility, hashes, the
//! definitions it depends on, and the size of its bytes, so a tool can list a
//! module, build the dependency graph or see what changed without decoding a
//! single body. Each definition's bytes are self-contained (their own strings)
//! and split in two: its **interface** (types, signatures, contracts) and then
//! its **bodies**, so checking code against a module can stop at interfaces.
//! A **bundle** is a whole program, its `strata.toml` and its module files, in
//! one file.
//!
//! Inside, integers are LEB128 (signed ones zigzag-encoded first), a string is
//! its index in a string table, a list is its length then its items, an option
//! is `0` or `1` then the value, an enum is a one-byte tag then its fields, a
//! struct is its fields in order. A tree has exactly one encoding.

use crate::ast::*;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use crate::fx::HashMap;

pub const MAGIC: &[u8; 6] = b"STRATA";
/// The format isn't settled yet, so it stays 1 (user, 2026-11-21): a change
/// to the layout doesn't bump it, it converts the repository's files instead
/// (see "Changing the format" in docs/binary-format.md). Numbering starts
/// once files must stay readable across versions.
pub const VERSION: u64 = 1;
const MODULE: u8 = 0;
const BUNDLE: u8 = 1;
/// Deepest nesting of expressions, patterns and types a file may have.
const MAX_DEPTH: usize = 10_000;

pub type Hash = [u8; 32];

/// A whole program, as a bundle holds it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Image {
    /// The package's `strata.toml`, as written; `None` for a script.
    pub manifest: Option<String>,
    pub program: Program,
}

/// A module file's header: what is in it, without decoding it.
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    /// Dotted module path; `""` for the root module.
    pub module: String,
    pub entries: Vec<Entry>,
}

/// One definition, as the header lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// `Decl::name`: `total`, `Order`, `test "totals"`, `impl Show for Order`, `use orders`.
    pub name: String,
    pub kind: &'static str,
    pub public: bool,
    /// Of its bytes: comments, docs and all.
    pub hash: Hash,
    /// Of what other code relies on: its interface, without comments or docs.
    pub interface_hash: Hash,
    /// Of what it does: interface and bodies, without comments or docs.
    pub code_hash: Hash,
    /// Qualified names of the definitions it refers to.
    pub deps: Vec<String>,
    /// Where its bytes start in the file, how many there are, and how many of
    /// them are its interface.
    pub offset: usize,
    pub size: usize,
    pub interface_size: usize,
}

/// Does `bytes` look like a binary Strata file (rather than source text)?
pub fn is_binary(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

// ── definitions ─────────────────────────────────────────────────────────────

/// A definition's bytes: its interface (strings, then the definition with
/// every body left out, written as `()`), then its bodies (the strings they
/// add, then the bodies). Self-contained: they mean the same in any file.
pub fn encode_definition(d: &Decl) -> Vec<u8> {
    encode_parts(d.clone()).0
}

/// The bytes, and how many of them are the interface.
fn encode_parts(mut d: Decl) -> (Vec<u8>, usize) {
    let bodies = take_bodies(&mut d.kind);
    let mut w = Writer::default();
    d.enc(&mut w);
    let mut out = w.flush(0);
    let interface = out.len();
    let n = w.strings.len();
    bodies.enc(&mut w);
    out.extend(w.flush(n));
    (out, interface)
}

pub fn decode_definition(bytes: &[u8]) -> Result<Decl, String> {
    let mut r = Reader::new(bytes);
    let mut d = r.part(Decl::dec)?;
    let bodies = r.part(Vec::<Expr>::dec)?;
    r.end()?;
    put_bodies(&mut d.kind, bodies).map_err(|e| r.err(&e))?;
    Ok(d)
}

/// Only the interface part of a definition's bytes: its bodies are `()`.
#[allow(dead_code)] // for checking against a module's interfaces alone (not yet)
pub fn decode_interface(bytes: &[u8]) -> Result<Decl, String> {
    Reader::new(bytes).part(Decl::dec)
}

/// The hash of a definition's bytes.
pub fn hash(d: &Decl) -> Hash {
    Sha256::digest(encode_definition(d)).into()
}

/// The hash of what a definition does: no comments or docs.
pub fn code_hash(d: &Decl) -> Hash {
    Sha256::digest(encode_parts(without_notes(d.clone())).0).into()
}

/// The hash of what other code relies on: the interface part, without
/// comments or docs (and with its visibility and module, which it holds).
pub fn interface_hash(d: &Decl) -> Hash {
    let mut d = d.clone();
    take_bodies(&mut d.kind);
    let mut w = Writer::default();
    without_notes(d).enc(&mut w);
    Sha256::digest(w.flush(0)).into()
}

/// The bodies of a definition, in order, each replaced by `()`: a function's
/// body, a test's, the bodies of a trait's methods and of an impl's methods.
/// Everything else (signatures, contracts, laws, types) is its interface.
fn take_bodies(k: &mut DeclKind) -> Vec<Expr> {
    let take = |e: &mut Expr| std::mem::replace(e, Expr::Unit);
    match k {
        DeclKind::Fn(f) => vec![take(&mut f.body)],
        DeclKind::Test(t) => vec![take(&mut t.body)],
        DeclKind::Trait(t) => t.methods.iter_mut().map(|m| take(&mut m.decl.body)).collect(),
        DeclKind::Impl(i) => i.methods.iter_mut().map(|m| take(&mut m.body)).collect(),
        DeclKind::Type(_) | DeclKind::Use(_) => vec![],
    }
}

fn put_bodies(k: &mut DeclKind, bodies: Vec<Expr>) -> Result<(), String> {
    let mut slots: Vec<&mut Expr> = match k {
        DeclKind::Fn(f) => vec![&mut f.body],
        DeclKind::Test(t) => vec![&mut t.body],
        DeclKind::Trait(t) => t.methods.iter_mut().map(|m| &mut m.decl.body).collect(),
        DeclKind::Impl(i) => i.methods.iter_mut().map(|m| &mut m.body).collect(),
        DeclKind::Type(_) | DeclKind::Use(_) => vec![],
    };
    if slots.len() != bodies.len() {
        return Err(format!("{} bodies for a definition that has {}", bodies.len(), slots.len()));
    }
    for (slot, b) in slots.iter_mut().zip(bodies) {
        **slot = b;
    }
    Ok(())
}

/// The definition without its `//` comments and `///` docs, anywhere in it.
pub fn without_notes(mut d: Decl) -> Decl {
    struct Strip;
    impl VisitMut for Strip {
        fn expr(&mut self, e: &mut Expr) {
            match e {
                Expr::Block(stmts, last) => {
                    stmts.retain(|s| !matches!(s, Stmt::Comment(_)));
                    // `{ // note\n x }` is `x`, as `{ x }` is
                    if stmts.is_empty() {
                        *e = std::mem::replace(&mut **last, Expr::Unit);
                        return self.expr(e);
                    }
                }
                Expr::Match(_, _, arms) => arms.iter_mut().for_each(|a| a.comments.clear()),
                _ => {}
            }
            walk_expr_mut(self, e)
        }
    }
    fn fields(fs: &mut [Field]) {
        for f in fs {
            f.comments.clear();
            f.doc.clear();
        }
    }
    fn func(f: &mut FnDecl) {
        f.comments.clear();
        f.doc.clear();
    }
    d.comments.clear();
    d.doc.clear();
    match &mut d.kind {
        DeclKind::Type(t) => match &mut t.body {
            TypeBody::Record(fs) => fields(fs),
            TypeBody::Sum(vs) => {
                for v in vs {
                    v.comments.clear();
                    if let Payload::Record(fs) = &mut v.payload {
                        fields(fs)
                    }
                }
            }
            TypeBody::Alias(_) => {}
        },
        DeclKind::Fn(f) => func(f),
        DeclKind::Trait(t) => {
            for m in &mut t.methods {
                m.comments.clear();
                m.doc.clear();
                func(&mut m.decl);
            }
            for l in &mut t.laws {
                l.comments.clear();
                l.doc.clear();
            }
        }
        DeclKind::Impl(i) => i.methods.iter_mut().for_each(func),
        DeclKind::Test(_) | DeclKind::Use(_) => {}
    }
    walk_decl_mut(&mut Strip, &mut d.kind);
    d
}

// ── module files ────────────────────────────────────────────────────────────

/// One module of `p` as a module file. `deps`: what each definition (by
/// qualified name) refers to, as `store::graph` gives it.
pub fn encode_module(p: &Program, module: &str, deps: &BTreeMap<String, BTreeSet<String>>) -> Vec<u8> {
    let mut w = Writer::default();
    let mut defs = Vec::new();
    module.to_string().enc(&mut w);
    let decls: Vec<&Decl> = p.decls.iter().filter(|d| d.module == module).collect();
    w.uint(decls.len() as u64);
    for d in decls {
        let (bytes, interface) = encode_parts(d.clone());
        d.name().enc(&mut w);
        w.byte(kind_tag(&d.kind));
        d.public.enc(&mut w);
        w.body.extend_from_slice(&Sha256::digest(&bytes));
        w.body.extend_from_slice(&interface_hash(d));
        w.body.extend_from_slice(&code_hash(d));
        deps.get(&d.qname()).cloned().unwrap_or_default().into_iter().collect::<Vec<_>>().enc(&mut w);
        w.uint(bytes.len() as u64);
        w.uint(interface as u64);
        defs.extend(bytes);
    }
    let mut out = MAGIC.to_vec();
    uint(&mut out, VERSION);
    out.push(MODULE);
    out.extend(w.flush(0));
    out.extend(defs);
    out
}

/// One module file per module of `p`, in the order the modules first appear.
pub fn encode_modules(p: &Program) -> Vec<(String, Vec<u8>)> {
    // stored with every reference qualified (see `resolve::stored_form`);
    // the header names definitions as stored (`impl std.cli.CommandLine for Options`)
    let p = &crate::resolve::stored_form(p);
    let deps = crate::store::graph(p);
    let mut modules: Vec<&Symbol> = Vec::new();
    for d in &p.decls {
        if !modules.contains(&&d.module) {
            modules.push(&d.module);
        }
    }
    modules.into_iter().map(|m| (m.to_string(), encode_module(p, m, &deps))).collect()
}

/// A module file's header, without decoding any definition.
pub fn read_header(bytes: &[u8]) -> Result<Header, String> {
    let mut r = Reader::new(bytes);
    if r.preamble()? != MODULE {
        return Err("a bundle, not a module file".into());
    }
    r.strings()?;
    let module = String::dec(&mut r)?;
    let n = r.len()?;
    let mut entries = Vec::with_capacity(n);
    let mut offset = 0;
    for _ in 0..n {
        let name = String::dec(&mut r)?;
        let kind = KINDS[r.tag("declaration", KINDS.len() as u8 - 1)? as usize];
        let public = bool::dec(&mut r)?;
        let mut hashes = [[0u8; 32]; 3];
        for h in &mut hashes {
            h.copy_from_slice(r.take(32)?);
        }
        let deps = Vec::<String>::dec(&mut r)?;
        let size = usize::dec(&mut r)?;
        let interface_size = usize::dec(&mut r)?;
        if interface_size > size {
            return Err(r.err("an interface larger than its definition"));
        }
        let [hash, interface_hash, code_hash] = hashes;
        entries.push(Entry { name, kind, public, hash, interface_hash, code_hash, deps, offset, size, interface_size });
        offset += size;
    }
    let start = r.pos;
    if bytes.len() - start != offset {
        return Err(format!("corrupt binary Strata file: the header lists {offset} bytes of definitions, the file has {}", bytes.len() - start));
    }
    for e in &mut entries {
        e.offset += start;
    }
    Ok(Header { module, entries })
}

impl Header {
    /// One definition, decoded; its module must be the file's.
    pub fn definition(&self, bytes: &[u8], e: &Entry) -> Result<Decl, String> {
        let d = decode_definition(&bytes[e.offset..e.offset + e.size]).map_err(|err| format!("{err} (in `{}`)", e.name))?;
        if d.module != self.module {
            return Err(format!("corrupt binary Strata file: `{}` is in module `{}`, the file is module `{}`", e.name, d.module, self.module));
        }
        Ok(d)
    }

    /// Only a definition's interface (bodies are `()`).
    #[allow(dead_code)] // for checking against a module's interfaces alone (not yet)
    pub fn interface(&self, bytes: &[u8], e: &Entry) -> Result<Decl, String> {
        decode_interface(&bytes[e.offset..e.offset + e.interface_size]).map_err(|err| format!("{err} (in `{}`)", e.name))
    }
}

/// Every definition of a module file. Fails if a definition doesn't match
/// what the header says of it (its name, kind, visibility or hash).
pub fn decode_module(bytes: &[u8]) -> Result<Vec<Decl>, String> {
    let h = read_header(bytes)?;
    h.entries
        .iter()
        .map(|e| {
            let d = h.definition(bytes, e)?;
            let actual: Hash = Sha256::digest(&bytes[e.offset..e.offset + e.size]).into();
            if d.name() != e.name || d.kind_str() != e.kind || d.public != e.public || actual != e.hash {
                return Err(format!("corrupt binary Strata file: `{}` doesn't match its header entry", e.name));
            }
            Ok(d)
        })
        .collect()
}

const KINDS: [&str; 6] = ["type", "fn", "test", "trait", "impl", "use"];

fn kind_tag(k: &DeclKind) -> u8 {
    match k {
        DeclKind::Type(_) => 0,
        DeclKind::Fn(_) => 1,
        DeclKind::Test(_) => 2,
        DeclKind::Trait(_) => 3,
        DeclKind::Impl(_) => 4,
        DeclKind::Use(_) => 5,
    }
}

// ── bundles ─────────────────────────────────────────────────────────────────

/// A whole program in one file: its `strata.toml` and its module files.
pub fn encode_bundle(img: &Image) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    uint(&mut out, VERSION);
    out.push(BUNDLE);
    match &img.manifest {
        None => out.push(0),
        Some(m) => {
            out.push(1);
            raw(&mut out, m.as_bytes());
        }
    }
    let modules = encode_modules(&img.program);
    uint(&mut out, modules.len() as u64);
    for (_, bytes) in &modules {
        raw(&mut out, bytes);
    }
    out
}

/// A bundle or a module file (`manifest` is `None` for a module file).
pub fn read(bytes: &[u8]) -> Result<Image, String> {
    let mut r = Reader::new(bytes);
    if r.preamble()? == MODULE {
        return Ok(Image { manifest: None, program: Program { decls: decode_module(bytes)? } });
    }
    let manifest = match r.tag("option", 1)? {
        0 => None,
        _ => {
            let n = r.len()?;
            Some(std::str::from_utf8(r.take(n)?).map(str::to_string).map_err(|_| "corrupt binary Strata file: a manifest that isn't UTF-8".to_string())?)
        }
    };
    let mut decls = Vec::new();
    for _ in 0..r.len()? {
        let n = r.len()?;
        let at = r.pos;
        decls.extend(decode_module(r.take(n)?).map_err(|e| format!("{e} (in the module file at byte {at})"))?);
    }
    r.end()?;
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

/// Bytes with their length in front.
fn raw(out: &mut Vec<u8>, bytes: &[u8]) {
    uint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

#[derive(Default)]
struct Writer {
    body: Vec<u8>,
    strings: Vec<String>,
    index: HashMap<String, u64>,
    /// The same, for names: found by number rather than by hashing their text.
    symbols: HashMap<Symbol, u64>,
}

impl Writer {
    /// The strings first used since string `from` (a table, or the part of
    /// one a later part adds), then what was written since the last flush.
    fn flush(&mut self, from: usize) -> Vec<u8> {
        let mut out = Vec::new();
        uint(&mut out, (self.strings.len() - from) as u64);
        for s in &self.strings[from..] {
            raw(&mut out, s.as_bytes());
        }
        out.append(&mut self.body);
        out
    }
    fn uint(&mut self, v: u64) {
        uint(&mut self.body, v)
    }
    fn byte(&mut self, b: u8) {
        self.body.push(b)
    }
    fn str(&mut self, s: &str) {
        let i = self.index_of(s);
        self.uint(i)
    }
    fn index_of(&mut self, s: &str) -> u64 {
        match self.index.get(s) {
            Some(&i) => i,
            None => {
                let i = self.strings.len() as u64;
                self.strings.push(s.to_string());
                self.index.insert(s.to_string(), i);
                i
            }
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
    strings: Vec<String>,
    /// Each string of the table as a symbol, once it has been read as one.
    symbols: Vec<Option<Symbol>>,
    depth: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Reader<'a> {
        Reader { bytes, pos: 0, strings: Vec::new(), symbols: Vec::new(), depth: 0 }
    }
    /// `STRATA`, the version, and the kind of file.
    fn preamble(&mut self) -> Result<u8, String> {
        if !is_binary(self.bytes) {
            return Err("not a binary Strata file (it doesn't start with `STRATA`)".into());
        }
        self.pos = MAGIC.len();
        let v = self.uint()?;
        if v != VERSION {
            return Err(format!("binary format version {v}, but this strata reads version {VERSION}: render it with the strata that wrote it, and read the text back in with `strata edit`"));
        }
        self.tag("file kind", BUNDLE)
    }
    /// A string table (or the strings a later part adds to it).
    fn strings(&mut self) -> Result<(), String> {
        for _ in 0..self.len()? {
            let len = self.len()?;
            let s = std::str::from_utf8(self.take(len)?).map(str::to_string);
            let s = s.map_err(|_| self.err("a string that isn't UTF-8"))?;
            self.strings.push(s);
            self.symbols.push(None);
        }
        Ok(())
    }
    /// A part: the strings it adds, then a value.
    fn part<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, String>) -> Result<T, String> {
        self.strings()?;
        f(self)
    }
    fn end(&self) -> Result<(), String> {
        if self.pos != self.bytes.len() {
            return Err(self.err(&format!("{} bytes left over", self.bytes.len() - self.pos)));
        }
        Ok(())
    }
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

/// A name: a string of the table, interned once per file however often it is used.
impl Bin for Symbol {
    fn enc(&self, w: &mut Writer) {
        let i = match w.symbols.get(self) {
            Some(&i) => i,
            None => {
                let i = w.index_of(self.as_str());
                w.symbols.insert(*self, i);
                i
            }
        };
        w.uint(i)
    }
    fn dec(r: &mut Reader) -> Result<Self, String> {
        let i = r.uint()? as usize;
        match r.symbols.get(i) {
            Some(Some(s)) => Ok(*s),
            Some(None) => {
                let s = Symbol::new(&r.strings[i]);
                r.symbols[i] = Some(s);
                Ok(s)
            }
            None => Err(r.err(&format!("string {i}, not in the table"))),
        }
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
record!(Field { comments, doc, name, ty, default });
record!(Variant { comments, name, payload });
record!(Param { name, ty, mutable, comptime });
record!(Bound { param, traits });
record!(FnDecl { doc, comments, name, type_params, bounds, params, ret, requires, ensures, terminates, body, comptime, derived });
record!(TestDecl { name, body });
record!(TraitMethod { comments, doc, decl, default });
record!(Law { comments, doc, name, params, body });
record!(TraitDecl { name, params, supers, methods, laws, derivable });
record!(Derivable { kind, requires });
record!(ImplDecl { type_params, bounds, trait_name, trait_args, for_type, methods, derived });
record!(UseDecl { path, items });
record!(Arm { comments, pat, body });
record!(LamParam { name, ty });
record!(CompFor { pat, iter, cond });
record!(FmtSpec { fill, align, plus, zero, width, precision });

tags!(BinOp { Add = 0, Sub = 1, Mul = 2, Div = 3, Mod = 4, Eq = 5, Ne = 6, Lt = 7, Le = 8, Gt = 9, Ge = 10, And = 11, Or = 12, Implies = 13 });
tags!(UnOp { Neg = 0, Not = 1 });
tags!(TryKind { Result = 0, Option = 1 });
tags!(CompKind { List = 0, Set = 1, Dict = 2 });

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
            Pattern::For(p, v, xs) => put!(w, 10, p, v, xs),
        }
    }
    fn dec(r: &mut Reader) -> Result<Self, String> {
        r.nest(|r| {
            Ok(match r.tag("pattern", 10)? {
                0 => Pattern::Wild,
                1 => Pattern::Bind(Bin::dec(r)?),
                2 => Pattern::Int(Bin::dec(r)?),
                3 => Pattern::Str(Bin::dec(r)?),
                4 => Pattern::Bool(Bin::dec(r)?),
                5 => Pattern::Ctor(Bin::dec(r)?, Bin::dec(r)?),
                6 => Pattern::List(Bin::dec(r)?, Bin::dec(r)?),
                7 => Pattern::Tuple(Bin::dec(r)?),
                8 => Pattern::Cond(Bin::dec(r)?),
                9 => Pattern::Or(Bin::dec(r)?),
                _ => Pattern::For(Bin::dec(r)?, Bin::dec(r)?, Bin::dec(r)?),
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
            Stmt::ComptimeFor(p, xs, b) => put!(w, 11, p, xs, b),
        }
    }
    fn dec(r: &mut Reader) -> Result<Self, String> {
        Ok(match r.tag("statement", 11)? {
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
            10 => Stmt::Comment(Bin::dec(r)?),
            _ => Stmt::ComptimeFor(Bin::dec(r)?, Bin::dec(r)?, Bin::dec(r)?),
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
            Expr::Range(lo, hi, inclusive) => put!(w, 27, lo, hi, inclusive),
            Expr::Index(x, r) => put!(w, 28, x, r),
            Expr::Comp(kind, elem, gens) => put!(w, 29, kind, elem, gens),
            Expr::Dict(tys, kvs) => put!(w, 30, tys, kvs),
            Expr::Set(tys, xs) => put!(w, 31, tys, xs),
            Expr::Or(x, fb) => put!(w, 32, x, fb),
            Expr::Comptime(x) => put!(w, 33, x),
            Expr::Describe(t) => put!(w, 34, t),
        }
    }
    fn dec(r: &mut Reader) -> Result<Self, String> {
        r.nest(|r| {
            Ok(match r.tag("expression", 34)? {
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
                26 => Expr::Interp(Bin::dec(r)?),
                27 => Expr::Range(Bin::dec(r)?, Bin::dec(r)?, Bin::dec(r)?),
                28 => Expr::Index(Bin::dec(r)?, Bin::dec(r)?),
                29 => Expr::Comp(Bin::dec(r)?, Bin::dec(r)?, Bin::dec(r)?),
                30 => Expr::Dict(Bin::dec(r)?, Bin::dec(r)?),
                31 => Expr::Set(Bin::dec(r)?, Bin::dec(r)?),
                32 => Expr::Or(Bin::dec(r)?, Bin::dec(r)?),
                33 => Expr::Comptime(Bin::dec(r)?),
                _ => Expr::Describe(Bin::dec(r)?),
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

    fn examples() -> Vec<(std::path::PathBuf, Image)> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut paths: Vec<_> = std::fs::read_dir(root.join("examples")).unwrap().map(|e| e.unwrap().path()).collect();
        paths.push(root.join("std"));
        paths.sort();
        paths
            .into_iter()
            .map(|path| {
                let program = crate::package::program(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                let manifest = std::fs::read_to_string(path.join(crate::package::FILE)).ok();
                (path, Image { manifest, program })
            })
            .collect()
    }

    #[test]
    fn integers() {
        for v in [0, 1, -1, 63, -64, 64, 1 << 40, i64::MAX, i64::MIN] {
            let mut w = Writer::default();
            v.enc(&mut w);
            let mut r = Reader::new(&w.body);
            assert_eq!(i64::dec(&mut r).unwrap(), v);
            assert_eq!(r.pos, w.body.len());
        }
        let mut out = Vec::new();
        uint(&mut out, 300);
        assert_eq!(out, [0xac, 0x02]);
    }

    #[test]
    fn every_example_round_trips() {
        for (path, img) in examples() {
            let bytes = encode_bundle(&img);
            let back = read(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            // the program is loaded as stored, and read back the same
            let stored = img.program.clone();
            assert_eq!(back, img, "{}", path.display());
            // canonical: encoding the decoded program gives the same bytes
            assert_eq!(encode_bundle(&back), bytes);
            for (m, file) in encode_modules(&img.program) {
                let own: Vec<Decl> = stored.decls.iter().filter(|d| d.module == m).cloned().collect();
                assert_eq!(decode_module(&file).unwrap(), own);
            }
        }
    }

    #[test]
    fn the_header_describes_each_definition() {
        let (_, img) = examples().into_iter().find(|(p, _)| p.ends_with("tasks")).unwrap();
        let stored = crate::resolve::stored_form(&img.program);
        let graph = crate::store::graph(&stored);
        for (m, file) in encode_modules(&img.program) {
            let h = read_header(&file).unwrap();
            assert_eq!(h.module, m);
            let own: Vec<&Decl> = stored.decls.iter().filter(|d| d.module == m).collect();
            assert_eq!(h.entries.len(), own.len());
            for (e, d) in h.entries.iter().zip(own) {
                assert_eq!((e.name.as_str(), e.kind, e.public), (d.name().as_str(), d.kind_str(), d.public));
                assert_eq!((e.hash, e.interface_hash, e.code_hash), (hash(d), interface_hash(d), code_hash(d)));
                assert_eq!(e.deps, graph.get(&d.qname()).cloned().unwrap_or_default().into_iter().collect::<Vec<_>>());
                // a definition is its bytes, wherever they are
                let bytes = &file[e.offset..e.offset + e.size];
                assert_eq!(bytes, encode_definition(d));
                assert_eq!(&h.definition(&file, e).unwrap(), d);
                // the interface alone: bodies left out
                let mut iface = d.clone();
                take_bodies(&mut iface.kind);
                assert_eq!(h.interface(&file, e).unwrap(), iface);
            }
        }
    }

    #[test]
    fn what_each_hash_covers() {
        let parse = |s: &str| crate::parser::parse_module_file(s, "m").unwrap().decls.remove(0);
        let base = parse("fn f(x: Int) -> Int requires x > 0 { x + 1 }");
        let commented = parse("/// doc\nfn f(x: Int) -> Int requires x > 0 {\n  // note\n  x + 1\n}");
        let body = parse("fn f(x: Int) -> Int requires x > 0 { x + 2 }");
        let contract = parse("fn f(x: Int) -> Int requires x > 1 { x + 1 }");
        let public = parse("pub fn f(x: Int) -> Int requires x > 0 { x + 1 }");
        assert_ne!(hash(&base), hash(&commented));
        assert_eq!(code_hash(&base), code_hash(&commented));
        assert_eq!(interface_hash(&base), interface_hash(&commented));
        assert_ne!(code_hash(&base), code_hash(&body));
        assert_eq!(interface_hash(&base), interface_hash(&body));
        assert_ne!(interface_hash(&base), interface_hash(&contract));
        assert_ne!(interface_hash(&base), interface_hash(&public));
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
  (a, _) := (1, 2.5)
  mut v: Int = -3
  v = v + 99999999999999999999999
  while v < 0 { v = v + 1 }
  for [h, ..t] in [[1]] { break }
  loop { continue }
  assert !true ==> false
  // c
  g := (y: Int) -> y % 2
  z := match S.A as s { A | B(_, _) => 1, C { x: 3, .. } => 2, B("s", true) => 4, where it is C => 3, _ => ?hole }
  o := Some(v)? + Ok(1)! + (Err(2) except e { e })
  r := R { a: 1, b: g } ; r2 := R { ..r, a: 2 }
  print(f"{x:>08.2} and {r.a}")
  mock Tr
  if "s".len() == 1 { return 1 } else { f(mut v) }
}
test "t" { assert f(1) == 1 }
use a.b.{c, d}
use e.*
"#;
        // not every line has to check: only parse, then round-trip the tree
        let program = crate::parser::parse_module_file(src, "m").unwrap_or_else(|e| panic!("{e}"));
        for d in &program.decls {
            assert_eq!(&decode_definition(&encode_definition(d)).unwrap(), d);
        }
        let stored = crate::resolve::stored_form(&program);
        let img = Image { manifest: None, program };
        assert_eq!(read(&encode_bundle(&img)).unwrap().program, stored);
    }

    #[test]
    fn bad_input_is_an_error_not_a_crash() {
        let (_, img) = examples().into_iter().find(|(p, _)| p.ends_with("orders.strata")).unwrap();
        let bytes = encode_bundle(&img);
        for n in 0..bytes.len() {
            assert!(read(&bytes[..n]).is_err(), "a file cut at {n} bytes decoded");
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(read(&extra).is_err());
        let mut newer = bytes.clone();
        newer[MAGIC.len()] = VERSION as u8 + 1;
        assert!(read(&newer).unwrap_err().contains("version"));
        // flipping any byte gives an error, never a panic: the header's hash
        // catches a change inside a definition that still decodes
        for i in MAGIC.len()..bytes.len() {
            let mut b = bytes.clone();
            b[i] ^= 0xff;
            if let Ok(other) = read(&b) {
                assert_eq!(other.program, img.program, "byte {i}");
            }
        }
    }

    #[test]
    fn deep_nesting_is_refused() {
        std::thread::Builder::new()
            .stack_size(1 << 30)
            .spawn(|| {
                let mut e = Expr::Unit;
                for _ in 0..2 * MAX_DEPTH {
                    e = Expr::Mut(Box::new(e));
                }
                let d = Decl { comments: vec![], doc: vec![], module: Symbol::EMPTY, public: false, kind: DeclKind::Test(TestDecl { name: "t".into(), body: e }) };
                assert!(decode_definition(&encode_definition(&d)).unwrap_err().contains("nesting too deep"));
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
