# std.agent

An LLM agent loop: the model calls tools, the loop runs them and answers, until a reply calls none.
The model is a `Model`: `Claude` calls Anthropic's Messages API over HTTPS, or a mock in tests. A tool is a function with `derive AsTool`.

## Types

### Role

```rust
type Role =
  | System
  | Agent
  | User
```

Who speaks. Each provider maps it to its own word (assistant, model,
developer, …).

### Message

```rust
type Message = { role: Role, content: List[Content] }
```

One turn of the conversation. The system prompt is the first message,
a `System` one.

### ToolCall

```rust
type ToolCall = { id: String, name: String, input: JsonValue }
```

The model asks to run the tool `name` with `input`. `id` pairs it with
its result.

### Content

```rust
type Content =
  | Text(String)
  /// The model's reasoning: its text, and the provider's signature to send back with it.
  | Reasoning { text: String, signature: Option[String] }
  | std.agent.ToolCall
  /// What the tool `id` gave back; `failed` marks an error the model should see. Only in a `User` message.
  | ToolResult { id: String, output: String, failed: Bool }
```

A part of a message.

### Chunk

```rust
type Chunk =
  | TextChunk(String)
  | ReasoningChunk(String)
  | std.agent.ToolCall
  | End(Message)
  | std.agent.AgentError
```

One piece of a streamed response. `End` carries the whole message; an
`AgentError` ends the stream.

### AgentError

```rust
type AgentError = HttpError | ApiError | BadReply | Refused | OutOfTokens | TooManyTurns | NoKey
```

Why a run stopped without an answer.

### ApiError

```rust
type ApiError = { status: Int, kind: String, message: String }
```

The API answered with an error: its status, kind and message.

### BadReply

```rust
type BadReply = { reason: String }
```

The API's answer isn't what this module reads.

### Refused

```rust
type Refused
```

The model declined to answer.

### OutOfTokens

```rust
type OutOfTokens
```

The reply reached the provider's output limit before it ended.

### TooManyTurns

```rust
type TooManyTurns = { turns: Int }
```

The model still called tools after this many turns.

### NoKey

```rust
type NoKey
```

The environment has no `ANTHROPIC_API_KEY`.

### Tool

```rust
type Tool = {
  name: String,
  description: String,
  schema: JsonValue,
  call: (JsonValue) -> Result[String, String],
}
```

A tool the model may call: its name, what it does, the JSON Schema of
its input, and the code that runs it on the model's input, giving the
output as JSON text or a failure the model sees.
`derive AsTool for f` makes one from a function.

### Stream

```rust
type Stream = { source: Source }
```

The chunks of a streamed message as they arrive (an `Iterator`), holding
the open connection. `Stream.of(chunks)` makes one from a list, for
mocks.

### Source

```rust
type Source =
  | Listed(List[Chunk])
  | Live(Sse)
```

Where a stream's chunks come from.

### Claude

```rust
type Claude = {
  net: Net,
  key: String,
  model: String = "claude-opus-5-5",
  max_tokens: Natural = 16000,
  /// `low`, `medium`, `high`, `xhigh` or `max`: how much the model thinks.
  effort: String = "high",
  url: String = "https://api.anthropic.com/v1/messages",
}
```

Claude through Anthropic's Messages API (`POST /v1/messages`). Make one
with `claude(net, env)`.

### Sse

```rust
type Sse = {
  claude: Claude,
  conversation: List[Message],
  tools: List[Tool],
  body: Option[Body],
  content: List[Content],
  /// The API's block index to its place in `content`.
  slots: Dict[Int, Int],
  /// A tool call's input as it arrives, by block index.
  inputs: Dict[Int, String],
  queued: List[Chunk],
  stop: String,
  done: Bool,
}
```

A streamed response being read: the request, the open body, the content
so far by the API's block index, and chunks ready to give.

## Methods

### Stream.of

```rust
fn of(chunks: List[Chunk]) -> Stream
```

A stream of these chunks.

### Claude.headers

```rust
fn headers(self) -> List[(String, String)]
```

The request's headers.

## Traits

### AsTool

```rust
derivable(Function) trait AsTool {
  /// The function as a tool.
  derived fn tool() -> Tool
}
```

A function the model can call. `derive AsTool for add` makes `add`,
used as a value, a `Tool`: its name, its doc comment as the description,
the JSON Schema of its parameters (each a type with a JSON form), and a
call that reads them from the model's input and writes the `Ok` value
back as JSON; an `Err` is a failed call the model sees.
`derive AsTool(bind: net) for search` leaves `net` out of the schema:
the use gives it, `search.with(net)`.

### Model

```rust
capability trait Model {
  /// The next message, the model's, given the conversation (a system
  /// message first, if any, and ending with a user message) and the tools.
  fn response(
    mut self,
    conversation: List[Message],
    tools: List[Tool],
  ) -> Result[Message, std.agent.AgentError]
  /// The next message as it is written: its chunks, then `End` with the
  /// whole message, or an `AgentError`.
  fn stream(mut self, conversation: List[Message], tools: List[Tool]) -> Stream
}
```

A model that continues a conversation: `Claude`, or a mock.

## Functions

### user

```rust
fn user(text: String) -> Message
```

A message from the user.

### system

```rust
fn system(text: String) -> Message
```

The system prompt, the conversation's first message.

### text_of

```rust
fn text_of(m: Message) -> String
```

The text of a message, its parts joined.

### calls_of

```rust
fn calls_of(m: Message) -> List[std.agent.ToolCall]
```

The tool calls of a message, in order.

### input_schema

```rust
fn input_schema(f: Type) -> JsonValue
```

The JSON Schema of a function's input: an object with a property per
parameter not bound, each required unless it is an `Option`.

### schema_of

```rust
fn schema_of(t: Type, depth: Int) -> JsonValue
```

The JSON Schema of a type's JSON form (`std.json`'s): `Int` an integer,
`List[T]` an array, a record an object of its fields, a union of atoms
an `enum` of their names, another union `anyOf` its members, a
refinement its base with the condition as its description. Past 8
levels (a recursive type), any value.

### answer

```rust
fn answer(tools: List[Tool], c: std.agent.ToolCall) -> Content
```

Runs the tool a call names, as its result. An unknown tool is a failed
result, so the model can correct itself.

### run

```rust
fn run(
  mut model: Model,
  mut conversation: List[Message],
  tools: List[Tool],
  max_turns: Natural,
) -> Result[List[Message], std.agent.AgentError]
```

Asks the model for replies until one calls no tools: each reply is added
to `conversation`, then the results of its tool calls, in one `User`
message. At most `max_turns` replies. Returns the conversation.

### run_streamed

```rust
fn run_streamed(
  mut model: Model,
  mut conversation: List[Message],
  tools: List[Tool],
  max_turns: Natural,
  mut console: Console,
  reasoning: Bool,
) -> Result[List[Message], std.agent.AgentError]
```

`run`, with each reply streamed: its text written to `console` as it
comes (its reasoning too, when `reasoning`), each tool call run once the
reply ends.

### claude

```rust
fn claude(net: Net, env: Environment) -> Result[Claude, std.agent.AgentError]
```

`Claude` with the key in the environment's `ANTHROPIC_API_KEY`.

### request_json

```rust
fn request_json(
  c: Claude,
  conversation: List[Message],
  tools: List[Tool],
  stream: Bool,
) -> String
```

The request's JSON: the `System` messages' text as `system`, the others
as `messages`, the model's reasoning summarized.

### read_message

```rust
fn read_message(
  body: String,
) -> Result[(List[Content], String), std.agent.AgentError]
```

A response's blocks as content, and why it stopped.

### read_event

```rust
fn read_event(mut body: Body) -> Result[Option[String], std.agent.AgentError]
```

The next event's data, its `data:` lines joined: `None` at the end of
the stream.

### on_event

```rust
fn on_event(mut s: Sse, data: String) -> Result[(), std.agent.AgentError]
```

What one event's data changes, and the chunks it gives.

## Implementations

- `impl Iterator[Chunk] for Stream`
- `impl Model for Claude`
