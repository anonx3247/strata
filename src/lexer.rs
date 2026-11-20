#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Int(i64),
    /// An integer literal too large for 64 bits, as written (digits only).
    BigInt(String),
    Float(f64),
    Str(String),
    /// `f"..."`: literal text and the source of each `{expr}`, with the
    /// line and column where it starts.
    FStr(Vec<FPart>),
    Ident(crate::symbol::Symbol),
    /// `?name`
    Hole(crate::symbol::Symbol),
    Doc(String),
    Sym(&'static str),
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FPart {
    Lit(String),
    Code(String, usize, usize),
}

/// The character an escape like `\n` stands for (regex escapes like `\d` are kept).
fn escape(c: char, s: &mut String) {
    match c {
        'n' => s.push('\n'),
        't' => s.push('\t'),
        '"' => s.push('"'),
        '\\' => s.push('\\'),
        other => {
            s.push('\\');
            s.push(other);
        }
    }
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
    "[", "]", ",", ":", ";", ".", "=", "<", ">", "+", "-", "*", "/", "%", "!", "|", "?",
];

/// A `//` comment. The parser attaches it to the definition, field, variant,
/// method, statement or match arm it belongs to.
#[derive(Debug, Clone)]
pub struct Comment {
    pub line: usize,
    pub col: usize,
    /// The comment as written, starting with `//`.
    pub text: String,
    /// It follows code on the same line (`x = 1 // why`).
    pub trailing: bool,
}

pub fn lex_with_comments(src: &str) -> Result<(Vec<Token>, Vec<Comment>), String> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let (mut line, mut col) = (1usize, 1usize);
    let mut out = Vec::new();
    let mut comments = Vec::new();
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
            } else {
                comments.push(Comment { line: sl, col: sc, text: text.trim_end().to_string(), trailing: !nl });
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
            col += i - start;
            match s.parse::<i64>() {
                Ok(n) => push(Tok::Int(n), &mut nl),
                Err(_) => push(Tok::BigInt(s), &mut nl),
            }
            continue;
        }
        if c == 'f' && chars.get(i + 1) == Some(&'"') {
            i += 2;
            col += 2;
            let mut parts = Vec::new();
            let mut lit = String::new();
            loop {
                let Some(&ch) = chars.get(i) else { return Err(format!("{sl}:{sc}: unterminated f-string")) };
                match ch {
                    '"' => {
                        i += 1;
                        col += 1;
                        break;
                    }
                    '\\' if i + 1 < chars.len() => {
                        escape(chars[i + 1], &mut lit);
                        i += 2;
                        col += 2;
                    }
                    '{' if chars.get(i + 1) == Some(&'{') => {
                        lit.push('{');
                        i += 2;
                        col += 2;
                    }
                    '}' if chars.get(i + 1) == Some(&'}') => {
                        lit.push('}');
                        i += 2;
                        col += 2;
                    }
                    '}' => return Err(format!("{line}:{col}: a lone `}}` in an f-string: write `}}}}`")),
                    '{' => {
                        if !lit.is_empty() {
                            parts.push(FPart::Lit(std::mem::take(&mut lit)));
                        }
                        i += 1;
                        col += 1;
                        let (cl, cc) = (line, col);
                        let start = i;
                        let mut depth = 0;
                        // find the matching `}`, skipping over nested strings
                        loop {
                            let Some(&k) = chars.get(i) else { return Err(format!("{cl}:{cc}: unterminated `{{` in an f-string")) };
                            match k {
                                '{' | '(' | '[' => depth += 1,
                                ')' | ']' => depth -= 1,
                                '}' if depth == 0 => break,
                                '}' => depth -= 1,
                                '"' => {
                                    i += 1;
                                    col += 1;
                                    while i < chars.len() && chars[i] != '"' {
                                        if chars[i] == '\\' {
                                            i += 1;
                                            col += 1;
                                        }
                                        i += 1;
                                        col += 1;
                                    }
                                }
                                '\n' => {
                                    line += 1;
                                    col = 0;
                                }
                                _ => {}
                            }
                            i += 1;
                            col += 1;
                        }
                        let code: String = chars[start..i].iter().collect();
                        if code.trim().is_empty() {
                            return Err(format!("{cl}:{cc}: empty `{{}}` in an f-string"));
                        }
                        parts.push(FPart::Code(code, cl, cc));
                        i += 1;
                        col += 1;
                    }
                    _ => {
                        if ch == '\n' {
                            line += 1;
                            col = 0;
                        }
                        lit.push(ch);
                        i += 1;
                        col += 1;
                    }
                }
            }
            if !lit.is_empty() {
                parts.push(FPart::Lit(lit));
            }
            push(Tok::FStr(parts), &mut nl);
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
            let s = crate::symbol::Symbol::new(&s);
            push(if hole { Tok::Hole(s) } else { Tok::Ident(s) }, &mut nl);
            continue;
        }
        if c == '"' {
            i += 1;
            let mut s = String::new();
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 1;
                    escape(chars[i], &mut s);
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
    Ok((out, comments))
}
