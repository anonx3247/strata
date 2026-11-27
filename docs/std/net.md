# std.net

TCP connections and listeners, through the `Net` capability.
`main` asks for `Net`; code that talks over a connection takes a `Stream`, so tests pass a mock.

## Types

### Port

```rust
type Port = Int
  where 0 <= it && it <= 65535
```

A TCP port. 0 asks the system for a free one, when listening.

### NetError

```rust
type NetError =
  /// The stream ended before what was asked for arrived.
  | Ended
  /// The task was cancelled while it waited, by a `race` it lost or a
  /// cancelled scope. A read gave up having consumed nothing.
  | Cancelled
  /// The bytes received aren't UTF-8 text (or hold a NUL). They are still
  /// there: `read_bytes` reads them.
  | NotText
  /// A line longer than the limit asked for.
  | TooLong { limit: Int }
  /// The connection was closed, by `close`.
  | Closed
  /// A write or connect was cancelled half-way, so the other end may have
  /// part of a message: the connection is unusable.
  | Broken
  /// The system refused: a name that doesn't resolve, a refused
  /// connection, a port in use. The message is the system's.
  | Failed { message: String }
```

Why a network operation failed.

### Net

```rust
capability Net {
  /// A connection to `host` (a name, or an address such as `127.0.0.1`) on
  /// `port`. Looking the name up waits without seeing a cancellation.
  fn connect(host: String, port: Port) -> Result[TcpStream, NetError]
  /// A TLS connection to `host` on `port` (`TcpStream.start_tls`).
  fn connect_tls(host: String, port: Port) -> Result[TcpStream, NetError]
  /// Waits for connections on `host`'s address (`""`: every address) and
  /// `port` (0: a free one the system picks; `listener.port()` says which).
  fn listen(host: String, port: Port) -> Result[Listener, NetError]
}
```

The system's network. Only the runtime makes one, for `main` and tests
that ask for it.

### TcpStream

```rust
type TcpStream = { id: U64 }
```

A TCP connection, made by `net.connect` or `listener.accept()`.
Copies are the same connection. Close it when done.

### Listener

```rust
type Listener = { id: U64 }
```

Waits for connections, made by `net.listen`. Close it when done.

## Methods

### Listener.accept

```rust
fn accept(self) -> Result[TcpStream, NetError]
```

The next connection, waiting for it.

### Listener.port

```rust
fn port(self) -> Result[Port, NetError]
```

The port it listens on: the one the system picked for port 0.

### Listener.close

```rust
fn close(self)
```

Stops listening.

### TcpStream.peer

```rust
fn peer(self) -> Result[String, NetError]
```

The other end's address and port, such as `127.0.0.1:52144`.

### TcpStream.start_tls

```rust
fn start_tls(self, host: String) -> Result[(), NetError]
```

Starts TLS on this connection, as a client of `host`: the server's
certificate must be valid for that name and signed by a root the
system trusts. Reads and writes then go through TLS. Built on macOS's
Secure Transport (TLS 1.2), loaded at the first use.

### TcpStream.local_port

```rust
fn local_port(self) -> Result[Port, NetError]
```

This end's port.

## Traits

### Stream

```rust
capability trait Stream {
  /// The next line, without its `\n` or `\r\n`. At the end of the stream,
  /// the last line if it has no `\n`, then `None`. A line longer than
  /// `limit` bytes is `TooLong`.
  fn read_line(mut self, limit: Natural) -> Result[Option[String], NetError]
  /// What has arrived, at most `limit` bytes and at least one character,
  /// waiting for it. `""` only at the end of the stream.
  fn read_text(mut self, limit: Positive) -> Result[String, NetError]
  /// Exactly `n` bytes, as text, waiting until they have all arrived.
  fn read_exact(mut self, n: Natural) -> Result[String, NetError]
  /// Up to `limit` bytes of what has arrived, at least one, waiting for
  /// it. `[]` only at the end of the stream.
  fn read_bytes(mut self, limit: Positive) -> Result[List[U8], NetError]
  /// Exactly `n` bytes, waiting until they have all arrived.
  fn read_exact_bytes(mut self, n: Natural) -> Result[List[U8], NetError]
  /// Sends `text` whole.
  fn write(mut self, text: String) -> Result[(), NetError]
  /// Sends `bytes` whole.
  fn write_bytes(mut self, bytes: List[U8]) -> Result[(), NetError]
  /// Says no more will be written: the other end reads the end of the
  /// stream. Reading still works.
  fn finish(mut self) -> Result[(), NetError]
  /// Closes the connection. Later operations give `Closed`.
  fn close(mut self)
}
```

A connection to read from and write to: a `TcpStream`, or a mock.
Reads wait for data; a cancelled task stops waiting at once.

## Implementations

- `impl Stream for TcpStream`
