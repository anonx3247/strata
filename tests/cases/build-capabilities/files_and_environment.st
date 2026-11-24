from std.io use Console

from std.fs use Disk, FsError

from std.process use Process

fn main(
  mut console: Console,
  mut files: Disk,
  env: Process,
) -> Result[(), String] {
  _ := mut files.remove("fsplay/b.txt")
  _ := mut files.remove("fsplay/a.txt")
  _ := mut files.remove("fsplay/sub/deep")
  _ := mut files.remove("fsplay/sub")
  _ := mut files.remove("fsplay")
  print(files.exists("fsplay"))
  print((mut files.make_dir("fsplay/sub/deep")))
  print((mut files.write("fsplay/a.txt", "one\n")))
  print((mut files.append("fsplay/a.txt", "two\n")))
  print(files.read("fsplay/a.txt"))
  print(files.read("fsplay/missing.txt"))
  print(files.read("fsplay/sub"))
  print((mut files.write("fsplay/b.txt", "x")))
  print(files.list("fsplay"))
  print(files.list("fsplay/a.txt"))
  print((mut files.remove("fsplay/nope")))
  print(env.args())
  print(env.var("HOME") is Some)
  print(env.var("STRATA_SURELY_UNSET_VAR"))
  mut console.print("done")
  Ok(())
}
