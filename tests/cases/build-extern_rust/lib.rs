use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Plain {
    pub x: i64,
}

#[derive(Serialize)]
pub struct Wrong {
    pub x: String,
}

pub fn inc(x: i64) -> i64 {
    x + 1
}

pub fn plain(n: i64) -> Plain {
    Plain { x: n }
}

pub fn double(p: Plain) -> Plain {
    Plain { x: p.x * 2 }
}

pub fn wrong() -> Wrong {
    Wrong { x: "not a number".into() }
}

pub fn negative() -> i64 {
    -5
}

pub fn big() -> i64 {
    1 << 60
}

pub fn noisy() {
    eprintln!("printed by Rust");
}

pub fn parsed(s: String) -> Result<i64, String> {
    s.parse::<i64>().map_err(|e| format!("{s:?}: {e}"))
}

pub fn panics(x: i64) -> i64 {
    if x > 0 {
        panic!("x is {x}");
    }
    x
}

pub fn takes_int(x: i64) -> i64 {
    x
}

pub mod words {
    pub fn count(s: String) -> usize {
        s.split_whitespace().count()
    }
}

pub fn huge(x: i64) -> i64 {
    x
}
