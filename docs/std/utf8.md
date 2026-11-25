# std.utf8

Text as UTF-8 bytes, and back.

A byte is an `Int` from 0 to 255, as `Files.read_bytes` gives them.

## Functions

### encode

```rust
fn encode(text: String) -> List[Int]
```

The UTF-8 bytes of `text`.

### decode

```rust
fn decode(bytes: List[Int]) -> Option[String]
```

The text `bytes` hold, or `None` if they aren't UTF-8.

### decode_range

```rust
fn decode_range(bytes: List[Int], start: Int, end: Int) -> Option[String]
```

The text bytes `start` to `end` (not included) hold, or `None` if they
aren't UTF-8: a byte out of 0..=255, a sequence cut short, an overlong
form, a surrogate, or a code point past U+10FFFF.
