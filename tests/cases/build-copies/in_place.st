// Updates that docs/language.md ("Values and copying") says are in place
// when nothing else holds the value: each run at two sizes, `--copies`
// reports none, and the built program runs them at 200,000 in well under
// the case's time limit (each is linear).

from std.process use Process

type Word = Int
  where 0 <= it && it < 4294967296

type Side = { words: Dict[Int, Int], n: Int }

type Book = { bids: Side, asks: Side }

type St = { xs: List[Int], d: Dict[Int, Int] }

type Refined = { words: Dict[Int, Word], list: List[Word] }

type Counter = { d: Dict[Int, Int], xs: List[Int], name: String }

type Acc = { n: Int, total: Int }

type In = { xs: List[Int], s: String }

type Out = { inner: In, n: Int }

impl Counter {
  fn add(mut self, i: Int) {
    self.d = insert(self.d, i, i)
    self.xs = push(self.xs, i)
    self.name += "x"
  }
}

/// A dict changed through a variable: inserts, and removes (O(1) each).
fn dict_local(n: Int) -> Int {
  mut d := Dict[Int, Int] {}
  for i in 0..n {
    d = insert(d, i, i)
    if i % 3 == 0 {
      d = remove(d, i - 3)
    }
  }
  len(d)
}

/// A set, the same.
fn set_local(n: Int) -> Int {
  mut s := Set[Int] {}
  for i in 0..n {
    s = insert(s, i)
    if i % 2 == 0 {
      s = remove(s, i / 2)
    }
  }
  len(s)
}

fn add_to(mut s: St, i: Int) {
  s.xs = push(s.xs, i)
  v := get(s.d, i / 2) ?? 0
  s.d = insert(s.d, i / 2, v + 1)
}

/// Fields of a record given to a `mut` parameter.
fn mut_param(n: Int) -> Int {
  mut s := St { xs: [], d: Dict[Int, Int] {} }
  for i in 0..n {
    add_to(mut s, i)
  }
  len(s.xs) + len(s.d)
}

fn bump(mut s: Side, i: Int) {
  s.words = insert(s.words, i, i)
  s.n += 1
}

/// A record inside a record, given to a `mut` parameter, and assigned
/// through two fields.
fn nested(n: Int) -> Int {
  mut b := Book {
    bids: Side { words: Dict[Int, Int] {}, n: 0 },
    asks: Side { words: Dict[Int, Int] {}, n: 0 },
  }
  for i in 0..n {
    bump(mut b.bids, i)
    b.asks.words = insert(b.asks.words, i, i)
    b.asks.n += 1
  }
  len(b.bids.words) + len(b.asks.words) + b.bids.n + b.asks.n
}

/// Containers of a refined element type: each update checks what it adds.
fn refined(n: Int) -> Int {
  mut r := Refined { words: Dict[Int, Word] {}, list: [] }
  for i in 0..n {
    r.words = insert(r.words, i, i % 1000)
    r.list = push(r.list, i % 7)
  }
  len(r.words) + len(r.list)
}

/// A `mut self` method.
fn method(n: Int) -> Int {
  mut c := Counter { d: Dict[Int, Int] {}, xs: [], name: "" }
  for i in 0..n {
    mut c.add(i)
  }
  len(c.d) + len(c.xs) + byte_len(c.name)
}

/// `+=` on a list and on a string, through a variable and a field.
fn extend(n: Int) -> Int {
  mut xs: List[Int] = []
  mut s := ""
  mut o := Out { inner: In { xs: [], s: "" }, n: 0 }
  for i in 0..n {
    xs += [i]
    s += "ab"
    o.inner.xs = push(o.inner.xs, i)
    o.inner.s = o.inner.s + "c"
  }
  len(xs) + byte_len(s) + len(o.inner.xs) + byte_len(o.inner.s)
}

/// A dict of lists, of records, and of dicts (the inner dict taken out of
/// the outer one before it changes).
fn containers(n: Int) -> Int {
  mut lists := Dict[Int, List[Int]] {}
  mut accs := Dict[Int, Acc] {}
  mut dicts := Dict[Int, Dict[Int, Int]] {}
  for i in 0..n {
    lists = insert(lists, i % 10, push(get(lists, i % 10) ?? [], i))
    a := get(accs, i % 100) ?? Acc { n: 0, total: 0 }
    accs = insert(accs, i % 100, Acc { n: a.n + 1, total: a.total + i })
    mut inner := get(dicts, i % 10) ?? Dict[Int, Int] {}
    dicts = remove(dicts, i % 10)
    inner = insert(inner, i, i)
    dicts = insert(dicts, i % 10, inner)
  }
  len(lists) + len(accs) + len(dicts)
}

/// A temporary dict changed: `insert(insert(…), …)`.
fn temporary(n: Int) -> Int {
  mut total := 0
  for i in 0..n {
    d := insert(insert(Dict[Int, Int] { 1: 1 }, i, i), i + 1, i)
    total += len(d)
  }
  total
}

fn all(n: Int) -> List[Int] {
  [
    dict_local(n),
    set_local(n),
    mut_param(n),
    nested(n),
    refined(n),
    method(n),
    extend(n),
    containers(n),
    temporary(n),
  ]
}

fn main(process: Process) {
  match get(process.args(), 0) {
    Some(a) => print(all(parse_int(a) ?? 0)),
    None => {
      print(all(1000))
      print(all(4000))
    },
  }
}
