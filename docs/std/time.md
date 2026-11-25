# std.time

The clock: the time now, waiting, and timers in `select`.

Time comes from outside the program, so it goes through a capability:
`main` (or a test) asks for the `SystemClock` and passes it on as a
`Clock`:

```rust
fn main(clock: SystemClock) {
  start := clock.now()
  clock.sleep(ms(10))
  print(elapsed(start, clock.now()).value >= 10000000)
}
```

A test that times things asks for a `VirtualClock` instead: its own time,
at 0, which jumps to the nearest deadline once every task waits, so a
timeout happens at once and exactly:

```rust
test "gives up" (clock: VirtualClock) {
  clock.sleep(seconds(30))  // returns at once; clock.now() is then 30 s
}
```

A test that needs a time that doesn't move writes its own mock of `Clock`.

Waiting is a wait like a `recv`: a cancelled task (a `race`'s loser) stops
sleeping at once. In a `select` (and so in a `race`), `clock.after(d) => …`
is an arm that happens once `d` has passed since the `select` began.

## Types

### Duration

```rust
distinct type Duration = Int
```

A length of time, in nanoseconds.

### Instant

```rust
distinct type Instant = Int
```

A point in time, counted from the clock's own start: only the time
between two instants means something (it is not the date).

### SystemClock

```rust
capability SystemClock {}
```

The system's monotonic clock. Only the runtime makes one, for `main` and
tests.

### VirtualClock

```rust
capability VirtualClock {}
```

A clock whose time moves by itself, for tests: each run (the program,
or each test) has its own, at 0 when it starts, shared by its tasks. When
every task of the run is waiting and one of them waits for this clock (a
sleep, an `after` timer), its time jumps to the nearest such deadline: a
test of timeouts runs at once, exactly. A test (or `main`) asks for it by
name: `test "times out" (clock: VirtualClock) { … }`.

## Traits

### Clock

```rust
capability trait Clock {
  /// The time now; it never goes back.
  fn now(self) -> Instant
  /// Wait until `d` has passed (at once for a duration of 0 or less). A
  /// cancelled task stops waiting at once.
  fn sleep(self, d: Duration)
  /// The calendar time: how long since 1970-01-01 00:00 UTC (the wall
  /// clock, which may go back when it is set; a stand-in's is its own time).
  fn since_epoch(self) -> Duration
}
```

The time, and waiting for it to pass: the system's clock, or a stand-in.

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
