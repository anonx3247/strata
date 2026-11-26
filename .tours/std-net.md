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
std.http.exchange std.http.fetch std.http.parse_url std.http.Url std.http.answer: the client (its request head and response head shared with `open`), and a connection per task for the server
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
`run` asks a `Model` for replies until one calls no tools, answering each reply's calls in one `User` message; `run_streamed` does the same over `stream`, writing text as it arrives. A message is a `Role` and `Content`s; a streamed reply is `Chunk`s, its errors a member of `Chunk`. `Claude` is a `Model`: the Messages API over HTTPS, the key from `ANTHROPIC_API_KEY` through `Environment`.

```strata-diff std.agent.run std.agent.run_streamed std.agent.Model std.agent.Chunk
```

```strata-also
std.agent.Role std.agent.Message std.agent.Content std.agent.ToolCall std.agent.user std.agent.system std.agent.text_of std.agent.calls_of: the conversation
std.agent.AgentError std.agent.ApiError std.agent.BadReply std.agent.Refused std.agent.OutOfTokens std.agent.TooManyTurns std.agent.NoKey std.agent.chunk_error: why a run stops
std.agent.Claude std.agent.claude std.agent.finished std.agent.request_json std.agent.blocks_of std.agent.read_message std.agent.block_content std.agent.api_error std.agent.field std.agent.text_field: the Messages API's JSON both ways
std.agent.answer: running one call
```

# Streaming without async
`stream` gives a `Stream`, an `Iterator[Chunk]`: each `next` reads server-sent events from the open body (`std.http.open`, `Body.read_line`) until one makes a chunk. A read blocks in the task iterating; nothing is colored. `Stream.of` makes one from a list, for mocks.

```strata-diff std.agent.Stream std.agent.on_event std.http.open std.http.Body
```

```strata-also
std.agent.Source std.agent.Sse std.agent.next_chunk std.agent.step std.agent.close_body std.agent.read_event std.agent.put_at: reading events into chunks
std.http.Streaming std.http.start std.http.some_bytes std.http.newline_in std.http.request_head std.http.read_head: a response read as it arrives
```

# Tools from functions: `derive Tool`
`Tool` is a trait (`name`, `description`, `schema`, `call`), derivable for functions: `derive Tool for` above a function makes it implement `Tool`, so `add` is a value in a `List[Tool]`, a list of trait values. The name, doc, a JSON Schema from its parameters' types, and a call decoding them. `derive Tool(bind: notes)` leaves `notes` to the use, `note.with(notes)`, a value holding it.

```strata-diff std.agent.Tool std.agent.input_schema std.agent.schema_of
```

```strata-also
std.agent.bound_names std.agent.is_option std.agent.shaped std.agent.record_schema std.agent.member_schema std.agent.field_schema std.agent.described: the schema of each kind of type
std.agent.tool_argument std.agent.tool_output: the call's input and output as JSON
```

# Derives with arguments, and derived values
The compiler side: `derive T(bind: x) for f` stores its arguments as the derive's clauses (`Self.clauses`). A `derived fn` taking `self` is a method of a type made for the function (`Tool$f`, a record of the bound parameters, implementing the trait), and `value_uses` puts that type's value where the function is used as a value, `f.with(x)` included. An unbound use is an error.

```strata-diff derive.derive_fn derive.value_uses derive.value_use parse.derive_args
```

```strata-also
ast.DeriveFnDecl: its clauses and values
binary.decl binary.put_decl: a flags byte in place of `derived`, clauses after
parse.derive_fn parse.derive_group parse.function_derive: arguments after each trait
text.simple_derive text.derive_args_text text.decl_body_text: printed back
derive.Expander derive.call_fn derive.fn_value derive.clause_values derive.bound_of derive.original derive.derive_all: bound parameters passed through, clauses shown to the derive
expanded: the rewrite applied to the whole program
```

# Fixes found on the way
A std union mixing its own variants with member types (`Content`) failed in codegen: its own sum was named unqualified. A type pattern naming a union of some members (`AgentError => c`) wasn't compiled. And a list literal of trait values didn't turn a call's result into one.

```strata-diff desugar.mixed_unions codegen.matches codegen.list_literal_of
```

# Routine
```strata-routine
std.agent.use*: the agent's imports
std.http.use*: std.net and std.utf8 imported, the module's comment says what it is now
std.net.use*: std.utf8 for bytes
```
