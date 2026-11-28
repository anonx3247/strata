from std.net use Net, TcpStream
from std.fs use Disk, Files
from std.time use SystemClock, Clock, ms
from std.http use HttpError, Response, fetch

fn port(clock: SystemClock, disk: Disk) -> Int {
  for _ in 0..500 {
    match disk.read("port") {
      Ok(text) => return parse_int(text) ?? 0,
      Err(_) => clock.sleep(ms(10)),
    }
  }
  0
}

/// `n` requests one after another; how many got a 200.
fn client(net: Net, base: String, k: Int, n: Int) -> Int {
  mut good := 0
  for i in 0..n {
    path := match i % 3 {
      0 => f"/page/p{(i * 7 + k) % 300}",
      1 => f"/tags/tag{i % 20}",
      _ => f"/other{i % 30}/x",
    }
    match fetch(net, base + path) {
      Ok(r) => if r.status == 200 {
        good += 1
      },
      Err(_) => (),
    }
  }
  good
}

fn main(clock: SystemClock, net: Net, disk: Disk) {
  p := port(clock, disk)
  base := f"http://127.0.0.1:{p}"
  n := 250
  total := scope {
    hs := [spawn client(net, base, k, n) for k in 0..8]
    mut t := 0
    for h in hs {
      t += await h
    }
    t
  }
  print(f"ok {total} of {8 * n}")
}
