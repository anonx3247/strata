# std.array

N-dimensional arrays of floats and integers, stored flat with a shape and strides.

## Types

### Length

```rust
type Length = I64
  where it >= 0
```

A length, an index or a count of elements: a 64-bit integer that isn't
negative.

### Array

```rust
type Array = {
  data: List[Float],
  shape: List[Length],
  strides: List[I64],
  offset: I64,
}
  where len(it.shape) == len(it.strides)
```

An n-dimensional array of 64-bit floats (`Float`), NumPy's `float64`: a
view of a flat list. The arithmetic takes bounds on the elements'
magnitudes (`within`), so that every float result is finite.

- Element `(i0, i1, …)` is `data[offset + i0 * strides[0] + i1 *

strides[1] + …]`. Shapes, strides, offsets and indices are `I64`s.

- `shape` has one length per axis. A 0-axis array holds one value.
- `transpose`, `slice`, `row` and `reshape` of a row-major array are

views: O(number of axes), the data is shared.

- Arithmetic, reductions and `matmul` read row-major data. A view that

isn't row-major is copied once first (`compact`, O(n)).

- The constructors (`Array.zeros`, `Array.full`, `Array.arange`,

`Array.from_list`, `Array.from_rows`) keep strides and offset inside
`data`.

- Changing an element (`mut a.set`) copies the data first when another

array shares it.

### ArrayI64

```rust
type ArrayI64 = {
  data: List[I64],
  shape: List[Length],
  strides: List[I64],
  offset: I64,
}
  where len(it.shape) == len(it.strides)
```

An n-dimensional array of 64-bit integers (`I64`), NumPy's `int64`: a view
of a flat list. Arithmetic wraps modulo 2^64, as NumPy's does.

- Element `(i0, i1, …)` is `data[offset + i0 * strides[0] + i1 *

strides[1] + …]`. Shapes, strides, offsets and indices are `I64`s.

- `shape` has one length per axis. A 0-axis array holds one value.
- `transpose`, `slice`, `row` and `reshape` of a row-major array are

views: O(number of axes), the data is shared.

- Arithmetic, reductions and `matmul` read row-major data. A view that

isn't row-major is copied once first (`compact`, O(n)).

- The constructors (`ArrayI64.zeros`, `ArrayI64.full`, `ArrayI64.arange`,

`ArrayI64.from_list`, `ArrayI64.from_rows`) keep strides and offset inside
`data`.

- Changing an element (`mut a.set`) copies the data first when another

array shares it.

### ArrayI32

```rust
type ArrayI32 = {
  data: List[I32],
  shape: List[Length],
  strides: List[I64],
  offset: I64,
}
  where len(it.shape) == len(it.strides)
```

An n-dimensional array of 32-bit integers (`I32`), NumPy's `int32`: a view
of a flat list. Arithmetic wraps modulo 2^32, as NumPy's does. Sums are
64-bit (`I64`, an `ArrayI64` along an axis), as NumPy's `sum` of `int32`
is.

- Element `(i0, i1, …)` is `data[offset + i0 * strides[0] + i1 *

strides[1] + …]`. Shapes, strides, offsets and indices are `I64`s.

- `shape` has one length per axis. A 0-axis array holds one value.
- `transpose`, `slice`, `row` and `reshape` of a row-major array are

views: O(number of axes), the data is shared.

- Arithmetic, reductions and `matmul` read row-major data. A view that

isn't row-major is copied once first (`compact`, O(n)).

- The constructors (`ArrayI32.zeros`, `ArrayI32.full`, `ArrayI32.arange`,

`ArrayI32.from_list`, `ArrayI32.from_rows`) keep strides and offset inside
`data`.

- Changing an element (`mut a.set`) copies the data first when another

array shares it.

## Methods

### Array.full

```rust
fn full(shape: List[Length], x: Float) -> Array
  ensures it.shape == shape
```

An array of `shape` with every element `x`.
Cost: O(n).

### Array.zeros

```rust
fn zeros(shape: List[Length]) -> Array
  ensures it.shape == shape
```

An array of `shape` filled with 0.0.
Cost: O(n).

### Array.ones

```rust
fn ones(shape: List[Length]) -> Array
  ensures it.shape == shape
```

An array of `shape` filled with 1.0.
Cost: O(n).

### Array.arange

```rust
fn arange(n: Length) -> Array
  ensures it.shape == [n]
```

0.0, 1.0, …, up to `n - 1`: one axis of length `n`.
Cost: O(n).

### Array.from_list

```rust
fn from_list(xs: List[Float]) -> Array
```

One axis holding `xs`.
Cost: O(1): the array shares the list.

### Array.from_rows

```rust
fn from_rows(rows: List[List[Float]]) -> Option[Array]
```

Two axes, one row per list. `None` when the rows differ in length.
Cost: O(n).

### Array.dense

```rust
fn dense(data: List[Float], shape: List[Length]) -> Array
  ensures it.shape == shape
```

A row-major array of `shape` over `data`.

### Array.ndim

```rust
fn ndim(self) -> Length
```

The number of axes.
Cost: O(1).

### Array.size

```rust
fn size(self) -> Length
  ensures it == count(self.shape)
```

The number of elements.
Cost: O(number of axes).

### Array.within

```rust
fn within(self, b: Float) -> Bool
  assumed ensures it ==> all(self.to_list(), x -> abs(x) <= b)
```

Whether every element is at most `b` in magnitude. The float arithmetic's contracts use it, so that no result overflows.
Cost: O(n), plus the copy `to_list` makes of a view that isn't row-major.

### Array.at_least

```rust
fn at_least(self, m: Float) -> Bool
  assumed ensures it ==> all(self.to_list(), x -> abs(x) >= m)
```

Whether every element is at least `m` in magnitude: `div`'s divisor is at least the inverse of `factor_bound`.
Cost: O(n), as `within`.

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

### Array.get

```rust
fn get(self, idx: List[I64]) -> Option[Float]
```

The element at `idx`, one index per axis. `None` when `idx` is out of bounds.
Cost: O(number of axes).

### Array.set

```rust
fn set(mut self, idx: List[I64], x: Float) -> Bool
```

Element `idx` set to `x`. Returns false, and changes nothing, when `idx` is out of bounds.
Cost: O(number of axes) when nothing else holds the data, else O(n): a copy.

### Array.reshape

```rust
fn reshape(self, shape: List[Length]) -> Array
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
fn slice(self, axis: Length, start: Length, stop: Length) -> Array
  requires axis.to_int() < len(self.shape)
  ensures len(it.shape) == len(self.shape)
```

Indices `start` to below `stop` of axis `axis`, both clamped to the axis.
Cost: O(number of axes): a view.

### Array.row

```rust
fn row(self, i: Length) -> Array
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

### Array.pair

```rust
fn pair(self, other: Array) -> (List[Float], List[Float], List[Length])
```

The elements of `self` and `other` in row-major order of the shape they broadcast to.

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
  requires self.within(add_bound()) && other.within(add_bound())
```

Element by element, broadcast. Both `within(add_bound())`, so no sum overflows.
Cost: O(n).

### Array.sub

```rust
fn sub(self, other: Array) -> Array
  requires broadcasts(self.shape, other.shape)
  requires self.within(add_bound()) && other.within(add_bound())
```

Element by element, broadcast. Both `within(add_bound())`, so no difference overflows.
Cost: O(n).

### Array.mul

```rust
fn mul(self, other: Array) -> Array
  requires broadcasts(self.shape, other.shape)
  requires self.within(factor_bound()) && other.within(factor_bound())
```

Element by element, broadcast. Both `within(factor_bound())`, so no product overflows.
Cost: O(n).

### Array.div

```rust
fn div(self, other: Array) -> Array
  requires broadcasts(self.shape, other.shape)
  requires self.within(factor_bound())
    && other.at_least(1.0 / factor_bound())
```

Element by element, broadcast. `self` `within(factor_bound())` and `other` `at_least` its inverse (so no element of `other` is 0.0): no quotient overflows.
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
  requires self.within(product_bound() / max(abs(k), 1.0))
  ensures it.shape == self.shape
```

Each element times `k`. Every element times `abs(k)` (or 1.0, if larger) at most `product_bound` in magnitude.
Cost: O(n).

### Array.shift

```rust
fn shift(self, k: Float) -> Array
  requires self.within(add_bound()) && abs(k) <= add_bound()
  ensures it.shape == self.shape
```

`k` added to each element. Every element and `k` at most `add_bound` in magnitude.
Cost: O(n).

### Array.sum

```rust
fn sum(self) -> Float
  requires self.size() < 1 << 40 && self.within(sum_bound())
```

The sum of the elements; 0.0 when there are none. They are added in row-major order into four interleaved partial sums, then those four, so the result can differ from a left-to-right sum in the last bits. At most 2^40 elements, `within(sum_bound())`, so no partial sum overflows.
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
fn argmax(self) -> Option[Length]
```

Where the largest element is, in row-major order (the first, for ties); `None` when there are none.
Cost: O(n).

### Array.sum_axis

```rust
fn sum_axis(self, axis: Length) -> Array
  requires axis.to_int() < len(self.shape)
  requires self.shape[axis.to_int()] < 1 << 40 && self.within(sum_bound())
  ensures it.shape == without_axis(self.shape, axis)
```

The sums along axis `axis`: the result has the other axes. A matrix's `sum_axis(0)` sums each column. The axis at most 2^40 long and the array `within(sum_bound())`, as `sum` requires.
Cost: O(n).

### Array.max_axis

```rust
fn max_axis(self, axis: Length) -> Array
  requires axis.to_int() < len(self.shape) && self.shape[axis.to_int()] > 0
  ensures it.shape == without_axis(self.shape, axis)
```

The largest elements along axis `axis`, which must not be empty.
Cost: O(n).

### Array.argmax_axis

```rust
fn argmax_axis(self, axis: Length) -> ArrayI64
  requires axis.to_int() < len(self.shape) && self.shape[axis.to_int()] > 0
  ensures it.shape == without_axis(self.shape, axis)
```

Where the largest element is along axis `axis` (the first, for ties), for each position of the other axes, in row-major order of those. The axis must not be empty.
Cost: O(n).

### Array.dot

```rust
fn dot(self, other: Array) -> Float
  requires len(self.shape) == 1 && self.shape == other.shape
  requires self.size() < 1 << 40
    && self.within(dot_bound())
    && other.within(dot_bound())
```

The dot product of two arrays of one axis and the same length, added as `sum` adds. At most 2^40 elements, both `within(dot_bound())`, so no partial sum overflows.
Cost: O(n).

### Array.matmul

```rust
fn matmul(self, other: Array) -> Array
  requires len(self.shape) == 2
    && len(other.shape) == 2
    && self.shape[1] == other.shape[0]
  requires self.shape[1] < 1 << 40
    && self.within(dot_bound())
    && other.within(dot_bound())
  ensures it.shape == [self.shape[0], other.shape[1]]
```

The matrix product of an m×k and a k×n matrix: an m×n matrix, added as `sum` adds. At most 2^40 columns of `self`, both `within(dot_bound())`, as `dot` requires.
Cost: O(m·k·n) multiply-adds, one dot product of a row of `self` and a column of `other` per element, after a row-major copy of `other`'s transpose (O(k·n)) so both are read in order.

### Array.mean

```rust
fn mean(self) -> Float
  requires count(self.shape) > 0
  requires self.size() < 1 << 40 && self.within(sum_bound())
```

The mean of the elements. The array must not be empty, and bounded as `sum` requires.
Cost: O(n).

### Array.mean_axis

```rust
fn mean_axis(self, axis: Length) -> Array
  requires axis.to_int() < len(self.shape) && self.shape[axis.to_int()] > 0
  requires self.shape[axis.to_int()] < 1 << 40 && self.within(sum_bound())
  ensures it.shape == without_axis(self.shape, axis)
```

The means along axis `axis`, which must not be empty, and bounded as `sum_axis` requires.
Cost: O(n).

### ArrayI64.full

```rust
fn full(shape: List[Length], x: I64) -> ArrayI64
  ensures it.shape == shape
```

An array of `shape` with every element `x`.
Cost: O(n).

### ArrayI64.zeros

```rust
fn zeros(shape: List[Length]) -> ArrayI64
  ensures it.shape == shape
```

An array of `shape` filled with 0.
Cost: O(n).

### ArrayI64.ones

```rust
fn ones(shape: List[Length]) -> ArrayI64
  ensures it.shape == shape
```

An array of `shape` filled with 1.
Cost: O(n).

### ArrayI64.arange

```rust
fn arange(n: Length) -> ArrayI64
  ensures it.shape == [n]
```

0, 1, …, up to `n - 1`: one axis of length `n`.
Cost: O(n).

### ArrayI64.from_list

```rust
fn from_list(xs: List[I64]) -> ArrayI64
```

One axis holding `xs`.
Cost: O(1): the array shares the list.

### ArrayI64.from_rows

```rust
fn from_rows(rows: List[List[I64]]) -> Option[ArrayI64]
```

Two axes, one row per list. `None` when the rows differ in length.
Cost: O(n).

### ArrayI64.dense

```rust
fn dense(data: List[I64], shape: List[Length]) -> ArrayI64
  ensures it.shape == shape
```

A row-major array of `shape` over `data`.

### ArrayI64.ndim

```rust
fn ndim(self) -> Length
```

The number of axes.
Cost: O(1).

### ArrayI64.size

```rust
fn size(self) -> Length
```

The number of elements.
Cost: O(number of axes).

### ArrayI64.is_compact

```rust
fn is_compact(self) -> Bool
```

Whether the array reads its data row-major from its start to its end, so the data is its elements in order.
Cost: O(number of axes).

### ArrayI64.to_list

```rust
fn to_list(self) -> List[I64]
```

The elements in row-major order.
Cost: O(1) when `is_compact`, else O(n): a copy.

### ArrayI64.compact

```rust
fn compact(self) -> ArrayI64
  ensures it.shape == self.shape
```

The same elements, row-major.
Cost: O(1) when `is_compact`, else O(n): a copy.

### ArrayI64.get

```rust
fn get(self, idx: List[I64]) -> Option[I64]
```

The element at `idx`, one index per axis. `None` when `idx` is out of bounds.
Cost: O(number of axes).

### ArrayI64.set

```rust
fn set(mut self, idx: List[I64], x: I64) -> Bool
```

Element `idx` set to `x`. Returns false, and changes nothing, when `idx` is out of bounds.
Cost: O(number of axes) when nothing else holds the data, else O(n): a copy.

### ArrayI64.reshape

```rust
fn reshape(self, shape: List[Length]) -> ArrayI64
  requires count(shape) == count(self.shape)
  ensures it.shape == shape
```

The same elements in `shape`, which must hold as many.
Cost: O(number of axes) when `is_compact` (a view), else O(n): a copy.

### ArrayI64.transpose

```rust
fn transpose(self) -> ArrayI64
  ensures it.shape == self.shape.reversed()
```

The axes in reverse order: element `(i, j)` of the result is element `(j, i)` of a matrix.
Cost: O(number of axes): a view.

### ArrayI64.slice

```rust
fn slice(self, axis: Length, start: Length, stop: Length) -> ArrayI64
  requires axis.to_int() < len(self.shape)
  ensures len(it.shape) == len(self.shape)
```

Indices `start` to below `stop` of axis `axis`, both clamped to the axis.
Cost: O(number of axes): a view.

### ArrayI64.row

```rust
fn row(self, i: Length) -> ArrayI64
  requires len(self.shape) > 0 && i < self.shape[0]
  ensures len(it.shape) == len(self.shape) - 1
```

Index `i` of axis 0, one axis fewer: a matrix's row `i`.
Cost: O(number of axes): a view.

### ArrayI64.map

```rust
fn map(self, f: (I64) -> I64) -> ArrayI64
  ensures it.shape == self.shape
```

`f` of each element.
Cost: O(n) calls of `f`.

### ArrayI64.pair

```rust
fn pair(self, other: ArrayI64) -> (List[I64], List[I64], List[Length])
```

The elements of `self` and `other` in row-major order of the shape they broadcast to.

### ArrayI64.zip_with

```rust
fn zip_with(self, other: ArrayI64, f: (I64, I64) -> I64) -> ArrayI64
  requires broadcasts(self.shape, other.shape)
```

`f` of each pair of elements, broadcast: the shapes must broadcast together.
Cost: O(n) calls of `f`, n the size of the broadcast shape.

### ArrayI64.add

```rust
fn add(self, other: ArrayI64) -> ArrayI64
  requires broadcasts(self.shape, other.shape)
```

Element by element, broadcast, modulo 2^64.
Cost: O(n).

### ArrayI64.sub

```rust
fn sub(self, other: ArrayI64) -> ArrayI64
  requires broadcasts(self.shape, other.shape)
```

Element by element, broadcast, modulo 2^64.
Cost: O(n).

### ArrayI64.mul

```rust
fn mul(self, other: ArrayI64) -> ArrayI64
  requires broadcasts(self.shape, other.shape)
```

Element by element, broadcast, modulo 2^64.
Cost: O(n).

### ArrayI64.neg

```rust
fn neg(self) -> ArrayI64
  ensures it.shape == self.shape
```

Each element negated, modulo 2^64.
Cost: O(n).

### ArrayI64.scale

```rust
fn scale(self, k: I64) -> ArrayI64
  ensures it.shape == self.shape
```

Each element times `k`, modulo 2^64.
Cost: O(n).

### ArrayI64.shift

```rust
fn shift(self, k: I64) -> ArrayI64
  ensures it.shape == self.shape
```

`k` added to each element, modulo 2^64.
Cost: O(n).

### ArrayI64.sum

```rust
fn sum(self) -> I64
```

The sum of the elements, modulo 2^64; 0 when there are none.
Cost: O(n).

### ArrayI64.max

```rust
fn max(self) -> Option[I64]
```

The largest element, `None` when there are none.
Cost: O(n).

### ArrayI64.min

```rust
fn min(self) -> Option[I64]
```

The smallest element, `None` when there are none.
Cost: O(n).

### ArrayI64.argmax

```rust
fn argmax(self) -> Option[Length]
```

Where the largest element is, in row-major order (the first, for ties); `None` when there are none.
Cost: O(n).

### ArrayI64.sum_axis

```rust
fn sum_axis(self, axis: Length) -> ArrayI64
  requires axis.to_int() < len(self.shape)
  ensures it.shape == without_axis(self.shape, axis)
```

The sums along axis `axis`: the result has the other axes. A matrix's `sum_axis(0)` sums each column.
Cost: O(n).

### ArrayI64.max_axis

```rust
fn max_axis(self, axis: Length) -> ArrayI64
  requires axis.to_int() < len(self.shape) && self.shape[axis.to_int()] > 0
  ensures it.shape == without_axis(self.shape, axis)
```

The largest elements along axis `axis`, which must not be empty.
Cost: O(n).

### ArrayI64.argmax_axis

```rust
fn argmax_axis(self, axis: Length) -> ArrayI64
  requires axis.to_int() < len(self.shape) && self.shape[axis.to_int()] > 0
  ensures it.shape == without_axis(self.shape, axis)
```

Where the largest element is along axis `axis` (the first, for ties), for each position of the other axes, in row-major order of those. The axis must not be empty.
Cost: O(n).

### ArrayI64.dot

```rust
fn dot(self, other: ArrayI64) -> I64
  requires len(self.shape) == 1 && self.shape == other.shape
```

The dot product of two arrays of one axis and the same length, modulo 2^64.
Cost: O(n).

### ArrayI64.matmul

```rust
fn matmul(self, other: ArrayI64) -> ArrayI64
  requires len(self.shape) == 2
    && len(other.shape) == 2
    && self.shape[1] == other.shape[0]
  ensures it.shape == [self.shape[0], other.shape[1]]
```

The matrix product of an m×k and a k×n matrix: an m×n matrix, modulo 2^64.
Cost: O(m·k·n) multiply-adds, one dot product of a row of `self` and a column of `other` per element, after a row-major copy of `other`'s transpose (O(k·n)) so both are read in order.

### ArrayI64.to_float

```rust
fn to_float(self) -> Array
  ensures it.shape == self.shape
```

The elements as floats (`Array`), each the nearest `Float`.
Cost: O(n).

### ArrayI32.full

```rust
fn full(shape: List[Length], x: I32) -> ArrayI32
  ensures it.shape == shape
```

An array of `shape` with every element `x`.
Cost: O(n).

### ArrayI32.zeros

```rust
fn zeros(shape: List[Length]) -> ArrayI32
  ensures it.shape == shape
```

An array of `shape` filled with 0.
Cost: O(n).

### ArrayI32.ones

```rust
fn ones(shape: List[Length]) -> ArrayI32
  ensures it.shape == shape
```

An array of `shape` filled with 1.
Cost: O(n).

### ArrayI32.arange

```rust
fn arange(n: Length) -> ArrayI32
  ensures it.shape == [n]
```

0, 1, …, up to `n - 1` (modulo 2^32): one axis of length `n`.
Cost: O(n).

### ArrayI32.from_list

```rust
fn from_list(xs: List[I32]) -> ArrayI32
```

One axis holding `xs`.
Cost: O(1): the array shares the list.

### ArrayI32.from_rows

```rust
fn from_rows(rows: List[List[I32]]) -> Option[ArrayI32]
```

Two axes, one row per list. `None` when the rows differ in length.
Cost: O(n).

### ArrayI32.dense

```rust
fn dense(data: List[I32], shape: List[Length]) -> ArrayI32
  ensures it.shape == shape
```

A row-major array of `shape` over `data`.

### ArrayI32.ndim

```rust
fn ndim(self) -> Length
```

The number of axes.
Cost: O(1).

### ArrayI32.size

```rust
fn size(self) -> Length
```

The number of elements.
Cost: O(number of axes).

### ArrayI32.is_compact

```rust
fn is_compact(self) -> Bool
```

Whether the array reads its data row-major from its start to its end, so the data is its elements in order.
Cost: O(number of axes).

### ArrayI32.to_list

```rust
fn to_list(self) -> List[I32]
```

The elements in row-major order.
Cost: O(1) when `is_compact`, else O(n): a copy.

### ArrayI32.compact

```rust
fn compact(self) -> ArrayI32
  ensures it.shape == self.shape
```

The same elements, row-major.
Cost: O(1) when `is_compact`, else O(n): a copy.

### ArrayI32.get

```rust
fn get(self, idx: List[I64]) -> Option[I32]
```

The element at `idx`, one index per axis. `None` when `idx` is out of bounds.
Cost: O(number of axes).

### ArrayI32.set

```rust
fn set(mut self, idx: List[I64], x: I32) -> Bool
```

Element `idx` set to `x`. Returns false, and changes nothing, when `idx` is out of bounds.
Cost: O(number of axes) when nothing else holds the data, else O(n): a copy.

### ArrayI32.reshape

```rust
fn reshape(self, shape: List[Length]) -> ArrayI32
  requires count(shape) == count(self.shape)
  ensures it.shape == shape
```

The same elements in `shape`, which must hold as many.
Cost: O(number of axes) when `is_compact` (a view), else O(n): a copy.

### ArrayI32.transpose

```rust
fn transpose(self) -> ArrayI32
  ensures it.shape == self.shape.reversed()
```

The axes in reverse order: element `(i, j)` of the result is element `(j, i)` of a matrix.
Cost: O(number of axes): a view.

### ArrayI32.slice

```rust
fn slice(self, axis: Length, start: Length, stop: Length) -> ArrayI32
  requires axis.to_int() < len(self.shape)
  ensures len(it.shape) == len(self.shape)
```

Indices `start` to below `stop` of axis `axis`, both clamped to the axis.
Cost: O(number of axes): a view.

### ArrayI32.row

```rust
fn row(self, i: Length) -> ArrayI32
  requires len(self.shape) > 0 && i < self.shape[0]
  ensures len(it.shape) == len(self.shape) - 1
```

Index `i` of axis 0, one axis fewer: a matrix's row `i`.
Cost: O(number of axes): a view.

### ArrayI32.map

```rust
fn map(self, f: (I32) -> I32) -> ArrayI32
  ensures it.shape == self.shape
```

`f` of each element.
Cost: O(n) calls of `f`.

### ArrayI32.pair

```rust
fn pair(self, other: ArrayI32) -> (List[I32], List[I32], List[Length])
```

The elements of `self` and `other` in row-major order of the shape they broadcast to.

### ArrayI32.zip_with

```rust
fn zip_with(self, other: ArrayI32, f: (I32, I32) -> I32) -> ArrayI32
  requires broadcasts(self.shape, other.shape)
```

`f` of each pair of elements, broadcast: the shapes must broadcast together.
Cost: O(n) calls of `f`, n the size of the broadcast shape.

### ArrayI32.add

```rust
fn add(self, other: ArrayI32) -> ArrayI32
  requires broadcasts(self.shape, other.shape)
```

Element by element, broadcast, modulo 2^32.
Cost: O(n).

### ArrayI32.sub

```rust
fn sub(self, other: ArrayI32) -> ArrayI32
  requires broadcasts(self.shape, other.shape)
```

Element by element, broadcast, modulo 2^32.
Cost: O(n).

### ArrayI32.mul

```rust
fn mul(self, other: ArrayI32) -> ArrayI32
  requires broadcasts(self.shape, other.shape)
```

Element by element, broadcast, modulo 2^32.
Cost: O(n).

### ArrayI32.neg

```rust
fn neg(self) -> ArrayI32
  ensures it.shape == self.shape
```

Each element negated, modulo 2^32.
Cost: O(n).

### ArrayI32.scale

```rust
fn scale(self, k: I32) -> ArrayI32
  ensures it.shape == self.shape
```

Each element times `k`, modulo 2^32.
Cost: O(n).

### ArrayI32.shift

```rust
fn shift(self, k: I32) -> ArrayI32
  ensures it.shape == self.shape
```

`k` added to each element, modulo 2^32.
Cost: O(n).

### ArrayI32.sum

```rust
fn sum(self) -> I64
```

The sum of the elements as an `I64`, modulo 2^64; 0 when there are none.
Cost: O(n).

### ArrayI32.max

```rust
fn max(self) -> Option[I32]
```

The largest element, `None` when there are none.
Cost: O(n).

### ArrayI32.min

```rust
fn min(self) -> Option[I32]
```

The smallest element, `None` when there are none.
Cost: O(n).

### ArrayI32.argmax

```rust
fn argmax(self) -> Option[Length]
```

Where the largest element is, in row-major order (the first, for ties); `None` when there are none.
Cost: O(n).

### ArrayI32.sum_axis

```rust
fn sum_axis(self, axis: Length) -> ArrayI64
  requires axis.to_int() < len(self.shape)
  ensures it.shape == without_axis(self.shape, axis)
```

The sums along axis `axis`: the result has the other axes. A matrix's `sum_axis(0)` sums each column.
Cost: O(n).

### ArrayI32.max_axis

```rust
fn max_axis(self, axis: Length) -> ArrayI32
  requires axis.to_int() < len(self.shape) && self.shape[axis.to_int()] > 0
  ensures it.shape == without_axis(self.shape, axis)
```

The largest elements along axis `axis`, which must not be empty.
Cost: O(n).

### ArrayI32.argmax_axis

```rust
fn argmax_axis(self, axis: Length) -> ArrayI64
  requires axis.to_int() < len(self.shape) && self.shape[axis.to_int()] > 0
  ensures it.shape == without_axis(self.shape, axis)
```

Where the largest element is along axis `axis` (the first, for ties), for each position of the other axes, in row-major order of those. The axis must not be empty.
Cost: O(n).

### ArrayI32.dot

```rust
fn dot(self, other: ArrayI32) -> I32
  requires len(self.shape) == 1 && self.shape == other.shape
```

The dot product of two arrays of one axis and the same length, modulo 2^32.
Cost: O(n).

### ArrayI32.matmul

```rust
fn matmul(self, other: ArrayI32) -> ArrayI32
  requires len(self.shape) == 2
    && len(other.shape) == 2
    && self.shape[1] == other.shape[0]
  ensures it.shape == [self.shape[0], other.shape[1]]
```

The matrix product of an m×k and a k×n matrix: an m×n matrix, modulo 2^32.
Cost: O(m·k·n) multiply-adds, one dot product of a row of `self` and a column of `other` per element, after a row-major copy of `other`'s transpose (O(k·n)) so both are read in order.

### ArrayI32.to_float

```rust
fn to_float(self) -> Array
  ensures it.shape == self.shape
```

The elements as floats (`Array`), each the nearest `Float`.
Cost: O(n).

### ArrayI32.to_i64

```rust
fn to_i64(self) -> ArrayI64
  ensures it.shape == self.shape
```

The elements as 64-bit integers (`ArrayI64`).
Cost: O(n).

## Functions

### count

```rust
fn count(shape: List[Length]) -> Length
```

The number of elements of an array of shape `shape`: the product of its
lengths, 1 for no axes. A product past 2^62 gives 2^62, more than any list
holds.
Cost: O(number of axes).

### row_major

```rust
fn row_major(shape: List[Length]) -> List[I64]
  ensures len(it) == len(shape)
```

The strides of a row-major array of shape `shape`: the last axis is 1,
each axis before it the product of the lengths after it.
Cost: O(number of axes).

### broadcast_shape

```rust
fn broadcast_shape(a: List[Length], b: List[Length]) -> Option[List[Length]]
```

The shape two arrays broadcast to, NumPy's rule: lengths are compared from
the last axis, and a length of 1 stretches to the other's. `None` when two
lengths differ and neither is 1.
Cost: O(number of axes).

### broadcasts

```rust
fn broadcasts(a: List[Length], b: List[Length]) -> Bool
```

Whether `a` and `b` broadcast together (`broadcast_shape` is not `None`).

### without_axis

```rust
fn without_axis(shape: List[Length], axis: Length) -> List[Length]
```

`shape` without axis `axis`.
Cost: O(number of axes).

### in_bounds

```rust
fn in_bounds(shape: List[Length], idx: List[I64]) -> Bool
```

Whether `idx` names an element of an array of `shape`: one index per axis,
each from 0 to below its length.

### full

```rust
fn full(shape: List[Length], x: Float) -> Array
  ensures it.shape == shape
```

An array of 64-bit floats of `shape` with every element `x`.
Cost: O(n).

### zeros

```rust
fn zeros(shape: List[Length]) -> Array
  ensures it.shape == shape
```

An array of 64-bit floats of `shape` filled with 0.0.
Cost: O(n).

### ones

```rust
fn ones(shape: List[Length]) -> Array
  ensures it.shape == shape
```

An array of 64-bit floats of `shape` filled with 1.0.
Cost: O(n).

### arange

```rust
fn arange(n: Length) -> Array
  ensures it.shape == [n]
```

0.0, 1.0, …, up to `n - 1`: one axis of length `n`.
Cost: O(n).

### from_list

```rust
fn from_list(xs: List[Float]) -> Array
```

One axis holding `xs`.
Cost: O(1): the array shares the list.

### from_rows

```rust
fn from_rows(rows: List[List[Float]]) -> Option[Array]
```

Two axes, one row per list. `None` when the rows differ in length.
Cost: O(n).

### add_bound

```rust
fn add_bound() -> Float
```

2^1022, about 4.5e307: the largest magnitude of the operands of `add`,
`sub` and `shift`, so their sums stay finite.

### product_bound

```rust
fn product_bound() -> Float
```

2^1023, about 9.0e307: the most `scale`'s products can reach.

### factor_bound

```rust
fn factor_bound() -> Float
```

2^511, about 6.7e153: the largest magnitude of the operands of `mul`, and
of `div`'s dividend; a divisor's are at least its inverse.

### sum_bound

```rust
fn sum_bound() -> Float
```

2^960, about 9.7e288: the largest magnitude `sum`, `mean` and the
reductions along an axis take, so that 2^40 elements add up to a finite
number.

### dot_bound

```rust
fn dot_bound() -> Float
```

2^480, about 3.1e144: the largest magnitude of the operands of `dot` and
`matmul`, so that 2^40 products add up to a finite number.
