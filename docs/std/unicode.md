# std.unicode

std.unicode: sets of code points, Unicode properties, simple case folding and grapheme clusters (Unicode 16.0.0).
A set of code points is a `List[(Int, Int)]` of inclusive ranges, sorted, neither overlapping nor touching.

## Types

### Graphemes

```rust
type Graphemes = { points: CodePoints, pending: Option[Int] }
```

An iterator over a string's extended grapheme clusters (UAX #29), each as a string, from `s.graphemes()`.
`pending` is the code point that ended the last cluster and starts the next.

## Traits

### GraphemeText

```rust
trait GraphemeText {
  fn graphemes(self) -> Graphemes
}
```

A string's graphemes as an iterator: `s.graphemes()`.

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

The code point after `c`, skipping the surrogates U+D800 to U+DFFF. So no set operation starts or ends a range on a surrogate.

### previous_code

```rust
fn previous_code(c: Int) -> Int
```

The code point before `c`, skipping the surrogates.

### canonical

```rust
fn canonical(rs: List[(Int, Int)]) -> List[(Int, Int)]
```

Sorts any ranges and merges those that overlap or touch.

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

Reads a table of std.unicode_tables. Each range is two numbers: the gap since the previous range's end + 1, then its length - 1.

### case_folding

```rust
fn case_folding() -> List[(Int, List[Int])]
```

Simple case folding: each code point that has others in its case class, with those others, in order.

### has_case_folding

```rust
fn has_case_folding(c: Int) -> Bool
```

Whether the code point `c` has other characters in its case class.

### case_fold

```rust
fn case_fold(a: List[(Int, Int)]) -> List[(Int, Int)]
```

`a` closed under simple case folding: every code point in `a`, and every other code point of the same case class.

### ascii_case_fold

```rust
fn ascii_case_fold(a: List[(Int, Int)]) -> List[(Int, Int)]
```

`a` closed under ASCII case folding, `a`-`z` with `A`-`Z`.

### perl_digit

```rust
fn perl_digit() -> List[(Int, Int)]
```

`\d`: decimal digits, the general category Nd.

### perl_space

```rust
fn perl_space() -> List[(Int, Int)]
```

`\s`: White_Space.

### perl_word

```rust
fn perl_word() -> List[(Int, Int)]
```

`\w`: word characters. Alphabetic, marks, Nd, Pc and Join_Control.

### is_white_space

```rust
fn is_white_space(c: Int) -> Bool
```

Whether `c` is White_Space, as Rust's `char::is_whitespace`.

### is_alphabetic

```rust
fn is_alphabetic(c: Int) -> Bool
```

Whether `c` is Alphabetic, as Rust's `char::is_alphabetic`.

### is_alphanumeric

```rust
fn is_alphanumeric(c: Int) -> Bool
```

Whether `c` is Alphabetic or a number, as Rust's `char::is_alphanumeric`.

### normalize

```rust
fn normalize(name: String) -> String
```

A property or value name for UAX #44 loose matching: lower case, without a leading `is`, spaces, `_` or `-`.

### property

```rust
fn property(name: String) -> Result[List[(Int, Int)], String]
```

The code points of `\p{name}`, matched loosely (`normalize`).
`name` is a binary property (`Alphabetic`), a general category (`L`, `Letter`, `Lu`), a script (`Greek`), `Any`, `ASCII` or `Assigned`.

### property_value

```rust
fn property_value(
  name: String,
  value: String,
) -> Result[List[(Int, Int)], String]
```

The code points of `\p{name=value}`.
`name` is a general category, a script, script extensions, an age, or a grapheme cluster, word or sentence break property.
An age includes every code point assigned by that version.

### grapheme_breaks

```rust
fn grapheme_breaks() -> List[(Int, Int, Int)]
```

Every range of code points whose Grapheme_Cluster_Break isn't `Other`, in order, as `(first, last, kind)`.

## Implementations

- `impl Iterator[String] for Graphemes`
- `impl FiniteIterator[String] for Graphemes`
- `impl GraphemeText for String`
