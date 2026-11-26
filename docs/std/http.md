# std.http

HTTP/1.1 requests and responses, a client (`send`, `fetch`, TLS for `https://`) and a server (`serve`, a `Router`), over any `std.net.Stream`.
A handler is a function `(Request) -> Response`, which tests call directly; the wire functions take a `Stream`, so tests pass a mock connection.

## Types

### Method

```rust
type Method = String
  where matches(it, "GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS")
```

An HTTP method in capitals: `GET`, `POST`, ….

### Request

```rust
type Request = {
  method: Method,
  /// The decoded path without the query: `/orders/12`.
  path: String,
  /// The decoded query parameters: `?sort=year` is `{"sort": "year"}`.
  query: Dict[String, String],
  /// Header names are lowercase.
  headers: Dict[String, String],
  /// The body as UTF-8 text.
  body: String,
  /// The path parts its route named (`:id` in `/users/:id`), set by `Router.handle`.
  params: Dict[String, String],
}
```

A request as the server read it (`read_request`), handed to a handler.

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

A response. The server refuses header values that hold line breaks.

### Pattern

```rust
type Pattern = String
  where matches(it, "/[-a-zA-Z0-9_.~:/]*")
```

A path whose segments may be names: `/users/:id`.

### Router

```rust
type Router = { routes: List[Route] }
```

Routes, tried in the order they were added. Dispatch is a linear scan.

### HttpError

```rust
type HttpError = NetError | BadMessage | BadUrl
```

Why an HTTP exchange failed: the network's error, a message that isn't
the HTTP/1.1 this module reads, or a URL it can't use.

### BadMessage

```rust
type BadMessage = { message: String }
```

What arrived isn't HTTP/1.1 as this module reads it, or passes a limit.

### BadUrl

```rust
type BadUrl = { url: String, message: String }
```

A URL `send` can't use.

### Url

```rust
type Url = { tls: Bool, host: String, port: Port, target: String }
```

A URL split for a request: `https://api.example.com:8443/v1/x?y=1` is
TLS to `api.example.com` on 8443, asking for `/v1/x?y=1`.

### Streaming

```rust
type Streaming = { status: Status, headers: Dict[String, String], body: Body }
```

A response whose body is still arriving: its status and headers, and the
body to read from the open connection. Made by `open`.

### Body

```rust
type Body = {
  conn: TcpStream,
  chunked: Bool,
  /// Bytes left: of the `content-length`, or of the current chunk. -1: up
  /// to the end of the stream.
  left: Int,
  /// Bytes read from the connection, not yet given out.
  pending: List[Int] = [],
  ended: Bool = false,
}
```

A response's body read from its connection as it arrives, by
`content-length`, by chunks, or up to the end of the stream. Close it
when done (`Body.close`).

## Methods

### Request.param

```rust
fn param(self, name: String) -> Option[String]
```

The path part the route named `name`: `req.param("id")` for `/users/:id`.
`None` if the route has no such part or the request didn't come through a `Router`.

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

Sends `method` requests whose path fits `pattern` to `handler`.

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

The response of the first route that fits, with the named path parts in `req.params`.

- A `HEAD` request uses the `GET` routes.
- No route for the path gives 404. Routes for it, but not for this method, give 405.

### Body.read_line

```rust
fn read_line(mut self) -> Result[Option[String], HttpError]
```

The next line of the body, without its `\n` or `\r\n`: `None` at its
end. A line longer than `line_limit()` is an error.

### Body.rest

```rust
fn rest(mut self) -> Result[String, HttpError]
```

The rest of the body, as text.

### Body.more

```rust
fn more(mut self) -> Result[Bool, HttpError]
```

Reads more of the body into `pending`: false at its end.

### Body.close

```rust
fn close(mut self)
```

Closes the connection.

## Functions

### get_request

```rust
fn get_request(path: String) -> Request
```

A `GET` request for `path` with no query, headers or body, for tests.

### respond

```rust
fn respond(status: Status, content_type: String, body: String) -> Response
```

A response with this status, content type and body.

### ok_html

```rust
fn ok_html(body: String) -> Response
```

200 with an HTML page, such as `std.web.document` makes.

### ok_text

```rust
fn ok_text(body: String) -> Response
```

200 with plain text.

### ok_json

```rust
fn ok_json[T: Json](value: T) -> Response
```

200 with `value` as JSON.
500 with the message when it has no JSON form, such as an integer beyond what a JSON number holds exactly.

### not_found

```rust
fn not_found() -> Response
```

404.

### redirect

```rust
fn redirect(location: String) -> Response
```

303: the browser goes to `location` with a `GET`. Used after a form is posted, so reloading doesn't post it again.

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

### line_limit

```rust
fn line_limit() -> Natural
```

The longest request or status line and header line read: 16 KB.

### body_limit

```rust
fn body_limit() -> Natural
```

The largest body read: 64 MB.

### parse_url

```rust
fn parse_url(url: String) -> Result[Url, BadUrl]
```

`http://` or `https://`, a host, maybe `:port`, then the target (`/`
when there is none).

### send

```rust
fn send(
  net: Net,
  method: Method,
  url: String,
  headers: List[(String, String)],
  body: String,
) -> Result[Response, HttpError]
```

Sends a request and reads the response, on a connection of its own
(TLS for `https://`), closed after. Sends `content-length`, `host` and
`connection: close` besides `headers`.

### fetch

```rust
fn fetch(net: Net, url: String) -> Result[Response, HttpError]
```

`GET url`.

### exchange

```rust
fn exchange(
  mut conn: Stream,
  method: Method,
  host: String,
  target: String,
  headers: List[(String, String)],
  body: String,
) -> Result[Response, HttpError]
```

Writes a request on `conn` and reads its response: any `Stream`, so a
test passes a mock. The request asks the server to close after.

### read_response

```rust
fn read_response(mut conn: Stream) -> Result[Response, HttpError]
```

A response as the server sent it: the status line, the headers
(lowercase names; a repeated header's values joined by `, `), the body
by `content-length`, chunks, or up to the end of the stream.

### read_request

```rust
fn read_request(mut conn: Stream) -> Result[Option[Request], HttpError]
```

The next request on `conn`: `None` if the client closed first.

### parse_query

```rust
fn parse_query(query: String) -> Dict[String, String]
```

`a=1&b=x%20y` as `{"a": "1", "b": "x y"}`. A `+` is a space. A name
given twice keeps its last value.

### percent_decode

```rust
fn percent_decode(s: String, plus: Bool) -> String
```

`%20` as a space, and `+` too when `plus` (a query). An escape that
isn't two hexadecimal digits, or bytes that aren't UTF-8, stay as
written.

### write_response

```rust
fn write_response(mut conn: Stream, r: Response) -> Result[(), NetError]
```

Writes `r` with its `content-length`. A header holding a line break is
left out.

### serve_connection

```rust
fn serve_connection(
  mut conn: Stream,
  handler: (Request) -> Response,
) -> Result[Natural, NetError]
```

Answers the requests on one connection with `handler` until the client
closes it or asks to (`connection: close`), then closes it. A request
it can't read gets a 400 and ends the connection. Returns how many
requests were answered.

### serve

```rust
fn serve(listener: Listener, handler: (Request) -> Response) -> NetError
```

Accepts connections on `listener` and answers each in a task of its
own (`serve_connection`), until accepting fails, such as when the task
is cancelled. Returns why it stopped.

### open

```rust
fn open(
  net: Net,
  method: Method,
  url: String,
  headers: List[(String, String)],
  body: String,
) -> Result[Streaming, HttpError]
```

Sends a request and reads the response's status and headers, leaving
its body to be read as it arrives: for server-sent events and other
long responses. TLS for `https://`. The connection is the body's.
