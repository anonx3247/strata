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
| definitions | `fn` `type` `distinct` `trait` `impl` `test` `law` `lemma` `extern` `pub` `use` `from` `as` `mock` `derive` `with` |
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

## 3. The tree

A module is stored as a tree. This section lists every node and its fields. Section 4 gives the bytes, section 2 the text each node is written as, and section 6 the typing rules a stored tree obeys.

The tree is written here as Strata types. `Option[T]`, `List[T]`, `Dict[K, V]` and `String` are core's. A field whose name would be a keyword takes a leading `_` (`_type`, `_trait`); a leading `_` means nothing on a field.

### 3.1 Ids, keys and node indices

```strata
distinct type Id = u32 where it != 0
distinct type NodeIndex = u32
type Key = { module: ModulePath, id: Id }
type NodeKey = { definition: Id, node: NodeIndex }
type ModulePath = { package: Name, path: List[Name] }
type Name = String where is_identifier(it)
```

- These have an `Id`: definitions (methods included), named types and atoms declared inside a sum (section 3.4), fields, and parameters of functions, methods and function types.
- An id is unique among the ids of its module, and it stays the same through edits, renames and moves within the module. `edit` draws a new id at random, never 0, and draws again while it is taken.
- A `Key` names something in any module: the module and the id.
- A definition's nodes are every value of the types `Definition`, `Expression`, `Pattern`, `Statement`, `Parameter`, `TypeParameter`, `Field`, `Arm` and `Contract` reachable from it, except through the `_type` field of an expression or a pattern. A type stored in that field is not a node of the definition and has no index.
- The nodes are numbered from 0 in postorder: a node's children first, in the order of their fields in this section and of their lists, then the node. Bodies are numbered last, so the interface can be read alone (section 4):
  1. first the definition with each body left out, in postorder, which ends with the definition itself;
  2. then each body, in postorder, in the order of the functions that have them.
- A `NodeKey` names a node in its module: its definition's id and its index. Passes keep their results in tables keyed by it.
- A local variable, a type parameter, a lambda's parameter and a pattern's binding have no id. A use refers to its binder by node index: `Local { binder }`.

```strata
law children_first(d: Definition, parent: NodeIndex, child: NodeIndex)
  requires is_child(d, parent, child) && !is_body(d, child)
  holds child < parent

law bodies_last(d: Definition, body: NodeIndex, other: NodeIndex)
  requires is_body(d, body) && !inside_a_body(d, other)
  holds other < body

law binder_in_scope(d: Definition, usage: NodeIndex)
  requires node(d, usage) is Local
  holds in_scope(d, binder_of(d, usage), usage)
```

Section 5 defines `in_scope`.

### 3.2 Modules, names, docs and comments

No node holds a name, a doc or a comment. The module holds them in tables, keyed by id or by node.

```strata
type Module = {
  doc: Doc,
  imports: List[Import],
  definitions: List[Definition],
  names: Dict[Id, Name],
  local_names: Dict[NodeKey, Name],
  docs: Dict[NodeKey, Doc],
  comments: Dict[NodeKey, Comment],
}

type Import =
  | WholeModule { module: ModulePath, alias: Option[Name] }
  | Names { module: ModulePath, names: List[ImportedName] }

type ImportedName = { key: Key, alias: Option[Name] }

type Doc = List[String]
type Comment = List[String]
```

Example: `use std.time as t` is `WholeModule { module: std.time, alias: Some("t") }`. `from std.time use Clock` is `Names { module: std.time, names: [ImportedName { key: <Clock's key>, alias: None }] }`.

- `names` gives the name of every id in the module, except an `Impl`'s, which has none.
- `local_names` gives the name of every node that binds a variable without an id (`Binding`, `TypeParameter`, the elements of named results) and of every `Hole`.
- `docs` holds the `///` lines of definitions and fields. `comments` holds the `//` lines of definitions, statements, arms, fields and parameters. A comment written above a node and one at the end of its line are both stored for that node. The printer chooses where to write it by the kind of node: above definitions and statements, at the end of the line for parameters and fields.
- `doc` is the module's `//!` lines. The file gives the module's path, so a module doesn't store it.
- A rename changes one entry of `names` or `local_names`, and no node.

```strata
law every_id_named(m: Module, id: Id)
  requires declared_in(m, id) && !is_impl(m, id)
  holds m.names.contains(id)

law every_binder_named(m: Module, n: NodeKey)
  requires binds_without_id(m, n) || node_of(m, n) is Hole
  holds m.local_names.contains(n)
```

### 3.3 Definitions

```strata
type Definition =
  | Function {
      type_parameters: List[TypeParameter],
      parameters: List[Parameter],
      result: Expression,
      contracts: List[Contract],
      body: Option[Expression],
    }
  | Extern {
      language: ForeignLanguage,
      parameters: List[Parameter],
      result: Expression,
      contracts: List[Contract],
    }
  | TypeDefinition {
      type_parameters: List[TypeParameter],
      value: Expression,
    }
  | Trait {
      type_parameters: List[TypeParameter],
      supertraits: List[Expression],
      methods: List[Definition.Function],
    }
  | Impl {
      type_parameters: List[TypeParameter],
      _trait: Expression,
      target: Expression,
      methods: List[Definition.Function],
    }
  | Test { body: Expression }
with {
  id: Id,
  public: Bool,
}

type ForeignLanguage = | C | Rust | Python | TypeScript
```

Examples:
- `fn area(s: Shape) -> f64 { … }` is a `Function` with one parameter, `result` the expression `f64`, no contracts, and a body. `names` maps its id to `area`.
- `type Positive = Int where it > 0` is a `TypeDefinition` whose `value` is a `Refinement` (section 3.4).
- `impl Mul[Int, Money] for Money { … }` is an `Impl` whose `_trait` is the expression `Mul[Int, Money]` and `target` the expression `Money`.

- A `Function` at the top level, or in an `Impl`, has a body. A method of a `Trait` may have none; then each impl gives it.
- Named results (`-> (low: s64, high: s64)`) are a `TupleType` result whose elements are named in `local_names`. Contracts refer to the result as `it` (section 3.6) and to its parts as `it.0`, `it.1`; the printer writes them by their names.
- An `Impl` has no name. Section 5 states how a reader finds it, by its trait and target.
- A method is a `Definition.Function`, so it has an id of its own. `public` on a method of a trait or impl is the trait's or impl's.
- Laws, lemmas and proofs are definitions too. Section 8 adds them. Task expressions come with section 9.

### 3.4 Expressions

Every expression has a type, which is itself an expression, in its field `_type` (section 6).

```strata
type Expression =
  // literals
  | Integer { value: LiteralNumber }
  | Decimal { digits: LiteralNumber, exponent: s32 }
  | Text { value: String }
  // names
  | Local { binder: NodeIndex }
  | Global { key: Key }
  | SelfType
  // calls
  | Call { target: Target, arguments: List[Argument] }
  | Lambda { parameters: List[Parameter], body: Expression }
  // values made of parts
  | Construct { named: Key, value: Expression }
  | RecordLiteral { base: Option[Expression], fields: List[FieldValue] }
  | Tuple { elements: List[Expression] }
  | ListLiteral { elements: List[Expression] }
  | FieldAccess { record: Expression, field: Key }
  | TupleElement { tuple: Expression, index: u32 }
  // control
  | Block { statements: List[Statement], result: Expression }
  | If { condition: Expression, then: Expression, otherwise: Expression }
  | Match { subject: Expression, arms: List[Arm] }
  | Return { value: Expression }
  | Break
  | Continue
  // logic that skips its right side
  | And { left: Expression, right: Expression }
  | Or { left: Expression, right: Expression }
  | Implies { left: Expression, right: Expression }
  | Not { value: Expression }
  | Is { subject: Expression, pattern: Pattern }
  // contracts and compile time
  | Old { value: Expression }
  | Comptime { value: Expression }
  // types
  | RecordType { fields: List[Field] }
  | TupleType { elements: List[Expression] }
  | Named { id: Id, _type: Expression }
  | Atom { id: Id }
  | SumType { members: List[Expression] }
  | FunctionType { parameters: List[Parameter], result: Expression }
  | Refinement { base: Expression, predicate: Expression }
  | TypeApplication { generic: Expression, arguments: List[Expression] }
  // unfinished code
  | Hole { choices: List[Expression] }
  // a marker, never stored (section 3.7)
  | Inferred
with {
  _type: Option[Expression],
}

type LiteralNumber = | Small(u64) | Large(Bytes)

type Target =
  | Static { definition: Key, written_as: Option[Key] }
  | Bound { type_parameter: NodeIndex, method: Key }
  | Dynamic { method: Key }
  | Value { function: Expression }

type Argument = { parameter: Id, value: Expression, mode: PassingMode }
type PassingMode = | Plain | Mutable | Capability
type FieldValue = { field: Key, value: Expression }
```

Literals:
- A `LiteralNumber` is a natural number. `Small` holds it when it is below 2^64, so most literals take one word and no allocation. `Large` holds a larger one as its bytes, least significant first, with no trailing zero byte. The compiler is written with the features of L0 to L2, which have no `Natural` (rule 2).
- `Integer` holds the literal's value, with no sign. `-1` is the call `Neg.neg(1)`.
- `Decimal` is the exact value `digits × 10^exponent`, with no trailing zero in `digits` (`1.50` is `{ digits: 15, exponent: -1 }`). No float is ever stored (rule 14).
- `Text` holds valid UTF-8. An f-string is stored as calls (section 2).

Names:
- `Local` is a use of a variable or parameter of this definition (`x` in `x + 1`). It holds the index of the node that binds it.
- `Global` is a use of a definition by its key: a function passed as a value (`shapes.map(area)`), a type (`Shape`), a generic (`List` in `List[Int]`), a trait.
- `SelfType` is `Self`: the type the enclosing trait or impl is for.

Calls:
- `Call.target` is what the call runs, decided at edit time:
  - `Static` names the definition that runs: a function, an impl's method, or core's primitive for an operator on a built-in type. `written_as` is the trait method the source named, so `a + b` on two `s64` is `Static { definition: <core's s64 addition>, written_as: Some(<Add.add>) }`. The printer writes `+` from `written_as`.
  - `Bound` is a method of a type parameter's bound. It is made static when the function is compiled for a type.
  - `Dynamic` calls a method through a trait value's method table. The trait value is the first argument.
  - `Value` calls a function value.
- `Call.arguments` holds the arguments given, in the order of the parameters. A parameter left out takes its default (section 7). A method's receiver is the first argument. `mode` records `mut` and `cap` as written at the call.

Values made of parts:
- `Construct` makes a value of a named type from its contents: `Circle { r: 1.0 }` is `Construct { named: <Circle>, value: RecordLiteral { … } }`, and `Some(x)` is `Construct { named: <Some>, value: Tuple { elements: [x] } }`.
- An atom is its type's one value, so a use of it is a `Global`: `None`, `true` and `false` are `Global { key: <None> }`, `Global { key: <True> }` and `Global { key: <False> }`.
- `RecordLiteral` holds the fields given, in the order the type declares them. Each field left out takes its value from `base` when there is one (`T { ...r, f: v }`), and otherwise its default. So a field added to a type later fills every stored literal with no rewrite. With no `Construct` around it, a `RecordLiteral` is a structural record (`{ x: 1, y: 2 }`).
- `FieldAccess` names the field by key: the field of the record's type, or of the bound that declares it when the record's type is a type parameter.
- Indexing has no node: `xs[i]` is a call of core's `Index` trait (section 12), `Static { …, written_as: Some(<Index's method>) }`, and the printer writes `xs[i]` from `written_as`. `xs[i] = v` and slices `xs[a..b]` are calls of the same trait. Any type gets `[ ]` by implementing `Index`.

Control and logic:
- `Return`, `Break` and `Continue` are expressions whose type is core's `Never`.
- `And`, `Or`, `Implies` and `Not` are nodes, not trait calls. `And`, `Or` and `Implies` evaluate their right side only when the left doesn't decide the result (`i < len(xs) && xs[i] > 0` never reads `xs[i]` out of range), and the prover uses the left side as a fact while proving the right.

Types:
- There are no variants. A sum is a sum of types: `SumType { members }`. Each member is a type declared elsewhere (`Global`), or a `Named` type or an `Atom` declared in the sum.
- `Named { id, _type }` is a type with a name of its own, over a record (`Circle { r: f64 }`) or a tuple of one or more elements (`Set(T)`).
- `Atom { id }` is a type with a name and exactly one value, written by its name: `None`, `Never`, `True`, `False`.
- A record has named fields (`Field`, with ids). A tuple has unnamed elements, which are only types; `.0`, `.1` read them.
- A sum with shared fields (`| A { x: s64 } | B with { id: Id }`) is stored with each shared field in every member's record, under one id. Every member must then be a `Named` record. The printer writes `with` when every member holds the same fields under the same ids.
- `TypeApplication` is a generic type with its arguments: `List[Int]`, `Result[Config, NotFound]`, `Patch[User]`, `Matrix[3, 4]`. It isn't a `Call`, because a call's arguments are by parameter id and a type parameter has none.
- `Refinement` binds `it` in its predicate. A `Local` for that `it` refers to the `Refinement` node.

The `_type` field:
- Every expression's `_type` is `Some(t)`, where `t` is an expression whose own type is `Type`.
- The one exception is the expression naming core's `Type` itself, whose `_type` is `None`. So the chain of types ends: the type of `s64` is `Type`, and `Type` stores no type.
- Nothing records whether a type was written. The printer leaves out every type that inference would give back exactly, and `view --types` writes them all, as `(e: T)` (section 2).

Unfinished code:
- A `Hole` is code still to write. `?body` is a `Hole` with no choices, named `body` in `local_names`.
- A merge conflict inside a definition is a `Hole` whose choices are both sides' code, ours first, then the base's when it differs from both (`?conflict_1`). Filling the hole picks a choice or writes new code.

```strata
law one_form(n: LiteralNumber)
  holds n is Large ==> value_of(n) >= 2 ** 64

law types_end(e: Expression)
  holds (e._type == None) == (e == Global { key: core.Type })
```

### 3.5 Statements, patterns, arms

```strata
type Statement =
  | Bind { pattern: Pattern, value: Expression }
  | Assign { place: Expression, value: Expression }
  | While {
      condition: Expression,
      invariants: List[Expression],
      _decreases: List[Expression],
      body: Expression,
    }
  | For {
      pattern: Pattern,
      iterable: Expression,
      invariants: List[Expression],
      unrolled: Bool,
      body: Expression,
    }
  | Assert { condition: Expression }
  | Evaluate { value: Expression }

type Pattern =
  | Wildcard
  | Binding { mutable: Bool }
  | LiteralPattern { value: Expression }
  | NamedPattern { named: Key, inner: Pattern }
  | RecordPattern { fields: List[FieldPattern], rest: Bool }
  | TuplePattern { elements: List[Pattern] }
  | TypeTest { tested: Expression, binding: Option[Pattern] }
  | Alternatives { patterns: List[Pattern] }
with {
  _type: Option[Expression],
}

type FieldPattern = { field: Key, pattern: Pattern }

type Arm = { pattern: Pattern, guard: Option[Expression], body: Expression }
```

Examples:
- `x := f(y)` is `Bind { pattern: Binding { mutable: false }, value: … }`, the binding named `x` in `local_names`.
- `mut total: s64 = 0` is a `Bind` whose `Binding` is mutable and whose type is `s64`.
- `x = e` is `Assign`. `x += e` is `Assign { place: x, value: Add.add(x, e) }`.
- `loop { … }` is `While` with the condition `true`. `comptime for` is `For` with `unrolled: true`.
- `Some(x)` as a pattern is `NamedPattern { named: <Some>, inner: TuplePattern { elements: [Binding] } }`. `None` as a pattern is `LiteralPattern { value: Global { key: <None> } }`. `e: NotFound => …` is a `TypeTest`.

- A `Binding` binds a variable. Its type is the variable's.
- `Assert`'s proof steps (`assert … by …`) come with section 8.
- `Evaluate` is an expression used as a statement.

### 3.6 Parameters, fields, contracts

```strata
type Parameter = {
  id: Id,
  _type: Expression,
  mode: ParameterMode,
  default: Option[Expression],
}
type ParameterMode = | Plain | Mutable | Capability | Comptime

type TypeParameter = {
  _type: Expression,
  bounds: List[Expression],
  variance: Variance,
}
type Variance = | Covariant | Contravariant | Invariant

type Field = {
  id: Id,
  _type: Expression,
  default: Option[Expression],
}

type Contract =
  | Requires { condition: Expression }
  | Ensures { condition: Expression }
  | Decreases { measures: List[Expression] }
with {
  _assumed: Bool,
}
```

- A parameter named `_x` is passed without its name (section 2). Its name keeps the `_`.
- A lambda's and a function type's parameters are `Parameter`s too. They have ids, because names are part of a function type.
- A `TypeParameter`'s `_type` is `Type` for a type parameter, or an integer, `Bool` or atom type for a value parameter (`type Matrix[R: s64, C: s64]`).
- `variance` is computed by elaborate (section 6) and is part of the type's interface.
- In an `ensures`, `it` is a `Local` whose binder is the `Function`. In a `requires` of a function with one parameter, `it` is that parameter.

### 3.7 Parsed trees and stored trees

`parse` and `desugar` give a tree with a marker for each type left to infer. `elaborate` fills them in and gives the stored tree.

- `Inferred` stands where a type was left out. Every expression whose type wasn't written gets `_type: Some(Inferred)` from `parse`.
- A constructor left out (`Some`, `Ok`, `Err`: section 6) has no marker, since `parse` can't tell where one is missing. `elaborate` puts in the `Construct` wherever the expected type asks for one, or reports where the types don't decide which. The printer leaves it out again where inference would put it back.
- A stored tree has no markers. A `Call`'s target and a node's type are known in every stored tree.

```strata
law stored_has_no_markers(d: Definition)
  requires stored(d)
  holds !any_node(d, n -> n is Inferred)
```

Section 2 states how `print`, `parse`, `desugar` and `elaborate` round-trip on stored trees.

## 4. The binary format and hashing

A module is stored as one module file, `NAME.strata`, holding the tree of section 3 in the encoding below. Text is never stored. A package is a folder of module files and a `strata.toml` (section 5).

### 4.1 Primitives

| notation | encoding |
|---|---|
| `uint` | unsigned LEB128: 7 bits per byte, least significant first, the high bit set on every byte but the last. Shortest form only, at most 64 bits. `300` is `ac 02`. |
| `int` | a 64-bit signed integer, zigzag-encoded (`0, -1, 1, -2` become `0, 1, 2, 3`), then written as a `uint`. |
| `bool` | one byte, `0` or `1`. |
| `id` | 4 bytes, least significant first: a `u32`, never 0. |
| `hash` | 32 bytes: a SHA-256 digest. |
| `offset` | 8 bytes, least significant first: a position in the file, in bytes from its start. Fixed width, so the offsets at the start of a file can be written before the sections they point to are laid out. |
| `bytes` | a `uint` length, then that many bytes. |
| `text` | `bytes` that are valid UTF-8. |
| `list(T)` | a `uint` count, then each item. |
| `option(T)` | a `bool`, then the value when it is `1`. |
| a record | its fields, in the order section 3 lists them. |
| a sum | a `uint` tag, then the fields of that member, in order. |

Every encoding is canonical: one value has exactly one encoding, and the decoder refuses any other (a `uint` not in its shortest form, a `bool` other than `0` or `1`, invalid UTF-8, a list out of its stated order).

### 4.2 The file

```text
module-file = "STRATA" version:uint kind:uint
              interfaces_start:offset  bodies_start:offset  constants_start:offset  evidence_start:offset
              header  interfaces  bodies  constants  evidence
```

- A file starts with the six ASCII bytes `STRATA` (`53 54 52 41 54 41`), then the format version, `2`, then `0` for a module file. Version 1 is the lab's format, which this one doesn't read.
- The four offsets say where the interfaces, the bodies, the constants and the evidence begin, so a reader jumps to the section it needs. The header starts right after them.
- The sections hold, in order:
  1. **the header** (section 4.3): the module's doc, imports, ids and one entry per definition;
  2. **the interfaces**: each definition's interface, in the order of the entries;
  3. **the bodies**: each definition's bodies, in the same order;
  4. **the constants** (section 4.5): every string and large number of the file, once each, then each definition's local names, docs and comments;
  5. **the evidence** (section 4.8).
- Checking code against the module, listing it, or recomputing its stamps reads the header and the interfaces, and the constants for names. Bodies are read only for the definitions being compiled, proven or shown.
- The file ends exactly after the evidence.

### 4.3 The header

The header is the module's table of contents. It holds what a reader needs without decoding any definition: the module's doc and imports, every id of the module and every key of another module it refers to, and one entry per definition with its visibility, hashes, dependencies, stamp and sizes. Listing a module, building the dependency graph, recomputing stamps and finding a definition's parts read only the header (and the pool, for names).

```text
header     = doc:list(uint)  imports:list(Import)  modules:list(ModulePath)
             declared:list(Declared)  refs:list(Ref)  entries:list(Entry)
ModulePath = package:uint  path:list(uint)
Import     = 0  module:uint  alias:option(uint)
           | 1  module:uint  names:list(ImportedName)
ImportedName = key:uint  alias:option(uint)
Declared   = id:id  name:option(uint)  owner:uint
Ref        = module:uint  id:id  name:uint
Entry      = definition:uint  public:bool  variant:uint
             interface_hash:hash  code_hash:hash  text_hash:hash
             dependencies:list(uint)  stamp:hash
             interface_size:uint  body_size:uint  text_size:uint
```

- Every name, path part, alias and line of doc in the header is a `uint`: an index into the constants pool (section 4.5).
- `doc` is the module's `//!` lines.
- `modules` lists every other module the file names, in imports or in references, sorted by package, then by path, comparing the bytes of their names. An `Import`'s and a `Ref`'s `module` is an index into it.
- `declared` lists every id of the module (section 3.1). For each definition in order: the definition's own id, then each id it declares (methods, named types and atoms, fields, parameters), in node order.
  - `name` is the id's name, from the module's `names` table. It is none only for an `Impl`.
  - `owner` is the index in `declared` of the definition that declares the id. A definition is its own owner.
- `refs` lists every key of another module that the file refers to, sorted by module index, then by id. `name` is the name its module gave it when this file was written. Section 5 says which name a reader uses when the two differ.
- **The id table** is `declared`, then `refs`. Everywhere else in the file, a key is a `uint`: its index in this table.
- `entries` has one entry per definition, in the module's order.
  - `definition` is the index in `declared` of the definition's id, `public` its `public`, and `variant` its tag in the `Definition` sum (section 4.6).
  - `dependencies` lists the keys whose interfaces the definition's check relied on (section 6), as id-table indices in increasing order.
  - The three hashes and the stamp are in section 4.7.
  - `interface_size`, `body_size` and `text_size` are the sizes in bytes of the definition's interface, of its bodies, and of its texts in the constants section. A definition's interface starts where the interfaces section starts plus the `interface_size`s of the entries before it, and likewise for the other two.

### 4.4 Interfaces and bodies

```text
interface = types:list(TypeEntry)  nodes:list(Node)
bodies    = types:list(TypeEntry)  bodies:list(list(Node))
TypeEntry = nodes:list(Node)
```

- A definition's **interface** is its nodes with each body left out. A **body** is the `body` of a `Function` (methods included) or of a `Test`; a definition's bodies come in the order of the functions that have them.
- **Nodes** are written in postorder (section 3.1), so the n-th node written is node index n, counting the interface's nodes and then each body's. Each node is:

```text
Node = tag:uint  fields
```

- `tag` is the node's tag in section 4.6. `Inferred` has no tag, since a stored tree has none.
- `fields` are the node's fields in the order section 3 lists them, each written as follows:
  - a node (an `Expression`, `Pattern`, `Statement`, `Parameter`, `TypeParameter`, `Field`, `Arm`, `Contract`, or a method's `Definition.Function`): nothing, since it was written before as a child;
  - an `Option` of a node: a `bool`, `1` when the child is there;
  - a `List` of nodes: a `uint`, the number of children;
  - a body (`Function.body`, `Test.body`): a `bool`, `1` when there is one; its nodes are in the bodies section;
  - the `_type` of an expression or pattern: a `uint`, `0` for none (only `Type` itself), else 1 plus an index into the definition's type list;
  - a `Key`, or an `Id` the node declares: a `uint`, its index in the id table;
  - a `NodeIndex`: a `uint`;
  - a `LiteralNumber`: tag `0` then a `uint` (`Small`), or tag `1` then a `uint`, the index of its constant (`Large`);
  - a `String` (`Text.value`): a `uint`, the index of its constant;
  - an `s32`: an `int`;
  - a `u32` (`TupleElement.index`): a `uint`;
  - a `Bool`: a `bool`;
  - a sum with no fields (`PassingMode`, `ParameterMode`, `Variance`, `ForeignLanguage`): a `uint`, its member's position from 0;
  - a record that isn't a node (`Argument`, `FieldValue`, `FieldPattern`): its fields in order, by these same rules;
  - a `Target`: its member's position from 0, then that member's fields by these same rules.
- A decoder keeps a stack of nodes. Reading a node, it takes its children off the stack, the last child first, and refuses a child of the wrong type. After a definition's interface, the stack holds the definition alone. After each body, it holds that body's root alone.

**Types.** Each distinct type stored in a `_type` field is written once per definition, as a `TypeEntry`: the type's nodes, in postorder. The interface's `types` hold the types the interface's nodes use, and the bodies' `types` the further types the bodies use, with indices continuing. A type enters the list the first time a node being written needs it, after the types its own nodes need. A type's nodes are not nodes of the definition (section 3.1), but a `Local` in a type (`Array[u8, size]`) refers to a node of the definition.

### 4.5 Constants

```text
constants = pool:list(Constant)  texts:list(DefinitionTexts)
Constant  = 0  text
          | 1  bytes
DefinitionTexts = local_names:list(NodeName)  docs:list(NodeLines)  comments:list(NodeLines)
NodeName  = node:uint  name:uint
NodeLines = node:uint  lines:list(uint)
```

- The **pool** holds every string of the file (names, local names, docs, comments, `Text` literals) as a `text`, and every `Large` number as its `bytes`, least significant first. Each distinct constant is there once.
- The pool is in order of first use, reading the file from its start: the header, the interfaces, the bodies, then the texts. Elsewhere a constant is a `uint`, its index in the pool.
- `texts` has one `DefinitionTexts` per definition, in the order of the entries: its entries of `local_names`, `docs` and `comments` (section 3.2), each sorted by node index.

### 4.6 Tags

| tags | nodes |
|---|---|
| 0–5 | `Definition`: `Function`, `Extern`, `TypeDefinition`, `Trait`, `Impl`, `Test` |
| 16–51 | `Expression`: `Integer`, `Decimal`, `Text`, `Local`, `Global`, `SelfType`, `Call`, `Lambda`, `Construct`, `RecordLiteral`, `Tuple`, `ListLiteral`, `FieldAccess`, `TupleElement`, `Block`, `If`, `Match`, `Return`, `Break`, `Continue`, `And`, `Or`, `Implies`, `Not`, `Is`, `Old`, `Comptime`, `RecordType`, `TupleType`, `Named`, `Atom`, `SumType`, `FunctionType`, `Refinement`, `TypeApplication`, `Hole` |
| 64–69 | `Statement`: `Bind`, `Assign`, `While`, `For`, `Assert`, `Evaluate` |
| 80–87 | `Pattern`: `Wildcard`, `Binding`, `LiteralPattern`, `NamedPattern`, `RecordPattern`, `TuplePattern`, `TypeTest`, `Alternatives` |
| 96–102 | `Parameter`, `TypeParameter`, `Field`, `Arm`, `Contract`'s `Requires`, `Ensures`, `Decreases` |

Within a row, tags are numbered from the first in the order listed: `Integer` is 16, `Hole` 51. The gaps leave room for the nodes of sections 8 and 9.

### 4.7 Hashes and stamps

All hashes are SHA-256. `strata` shows a hash as the hex of its first 8 bytes.

The hashes are taken over a definition's **hashed form**: its interface and bodies as stored, with each index into a file-wide table written out in full. A constant is written as its `text` or `bytes`. A key of the file's own module is `0` then its `id`; a key of another module is `1`, its `ModulePath` with each part as `text`, then its `id`. So a definition's hashes don't depend on the rest of the file.

| hash | of | changes when |
|---|---|---|
| `interface_hash` | `public`, the variant's tag, and the hashed interface part | what other code relies on changes: types, signatures, contracts, visibility |
| `code_hash` | `interface_hash` and the hashed body part | anything the definition does changes |
| `text_hash` | `code_hash`, the names of the ids the definition declares (in `declared` order), and its local names, docs and comments, each written out in full | anything a reader of this definition sees changes, names, docs and comments included |

- No hash holds a name of another definition. So a rename changes the `text_hash` of the renamed definition only. Every cache keyed by `interface_hash` or `code_hash` (checks, proofs, compiled code) stays valid. Compiled code names its symbols by key.
- `stamp` is the SHA-256 of the `interface_hash`es of the definition's `dependencies`, written one after another, in their order, as they were when the definition was last checked.
- A reader recomputes each stamp from the current headers of the modules it reads. A definition whose stamp differs is checked again before it is used (after a pull, a merge, a checkout or a fetched dependency). `strata check --full` checks every definition whatever its stamp.

```strata
law stamp_unchanged_check_holds(d: StoredDefinition, now: Headers)
  requires stamp_of(d.dependencies, now) == d.stamp
  holds checks(d, now)
```

### 4.8 Evidence

```text
evidence           = list(DefinitionEvidence)
DefinitionEvidence = definition:uint  stamp:hash  obligations:list(ObligationEvidence)
```

- The evidence part keeps the result of each proof, so a proof is replayed from the facts it used instead of searched for again (section 8). `definition` is an index into `entries`, and the list is in that order.
- Its `stamp` is the proof's: the SHA-256 of the hashes the proof relied on, as section 8 lists them. Evidence whose stamp differs from the current one is stale. It is ignored and the proof is redone, never trusted.
- Section 8 gives `ObligationEvidence`: the obligation's node, its verdict, the solver and its version, the resources used and the facts used (the unsat core).
- The evidence is outside every hash. Adding or dropping it never changes a definition's hashes.
- How large the evidence part grows on core, std and the compiler is to measure in the lab, before deciding whether it moves to a file of its own.

### 4.9 Reading safely

A decoder never crashes on a file. It reports the first of these as an error:

- a wrong magic, version or kind;
- a primitive not in canonical form, or a length past the end of the file;
- an offset or an index out of its range (the sections, the id table, `modules`, `declared`, the type list, the pool, `entries`);
- a constant given twice in the pool, or out of its order of first use;
- an unknown tag, a child of the wrong type, or a stack holding anything but the expected root at the end of a part;
- a key given twice, an id that is 0, or two ids of the module that are equal;
- a list out of its stated order, or sizes that don't add up to the length of their section;
- a `Large` number below 2^64, or a `Decimal` whose `digits` end in a zero.

The decoder checks no hash: hashes are computed when a definition is stored. `strata check --full` recomputes them.

### 4.10 Laws

```strata
law decode_encode(m: Module)
  requires stored(m)
  holds decode(encode(m)) == Ok(m)

law encode_decode(b: Bytes)
  requires decode(b) is Ok
  holds encode(decode(b)!) == b
```

The second law is what canonical means: any file the decoder accepts is exactly the file the encoder writes for its module.
