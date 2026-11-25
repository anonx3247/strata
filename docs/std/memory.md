# std.memory

Memory, for programs that must be fast: arenas. Each function here is
what it says (`f()`); the compiler also runs `f` with what it makes in an
arena: allocated together (no free list), never freed one by one nor
walked when dropped, all freed at once. Objects in an arena are counted
as any other, so one held once is still changed in place.

## Functions

### arena

```rust
fn arena[A](f: () -> A) -> A
```

`f()`, with what it makes in an arena freed when it returns: its result
is copied out first (what the arena made of it; what it holds from
before is shared). For work that makes much and keeps little: a
compiler checking one function. The result can't hold a function, a
trait value, a task or a channel's end. An arena is one thread's (a
task's work can have its own); one open when its thread starts a task
is kept (never freed), as the task may hold what it made.

### arena_kept

```rust
fn arena_kept[A](f: () -> A) -> A
```

`f()`, with what it makes in an arena never freed: its result, and all
`f` made, lives until the program ends, never freed nor walked when
dropped. For what a program keeps to its end anyway: a compiler's
decoded modules.
