from std.net use Net, NetError
from std.fs use Disk, Files
from std.time use SystemClock, Clock, ms
from std.memory use frozen
from std.http use Request, Response, Router, ok_text, ok_html, not_found, serve_arenas

/// What every handler reads: pages and settings, made once.
type Site = { title: String, pages: Dict[String, String], tags: List[String] }

fn site() -> Site {
  mut pages: Dict[String, String] = Dict {}
  for i in 0..300 {
    mut pages.insert(f"p{i}", repeat(f"<p>page {i} text</p>", 100))
  }
  Site { title: "bench", pages, tags: [f"tag{i}" for i in 0..200] }
}

fn page(s: Site, req: Request) -> Response {
  name := get(req.params, "name") ?? ""
  match get(s.pages, name) {
    Some(body) => ok_html(f"<h1>{s.title}: {name}</h1>{byte_slice(body, 0, 200)}"),
    None => not_found(),
  }
}

fn tagged(s: Site, req: Request) -> Response {
  q := get(req.params, "q") ?? ""
  hits := s.tags.filter(t -> starts_with(t, q))
  ok_text(join(hits, ","))
}

fn router(s: Site) -> Router {
  mut r := Router.new()
  for i in 0..30 {
    mut r.get(f"/other{i}/:x", _ -> ok_text(f"other {i}"))
  }
  mut r.get("/page/:name", req -> page(s, req))
  mut r.get("/tags/:q", req -> tagged(s, req))
  mut r.get("/", _ -> ok_text(s.title))
  r
}

/// Returns once a file `stop` exists.
fn stopped(clock: SystemClock, disk: Disk) -> String {
  loop {
    match disk.read("stop") {
      Ok(_) => return "stopped",
      Err(_) => clock.sleep(ms(20)),
    }
  }
  "stopped"
}

fn main(clock: SystemClock, net: Net, disk: Disk) -> Result[(), NetError] {
  listener := net.listen("127.0.0.1", 0)!
  port := listener.port()!
  mut d := disk
  match mut d.write("port.tmp", f"{port}") {
    _ => (),
  }
  match mut d.rename("port.tmp", "port") {
    _ => (),
  }
  r := router(site())
  why := race {
    frozen(r, fr -> f"{serve_arenas(listener, req -> fr.handle(req))}") as e => e,
    stopped(clock, disk) as e => e,
  }
  print(f"server: {why}")
  listener.close()
  Ok(())
}
