from std.fs use Disk, Files, FsError
from std.process use Process, Environment, ExitCode

fn copy(mut f: Files, from: String, to: String) -> Result[Int, FsError] {
  text := f.read(from)!
  mut f.write(to, text + "!")!
  Ok(len(text.bytes()))
}

fn main(mut disk: Disk, process: Process) -> Result[ExitCode, String] {
  dir := "/tmp/strata-codegen-c4"
  mut disk.make_dir(dir) except e {
    return Err(f"no dir: {e}")
  }
  mut disk.write(dir + "/a.txt", "hello") except e {
    return Err(f"{e}")
  }
  print(copy(mut disk, dir + "/a.txt", dir + "/b.txt"))
  print(disk.read(dir + "/b.txt"))
  print(disk.read(dir + "/missing.txt"))
  print(disk.exists(dir + "/b.txt"))
  print(disk.list(dir))
  print(disk.read_bytes(dir + "/a.txt"))
  mut disk.remove(dir + "/a.txt") except e {
    return Err(f"{e}")
  }
  mut disk.remove(dir + "/b.txt") except e {
    return Err(f"{e}")
  }
  print(disk.list(dir))
  print(process.args())
  print(process.var("STRATA_SURELY_UNSET"))
  print(process.var("HOME") is Some)
  Ok(3)
}
