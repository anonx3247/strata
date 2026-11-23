//! Sized integers (`I8` … `U64`, core's `builtin type`s): machine integers
//! of a fixed width, each a type of its own. An operation on them is `Int`'s,
//! then the result modulo 2^bits (two's complement for the signed ones);
//! they don't mix with `Int` or with each other.

use super::Ty;

/// A sized integer type: its width in bits, and whether it is signed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Width {
    pub bits: u32,
    pub signed: bool,
}

/// The sized integer types, by name.
pub const NAMES: [&str; 8] = ["I8", "I16", "I32", "I64", "U8", "U16", "U32", "U64"];

impl Width {
    /// The sized type named `n` (`U32`), if it is one.
    pub fn of(n: &str) -> Option<Width> {
        let (signed, bits) = match n.as_bytes().first()? {
            b'I' => (true, &n[1..]),
            b'U' => (false, &n[1..]),
            _ => return None,
        };
        match bits {
            "8" | "16" | "32" | "64" => Some(Width { bits: bits.parse().ok()?, signed }),
            _ => None,
        }
    }

    /// The sized type a checker type is, if it is one.
    pub fn of_ty(t: &Ty) -> Option<Width> {
        match t {
            Ty::Con(n, a) if a.is_empty() => Width::of(n.as_str()),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        NAMES[(self.bits.trailing_zeros() - 3) as usize + if self.signed { 0 } else { 4 }]
    }

    pub fn min(self) -> i128 {
        if self.signed { -(1i128 << (self.bits - 1)) } else { 0 }
    }

    pub fn max(self) -> i128 {
        if self.signed { (1i128 << (self.bits - 1)) - 1 } else { (1i128 << self.bits) - 1 }
    }

    pub fn holds(self, n: i128) -> bool {
        self.min() <= n && n <= self.max()
    }

    /// Its range as people read it (`0 ..= 255`).
    pub fn range(self) -> String {
        format!("{} ..= {}", self.min(), self.max())
    }

    /// The conversion that checks the range (`to_u32`) and the one that wraps (`wrap_u32`).
    pub fn to_fn(self) -> String {
        format!("to_{}", self.name().to_lowercase())
    }

    pub fn wrap_fn(self) -> String {
        format!("wrap_{}", self.name().to_lowercase())
    }

    /// `n` wrapped into the range (two's complement for a signed type).
    pub fn wrap(self, n: i128) -> i128 {
        let m = n.rem_euclid(1i128 << self.bits);
        if self.signed && m > self.max() { m - (1i128 << self.bits) } else { m }
    }
}

/// A conversion builtin's sized type: `to_u32` and `wrap_u32` give a `U32`.
pub fn conversion(f: &str) -> Option<(Width, bool)> {
    let (wraps, rest) = match f.strip_prefix("to_") {
        Some(r) => (false, r),
        None => (true, f.strip_prefix("wrap_")?),
    };
    let w = Width::of(&rest.to_uppercase())?;
    (rest.chars().all(|c| !c.is_uppercase())).then_some((w, wraps))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths() {
        let u8 = Width::of("U8").unwrap();
        assert_eq!((u8.min(), u8.max(), u8.name()), (0, 255, "U8"));
        let i64 = Width::of("I64").unwrap();
        assert_eq!((i64.min(), i64.max(), i64.name()), (i64::MIN as i128, i64::MAX as i128, "I64"));
        assert_eq!(Width::of("U7"), None);
        assert_eq!(Width::of("Int"), None);
        assert_eq!(Width::of("I8").unwrap().wrap(200), -56);
        assert_eq!(Width::of("U32").unwrap().wrap(-1), 4294967295);
        assert_eq!(conversion("wrap_u32"), Some((Width::of("U32").unwrap(), true)));
        assert_eq!(conversion("to_i8"), Some((Width::of("I8").unwrap(), false)));
        assert_eq!(conversion("to_int"), None);
    }
}
