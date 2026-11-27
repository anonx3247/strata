from std.net use Net, NetError
from std.fs use Disk, Files
from std.http use Request, Response, Router, ok_text, serve

fn boom(req: Request) -> Response {
  parts := split(req.path, "/")
  ok_text(parts[len(parts) + 1])
}

fn handle(req: Request) -> Response {
  mut r := Router.new()
  mut r.get("/", _ -> ok_text("up"))
  mut r.get("/boom", boom)
  r.handle(req)
}

fn main(net: Net, disk: Disk) -> Result[(), NetError] {
  listener := net.listen("127.0.0.1", 0)!
  port := listener.port()!
  mut d := disk
  match mut d.write("port.tmp", f"{port}") {
    _ => (),
  }
  match mut d.rename("port.tmp", "port") {
    _ => (),
  }
  print(f"server stopped: {serve(listener, handle)}")
  listener.close()
  Ok(())
}
