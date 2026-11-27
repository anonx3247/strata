from std.net use Net, NetError, Listener
from std.http use Request, Response, Router, HttpError, ok_text, ok_json, send, fetch, serve

fn routes() -> Router {
  mut r := Router.new()
  mut r.get("/hello/:name", req -> ok_text(f"hello {req.param("name") ?? "?"}"))
  mut r.post("/echo", req -> ok_text(f"{req.body} ({req.query})"))
  mut r.get("/list", _ -> ok_json([1, 2, 3]))
  r
}

fn handle(req: Request) -> Response {
  routes().handle(req)
}

fn calls(net: Net, port: Int) -> List[String] {
  base := f"http://127.0.0.1:{port}"
  [
    shown(fetch(net, base + "/hello/ada%20l")),
    shown(send(net, "POST", base + "/echo?x=1&y=a+b", [("content-type", "text/plain")], "é body")),
    shown(fetch(net, base + "/list")),
    shown(fetch(net, base + "/nope")),
    shown(fetch(net, "ftp://x")),
  ]
}

fn shown(r: Result[Response, HttpError]) -> String {
  match r {
    Ok(resp) => f"{resp.status} {get(resp.headers, "content-type") ?? ""} {resp.body}",
    Err(e) => f"error {e}",
  }
}

fn main(net: Net) -> Result[(), NetError] {
  listener := net.listen("127.0.0.1", 0)!
  port := listener.port()!
  out := race {
    serve(listener, handle) as e => [f"server stopped: {e}"],
    calls(net, port) as xs => xs,
  }
  for line in out {
    print(line)
  }
  listener.close()
  Ok(())
}
