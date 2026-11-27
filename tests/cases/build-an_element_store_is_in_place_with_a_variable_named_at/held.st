/// Where each `(` at an even position is closed: 20,000 stores into
/// `ends`, the index popped off another list into a variable named `at`.
fn ends_of(n: Int) -> List[Int] {
  mut ends := []
  for _ in 0..n {
    mut ends.push(-1)
  }
  mut open := []
  for k in 0..n {
    if k % 2 == 0 {
      mut open.push(k)
    } else {
      at := mut open.pop() ?? -1
      if at >= 0 && at < n {
        ends[at] = k + 1
      }
    }
  }
  ends
}

/// The other list read in the stored value and in the index.
fn mirrored(n: Int) -> List[Int] {
  mut starts := []
  mut ends := []
  for i in 0..n {
    mut starts.push(i)
    mut ends.push(0)
  }
  for i in 0..n {
    j := n - 1 - i
    if j >= 0 && j < n {
      ends[i] = starts[j]
    }
  }
  for i in 0..n {
    if i >= 0 && i < n {
      ends[starts[i]] = ends[i] + 1
    }
  }
  ends
}

fn main() {
  n := 40000
  e := ends_of(n)
  print(get(e, 0) ?? 0)
  print(get(e, 1) ?? 0)
  m := mirrored(n)
  print(get(m, 0) ?? 0)
  at := 1
  mut xs := [5, 6]
  xs[at] = 7
  print(xs)
}
