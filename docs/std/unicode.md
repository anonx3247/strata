# std.unicode

Sets of code points, Unicode's properties and simple case
folding (Unicode 16.0.0, from std.unicode_tables).

A set of code points is a `List[(Int, Int)]`: inclusive `(first, last)` ranges in
order, neither overlapping nor touching. Surrogates (U+D800 to U+DFFF) are
not characters: stepping past U+D7FF goes to U+E000, as Rust's `char`
does, so `negate` and `difference` never start or end a range on one.

```rust
from std.unicode use property, contains_code
greek := property("Greek")!            // List[(Int, Int)]
contains_code(greek, ord("λ"))         // true
```

## Types

### Graphemes

```rust
type Graphemes = { points: CodePoints, pending: Option[Int] }
```

A string's extended grapheme clusters (Unicode's UAX #29), each as a
string, from its code points: `s.graphemes()`. `pending` is the code point
that ended the last cluster (the next one's first).

## Traits

### GraphemeText

```rust
trait GraphemeText {
  fn graphemes(self) -> Graphemes
}
```

A string's graphemes, as an iterator: `s.graphemes()`.

## Functions

### max_code

```rust
fn max_code() -> Int where it == 1114111
```

The largest code point.

### next_code

```rust
fn next_code(c: Int) -> Int
```

The code point after `c`, past the surrogates.

### previous_code

```rust
fn previous_code(c: Int) -> Int
```

The code point before `c`, past the surrogates.

### canonical

```rust
fn canonical(rs: List[(Int, Int)]) -> List[(Int, Int)]
```

Any ranges made into a set: sorted, and merged where they overlap or
touch.

### union

```rust
fn union(a: List[(Int, Int)], b: List[(Int, Int)]) -> List[(Int, Int)]
```

Every code point in `a` or `b`.

### intersect

```rust
fn intersect(a: List[(Int, Int)], b: List[(Int, Int)]) -> List[(Int, Int)]
```

Every code point in both `a` and `b`.

### difference

```rust
fn difference(a: List[(Int, Int)], b: List[(Int, Int)]) -> List[(Int, Int)]
```

Every code point in `a` but not in `b`.

### symmetric_difference

```rust
fn symmetric_difference(
  a: List[(Int, Int)],
  b: List[(Int, Int)],
) -> List[(Int, Int)]
```

Every code point in exactly one of `a` and `b`.

### negate

```rust
fn negate(a: List[(Int, Int)]) -> List[(Int, Int)]
```

Every code point not in `a`.

### contains_code

```rust
fn contains_code(a: List[(Int, Int)], c: Int) -> Bool
```

Whether the code point `c` is in `a`.

### decode

```rust
fn decode(table: String) -> List[(Int, Int)]
```

The ranges written in a table of std.unicode_tables: two numbers per
range, the gap since the previous range's end (+ 1), then its length - 1.

### case_folding

```rust
fn case_folding() -> List[(Int, List[Int])]
```

Simple case folding: each code point that has others in its case class,
with those others, in order.

### has_case_folding

```rust
fn has_case_folding(c: Int) -> Bool
```

Whether the code point `c` has other characters in its case class.

### case_fold

```rust
fn case_fold(a: List[(Int, Int)]) -> List[(Int, Int)]
```

The set closed under simple case folding: every code point in `a`, and
every other code point of the same case class.

### ascii_case_fold

```rust
fn ascii_case_fold(a: List[(Int, Int)]) -> List[(Int, Int)]
```

The set closed under ASCII case folding (`a`-`z` with `A`-`Z`).

### perl_digit

```rust
fn perl_digit() -> List[(Int, Int)]
```

`\d`: decimal digits (the general category Nd).

### perl_space

```rust
fn perl_space() -> List[(Int, Int)]
```

`\s`: White_Space.

### perl_word

```rust
fn perl_word() -> List[(Int, Int)]
```

`\w`: word characters (Alphabetic, marks, Nd, Pc and Join_Control).

### is_white_space

```rust
fn is_white_space(c: Int) -> Bool
```

Whether `c` is White_Space (Rust's `char::is_whitespace`).

### is_alphabetic

```rust
fn is_alphabetic(c: Int) -> Bool
```

Whether `c` is Alphabetic (Rust's `char::is_alphabetic`).

### is_alphanumeric

```rust
fn is_alphanumeric(c: Int) -> Bool
```

Whether `c` is Alphabetic or a number (Rust's `char::is_alphanumeric`).

### normalize

```rust
fn normalize(name: String) -> String
```

A property or value name as Unicode's loose matching compares them:
without `is` in front, spaces, `_` or `-`, in lower case (UAX #44).

### property

```rust
fn property(name: String) -> Result[List[(Int, Int)], String]
```

The code points `\p{name}` means: a binary property (`Alphabetic`), a
general category (`L`, `Letter`, `Lu`), a script (`Greek`), or `Any`,
`ASCII`, `Assigned`; loosely matched (`normalize`).

### property_value

```rust
fn property_value(
  name: String,
  value: String,
) -> Result[List[(Int, Int)], String]
```

The code points `\p{name=value}` means: a general category, a script,
script extensions, an age (every code point assigned by that version),
or a grapheme cluster, word or sentence break property.

### grapheme_breaks

```rust
fn grapheme_breaks() -> List[(Int, Int, Int)]
```

Every range of code points with a Grapheme_Cluster_Break value other
than `Other`, in order: `(first, last, kind)`.

## Implementations

- `impl Iterator[String] for Graphemes`
- `impl FiniteIterator[String] for Graphemes`
- `impl GraphemeText for String`
