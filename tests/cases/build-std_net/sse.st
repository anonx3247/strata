from std.agent use Sse, Claude, Chunk, on_event, request_json, system, user, Message, Agent, Text, Reasoning, ToolCall, ToolResult, User
from std.net use Net

/// The events of a streamed reply: reasoning, text, a tool call whose
/// input arrives in two parts, then the stop reason.
fn events() -> List[String] {
  [
    "{\"type\":\"message_start\",\"message\":{\"id\":\"m\"}}",
    "{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\",\"signature\":\"\"}}",
    "{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"add them\"}}",
    "{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig\"}}",
    "{\"type\":\"content_block_stop\",\"index\":0}",
    "{\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}",
    "{\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Adding \"}}",
    "{\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"now.\"}}",
    "{\"type\":\"content_block_stop\",\"index\":1}",
    "{\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"add\",\"input\":{}}}",
    "{\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"a\\\": 2, \"}}",
    "{\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"b\\\": 40}\"}}",
    "{\"type\":\"content_block_stop\",\"index\":2}",
    "{\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"}}",
    "{\"type\":\"message_stop\"}",
  ]
}

fn main(net: Net) {
  claude := Claude { net, key: "unused" }
  mut s := Sse {
    claude,
    conversation: [],
    tools: [],
    body: None,
    content: [],
    slots: dict([]),
    inputs: dict([]),
    queued: [],
    stop: "",
    done: false,
  }
  for e in events() {
    match on_event(mut s, e) {
      Ok(_) => (),
      Err(x) => print(x),
    }
  }
  for c in s.queued {
    print(c)
  }
  print(s.content)
  print(s.stop)
  print(on_event(mut s, "{\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}"))
  conversation := [
    system("Be brief."),
    user("2 + 40?"),
    Message { role: Agent, content: s.content },
    Message { role: User, content: [ToolResult { id: "t1", output: "42", failed: false }] },
  ]
  print(request_json(claude, conversation, [], true))
}
