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

The tree is written here as Strata types. `Natural` is a natural number of any size, `Int` an integer of any size. `Option[T]`, `List[T]`, `Dict[K, V]` and `String` are core's. A field whose name would be a keyword takes a leading `_` (`_type`, `_trait`); a leading `_` means nothing on a field.

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
- A definition's nodes are every value of the types `Definition`, `Expression`, `Pattern`, `Statement`, `Parameter`, `TypeParameter`, `Field`, `Arm` and `Contract` reachable from it. They are numbered from 0 in postorder: a node's children first, in the order of their fields in this section and of their lists, then the node. The definition is the last node.
- A `NodeKey` names a node in its module: its definition's id and its index. Passes keep their results in tables keyed by it.
- A local variable, a type parameter, a lambda's parameter and a pattern's binding have no id. A use refers to its binder by node index: `Local { binder }`.

```strata
law children_first(d: Definition, parent: NodeIndex, child: NodeIndex)
  requires is_child(d, parent, child)
  holds child < parent

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
- `type Positive = Int where it > 0` is a `TypeDefinition` whose `value` is a `Refinement` (section 3.5).
- `impl Mul[Int, Money] for Money { … }` is an `Impl` whose `_trait` is the expression `Mul[Int, Money]` and `target` the expression `Money`.

- A `Function` at the top level, or in an `Impl`, has a body. A method of a `Trait` may have none; then each impl gives it.
- Named results (`-> (low: s64, high: s64)`) are a `TupleType` result whose elements are named in `local_names`. Contracts refer to the result as `it` (section 3.7) and to its parts as `it.0`, `it.1`; the printer writes them by their names.
- An `Impl` has no name. Section 5 states how a reader finds it, by its trait and target.
- A method is a `Definition.Function`, so it has an id of its own. `public` on a method of a trait or impl is the trait's or impl's.
- Laws, lemmas and proofs are definitions too. Section 8 adds them. Task expressions come with section 9.

### 3.4 Expressions

Every expression has a type, which is itself an expression, in its field `_type` (section 6).

```strata
type Expression =
  // literals
  | Integer { value: Natural }
  | Decimal { digits: Natural, exponent: Int }
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
  // markers, never stored (section 3.7)
  | Inferred
  | ImplicitConstructor { value: Expression }
with {
  _type: Option[Expression],
}

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

`parse` and `desugar` give a tree with markers for what is left to infer. `elaborate` fills them in and gives the stored tree.

- `Inferred` stands where a type was left out. Every expression whose type wasn't written gets `_type: Some(Inferred)` from `parse`.
- `ImplicitConstructor { value }` stands where a constructor was left out (`Some`, `Ok`, `Err`: section 6). `elaborate` replaces it by the `Construct` the types decide, or reports where they don't.
- A stored tree has no markers. A `Call`'s target and a node's type are known in every stored tree.

```strata
law stored_has_no_markers(d: Definition)
  requires stored(d)
  holds !any_node(d, n -> n is Inferred || n is ImplicitConstructor)
```

Section 2 states how `print`, `parse`, `desugar` and `elaborate` round-trip on stored trees.
