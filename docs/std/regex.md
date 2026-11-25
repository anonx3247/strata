# std.regex

## Types

### Regex

```rust
type Regex =
  | Empty
  | Literal(String)
  | Class(List[(Int, Int)])
  | Look(Assertion)
  | Repeat { min: Int, max: Option[Int], greedy: Bool, sub: Regex }
  | Capture { index: Int, name: Option[String], sub: Regex }
  | Concat(List[Regex])
  | Alternation(List[Regex])
```

A regular expression: what `parse` makes of a pattern. `Literal` is one
or more characters in order; `Class` one character from a set of code
points (inclusive ranges, in order; with none it matches nothing); `Look`
a position (the start or end of the text or a line, a word boundary).

### Assertion

```rust
type Assertion =
  | Start
  | End
  | StartLF
  | EndLF
  | StartCRLF
  | EndCRLF
  | WordAscii
  | WordAsciiNegate
  | WordUnicode
  | WordUnicodeNegate
  | WordStartAscii
  | WordEndAscii
  | WordStartUnicode
  | WordEndUnicode
  | WordStartHalfAscii
  | WordEndHalfAscii
  | WordStartHalfUnicode
  | WordEndHalfUnicode
```

What a `Look` asserts about the position it is at: the start or end of
the text (`\\A`, `\\z`, and `^`, `$` without `m`); of a line, `\\n`
ending lines (`(?m)^`, `LF`) or `\\r`, `\\n` and `\\r\\n` (`(?mR)^`,
`CRLF`); a word boundary (`\\b`) or not (`\\B`, `Negate`), with ASCII
word characters (`(?-u)`) or Unicode's (`\\w`); a word starting
(`\\b{start}`, `\\<`) or ending; `Half`: only no word character before
(`\\b{start-half}`) or after.

### Inst

```rust
type Inst =
  | Chars(List[(Int, Int)])
  | Split(Int, Int)
  | Jump(Int)
  | Save(Int)
  | Assert(Assertion)
  | Done
```

One step of a compiled regex (`compile`). `Chars`: one character from
these ranges, then the next step. `Split(a, b)`: both `a` and `b`, `a`
preferred. `Jump(a)`: go on at `a`. `Save(k)`: the position is capture
slot `k` (`2 * index` where a group starts, `2 * index + 1` where it
ends). `Assert(a)`: go on only if `a` holds here. `Done`: a match.

### Automaton

```rust
type Automaton = {
  bounds: List[Int],
  classes: Int,
  next: List[Int],
  accepting: List[Bool],
  start: Int,
}
```

A deterministic automaton over code points. The characters are split
into classes: class `i` is from `bounds[i]` to `bounds[i + 1] - 1`
(the last to U+10FFFF). `next[s * classes + i]` is the state after
state `s` reads a character of class `i`; state 0 matches nothing more
(a dead end). `accepting[s]`: the text matches if it ends in state `s`.

### Matcher

```rust
type Matcher =
  | Deterministic(Automaton)
  | Pike(List[Inst])
  | NotARegex(String)
```

How a pattern is matched: by a DFA, by the Pike VM (a pattern a DFA
can't do: word boundaries, lines, or too many states), or not at all
(why it isn't a regex).

### SmtModel

```rust
type SmtModel =
  | ExactModel(String)
  | Between(String, String)
```

How the prover models `matches(s, pattern)`: `ExactModel(r)` is `s` in
`r`. A class of more than `max_smt_ranges()` ranges (`\\w`, `\\p{L}`) is
more than Z3 decides in reasonable time: `Between(under, over)` is a
subset of the class's ranges and a superset with fewer gaps, so
`under <= pattern <= over` (regex operators are monotone).

### SmtMode

```rust
type SmtMode =
  | Exactly
  | Under
  | Over
```

How `to_smt` writes a class: as it is, or with at most
`max_smt_ranges()` ranges, fewer characters (`Under`) or more (`Over`).

## Functions

### parse

```rust
fn parse(pattern: String) -> Result[Regex, String]
```

A pattern read as a `Regex`, or why it isn't one.

### show

```rust
fn show(r: Regex) -> String
```

A regex as text that shows its tree exactly: `lit(61 62)` (code points
in hex), `cls(30-39 61-66)`, `look(Start)`, `rep(0,*,greedy,…)`,
`cap(1,name,…)` (`-` for no name), `cat(…)`, `alt(…)`, `empty`.

### class_of

```rust
fn class_of(rs: List[(Int, Int)]) -> Regex
```

A class: one that matches a single character is that literal.

### repetition

```rust
fn repetition(low: Int, high: Option[Int], greedy: Bool, sub: Regex) -> Regex
```

`sub` repeated from `min` to `max` times.

### concat

```rust
fn concat(rs: List[Regex]) -> Regex
```

The regexes one after the other: nested sequences flattened, adjacent
literals joined, `Empty` left out.

### alternation

```rust
fn alternation(rs: List[Regex]) -> Regex
```

Any one of the regexes: nested alternations flattened, single characters
and classes merged into one class, a common prefix taken out.

### max_steps

```rust
fn max_steps() -> Int
```

The most steps a compiled regex may have (`a{1000}{1000}` would have a
million).

### compile

```rust
fn compile(r: Regex) -> Result[List[Inst], String]
```

The regex as steps of a Thompson NFA (ending with `Done`), or why it
can't be: too many steps.

### is_full_match

```rust
fn is_full_match(prog: List[Inst], text: String) -> Bool
```

Whether `prog` (`compile`) matches the whole of `text`.

### full_match

```rust
fn full_match(text: String, pattern: String) -> Result[Bool, String]
```

Whether `pattern` matches the whole of `text` (what `matches(text,
pattern)` means), or why `pattern` isn't a regex.

### match_code

```rust
fn match_code(text: String, pattern: String) -> Int
```

`full_match` for compiled code: 1 or 0, or -1 when `pattern` isn't a
regex (`pattern_error` says why).

### pattern_error

```rust
fn pattern_error(pattern: String) -> String
```

Why `pattern` isn't a regex (`""` if it is).

### max_dfa_states

```rust
fn max_dfa_states() -> Int
```

The most states, and table cells (states × classes), a DFA may have.

### max_dfa_cells

```rust
fn max_dfa_cells() -> Int
```

### matcher

```rust
fn matcher(pattern: String) -> Matcher
```

How to match `pattern` (what compiled code uses for a literal pattern,
made while compiling).

### run_matcher

```rust
fn run_matcher(m: Matcher, text: String) -> Bool
```

Whether `m` matches the whole of `text` (never, for `NotARegex`).

### run_automaton

```rust
fn run_automaton(a: Automaton, text: String) -> Bool
```

Whether the automaton ends in an accepting state on `text`.

### automaton

```rust
fn automaton(prog: List[Inst]) -> Option[Automaton]
```

The DFA of a compiled regex (subset construction), or `None` if it
asserts more than the start or end of the text, or has too many states.

### max_product_states

```rust
fn max_product_states() -> Int
```

The most pairs of states `relations` looks at.

### relations

```rust
fn relations(a: String, b: String) -> (Bool, Bool, Bool)
```

How the languages of patterns `a` and `b` (the texts each matches
whole) relate: `(a's within b's, b's within a's, none in common)`,
each `false` when it does not hold or can't be decided (a pattern a
DFA can't match, or too large a search). Decided on the product of
the two DFAs; the prover uses it.

### max_extra_repeats

```rust
fn max_extra_repeats() -> Int
```

The most repetitions past its minimum `samples` makes of an unbounded
repetition (`*`, `+`, `{n,}`).

### samples

```rust
fn samples(pattern: String, n: Int, seed: Int) -> Result[List[String], String]
```

`n` texts that `pattern` matches whole, made from its tree: the same
ones for the same `seed`. Assertions other than the text's start and
end may not hold in a text made, so each is kept only if it matches;
fewer than `n` if most don't. Or why there are none: the pattern isn't
a regex, or no text made matched (`a\\bb`).

### max_smt_char

```rust
fn max_smt_char() -> Int where it == 196607
```

The last code point an SMT-LIB string holds (the standard's alphabet
stops at U+2FFFF, as Z3 and cvc5 do): classes are clipped there, and a
literal past it can't be written.

### max_smt_ranges

```rust
fn max_smt_ranges() -> Int where it == 24
```

Classes with more ranges than this are approximated.

### smt_model

```rust
fn smt_model(pattern: String) -> Option[SmtModel]
```

The prover's model of `pattern`, or `None` when its meaning can't be
written exactly: word boundaries, line anchors, anchors not certain to
be at the very start or end, literals past `max_smt_char()`, or no
regex at all.

### smt_exact

```rust
fn smt_exact(pattern: String) -> Option[String]
```

`pattern` translated exactly, however large its classes.
