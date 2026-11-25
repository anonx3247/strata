# std.http

Requests, responses and routes, for `strata serve`.

A program serves the web by defining `handle`. A `Router` dispatches on
the method and the path, as Hono and Express do:

```rust
from std.http use Request, Response, Router, ok_text, redirect, not_found

fn app() -> Router {
  let mut app = Router.new()
  app.get("/", req -> redirect("/users"))
  app.get("/users/:id", show_user)
  app
}

fn show_user(req: Request) -> Response {
  let id = req.param("id") except { return not_found() }
  ok_text(f"user {id}")
}

fn handle(req: Request) -> Response {
  app().handle(req)
}
```

`strata serve` calls `handle` once per request. Like `main`, it can also
ask for `Console`, `Files` and `Environment`. It is a plain function, so
tests call it with a `Request` and look at the `Response`.

## Types

### Method

```rust
type Method = String
  where matches(it, "GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS")
```

An HTTP method, in capitals: `GET`, `POST`, ….

### Request

```rust
type Request = {
  method: Method,
  /// The path, decoded, without the query: `/orders/12`.
  path: String,
  /// The query's parameters, decoded: `?sort=year` is `{"sort": "year"}`.
  query: Dict[String, String],
  /// The headers, with lowercase names.
  headers: Dict[String, String],
  /// The body, as text (UTF-8).
  body: String,
  /// The parts of the path its route named (`:id` in `/users/:id`), set by
  /// `Router.handle`.
  params: Dict[String, String],
}
```

A request, as the server read it.

### Status

```rust
type Status = Int
  where 100 <= it && it <= 599
```

An HTTP status code.

### Response

```rust
type Response = { status: Status, headers: Dict[String, String], body: String }
```

A response. Header values can't hold line breaks (the server refuses
them).

### Pattern

```rust
type Pattern = String
  where matches(it, "/[-a-zA-Z0-9_.~:/]*")
```

A route's pattern: a path whose segments may be names (`/users/:id`).

### Router

```rust
type Router = { routes: List[Route] }
```

Routes, tried in the order they were added.

## Methods

### Request.param

```rust
fn param(self, name: String) -> Option[String]
```

The part of the path the route named `name`: `req.param("id")` for
`/users/:id`. `None` if the route has no such part (or the request
didn't come through a `Router`).

### Router.new

```rust
fn new() -> Router
```

A router with no routes.

### Router.on

```rust
fn on(
  mut self,
  method: Method,
  pattern: Pattern,
  handler: (Request) -> Response,
)
```

Send `method` requests whose path fits `pattern` to `handler`.

### Router.get

```rust
fn get(mut self, pattern: Pattern, handler: (Request) -> Response)
```

### Router.post

```rust
fn post(mut self, pattern: Pattern, handler: (Request) -> Response)
```

### Router.put

```rust
fn put(mut self, pattern: Pattern, handler: (Request) -> Response)
```

### Router.patch

```rust
fn patch(mut self, pattern: Pattern, handler: (Request) -> Response)
```

### Router.delete

```rust
fn delete(mut self, pattern: Pattern, handler: (Request) -> Response)
```

### Router.handle

```rust
fn handle(self, req: Request) -> Response
```

The response of the first route that fits the request, with the path's
named parts in `req.params`. A `HEAD` request uses the `GET` routes.
No route for the path: 404; routes for it, but not this method: 405.

## Functions

### get_request

```rust
fn get_request(path: String) -> Request
```

A `GET` request for `path`, with no query, headers or body: for tests.

### respond

```rust
fn respond(status: Status, content_type: String, body: String) -> Response
```

A response with the given status, content type and body.

### ok_html

```rust
fn ok_html(body: String) -> Response
```

200, an HTML page (such as `std.web.document` makes).

### ok_text

```rust
fn ok_text(body: String) -> Response
```

200, plain text.

### ok_json

```rust
fn ok_json[T: Json](value: T) -> Response
```

200, `value` as JSON (`std.json`'s `Json`); 500, with the message, when
it has no JSON (an integer beyond what a JSON number holds exactly).

### not_found

```rust
fn not_found() -> Response
```

404.

### redirect

```rust
fn redirect(location: String) -> Response
```

303: the browser goes to `location` with a `GET` (after a form is posted,
so reloading doesn't post it again).

### method_not_allowed

```rust
fn method_not_allowed() -> Response
```

405: the path exists, but not for this method.

### match_path

```rust
fn match_path(pattern: String, path: String) -> Option[Dict[String, String]]
```

The names `pattern` gives to the parts of `path`, if it fits.
