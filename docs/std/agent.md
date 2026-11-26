# std.agent

An LLM agent loop: the model asks for tools, the loop runs them and sends the results back, until the model is done.
The model is a `Model`: `Claude` calls Anthropic's Messages API over HTTPS; tests pass a mock.

## Types

### Message

```rust
type Message = { role: Role, content: List[Block] }
```

One turn of the conversation.

### Role

```rust
type Role = String
  where it == "user" || it == "assistant"
```

Who speaks: `user` (the prompt and tool results) or `assistant` (the model).

### Block

```rust
type Block =
  /// Text, from either side.
  | Said { text: String }
  /// The model asks to run the tool `name` with `input`. `id` pairs it with its result.
  | Call { id: String, name: String, input: JsonValue }
  /// What a tool gave back for the call `id`. `failed` marks an error the model should see.
  | Answer { id: String, text: String, failed: Bool }
  /// A block the loop doesn't read (thinking, a model fallback), sent back exactly as it came.
  | Kept { json: JsonValue }
```

A part of a message.

### Tool

```rust
type Tool = {
  name: String,
  description: String,
  schema: JsonValue,
  run: (JsonValue) -> Result[String, String],
}
```

A tool the model may call: its name, what it does, the JSON Schema of
its input, and the function that runs it. An `Err` is shown to the model
as a failed call, so it can try again.

### Param

```rust
type Param = { name: String, kind: String, description: String }
```

A property of a tool's input: its name, JSON type (`string`, `integer`,
`number`, `boolean`) and what it means. Every property is required.

### Reply

```rust
type Reply = { content: List[Block], stop: String }
```

The model's answer to a conversation: its blocks, and why it stopped
(`end_turn`, `tool_use`, `max_tokens`, `refusal`, `pause_turn`, …).

### AgentError

```rust
type AgentError = HttpError | ApiError | BadReply | Refused | OutOfTokens | TooManyTurns | NoKey
```

Why the loop stopped without an answer.

### ApiError

```rust
type ApiError = { status: Int, kind: String, message: String }
```

The API answered with an error: its status, kind and message.

### BadReply

```rust
type BadReply = { message: String }
```

The API's answer isn't the JSON this module reads.

### Refused

```rust
type Refused
```

The model declined (`stop_reason` `refusal`).

### OutOfTokens

```rust
type OutOfTokens
```

The reply reached `max_tokens` before it ended.

### TooManyTurns

```rust
type TooManyTurns = { turns: Int }
```

The model was still calling tools after this many turns.

### NoKey

```rust
type NoKey
```

The environment has no `ANTHROPIC_API_KEY`.

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

Claude through Anthropic's Messages API (`POST /v1/messages`), one
HTTPS request per turn, not streamed. Make one with `claude(net, env)`.

### Run

```rust
type Run = { messages: List[Message], answer: String, turns: Natural }
```

A finished run: the whole conversation, and the model's last text.

## Traits

### Model

```rust
capability trait Model {
  /// The next assistant message, given the system prompt, the
  /// conversation so far (it ends with a user message) and the tools.
  fn reply(
    mut self,
    system: String,
    messages: List[Message],
    tools: List[Tool],
  ) -> Result[Reply, AgentError]
}
```

A model that continues a conversation: `Claude`, or a mock.

## Functions

### tool

```rust
fn tool[T: Json](
  name: String,
  description: String,
  params: List[Param],
  run: (T) -> Result[String, String],
) -> Tool
```

A tool whose input is read as a `T` (a record that derives `Json`).
The schema is an object with `params` as its properties. Input that
doesn't read as a `T` is a failed call, with the reason.

### object_schema

```rust
fn object_schema(params: List[Param]) -> JsonValue
```

The JSON Schema of an object with these properties, all required.

### claude

```rust
fn claude(net: Net, env: Environment) -> Result[Claude, AgentError]
```

`Claude` with the key in the environment's `ANTHROPIC_API_KEY`.

### request_json

```rust
fn request_json(
  model: String,
  max_tokens: Natural,
  effort: String,
  system: String,
  messages: List[Message],
  tools: List[Tool],
) -> String
```

The request's JSON. A refusal is retried on the API's fallback model
(`fallbacks: "default"`).

### block_json

```rust
fn block_json(b: Block) -> JsonValue
```

A block as the API writes it.

### read_reply

```rust
fn read_reply(body: String) -> Result[Reply, AgentError]
```

A successful response's blocks and stop reason.

### read_block

```rust
fn read_block(j: JsonValue) -> Block
```

A response block: text and tool calls are read, the rest kept as it came.

### run

```rust
fn run(
  mut model: Model,
  system: String,
  prompt: String,
  tools: List[Tool],
  max_turns: Natural,
) -> Result[Run, AgentError]
```

Asks `model` about `prompt`, running the tools it calls and sending
their results back, until it ends its turn: at most `max_turns` model
calls. A call to a tool not in `tools` gets a failed result, so the
model can correct itself. The calls of one turn run in order.

### text_of

```rust
fn text_of(content: List[Block]) -> String
```

The text blocks of a message, joined.

## Implementations

- `impl Model for Claude`
