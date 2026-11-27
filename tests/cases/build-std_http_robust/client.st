from std.net use Net, TcpStream
from std.fs use Disk, Files
from std.time use SystemClock, Clock, ms, seconds
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

fn shown(r: Result[Response, HttpError]) -> String {
  match r {
    Ok(resp) => f"{resp.status} {resp.body}",
    Err(e) => f"error {e}",
  }
}

fn call(clock: SystemClock, net: Net, url: String) -> String {
  race {
    shown(fetch(net, url)) as s => s,
    clock.sleep(seconds(5)) => "timed out",
  }
}

/// The status line of a request with both `content-length` and chunks.
fn smuggled(net: Net, p: Int) -> String {
  match net.connect("127.0.0.1", p) {
    Ok(c) => {
      mut s := c
      sent := mut s.write("POST / HTTP/1.1\r\nhost: x\r\ncontent-length: 5\r\ntransfer-encoding: chunked\r\n\r\n0\r\n\r\n")
      line := match sent {
        Ok(_) => match mut s.read_line(200) {
          Ok(Some(l)) => trim(l),
          Ok(None) => "closed",
          Err(e) => f"error {e}",
        },
        Err(e) => f"error {e}",
      }
      mut s.close()
      line
    },
    Err(e) => f"error {e}",
  }
}

fn main(clock: SystemClock, net: Net, disk: Disk) {
  p := port(clock, disk)
  base := f"http://127.0.0.1:{p}"
  mut held: List[TcpStream] = []
  for _ in 0..64 {
    match net.connect("127.0.0.1", p) {
      Ok(c) => mut held.push(c),
      Err(_) => (),
    }
  }
  clock.sleep(ms(300))
  for c in held {
    mut c2 := c
    mut c2.close()
  }
  clock.sleep(ms(300))
  print(f"after running out of files: {call(clock, net, base + "/")}")
  print(f"a handler that fails: {call(clock, net, base + "/boom")}")
  print(f"after it: {call(clock, net, base + "/")}")
  print(f"both content-length and chunks: {smuggled(net, p)}")
}
