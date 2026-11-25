from std.collections use SortedDict, SortedSet, Heap, sorted_dict, sorted_set, heap

type Job = { name: String, priority: Int }

fn main() {
  // a dictionary in key order, with the nearest keys around a missing one
  mut d: SortedDict[String, Int] = SortedDict {}
  for (i, w) in enumerate(["pear", "apple", "fig", "kiwi", "banana", "cherry", "apple"]) {
    mut d.insert(w, i)
  }
  print(d.items())
  print((d.len(), d.get("fig"), d.get("grape")))
  print((d.floor("grape"), d.ceiling("grape")))
  print((d.before("fig"), d.after("fig")))
  print((d.first(), d.last()))
  print(d.between("b", "g"))
  print((mut d.remove("fig"), mut d.remove("fig"), d.len()))
  print((mut d.pop_first(), mut d.pop_last()))
  print((d.keys(), d.values()))
  // a copy is its own value
  e := d
  mut d.insert("zucchini", 9)
  print((e.len(), d.len()))
  // many keys: removing every other one keeps the rest in order
  mut big := sorted_dict([((i * 37) % 1000, i) for i in 0..1000])
  for k in 0..1000 {
    if k % 2 == 0 {
      mut big.remove(k)
    }
  }
  print((big.len(), big.first(), big.last(), big.between(500, 510).map(e -> e.0)))
  // a set
  mut s := sorted_set([5, 3, 9, 1, 3])
  mut s.insert(4)
  print((s.to_list(), mut s.remove(9), mut s.remove(9), s.to_list()))
  print((s.floor(2), s.ceiling(2), s.before(1), s.after(5), s.between(2, 5)))
  let words: SortedSet[(Int, String)] = sorted_set([(2, "b"), (1, "z"), (2, "a")])
  print(words.to_list())
  // heaps: smallest first, by a key, by an order of one's own
  mut h: Heap[Int] = Heap {}
  for x in [5, 3, 9, 1, 3, 8] {
    mut h.push(x)
  }
  print((h.peek(), h.len(), h.to_list()))
  print((mut h.pop(), mut h.pop(), h.len()))
  mut jobs := Heap.by((j: Job) -> j.priority)
  mut jobs.push(Job { name: "mail", priority: 3 })
  mut jobs.push(Job { name: "build", priority: 1 })
  mut jobs.push(Job { name: "deploy", priority: 2 })
  print(jobs.to_list().map(j -> j.name))
  mut most := Heap.with((a: Int, b: Int) -> compare(b, a))
  for x in [5, 3, 9] {
    mut most.push(x)
  }
  print(most.to_list())
  // heap sort, and a heap made from a list in O(n)
  mut sorted := []
  mut from := heap([(i * 7919) % 101 for i in 0..101])
  while !from.is_empty() {
    match mut from.pop() {
      Some(x) => {
        mut sorted.push(x)
      },
      None => (),
    }
  }
  print(sorted == range(0, 101))
  let empty: Heap[String] = Heap {}
  print((empty.peek(), empty.to_list()))
}
