---
title: Live edits of a running program
packages: compiler
---

# Why
`strata run` already swaps proven code into a running program when the
verifier alongside it finishes. The same slots can take edits: a server
started with `strata run --live` should pick up a changed function body
as soon as the edit checks and verifies, without restarting, and refuse
anything that isn't safe to change under a running program. Measured on
the case's loop: about 290 ms from `strata edit` returning to the new code
printing.

# Background
A swappable run calls each of its own functions through a stub that loads
its slot (c/jit.c's `stub`, `swap_in`). Verifying alongside starts a
process of its own and swaps its module in once, when it ends:

```strata start_alongside
```

```strata alongside_command
```

Each module file's header lists its definitions with an interface hash
and a code hash, read without decoding anything:

```strata binary.read_header
```

# Intuition
`message()` returns `"hello"`; the loop in `main` prints it when it
changes. Edit the body to `"world"`: same interface hash, new code hash,
so the edit may swap; it checks and verifies, so it is compiled and the
slot of `message` is pointed at `message.live.1`. Edit the signature to
`-> Int` instead: the interface hash changed, the running callers were
compiled for a `String`, so the edit is refused and `"world"` keeps
printing.

```strata-refs live_edit
```

# Which edits may swap
Bodies only, judged from the headers of the files as they are against
the definitions running.

```strata-diff live_changes live_snapshot
```

# Watching, and one process per edit
The watcher polls the files, judges each edit, and starts a fresh
`strata live-edit` for check, verify and codegen; it then tells the run on
its pipe and waits for the run's answer, so it knows which code runs.

```strata-diff live_edits_command live_edit live_edit_command
```

```strata-also
LiveEdit: how an edit went
live_refused text_or_empty: the run told why; reading a note
start_live jit_live jit_live_tell jit_live_now jit_live_sleep: starting the watcher, and its C side (c/jit.c)
run_command: `--live` starts the watcher instead of a verifier alongside, and compiles with `--live`
strata_command: the `live-edits` and `live-edit` commands
```

# Each function from its own code alone
A frame running old code calls new code, and an edited caller can change
what codegen concludes about a callee it never touched. Tested on a
program with state through 28 edits (`cli-run_live_state`), three ways
this went wrong showed up (`cli-run_live_frames`): an argument an old frame
put on its stack, kept by an edited callee; an entry check left out once
every call proved it, though the old frame's call doesn't; and a lambda
changed alone, whose maker's IR didn't change (fixed in c/jit.c's
`live_own`, which follows references). `--live` compiles every function
from its own code: every parameter kept, no proven updates in place, no
regions, entry checks kept.

```strata-diff codegen.Gen codegen.generate codegen.proven_entry
```

```strata-also
codegen_with: `--live` read anywhere in codegen's arguments
core_rt_text computed_value repl_eval: `generate`'s new `live` argument, false
```
