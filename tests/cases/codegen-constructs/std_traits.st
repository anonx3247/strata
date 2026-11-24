from std.fs use Files, FsError, mem_files

type Ctx = { files: Option[Files], n: Int }

fn has(c: Ctx, path: String) -> Bool {
  match c.files {
    Some(fs) => fs.exists(path),
    None => false,
  }
}

fn without(d: Dict[String, Int], k: String) -> Dict[String, Int] {
  remove(d, k)
}

fn main() {
  c := Ctx { files: Some(mem_files([("a.txt", "x")])), n: 1 }
  print(has(c, "a.txt"))
  print(has(c, "b.txt"))
  print(without(Dict { "a": 1, "b": 2 }, "a"))
  print(len("e\u{301}x".graphemes()))
}
