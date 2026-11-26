# std.array

N-dimensional arrays of floats, stored flat with a shape and strides.

## Types

### Array

```rust
type Array = {
  data: List[Float],
  shape: List[Natural],
  strides: List[Int],
  offset: Natural,
}
  where len(it.shape) == len(it.strides)
```

An n-dimensional array of `Float`s: a view of a flat list.

- Element `(i0, i1, …)` is `data[offset + i0 * strides[0] + i1 * strides[1] + …]`.
- `shape` has one length per axis. A 0-axis array holds one value.
- `transpose`, `slice`, `row` and `reshape` of a row-major array are views: O(number of axes), the data is shared.
- Arithmetic, reductions and `matmul` read row-major data. A view that isn't row-major is copied once first (`compact`, O(n)).
- Build arrays with `zeros`, `ones`, `full`, `arange`, `from_list` and `from_rows`. Their strides and offset stay inside `data`.
- Changing an element (`mut a.set`) copies the data first when another array shares it.

### Matrix

```rust
type Matrix = Array
  where len(it.shape) == 2
```

An array of two axes: rows, then columns.

## Methods

### Array.ndim

```rust
fn ndim(self) -> Natural
```

The number of axes.
Cost: O(1).

### Array.size

```rust
fn size(self) -> Natural
```

The number of elements.
Cost: O(number of axes).

### Array.is_compact

```rust
fn is_compact(self) -> Bool
```

Whether the array reads its data row-major from its start to its end, so the data is its elements in order.
Cost: O(number of axes).

### Array.to_list

```rust
fn to_list(self) -> List[Float]
```

The elements in row-major order.
Cost: O(1) when `is_compact`, else O(n): a copy.

### Array.compact

```rust
fn compact(self) -> Array
  ensures it.shape == self.shape
```

The same elements, row-major.
Cost: O(1) when `is_compact`, else O(n): a copy.

### Array.position

```rust
fn position(self, idx: List[Int]) -> Int
```

Where element `idx` is in `data`.

### Array.get

```rust
fn get(self, idx: List[Int]) -> Option[Float]
```

The element at `idx`, one index per axis. `None` when `idx` is out of bounds.
Cost: O(number of axes).

### Array.set

```rust
fn set(mut self, idx: List[Int], x: Float) -> Bool
```

Element `idx` set to `x`. Returns false, and changes nothing, when `idx` is out of bounds.
Cost: O(number of axes) when nothing else holds the data, else O(n): a copy.

### Array.reshape

```rust
fn reshape(self, shape: List[Natural]) -> Array
  requires count(shape) == count(self.shape)
  ensures it.shape == shape
```

The same elements in `shape`, which must hold as many.
Cost: O(number of axes) when `is_compact` (a view), else O(n): a copy.

### Array.transpose

```rust
fn transpose(self) -> Array
  ensures it.shape == self.shape.reversed()
```

The axes in reverse order: element `(i, j)` of the result is element `(j, i)` of a matrix.
Cost: O(number of axes): a view.

### Array.slice

```rust
fn slice(self, axis: Natural, start: Natural, stop: Natural) -> Array
  requires axis < len(self.shape)
  ensures len(it.shape) == len(self.shape)
```

Indices `start` to below `stop` of axis `axis`, both clamped to the axis.
Cost: O(number of axes): a view.

### Array.row

```rust
fn row(self, i: Natural) -> Array
  requires len(self.shape) > 0 && i < self.shape[0]
  ensures len(it.shape) == len(self.shape) - 1
```

Index `i` of axis 0, one axis fewer: a matrix's row `i`.
Cost: O(number of axes): a view.

### Array.map

```rust
fn map(self, f: (Float) -> Float) -> Array
  ensures it.shape == self.shape
```

`f` of each element.
Cost: O(n) calls of `f`.

### Array.zip_with

```rust
fn zip_with(self, other: Array, f: (Float, Float) -> Float) -> Array
  requires broadcasts(self.shape, other.shape)
```

`f` of each pair of elements, broadcast: the shapes must broadcast together.
Cost: O(n) calls of `f`, n the size of the broadcast shape.

### Array.add

```rust
fn add(self, other: Array) -> Array
  requires broadcasts(self.shape, other.shape)
```

Element by element, broadcast.
Cost: O(n).

### Array.sub

```rust
fn sub(self, other: Array) -> Array
  requires broadcasts(self.shape, other.shape)
```

Element by element, broadcast.
Cost: O(n).

### Array.mul

```rust
fn mul(self, other: Array) -> Array
  requires broadcasts(self.shape, other.shape)
```

Element by element, broadcast.
Cost: O(n).

### Array.div

```rust
fn div(self, other: Array) -> Array
  requires broadcasts(self.shape, other.shape)
```

Element by element, broadcast. A division by 0.0 fails, as `/` on floats does.
Cost: O(n).

### Array.neg

```rust
fn neg(self) -> Array
  ensures it.shape == self.shape
```

Each element negated.
Cost: O(n).

### Array.scale

```rust
fn scale(self, k: Float) -> Array
  ensures it.shape == self.shape
```

Each element times `k`.
Cost: O(n).

### Array.shift

```rust
fn shift(self, k: Float) -> Array
  ensures it.shape == self.shape
```

`k` added to each element.
Cost: O(n).

### Array.sum

```rust
fn sum(self) -> Float
```

The sum of the elements; 0.0 when there are none. They are added in row-major order into four interleaved partial sums, then those four, so the result can differ from a left-to-right sum in the last bits.
Cost: O(n).

### Array.mean

```rust
fn mean(self) -> Float
  requires count(self.shape) > 0
```

The mean of the elements. The array must not be empty.
Cost: O(n).

### Array.max

```rust
fn max(self) -> Option[Float]
```

The largest element, `None` when there are none.
Cost: O(n).

### Array.min

```rust
fn min(self) -> Option[Float]
```

The smallest element, `None` when there are none.
Cost: O(n).

### Array.argmax

```rust
fn argmax(self) -> Option[Natural]
```

Where the largest element is, in row-major order (the first, for ties); `None` when there are none.
Cost: O(n).

### Array.lanes

```rust
fn lanes(self, axis: Natural) -> List[Float]
  requires axis < len(self.shape)
```

The array with axis `axis` moved last, row-major: each run of `shape[axis]` elements is one reduction's input.

### Array.sum_axis

```rust
fn sum_axis(self, axis: Natural) -> Array
  requires axis < len(self.shape)
  ensures it.shape == without_axis(self.shape, axis)
```

The sums along axis `axis`: the result has the other axes. A matrix's `sum_axis(0)` sums each column.
Cost: O(n).

### Array.mean_axis

```rust
fn mean_axis(self, axis: Natural) -> Array
  requires axis < len(self.shape) && self.shape[axis] > 0
  ensures it.shape == without_axis(self.shape, axis)
```

The means along axis `axis`, which must not be empty.
Cost: O(n).

### Array.max_axis

```rust
fn max_axis(self, axis: Natural) -> Array
  requires axis < len(self.shape) && self.shape[axis] > 0
  ensures it.shape == without_axis(self.shape, axis)
```

The largest elements along axis `axis`, which must not be empty.
Cost: O(n).

### Array.argmax_axis

```rust
fn argmax_axis(self, axis: Natural) -> List[Natural]
  requires axis < len(self.shape) && self.shape[axis] > 0
```

Where the largest element is along axis `axis` (the first, for ties), for each position of the other axes, in row-major order. The axis must not be empty.
Cost: O(n).

## Functions

### count

```rust
fn count(shape: List[Natural]) -> Natural
```

The number of elements of an array of shape `shape`: the product of its
lengths, 1 for no axes.
Cost: O(number of axes).

### row_major

```rust
fn row_major(shape: List[Natural]) -> List[Int]
  ensures len(it) == len(shape)
```

The strides of a row-major array of shape `shape`: the last axis is 1,
each axis before it the product of the lengths after it.
Cost: O(number of axes).

### full

```rust
fn full(shape: List[Natural], x: Float) -> Array
  ensures it.shape == shape
```

An array of `shape` with every element `x`.
Cost: O(n).

### zeros

```rust
fn zeros(shape: List[Natural]) -> Array
  ensures it.shape == shape
```

An array of `shape` filled with 0.0.
Cost: O(n).

### ones

```rust
fn ones(shape: List[Natural]) -> Array
  ensures it.shape == shape
```

An array of `shape` filled with 1.0.
Cost: O(n).

### arange

```rust
fn arange(n: Natural where it < 1 << 53) -> Array
  ensures it.shape == [n]
```

0.0, 1.0, …, up to `n - 1`: one axis of length `n`.
Cost: O(n).

### from_list

```rust
fn from_list(xs: List[Float]) -> Array
  ensures it.shape == [len(xs)]
```

One axis holding `xs`.
Cost: O(1): the array shares the list.

### from_rows

```rust
fn from_rows(rows: List[List[Float]]) -> Option[Array]
```

Two axes, one row per list. `None` when the rows differ in length.
Cost: O(n).

### broadcast_shape

```rust
fn broadcast_shape(
  a: List[Natural],
  b: List[Natural],
) -> Option[List[Natural]]
```

The shape two arrays broadcast to, NumPy's rule: lengths are compared from
the last axis, and a length of 1 stretches to the other's. `None` when two
lengths differ and neither is 1.
Cost: O(number of axes).

### broadcasts

```rust
fn broadcasts(a: List[Natural], b: List[Natural]) -> Bool
```

Whether `a` and `b` broadcast together (`broadcast_shape` is not `None`).

### without_axis

```rust
fn without_axis(shape: List[Natural], axis: Natural) -> List[Natural]
```

`shape` without axis `axis`.
Cost: O(number of axes).

### in_bounds

```rust
fn in_bounds(shape: List[Natural], idx: List[Int]) -> Bool
```

Whether `idx` names an element of an array of `shape`: one index per axis,
each from 0 to below its length.

### dot

```rust
fn dot(a: Array where len(it.shape) == 1, b: Array) -> Float
  requires a.shape == b.shape
```

The dot product of two arrays of one axis and the same length, added as
`sum` adds.
Cost: O(n).

### matmul

```rust
fn matmul(a: Matrix, b: Matrix) -> Matrix
  requires a.shape[1] == b.shape[0]
  ensures it.shape == [a.shape[0], b.shape[1]]
```

The matrix product of an m×k and a k×n matrix: an m×n matrix.
Cost: O(m·k·n) multiply-adds, one dot product of a row of `a` and a column
of `b` per element, after a row-major copy of `b`'s transpose (O(k·n)) so
both are read in order.
