#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Int(i64),
    Float(f64),
    Str(String),
    Ident(String),
    /// `?name`
    Hole(String),
    Doc(String),
    Sym(&'static str),
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub line: usize,
    pub col: usize,
    /// true if a newline separates this token from the previous one.
    /// Used so `f\n(x)` is two statements rather than a call.
    pub nl_before: bool,
}

const SYMS: &[&str] = &[
    "==>", "...", "..", "->", "=>", "==", "!=", "<=", ">=", "&&", "||", "++", "(", ")", "{", "}",
    "[", "]", ",", ":", ";", ".", "=", "<", ">", "+", "-", "*", "/", "%", "!", "|",
];

pub fn lex(src: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let (mut line, mut col) = (1usize, 1usize);
    let mut out = Vec::new();
    let mut nl = true;
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            nl = true;
            i += 1;
            line += 1;
            col = 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            col += 1;
            continue;
        }
        let (sl, sc) = (line, col);
        let mut push = |tok: Tok, nl: &mut bool| {
            out.push(Token { tok, line: sl, col: sc, nl_before: *nl });
            *nl = false;
        };
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            let start = i;
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            if let Some(doc) = text.strip_prefix("///") {
                push(Tok::Doc(doc.trim().to_string()), &mut nl);
            }
            col += i - start;
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
                i += 1;
            }
            // a fraction makes it a Float: `1.5` (but not `xs.0.1` or `1..n`)
            let after_dot = i + 1 < chars.len() && chars[i + 1].is_ascii_digit();
            let prev_dot = start > 0 && chars[start - 1] == '.';
            if i < chars.len() && chars[i] == '.' && after_dot && !prev_dot {
                i += 1;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
                    i += 1;
                }
                let s: String = chars[start..i].iter().filter(|c| **c != '_').collect();
                let f = s.parse::<f64>().map_err(|e| format!("{line}:{col}: bad float: {e}"))?;
                col += i - start;
                push(Tok::Float(f), &mut nl);
                continue;
            }
            let s: String = chars[start..i].iter().filter(|c| **c != '_').collect();
            let n = s.parse::<i64>().map_err(|e| format!("{line}:{col}: bad int: {e}"))?;
            col += i - start;
            push(Tok::Int(n), &mut nl);
            continue;
        }
        if c.is_alphabetic() || c == '_' || (c == '?' && i + 1 < chars.len() && chars[i + 1].is_alphabetic()) {
            let hole = c == '?';
            let start = if hole { i + 1 } else { i };
            i = start;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let s: String = chars[start..i].iter().collect();
            col += i - start + hole as usize;
            push(if hole { Tok::Hole(s) } else { Tok::Ident(s) }, &mut nl);
            continue;
        }
        if c == '"' {
            i += 1;
            let mut s = String::new();
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 1;
                    match chars[i] {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        '"' => s.push('"'),
                        '\\' => s.push('\\'),
                        // keep regex escapes like \d intact
                        other => {
                            s.push('\\');
                            s.push(other);
                        }
                    }
                } else {
                    if chars[i] == '\n' {
                        line += 1;
                    }
                    s.push(chars[i]);
                }
                i += 1;
            }
            if i >= chars.len() {
                return Err(format!("{sl}:{sc}: unterminated string"));
            }
            i += 1;
            col += 2 + s.len();
            push(Tok::Str(s), &mut nl);
            continue;
        }
        let rest: String = chars[i..chars.len().min(i + 3)].iter().collect();
        if let Some(sym) = SYMS.iter().find(|s| rest.starts_with(*s)) {
            i += sym.len();
            col += sym.len();
            push(Tok::Sym(sym), &mut nl);
            continue;
        }
        return Err(format!("{line}:{col}: unexpected character {c:?}"));
    }
    out.push(Token { tok: Tok::Eof, line, col, nl_before: true });
    Ok(out)
}
