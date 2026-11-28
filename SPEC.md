# The Strata language

This document specifies the Strata language as the rebuild implements it. Each section states the rules exactly; nothing is left to convention. Where a rule is a law, it is written as a law.

Notation. Grammars are EBNF: `=` defines, `|` separates choices, `[ x ]` is optional, `{ x }` repeats zero or more times, `"x"` is literal text, `'a'..'z'` is a range of characters, and `-` removes a set (`any - "\""`). A rule is followed by an example.

## 1. Lexical structure

The lexer turns a module's text into a list of tokens. The parser (section 2) reads the tokens. Tokens are never stored: the stored form of a module is its tree (section 3), and text exists only to be read and edited.

### 1.1 Source text

- A source text is a sequence of bytes that must be valid UTF-8. Invalid UTF-8 is an error at the first byte that is not part of a valid sequence.
- A byte order mark (U+FEFF) at the very start is skipped. Anywhere else it is an error.
- Each CR LF pair is read as one LF. A CR not followed by LF is an error.
- Outside string literals and comments, the only characters allowed are printable ASCII (U+0020 to U+007E), tab (U+0009) and LF (U+000A). Any other character there is an error.
- Inside comments, any character but LF is allowed. Inside string literals, section 1.6 says which characters are allowed.
- A position is a byte offset into the text. Messages show it as `line:column`, both counted from 1, the column in code points.

### 1.2 Tokens

```ebnf
token     = identifier | keyword | wildcard | hole
          | integer | decimal | string | raw_string | f_string
          | module_doc | doc_comment | comment
          | punctuation | newline ;
space     = " " | "\t" ;
```

- The lexer reads from left to right. At each point it skips spaces and tabs, then reads the longest token that starts there.
- Each token records its kind, its value, its position, and `space_before`: true when a space, a tab, an LF or a comment comes directly before it, or when it is the first token.
- After the last token comes `end`.
- The lexer is total. It returns the whole list of tokens, or the first error with its position. It never skips a character it cannot read.

### 1.3 Identifiers and keywords

```ebnf
identifier = ( letter | "_" ) { letter | digit | "_" } - keyword - "_" ;
letter     = 'a'..'z' | 'A'..'Z' ;
digit      = '0'..'9' ;
wildcard   = "_" ;
```

Example: `total`, `Point`, `_x`, `s12`, `u3`.

- Identifiers are ASCII only. Case has no lexical meaning; section 5 states the naming conventions.
- `_` alone is the wildcard token, not an identifier. A leading `_` is part of a name: `_x` is an identifier, and section 2 gives it a meaning on parameters (a parameter named `_x` is passed without its name).
- A letter, digit or `_` directly after an identifier is part of it. So `x1` is one identifier.

The keywords are reserved: none of them can be an identifier anywhere, including as a field name after `.`.

| group | keywords |
|---|---|
| definitions | `fn` `type` `trait` `impl` `test` `law` `lemma` `extern` `pub` `use` `from` `as` `mock` `derive` `with` |
| bindings | `mut` `cap` `comptime` |
| control | `if` `else` `match` `while` `for` `in` `loop` `return` `break` `continue` `assert` `except` `or` |
| values | `true` `false` `is` `self` `Self` `it` `old` |
| tasks | `spawn` `scope` `await` `select` `race` |
| contracts | `requires` `ensures` `holds` `invariant` `decreases` `where` `assumed` `by` |
| proof steps | `assume` `calc` `simplify` `once` `unfold` `witness` `arithmetic` `nonlinear` `bits` `compute` `auto` |

The groups are only for reading; every keyword is reserved alike. Arenas, roots and frozen values are library functions (`std.memory`), not keywords; section 10 decides whether any of them becomes a block with a keyword.

The proof steps follow the proposed proof syntax (IDEAS.md "Proofs for the kernel"). Section 8 fixes the final list, and any change to it is made here too.

The names of built-in types (`Int`, `Natural`, `Real`, `Bool`, `String`, `s1` … `s128`, `u1` … `u128`, `f16`, `f32`, `f64`, `bf16`) are identifiers, not keywords. Core declares them (rule 12), and section 5 forbids binding them to anything else.

### 1.4 Holes

```ebnf
hole = "?" identifier ;
```

Example: `?body`, `?conflict_1`.

- A `?` followed directly by a letter or `_` starts a hole, and the hole takes the longest identifier that follows. Any other `?` is punctuation.

### 1.5 Number literals

```ebnf
integer     = decimal_digits | "0x" hex_digits | "0b" binary_digits ;
decimal_digits = "0" | '1'..'9' { [ "_" ] digit } ;
hex_digits  = hex_digit { [ "_" ] hex_digit } ;
hex_digit   = digit | 'a'..'f' | 'A'..'F' ;
binary_digits = binary_digit { [ "_" ] binary_digit } ;
binary_digit = "0" | "1" ;
decimal     = decimal_digits "." digit { [ "_" ] digit } [ exponent ]
            | decimal_digits exponent ;
exponent    = ( "e" | "E" ) [ "+" | "-" ] digit { digit } ;
```

Examples: `0`, `1_000`, `0xff`, `0b1010`, `1.5`, `0.000_1`, `6.02e23`, `1e-3`.

- An integer literal's value is a natural number of any size. A decimal literal's value is the exact rational number its digits write. Neither has a type of its own: section 6 gives each literal its type from where it is used. Section 6 also says when a decimal literal is a `Real` (exact) or a float (the nearest float, ties to even).
- `_` stands only between two digits. A leading zero followed by another digit (`007`) is an error.
- A letter, digit or `_` directly after a literal is an error (`12abc`, `0xfg`, `1_`, `5u8`). There are no suffixes: a literal's type comes from its context or from `(e: T)`.
- A literal has no sign. `-1` is the operator `-` applied to `1` (section 2 gives its stored form).
- After the punctuation `.`, a number is read as an integer only, with no fraction and no exponent. So `pair.0.1` is `pair`, `.`, `0`, `.`, `1`.
- `1.` and `.5` are not decimal literals: both sides of the `.` need digits. `1..n` is `1`, `..`, `n`.

### 1.6 String literals

```ebnf
string      = "\"" { string_char | escape } "\"" ;
string_char = any code point - "\"" - "\\" - control ;
control     = U+0000..U+001F | U+007F ;
escape      = "\\n" | "\\t" | "\\r" | "\\0" | "\\\\" | "\\\"" | "\\{" | "\\}"
            | "\\u{" hex_digit { hex_digit } "}" ;
raw_string  = "r\"" { any code point - "\"" - control } "\"" ;
```

Examples: `"total: 3\n"`, `"tab\there"`, `"\u{e9}"`, `r"\d+\.\d+"`.

- A string literal's value is a sequence of Unicode scalar values, as UTF-8 bytes.
- The escapes stand for: LF, tab, CR, U+0000, `\`, `"`, `{`, `}`, and the code point with that hexadecimal number. `\u{…}` takes 1 to 6 hex digits, and its value must be a scalar value (at most U+10FFFF and not a surrogate, U+D800 to U+DFFF).
- Any other character after `\` is an error.
- A string can't hold a raw control character, LF included. Write it as an escape.
- A raw string has no escapes. Its value is exactly the characters between its quotes, so it can't hold a `"` or a control character.
- Where a token starts, `r"` starts a raw string and `f"` an f-string. So a variable `r` or `f` followed by a string needs a space between them. Inside a longer identifier (`for"x"`) the identifier is read first.
- `\{` and `\}` mean `{` and `}` in plain strings too, so text can move between a string and an f-string unchanged.
- Which form the printer writes (plain or raw) is a notation setting (section 2).

### 1.7 f-strings

```ebnf
f_string      = "f\"" { string_char - "{" - "}" | escape | interpolation } "\"" ;
interpolation = "{" tokens "}" ;
```

Example: `f"{name} has {len(xs)} items"`, `f"\{literal braces\}"`.

- The text parts follow the rules of plain strings, escapes included. A `{` or `}` in the text is written `\{` or `\}`. A raw `}` in the text is an error.
- An interpolation holds the tokens of one expression, read by the rules of this section. Its end is the `}` that matches its `{`: the lexer counts `{` and `}` tokens, and braces inside string tokens don't count.
- An interpolation can hold strings and f-strings. It can't hold an LF, a comment or an empty expression (`{}` is an error).
- There are no format specs. A `:` in an interpolation is part of the expression: `f"{(n: u8)}"` is the expression `(n: u8)`.
- The token's value is a list of parts, each a text or a list of tokens. Section 2 gives its stored form, the concatenation of each part's `Display` text.

### 1.8 Comments and doc comments

```ebnf
module_doc  = "//!" { any code point - LF } ;
doc_comment = "///" { any code point - LF } ;   (* the fourth character is not "/" *)
comment     = "//" { any code point - LF } ;    (* neither of the above *)
```

Example:

```strata
//! Totals of orders, per customer.

/// The sum of a customer's orders, in cents.
fn total(orders: List[Order]) -> s64 {
  // Kahan summation would go here if amounts were floats.
  …
}
```

- Every comment runs to the end of its line. There are no block comments.
- A comment's text is what follows `//!`, `///` or `//`, with one leading space removed if there is one, and trailing spaces and tabs removed. `////` starts a plain comment.
- `//!` lines are the module's doc. They may come only before the module's first token that isn't a comment. A `//!` anywhere else is an error.
- `///` lines are the doc of the definition, field, variant or method that follows them. Section 2 says where one may stand. A `///` anywhere else is an error.
- All three are tokens, kept in order in the lexer's list. The parser keeps plain comments aside, at their place, and section 3 says where each is stored in the tree. A comment also counts as space for `space_before`.

### 1.9 Punctuation

```ebnf
punctuation = "==>" | "..." | "..=" | ".." | "|>" | "**" | "<<" | ">>" | "??" | "!!"
            | ":=" | "->" | "=>" | "==" | "!=" | "<=" | ">=" | "&&" | "||"
            | "+=" | "-=" | "*=" | "/=" | "%="
            | "(" | ")" | "[" | "]" | "{" | "}" | "," | ":" | "." | "="
            | "<" | ">" | "+" | "-" | "*" | "/" | "%" | "!" | "?" | "|" | "&" | "^" ;
```

- The longest match wins: `a!=b` is `a`, `!=`, `b`, and `x..=y` is `x`, `..=`, `y`.
- `..` and `..=` are ranges only: `a..b`, `a..=b`, and slices with an open end, `xs[..b]`.
- `**` is the power operator: `a ** b` is `Pow.pow(a, b)`, and `x**2` is `x`, `**`, `2` (section 2 gives its precedence).
- `|>` is the pipe: `a |> f` is the call `f(a)` (section 2).
- `...` is the spread: it puts a record's fields where fields are expected, in a call (`f(a, ...p)`) or in a record literal (`T { ...r, f: v }`, a record update).
- `$`, `;`, `@`, `#`, `~`, `` ` ``, `'` and `\` outside a string are errors. No program can write a generated name. The tree tells generated nodes apart by their node ids and core forms (section 3).

### 1.10 Newlines

Each LF is a `newline` token. Plain comments (`//`) take no part in layout: the rules below and the grammar of section 2 read the tokens with plain comments set aside, and there two or more `newline` tokens in a row count as one. Indentation never matters. Braces delimit blocks.

A newline ends a statement unless the lexer drops it. It drops a newline when any of these holds:

1. The innermost bracket open at that point is `(` or `[`.
2. The token before the newline is `{`, `,`, or a continuing operator.
3. The token after the newline is `}`, `)`, `]`, `.`, a continuing operator other than `-`, or a continuing keyword.

The continuing operators are `==>` `..` `..=` `|>` `**` `<<` `>>` `??` `!!` `:=` `->` `=>` `==` `!=` `<=` `>=` `&&` `||` `+=` `-=` `*=` `/=` `%=` `:` `=` `<` `>` `+` `-` `*` `/` `%` `|` `&` `^`.

The continuing keywords are `else` `or` `except` `with` `where` `requires` `ensures` `holds` `decreases` `invariant` `by` `as`.

- A line that begins with `-` starts a new statement, because `-x` can begin one. To continue a sum onto the next line, end the line with the operator: `total +` then `-x`.
- A line that begins with `!` or `?` starts a new statement.

Examples:

```strata
total := price
  * quantity          // continues: the line starts with `*`
ok := a &&
  b                   // continues: the line ends with `&&`
xs.map(f)
  .sum()              // continues: the line starts with `.`
print(t, view)
  |> parse
  |> desugar         // continues: the lines start with `|>`
f(a,
  b)                  // dropped: inside `(`
print(x)
(y)                   // two statements: the newline is kept
```

Calls and indexing need their bracket to touch the token before it. The parser (section 2) reads `(` or `[` as a call, type arguments or an index only when its `space_before` is false. So `f (x)` and `f` followed by `(x)` on the next line are not calls.

### 1.11 Laws

Two tokens are equal when their kinds, values and `space_before` are equal. Positions are not compared.

The lexer and the token printer `render` are tied by a law, proven as rule 14 asks:

```strata
law lex_render(ts: Tokens) requires lexable(ts) holds lex(render(ts)) == Ok(ts)
```

- `lexable(ts)` holds for every token list that `lex` can return. So for any text `s` that lexes, `lex(render(lex(s)!)) == lex(s)`.
- `render` writes each token in one canonical form:
  - an integer in decimal, without `_`;
  - a decimal literal as the shortest text the `decimal` rule accepts for its value, with a lowercase `e` and no `+`; on a tie, the text without an exponent;
  - a string with `\"`, `\\` and control characters escaped (`\n`, `\t`, `\r`, `\0`, else `\u{…}` in lowercase hex), and every other character as itself;
  - a raw string as it was written;
  - a comment as `//`, `///` or `//!`, then a space and its text when the text isn't empty;
  - a newline as one LF.
- Between two tokens, `render` writes one space when the second's `space_before` is true, or when the two would otherwise read as one token.
- This law is about tokens only. Section 2 states the round-trip law of the parser and printer on trees.

### 1.12 Errors

The lexer reports the first of these, at its position:

| error | example |
|---|---|
| invalid UTF-8 | a lone byte `0xFF` |
| byte order mark not at the start | |
| CR not followed by LF | |
| a character not allowed outside strings and comments | `é`, `$`, `;`, a control character |
| a malformed number | `007`, `1_`, `0x`, `12abc`, `5u8` |
| an unknown escape | `"\d"` |
| an escape `\u{…}` out of range or a surrogate | `"\u{d800}"` |
| a control character in a string | an LF inside `"…"` |
| a string not closed on its line | `"abc` |
| `}` alone in an f-string's text | `f"a}b"` |
| an empty or unclosed interpolation | `f"{}"`, `f"{x"` |
| `//!` after the first token | |

### 1.13 Differences from the lab

Each difference is a decision recorded in REBUILD.md.

| the lab | the rebuild | tried in the lab |
|---|---|---|
| identifiers of Unicode letters and digits | ASCII only | to try: measure the names it breaks |
| `I8` … `U64`, wrapping | `s1` … `s128`, `u1` … `u128`, names reserved | #655 (keep) |
| unknown escapes kept as written; `\n \t \" \\` only | a fixed set with `\r \0 \{ \} \u{…}`; any other is an error; raw strings `r"…"` | to try |
| `{{` and `}}` in f-strings | `\{` and `\}` | to try |
| format specs in f-strings | none | to try |
| raw LF allowed in strings | an error; write `\n` | to try |
| decimal digits only | `0x`, `0b`, exponents; leading zeros an error | to try |
| module doc as a first `//` comment | `//!` | to try |
| markup `<p>…</p>`, three lexer modes | deferred: not in this spec until std.web needs it | not needed (deferred) |
| `$` names in stored trees | no `$` at all | to try |
| `;`, `++` punctuation | none | not needed (removal) |
| no power operator | `a ** b` for `Pow.pow(a, b)` | to try |
| no pipes | `a \|> f` for `f(a)` | to try: trial 5, #661 |
| record updates written `T { ..r, f: v }` | `T { ...r, f: v }`, as spreads write `f(...p)` | to try |
| some keywords contextual | every keyword reserved, proof steps included | to try: measure the names it breaks |
| newline continues only before an operator or `.` | also after an operator, `,` or `{`, and before a continuing keyword | to try |
| CR, BOM: undefined | CR LF read as LF, BOM skipped at the start | to try |
