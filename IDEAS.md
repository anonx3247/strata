# Ideas

Language and tooling ideas we want to keep, but haven't designed or scheduled.
Nothing here is decided until it moves into HANDOFF.md. Add new ideas at the
end of their section, with the date they came up.

## Language

### Default values for record fields (2026-11-20)

A record's definition can give a field a default, written explicitly where the
field is declared. Nothing gets a default implicitly.

```rust
type Options = {
  /// Say it this many times.
  times: Int where 1 <= it && it <= 10 = 1,
  /// Shout the greeting.
  loud: Bool = false,
  names: List[String] = [],
}

let o = Options { names: ["Ada"] }   // times: 1, loud: false
```

Questions to settle:
- **Syntax**: `field: T = value` looks natural. Check that it doesn't clash with
  a refinement that ends in a comparison.
- **What a default can be**: only literals and constructors, or any expression
  of the earlier fields? A default must satisfy the field's refinement, and
  the checker can prove that at definition time.
- **Where defaults apply**:
  - constructors (a missing field takes its default);
  - JSON input (a missing key takes its default; today it's an error);
  - command-line options (`CommandLine` would read `times: Int = 1` as an
    optional `--times` without needing `Option`).
- **Generated values**: mocks and fuzzing still generate every field.

### List comprehensions in the style of refinements (2026-11-20)

Reuse the refinement syntax (`where`, and maybe `it`) to filter and map lists,
instead of chaining `.filter(...).map(...)`. Possible shapes:

```rust
xs where it > 0                      // filter: the elements that satisfy the condition
[x * x for x in xs where x > 0]      // map + filter, with a named element
```

Questions to settle:
- Whether `xs where …` on a value reads too much like a type, given that
  `Int where …` already means a refinement type.
- Several generators (`for x in xs, y in ys`), and dict and set forms.
- **Laziness**: comprehensions would stay eager for now. Lazy evaluation (and
  so infinite lists) is not wanted as a general mechanism. If it comes, it
  comes with iterators, which are already planned as their own language
  feature. A comprehension over an iterator could then be lazy.
- The prover could model `xs where p` directly (every element satisfies `p`,
  and the length is at most `len(xs)`), which `filter` with a lambda can't
  easily give it.
