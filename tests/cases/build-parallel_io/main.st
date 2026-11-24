from std.fs use Disk, Files
from std.process use Launcher, Commands, RunOptions

/// One task's work: `rounds` times, a file written, read back and removed in
/// its own folder, and `cat` fed a text it must give back. A file `keep` in
/// the folder must survive. What went wrong, or "" when nothing did.
fn worker(id: Int, base: String, rounds: Int, mut disk: Disk, mut launcher: Launcher) -> String {
  dir := base + "/t" + f"{id}"
  _ := mut disk.make_dir(dir)
  keep := dir + "/keep-this-file-" + f"{id}"
  _ := mut disk.write(keep, "keep")
  mut wrong := ""
  for i in 0..rounds {
    path := dir + "/f" + f"{i}"
    text := f"task {id} round {i}\n"
    _ := mut disk.write(path, text)
    if (disk.read(path) !! "") != text {
      wrong = wrong + f"read {path} wrong; "
    }
    match mut launcher.run_with("cat", [], "", RunOptions { input: text }) {
      Ok(fin) => if fin.out != text {
        wrong = wrong + f"cat gave {fin.out} for {text}; "
      },
      Err(e) => {
        wrong = wrong + f"cat: {e}; "
      },
    }
    _ := mut disk.remove(path)
  }
  if !disk.exists(keep) {
    wrong = wrong + f"{keep} was removed; "
  }
  wrong
}

fn main(mut disk: Disk, mut launcher: Launcher) -> Int {
  base := "work"
  _ := mut disk.remove_all(base)
  _ := mut disk.make_dir(base)
  results := scope { await [spawn {
    mut d := disk
    mut l := launcher
    worker(id, base, 200, mut d, mut l)
  } for id in 0..16] }
  bad := results.filter(r -> r != "")
  for r in bad {
    print(r)
  }
  _ := mut disk.remove_all(base)
  if len(bad) == 0 {
    print("ok: 16 tasks x 200 rounds")
    0
  } else {
    print(f"{len(bad)} tasks went wrong")
    1
  }
}
