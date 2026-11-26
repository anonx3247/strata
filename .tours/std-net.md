---
title: std.net, HTTP over it, and an agent loop
packages: compiler std
---

# Why
std had HTTP types but no way to talk to a network: `main` couldn't ask for sockets, so no client, no server, and no program could call an LLM. This adds, in order: TCP sockets as a capability (`std.net`), HTTP/1.1 over them (`std.http`'s client and server, HTTPS through the system's TLS), and an agent loop (`std.agent`) whose model is a trait, so tests run offline against a mock.

# Background
I/O in std goes through capabilities: the runtime makes `Disk`, `Process` or `Sqlite` for `main`, and code takes a trait (`Files`, `Database`) so tests pass a mock. std.sqlite reaches C through `extern c`; std.python calls a runtime function the same way. The task model is built: a `race` cancels its losers, and a task blocked reading stdin is woken through its wake pipe (runtime.c's `wait_readable`). IDEAS.md proposed sockets on exactly that wait.

```strata std.sqlite.Sqlite
```

```strata std.http.Request
```

# Intuition
Two tasks in one program: the server `accept`s and echoes lines, the client connects to `127.0.0.1` on the port the system picked, writes two lines, `finish`es, and reads two answers. A third read waits for a line nobody sends, in a `race` with `clock.sleep(ms(50))`: the timer wins, the read's task is cancelled through its wake pipe, and nothing was consumed. Each call is a plain blocking call; no function is `async`.

```strata-refs std.net.Stream
```

# Sockets: `Net`, `Stream`, `TcpStream`
`Net` makes connections and listeners; `Stream` is what code reads and writes, implemented by `TcpStream` and by tests' mocks (the byte methods have defaults over the text ones). Each method is an `extern c` call to runtime.c's `std_net_*` functions; a connection is a `U64` handle into the runtime's slot table, so a closed handle gives `Closed`. Failures are `NetError` values.

```strata-diff std.net.Net std.net.Stream std.net.NetError std.net.TcpStream
```

```strata-also
std.net.Listener: a listener
std.net.got std.net.net_error std.net.port_of std.net.c_size std.net.encode_bytes: reading the runtime's status, sizes as `I64`
std.net.Port: a port, 0 to 65535
std.net.std_net_*: the runtime's socket functions, declared `extern c`
```

# HTTP/1.1 over any Stream
The wire functions take a `Stream`: `exchange` writes a request and `read_response` reads it back (content-length, chunks, or to the end); `read_request` and `write_response` are the server's half. `send` and `fetch` open a connection per request, TLS for `https://`. `serve` accepts in a loop and answers each connection in a task until it is cancelled.

```strata-diff std.http.send std.http.read_response std.http.serve_connection std.http.serve
```

```strata-also
std.http.exchange std.http.fetch std.http.parse_url std.http.Url std.http.answer: the client, and a connection per task for the server
std.http.read_request std.http.write_response std.http.read_headers std.http.read_body std.http.read_chunks std.http.read_to_end std.http.reason: the wire format
std.http.parse_query std.http.percent_decode std.http.parse_hex: query strings and escapes
std.http.HttpError std.http.BadMessage std.http.BadUrl: the errors, `NetError`'s members included
std.http.next_line std.http.exact std.http.exact_bytes std.http.some_text std.http.send_text: the stream's calls with HTTP's error type, so `verify` proves their `!`
std.http.line_limit std.http.body_limit std.http.crlf: limits, and `\r\n` (Strata strings have no `\r`)
std.http.Request: its doc no longer names `strata serve`
```

# `!` into a union of unions
`HttpError` is `NetError | BadMessage | BadUrl`, and `next_line(…)!` returns a `NetError` from a function returning `HttpError`. The checker accepted only the same error type; it now accepts any type that `fits` the declared one (its variants, a union's members), as the docs already said. Codegen already converted the value.

```strata-diff check.early_error
```

# The agent loop
`run` calls a `Model` until it ends its turn, running each tool it calls and sending the results back in one message. `Claude` is a `Model`: one HTTPS request per turn to the Messages API, the key read from `ANTHROPIC_API_KEY` through `Environment`. Blocks the loop doesn't read (thinking) are kept and sent back as they came. `tool` makes a tool from a function on a record that derives `Json`.

```strata-diff std.agent.run std.agent.Model std.agent.Claude std.agent.tool
```

```strata-also
std.agent.claude: the request, the key from the environment
std.agent.request_json std.agent.block_json std.agent.read_reply std.agent.read_block std.agent.api_error std.agent.field std.agent.text_field: the API's JSON both ways
std.agent.Message std.agent.Role std.agent.Block std.agent.Reply std.agent.Run std.agent.Tool std.agent.Param std.agent.object_schema: the conversation and the tools
std.agent.AgentError std.agent.ApiError std.agent.BadReply std.agent.Refused std.agent.OutOfTokens std.agent.TooManyTurns std.agent.NoKey: why a run stops
std.agent.call_tool std.agent.text_of: running one call, the answer's text
```

# Routine
```strata-routine
std.agent.use*: the agent's imports
std.http.use*: std.net and std.utf8 imported, the module's comment says what it is now
std.net.use*: std.utf8 for bytes
```
