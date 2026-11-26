// std.array: construction, views, broadcasting, reductions and matmul
from std.array use Array, ArrayI64, ArrayI32, zeros, ones, full, arange, from_list, from_rows, broadcast_shape

fn main() {
  a := arange(6).reshape([2, 3])
  print((a.shape, a.strides, a.to_list()))
  print((a.get([1, 2]), a.get([2, 0]), a.get([0])))
  t := a.transpose()
  print((t.shape, t.is_compact(), t.to_list(), t.get([2, 1])))
  print(t.compact().is_compact())
  print(a.row(1).to_list())
  print(a.slice(1, 1, 3).to_list())
  print(t.slice(0, 1, 9).to_list())
  print(a.add(a).to_list())
  print(a.sub(ones([2, 3])).to_list())
  // broadcasting: a row added to each row, a column multiplied into each column
  print(a.add(from_list([10.0, 20.0, 30.0])).to_list())
  col := from_list([1.0, -1.0]).reshape([2, 1])
  print(a.mul(col).to_list())
  print(a.div(full([], 2.0)).to_list())
  print((broadcast_shape([2, 3], [3]), broadcast_shape([2, 1], [1, 4]), broadcast_shape([2, 3], [2])))
  print((a.sum(), a.mean(), a.max(), a.min(), a.argmax()))
  print((a.sum_axis(0).to_list(), a.sum_axis(1).to_list()))
  print((a.mean_axis(0).to_list(), a.max_axis(1).to_list(), t.argmax_axis(0).to_list()))
  print((zeros([0, 3]).sum_axis(0).to_list(), zeros([2, 0]).sum_axis(1).to_list(), zeros([0]).max()))
  print(a.neg().scale(2.0).shift(1.0).to_list())
  print(a.map(x -> x * x).to_list())
  print(a.zip_with(t.transpose(), (x, y) -> x - y).to_list())
  print(from_list([1.0, 2.0, 3.0]).dot(from_list([4.0, 5.0, 6.0])))
  m := a.matmul(t)
  print((m.shape, m.to_list()))
  print(t.matmul(a).to_list())
  print(a.transpose().transpose().matmul(ones([3, 1])).to_list())
  print(match from_rows([[1.0, 2.0], [3.0, 4.0]]) {
    Some(r) => r.matmul(r).to_list(),
    None => [],
  })
  print(from_rows([[1.0], [2.0, 3.0]]) == None)
  mut b := zeros([2, 2])
  ok := mut b.set([1, 0], 5.0)
  bad := mut b.set([2, 0], 1.0)
  print((ok, bad, b.to_list()))
  // a 3-axis array: sums over each axis, a transposed view's reductions
  c := arange(24).reshape([2, 3, 4])
  print((c.sum_axis(0).shape, c.sum_axis(1).to_list(), c.sum_axis(2).to_list()))
  print(c.transpose().sum_axis(0).to_list())
  print(c.row(1).row(2).to_list())
  // integer arrays: I64 and I32, wrapping as NumPy's do; I32 sums are I64
  i := ArrayI64.arange(6).reshape([2, 3])
  print((i.to_list(), i.transpose().to_list(), i.sum(), i.sum_axis(0).to_list(), i.max_axis(1).to_list()))
  print((i.add(ArrayI64.from_list([10, 20, 30])).to_list(), i.mul(i).to_list(), i.matmul(i.transpose()).to_list()))
  print((ArrayI64.full([2], 9223372036854775807).shift(1).to_list(), i.to_float().mean()))
  j := ArrayI32.arange(6).reshape([3, 2])
  print((j.to_list(), j.sum(), j.sum_axis(1).to_list(), j.matmul(j.transpose()).shape, j.argmax(), j.to_i64().to_list()))
  print((ArrayI32.full([3], 2147483647).sum(), ArrayI32.full([1], 2147483647).shift(1).to_list(), j.get([2, 1]), j.get([3, 0])))
}
