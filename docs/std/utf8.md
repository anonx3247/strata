# std.utf8

Text as UTF-8 bytes, and back. A byte is an `Int` from 0 to 255.

## Functions

### encode

```rust
fn encode(text: String) -> List[Int]
```

`text` as UTF-8 bytes.

### decode

```rust
fn decode(bytes: List[Int]) -> Option[String]
```

`None` if the bytes aren't UTF-8.

### decode_range

```rust
fn decode_range(bytes: List[Int], start: Int, end: Int) -> Option[String]
```

Decodes `bytes[start..end]`. `None` if they aren't UTF-8:
a byte out of 0..=255, a cut sequence, an overlong form, a surrogate, or a code point past U+10FFFF.
