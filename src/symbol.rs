//! Interned names. Every identifier in the syntax tree (a variable, a
//! function, a type, a field, a module path) is a `Symbol`: a number standing
//! for its text in one table shared by the whole process. Two symbols are
//! equal exactly when their texts are, so comparing and hashing names is
//! comparing and hashing a `u32`, and copying a name copies four bytes.
//!
//! The text is there when needed (`as_str`, `Deref`, `Display`). Symbols are
//! ordered by their text, not their number, so sorted output doesn't depend
//! on the order names happened to be interned in.
//!
//! The table only grows: a name, once seen, stays for the life of the
//! process (a program's names are few).

use crate::fx::HashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Symbol(u32);

/// Names known to every program get fixed numbers, interned first (in this order).
const KNOWN: &[&str] = &["", "self", "Self", "it", "Int", "Float", "Bool", "String", "List", "Dict", "Set", "Option", "Some", "None", "Result", "Ok", "Err"];

/// The texts, by number, in chunks that never move once allocated: reading a
/// symbol's text takes no lock (two atomic loads). Chunk `k` holds `CHUNK << k`
/// names, so a few chunks cover any program.
const CHUNK: usize = 1024;
const CHUNKS: usize = 20;
static TEXTS: [OnceLock<Box<[OnceLock<&'static str>]>>; CHUNKS] = [const { OnceLock::new() }; CHUNKS];

/// Text to number, for interning; taken only to add or look up a name by text.
static IDS: OnceLock<Mutex<HashMap<&'static str, u32>>> = OnceLock::new();

/// Where name `i` lives: (chunk, index in it).
fn slot(i: usize) -> (usize, usize) {
    // chunk k starts at CHUNK * (2^k - 1)
    let k = (i / CHUNK + 1).ilog2() as usize;
    (k, i - CHUNK * ((1 << k) - 1))
}

fn ids() -> &'static Mutex<HashMap<&'static str, u32>> {
    IDS.get_or_init(|| {
        let mut ids = HashMap::default();
        for (i, k) in KNOWN.iter().enumerate() {
            store(i, k);
            ids.insert(*k, i as u32);
        }
        Mutex::new(ids)
    })
}

fn store(i: usize, s: &'static str) {
    let (k, j) = slot(i);
    let chunk = TEXTS[k].get_or_init(|| (0..CHUNK << k).map(|_| OnceLock::new()).collect());
    chunk[j].set(s).expect("a symbol's text is set once");
}

impl Symbol {
    pub const EMPTY: Symbol = Symbol(0);

    pub fn new(s: &str) -> Symbol {
        let mut ids = ids().lock().unwrap();
        if let Some(&i) = ids.get(s) {
            return Symbol(i);
        }
        let i = ids.len();
        assert!(slot(i).0 < CHUNKS, "too many distinct names");
        let s: &'static str = Box::leak(s.to_string().into_boxed_str());
        store(i, s);
        ids.insert(s, i as u32);
        Symbol(i as u32)
    }

    pub fn as_str(self) -> &'static str {
        if self.0 == 0 {
            return "";
        }
        let (k, j) = slot(self.0 as usize);
        TEXTS[k].get().and_then(|c| c[j].get()).expect("a symbol made by `Symbol::new`")
    }
}

/// `x.sym()`: `x` as a symbol, free when it already is one.
pub trait Sym {
    fn sym(&self) -> Symbol;
}

impl Sym for Symbol {
    fn sym(&self) -> Symbol {
        *self
    }
}

impl Sym for str {
    fn sym(&self) -> Symbol {
        Symbol::new(self)
    }
}

impl Default for Symbol {
    fn default() -> Symbol {
        Symbol::EMPTY
    }
}

impl std::ops::Deref for Symbol {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for Symbol {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Display for Symbol {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::fmt::Debug for Symbol {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

impl PartialOrd for Symbol {
    fn partial_cmp(&self, other: &Symbol) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Symbol {
    fn cmp(&self, other: &Symbol) -> std::cmp::Ordering {
        if self.0 == other.0 { std::cmp::Ordering::Equal } else { self.as_str().cmp(other.as_str()) }
    }
}

impl From<&str> for Symbol {
    fn from(s: &str) -> Symbol {
        Symbol::new(s)
    }
}

impl From<String> for Symbol {
    fn from(s: String) -> Symbol {
        Symbol::new(&s)
    }
}

impl From<&String> for Symbol {
    fn from(s: &String) -> Symbol {
        Symbol::new(s)
    }
}

impl From<Symbol> for String {
    fn from(s: Symbol) -> String {
        s.as_str().to_string()
    }
}

macro_rules! eq_text {
    ($($t:ty),*) => {$(
        impl PartialEq<$t> for Symbol {
            fn eq(&self, other: &$t) -> bool {
                self.as_str() == AsRef::<str>::as_ref(other)
            }
        }
        impl PartialEq<Symbol> for $t {
            fn eq(&self, other: &Symbol) -> bool {
                AsRef::<str>::as_ref(self) == other.as_str()
            }
        }
    )*};
}
eq_text!(str, &str, String);

/// Names joined by `sep`, like `[String]::join`.
pub fn join(names: &[Symbol], sep: &str) -> String {
    names.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(sep)
}

impl serde::Serialize for Symbol {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for Symbol {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Symbol, D::Error> {
        String::deserialize(d).map(Symbol::from)
    }
}

#[cfg(test)]
mod tests {
    use super::Symbol;

    #[test]
    fn same_text_same_symbol() {
        let a = Symbol::new("orders.Line");
        assert_eq!(a, Symbol::from("orders.Line".to_string()));
        assert_ne!(a, Symbol::new("orders.line"));
        assert_eq!(a, "orders.Line");
        assert_eq!(a.as_str(), "orders.Line");
        assert_eq!(Symbol::new("Int"), Symbol::new("Int"));
        assert_eq!(Symbol::default().as_str(), "");
        // ordered by text, whatever order they were interned in
        let (z, b) = (Symbol::new("zzz-interned-first"), Symbol::new("bbb-interned-second"));
        assert!(b < z);
    }
}
