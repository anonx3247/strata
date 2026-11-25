# std.collections

A dictionary and a set kept in their keys' order, and
a priority queue.

`Dict` and `Set` are hash tables: the fastest way to find a key, but in no
order. Use these when the order matters:

- `SortedDict[K, V]`: keys kept sorted by their order (`compare`, the one
  `sort` sorts by). Find, add and remove in O(log n), like a `Dict`'s
  O(1) but slower by that factor; and also the smallest and largest keys
  (`first`, `last`), the nearest key at or below one (`floor`), at or
  above it (`ceiling`), strictly below or above (`before`, `after`), and
  the entries between two keys (`between`), each in O(log n) (plus the
  entries given back). Iterating (`items`, `keys`, `values`) is in order.
- `SortedSet[T]`: the same for a set of values.
- `Heap[T]`: a priority queue, a binary heap in a `List`: add in O(log n),
  take the smallest in O(log n), look at it in O(1). Smallest by the
  values' order, by a key (`Heap.by(key)`), or by a comparison of your own
  (`Heap.with(order)`): a largest-first heap is `Heap.with((a, b) ->
  compare(b, a))`.

Use a `Dict` or `Set` when nothing needs the order (they are faster), and
`sort` once when the order is needed only at the end.

These are values like any other: `mut d.insert(k, v)` changes `d` alone,
in place when nothing else holds it (else what it changes is copied: for
a `SortedDict`, the O(log n) nodes on the way to the key; for a `Heap`,
its list). Two `SortedDict`s with the same entries can be built in
different shapes, so compare their `items()`, not the values themselves.

## Types

### Tree

```rust
type Tree[K, V] =
  | Leaf
  | Node { key: K, value: V, left: Tree[K, V], right: Tree[K, V], height: Int }
```

An AVL tree of entries by key: a node's two subtrees differ in height by
at most one, so a path from the root has at most 1.44 log2(n) nodes.

### SortedDict

```rust
type SortedDict[K, V] = { root: Tree[K, V] = Leaf, size: Int = 0 }
```

A dictionary whose keys are kept in their order (`compare`'s):
`let d: SortedDict[String, Int] = SortedDict {}` is an empty one, and
`sorted_dict(pairs)` one of the pairs.

### SortedSet

```rust
type SortedSet[T] = { root: Tree[T, ()] = Leaf, size: Int = 0 }
```

A set whose elements are kept in their order (`compare`'s):
`let s: SortedSet[Int] = SortedSet {}` is an empty one, and
`sorted_set(xs)` one of the elements of a list.

### Heap

```rust
type Heap[T] = { items: List[T] = [], order: Option[(T, T) -> Int] = None }
```

A priority queue: the smallest element first, by `order` (-1, 0 or 1,
as `compare` gives) or else by `compare`. `let h: Heap[Int] = Heap {}`
is an empty one, `heap(xs)` one of the elements of a list.

## Methods

### SortedDict.len

```rust
fn len(self) -> Int
```

The number of entries.
Cost: O(1).

### SortedDict.is_empty

```rust
fn is_empty(self) -> Bool
```

Whether it has no entries.
Cost: O(1).

### SortedDict.contains

```rust
fn contains(self, k: K) -> Bool
```

Whether `k` is one of its keys.
Cost: O(log n) comparisons.

### SortedDict.get

```rust
fn get(self, k: K) -> Option[V]
```

The value of `k`, if it is one of its keys.
Cost: O(log n) comparisons.

### SortedDict.insert

```rust
fn insert(mut self, k: K, v: V)
```

`k` holding `v` (replacing its value if it has one).
Cost: O(log n) comparisons and new nodes.

### SortedDict.remove

```rust
fn remove(mut self, k: K) -> Option[V]
```

`k` taken out: its value, if it was one of the keys.
Cost: O(log n) comparisons and new nodes.

### SortedDict.first

```rust
fn first(self) -> Option[(K, V)]
```

The entry of the smallest key.
Cost: O(log n).

### SortedDict.last

```rust
fn last(self) -> Option[(K, V)]
```

The entry of the largest key.
Cost: O(log n).

### SortedDict.pop_first

```rust
fn pop_first(mut self) -> Option[(K, V)]
```

The entry of the smallest key, taken out.
Cost: O(log n).

### SortedDict.pop_last

```rust
fn pop_last(mut self) -> Option[(K, V)]
```

The entry of the largest key, taken out.
Cost: O(log n).

### SortedDict.floor

```rust
fn floor(self, k: K) -> Option[(K, V)]
```

The entry of the largest key at or below `k`.
Cost: O(log n) comparisons.

### SortedDict.ceiling

```rust
fn ceiling(self, k: K) -> Option[(K, V)]
```

The entry of the smallest key at or above `k`.
Cost: O(log n) comparisons.

### SortedDict.before

```rust
fn before(self, k: K) -> Option[(K, V)]
```

The entry of the largest key strictly below `k`.
Cost: O(log n) comparisons.

### SortedDict.after

```rust
fn after(self, k: K) -> Option[(K, V)]
```

The entry of the smallest key strictly above `k`.
Cost: O(log n) comparisons.

### SortedDict.between

```rust
fn between(self, lo: K, hi: K) -> List[(K, V)]
```

The entries whose keys are from `lo` up to (not including) `hi`, in
order.
Cost: O(log n + m) for m entries given.

### SortedDict.items

```rust
fn items(self) -> List[(K, V)]
```

The entries, in their keys' order.
Cost: O(n).

### SortedDict.keys

```rust
fn keys(self) -> List[K]
```

The keys, in order.
Cost: O(n).

### SortedDict.values

```rust
fn values(self) -> List[V]
```

The values, in their keys' order.
Cost: O(n).

### SortedSet.len

```rust
fn len(self) -> Int
```

The number of elements.
Cost: O(1).

### SortedSet.is_empty

```rust
fn is_empty(self) -> Bool
```

Whether it has no elements.
Cost: O(1).

### SortedSet.contains

```rust
fn contains(self, x: T) -> Bool
```

Whether `x` is one of its elements.
Cost: O(log n) comparisons.

### SortedSet.insert

```rust
fn insert(mut self, x: T)
```

`x` added (nothing changes if it is already there).
Cost: O(log n) comparisons and new nodes.

### SortedSet.remove

```rust
fn remove(mut self, x: T) -> Bool
```

`x` taken out: whether it was there.
Cost: O(log n) comparisons and new nodes.

### SortedSet.first

```rust
fn first(self) -> Option[T]
```

The smallest element.
Cost: O(log n).

### SortedSet.last

```rust
fn last(self) -> Option[T]
```

The largest element.
Cost: O(log n).

### SortedSet.pop_first

```rust
fn pop_first(mut self) -> Option[T]
```

The smallest element, taken out.
Cost: O(log n).

### SortedSet.pop_last

```rust
fn pop_last(mut self) -> Option[T]
```

The largest element, taken out.
Cost: O(log n).

### SortedSet.floor

```rust
fn floor(self, x: T) -> Option[T]
```

The largest element at or below `x`.
Cost: O(log n) comparisons.

### SortedSet.ceiling

```rust
fn ceiling(self, x: T) -> Option[T]
```

The smallest element at or above `x`.
Cost: O(log n) comparisons.

### SortedSet.before

```rust
fn before(self, x: T) -> Option[T]
```

The largest element strictly below `x`.
Cost: O(log n) comparisons.

### SortedSet.after

```rust
fn after(self, x: T) -> Option[T]
```

The smallest element strictly above `x`.
Cost: O(log n) comparisons.

### SortedSet.between

```rust
fn between(self, lo: T, hi: T) -> List[T]
```

The elements from `lo` up to (not including) `hi`, in order.
Cost: O(log n + m) for m elements given.

### SortedSet.to_list

```rust
fn to_list(self) -> List[T]
```

The elements, in order.
Cost: O(n).

### Heap.by

```rust
fn by[B](key: (T) -> B) -> Heap[T]
```

An empty heap, smallest first by `key`'s values.
Cost: O(1); each comparison then calls `key` twice.

### Heap.with

```rust
fn with(order: (T, T) -> Int) -> Heap[T]
```

An empty heap, smallest first by `order` (-1, 0 or 1, as `compare`
gives): `Heap.with((a, b) -> compare(b, a))` is largest first.
Cost: O(1).

### Heap.less

```rust
fn less(self, a: T, b: T) -> Bool
```

Whether `a` comes before `b`.
Cost: one comparison.

### Heap.len

```rust
fn len(self) -> Int
```

The number of elements.
Cost: O(1).

### Heap.is_empty

```rust
fn is_empty(self) -> Bool
```

Whether it has no elements.
Cost: O(1).

### Heap.peek

```rust
fn peek(self) -> Option[T]
```

The smallest element.
Cost: O(1).

### Heap.push

```rust
fn push(mut self, x: T)
```

`x` added.
Cost: O(log n) comparisons (O(n) when the list is shared).

### Heap.pop

```rust
fn pop(mut self) -> Option[T]
```

The smallest element, taken out.
Cost: O(log n) comparisons (O(n) when the list is shared).

### Heap.sift_down

```rust
fn sift_down(mut self, start: Int, x: T)
```

`x` put at `i` (or below, as far as it goes), the elements on its way
moved up.
Cost: O(log n) comparisons.

### Heap.to_list

```rust
fn to_list(self) -> List[T]
```

The elements, smallest first.
Cost: O(n log n).

## Functions

### sorted_dict

```rust
fn sorted_dict[K, V](pairs: List[(K, V)]) -> SortedDict[K, V]
```

A `SortedDict` of the pairs (a later pair of the same key wins).
Cost: O(n log n).

### sorted_set

```rust
fn sorted_set[T](xs: List[T]) -> SortedSet[T]
```

A `SortedSet` of the elements of `xs`.
Cost: O(n log n).

### heap

```rust
fn heap[T](xs: List[T]) -> Heap[T]
```

A heap of the elements of `xs`, smallest first by their order.
Cost: O(n).
