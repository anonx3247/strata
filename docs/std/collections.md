# std.collections

std.collections: ordered dictionaries and sets (AVL trees), and a priority queue (a binary heap).
Use `Dict` or `Set`, which are hash tables, when the order doesn't matter: they are faster.

## Types

### Tree

```rust
type Tree[K, V] =
  | Leaf
  | Node { key: K, value: V, left: Tree[K, V], right: Tree[K, V], height: Int }
```

An AVL tree of entries by key. A node's two subtrees differ in height by at most one, so a path from the root has at most 1.44 log2(n) nodes.

### SortedDict

```rust
type SortedDict[K, V] = { root: Tree[K, V] = Leaf, size: Int = 0 }
```

Ordered dictionary: an AVL tree keyed by `compare`.

- get, insert, remove, first, last, floor, ceiling: O(log n). Iterates in key order.
- `SortedDict {}` is empty. `sorted_dict(pairs)` builds one from pairs.
- A change is in place when nothing else holds the dictionary. Otherwise it copies the O(log n) nodes on the path to the key.
- Two dictionaries with the same entries can differ in shape. Compare their `items()`.

### SortedSet

```rust
type SortedSet[T] = { root: Tree[T, ()] = Leaf, size: Int = 0 }
```

Ordered set: an AVL tree keyed by `compare`.
contains, insert, remove: O(log n). Iterates in order.
`SortedSet {}` is empty. `sorted_set(xs)` builds one from a list.

### Heap

```rust
type Heap[T] = { items: List[T] = [], order: Option[(T, T) -> Int] = None }
```

Priority queue: a binary heap in a `List`, smallest first.
push, pop: O(log n). peek: O(1).
Ordered by `order` (-1, 0 or 1, as `compare` gives), else by `compare`.
`Heap {}` is empty. `heap(xs)` builds one from a list. A change copies the list when something else holds it.

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
  ensures it == (find(self.root, k) != None)
```

Whether `k` is one of its keys.
Cost: O(log n) comparisons.

### SortedDict.get

```rust
fn get(self, k: K) -> Option[V]
  ensures it == find(self.root, k)
```

The value of `k`, if it is one of its keys.
Cost: O(log n) comparisons.

### SortedDict.insert

```rust
fn insert(mut self, k: K, v: V)
  ensures self.size == old self.size
    + if find(old self.root, k) == None {
      1
    } else {
      0
    }
  ensures count(self.root) == count(old self.root)
    + if find(old self.root, k) == None {
      1
    } else {
      0
    }
```

`k` holding `v` (replacing its value if it has one).
Cost: O(log n) comparisons and new nodes.

### SortedDict.remove

```rust
fn remove(mut self, k: K) -> Option[V]
  ensures it == find(old self.root, k)
  ensures self.size == old self.size - if it == None {
    0
  } else {
    1
  }
  ensures count(self.root) == count(old self.root) - if it == None {
    0
  } else {
    1
  }
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
  ensures self.size == old self.size - if it == None {
    0
  } else {
    1
  }
  ensures count(self.root) == count(old self.root) - if it == None {
    0
  } else {
    1
  }
```

The entry of the smallest key, taken out.
Cost: O(log n).

### SortedDict.pop_last

```rust
fn pop_last(mut self) -> Option[(K, V)]
  ensures self.size == old self.size - if it == None {
    0
  } else {
    1
  }
  ensures count(self.root) == count(old self.root) - if it == None {
    0
  } else {
    1
  }
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
  ensures it == (find(self.root, x) != None)
```

Whether `x` is one of its elements.
Cost: O(log n) comparisons.

### SortedSet.insert

```rust
fn insert(mut self, x: T)
  ensures self.size == old self.size
    + if find(old self.root, x) == None {
      1
    } else {
      0
    }
  ensures count(self.root) == count(old self.root)
    + if find(old self.root, x) == None {
      1
    } else {
      0
    }
```

`x` added (nothing changes if it is already there).
Cost: O(log n) comparisons and new nodes.

### SortedSet.remove

```rust
fn remove(mut self, x: T) -> Bool
  ensures it == (find(old self.root, x) != None)
  ensures self.size == old self.size - if it {
    1
  } else {
    0
  }
  ensures count(self.root) == count(old self.root) - if it {
    1
  } else {
    0
  }
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
  ensures self.size == old self.size - if it == None {
    0
  } else {
    1
  }
  ensures count(self.root) == count(old self.root) - if it == None {
    0
  } else {
    1
  }
```

The smallest element, taken out.
Cost: O(log n).

### SortedSet.pop_last

```rust
fn pop_last(mut self) -> Option[T]
  ensures self.size == old self.size - if it == None {
    0
  } else {
    1
  }
  ensures count(self.root) == count(old self.root) - if it == None {
    0
  } else {
    1
  }
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
