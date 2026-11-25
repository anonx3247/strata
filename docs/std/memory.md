# std.memory

std.memory: arenas, for programs that allocate much and keep little.
An arena allocates objects together, with no free list, and frees them all at once.

## Functions

### arena

```rust
fn arena[A](f: () -> A) -> A
```

Runs `f` with its allocations in an arena, freed when it returns.
The result is copied out of the arena first. What it holds from before `f` is shared.
The result can't hold a function, a trait value, a task or a channel's end.
An arena belongs to one thread. A task started while it is open keeps it alive to the program's end.

### arena_kept

```rust
fn arena_kept[A](f: () -> A) -> A
```

Runs `f` with its allocations in an arena that is never freed.
Nothing it made is freed or walked when dropped. For data a program keeps to its end.
