// A server-like loop: 300,000 requests, each parsed into a record, looked
// up in a bounded cache (a `Dict` reset when full) and answered with a
// string; the last 100 log lines are kept. Its memory should stay flat.

type Request = { method: String, path: String, id: Int }

fn request(i: Int) -> String = f"GET /items/{i % 5000}?q={i % 17}"

fn parse_request(line: String) -> Request {
  parts := split(line, " ")
  path := get(parts, 1) ?? "/"
  segs := split(path, "/")
  last := get(segs, 2) ?? "0"
  id := parse_int(get(split(last, "?"), 0) ?? "0") ?? 0
  Request { method: get(parts, 0) ?? "GET", path: path, id: id }
}

fn answer(r: Request, cache: Dict[Int, String]) -> String {
  match get(cache, r.id) {
    Some(body) => f"200 {r.method} {body}",
    None => f"200 {r.method} item {r.id} at {r.path}",
  }
}

fn main() {
  mut cache := Dict[Int, String] {}
  mut log := []
  mut bytes := 0
  for i in 0..300000 {
    r := parse_request(request(i))
    resp := answer(r, cache)
    if len(cache) >= 2000 {
      cache = Dict[Int, String] {}
    }
    mut cache.insert(r.id, f"item {r.id}")
    bytes += byte_len(resp)
    mut log.push(resp)
    if len(log) > 100 {
      log = drop(log, 1)
    }
  }
  print(f"{bytes} bytes answered, {len(log)} log lines kept")
}
