# std.lemmas

Facts about the builtin collections, proven once by `verify`, for
proofs to use (`assert … by frame_with(d, k, v, done)`). A lemma is never
compiled: it exists only for proofs (docs/verification.md, "Lemmas").

## Functions

### frame_with

```rust
lemma frame_with[K, V](d: Dict[K, V], k: K, v: V, ks: Set[K])
  ensures d.with(k, v).excluding(ks.with(k)) == d.excluding(ks.with(k))
```

Writing `k` changes nothing outside `ks` and `k`: the frame of `with`.

### frame_without

```rust
lemma frame_without[K, V](d: Dict[K, V], k: K, ks: Set[K])
  ensures d.without(k).excluding(ks.with(k)) == d.excluding(ks.with(k))
```

Removing `k` changes nothing outside `ks` and `k`: the frame of
`without`.

### frame_grows

```rust
lemma frame_grows[K, V](a: Dict[K, V], b: Dict[K, V], ks: Set[K], k: K)
  requires a.excluding(ks) == b.excluding(ks)
  ensures a.excluding(ks.with(k)) == b.excluding(ks.with(k))
```

Two dicts the same outside `ks` are the same outside `ks` and one more
key: a frame grows as a loop visits keys.

### with_reads

```rust
lemma with_reads[K, V](d: Dict[K, V], k: K, v: V)
  ensures get(d.with(k, v), k) == Some(v)
```

A key written reads back.

### with_keeps

```rust
lemma with_keeps[K, V](d: Dict[K, V], k: K, v: V, j: K)
  requires j != k
  ensures get(d.with(k, v), j) == get(d, j)
```

Writing `k` leaves every other key as it was.

### with_twice

```rust
lemma with_twice[K, V](d: Dict[K, V], k: K, v: V, w: V)
  ensures d.with(k, v).with(k, w) == d.with(k, w)
```

The second write of a key is the one kept.
