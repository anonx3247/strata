# std.time

std.time: the clock, waiting, and timers in `select`.
`main` asks for a `SystemClock`. A test asks for a `VirtualClock`, whose timeouts happen at once.

## Types

### Duration

```rust
distinct type Duration = Int
```

Nanoseconds.

### Instant

```rust
distinct type Instant = Int
```

Nanoseconds since the clock's own start. Only the difference of two instants means something.

### SystemClock

```rust
capability SystemClock {}
```

The system's monotonic clock. Only the runtime makes one, for `main` and tests.

### VirtualClock

```rust
capability VirtualClock {}
```

A clock for tests whose time jumps instead of passing.
Each run (the program, or each test) has its own, at 0 at the start, shared by its tasks.
When every task of the run waits and one waits on this clock, its time jumps to the nearest deadline.
A test asks for it by name: `test "times out" (clock: VirtualClock) { … }`.

## Traits

### Clock

```rust
capability trait Clock {
  /// Never goes back.
  fn now(self) -> Instant
  /// Returns at once for a duration of 0 or less. A cancelled task stops waiting at once.
  fn sleep(self, d: Duration)
  /// Wall-clock time since 1970-01-01 00:00 UTC. It may go back when the system clock is set.
  /// A clock without a calendar gives its own time.
  fn since_epoch(self) -> Duration
}
```

A source of time: `SystemClock`, `VirtualClock` or a mock.
In a `select`, `clock.after(d) => …` is an arm that fires once `d` has passed since the `select` began.

## Functions

### nanos

```rust
fn nanos(n: Int) -> Duration
```

`n` nanoseconds.

### micros

```rust
fn micros(n: Int) -> Duration
```

`n` microseconds.

### ms

```rust
fn ms(n: Int) -> Duration
```

`n` milliseconds.

### seconds

```rust
fn seconds(n: Int) -> Duration
```

`n` seconds.

### elapsed

```rust
fn elapsed(start: Instant, end: Instant) -> Duration
```

The time from `start` to `end`.

## Implementations

- `impl Clock for SystemClock`
- `impl Clock for VirtualClock`
