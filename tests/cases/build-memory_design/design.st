from std.memory use frozen, root

/// Settings every task reads.
type Site = { name: String, pages: Dict[String, String], tags: List[String], greet: (String) -> String }

fn site(n: Int) -> Site {
  mut pages: Dict[String, String] = Dict {}
  for i in 0..n {
    mut pages.insert(f"p{i}", repeat("ab", i))
  }
  Site { name: "site", pages, tags: [f"t{i}" for i in 0..n], greet: s -> f"hi {s}" }
}

/// The bytes of the pages whose names start with `prefix`.
fn bytes_of(s: Site, prefix: String) -> Int {
  mut total := 0
  for (k, v) in s.pages {
    if starts_with(k, prefix) {
      total += byte_len(v)
    }
  }
  total
}

/// Tasks reading one frozen site; the result copied out of the snapshot.
fn shared_reads(s: Site) -> (Int, String, List[String]) {
  frozen(s, fs -> {
    total := scope {
      hs := [spawn bytes_of(fs, f"p{k}") for k in 0..4]
      mut t := 0
      for h in hs {
        t += await h
      }
      t
    }
    inner := frozen(fs.tags, ts -> len(ts) + len(fs.tags))
    g := fs.greet
    (total + inner, g(fs.name), fs.tags[0..3])
  })
}

type Reply = { status: Int, body: String, tags: List[String] }

fn answer(tags: List[String], i: Int) -> Reply {
  Reply { status: 200, body: f"r{i}", tags }
}

/// Requests answered each in a root: what the root captures and awaits is
/// copied in, and a record made in it holds the awaited reply.
fn rooted(tags: List[String], n: Int) -> Int {
  mut done := 0
  for i in 0..n {
    (kept, r) := root(() -> {
      reply := scope {
        h := spawn answer(tags, i)
        await h
      }
      head := Reply { ..reply, body: "" }
      (tags, head)
    })
    done += len(kept) + r.status + byte_len(r.body) + len(r.tags)
  }
  done
}

fn main() {
  s := site(40)
  (total, greeting, first) := shared_reads(s)
  print(total)
  print(greeting)
  print(first)
  print(rooted(s.tags, 100))
  print(bytes_of(s, "p1"))
}
