Ideas for possible future changes to Strata and its tools: not built, not decided. Bugs are in BUGS.md, design problems in ISSUES.md, decisions in REBUILD.md.

## Language

### Named arguments at calls
Proposed (user, 2026-11-27): arguments named at every call (`send(to: user, message: text)`), as in Swift and Flutter, so parameters get full-word names that say what a comment would. Rules so far:
- **Punning** as in records: an argument that is a variable of the parameter's name is written bare (`send(to: admin, message)`).
- **One-parameter functions are named too** (the name is often useful). The first parameter (after `self` in a method) may have a name starting with `_`: it is then written without a name at calls (`fn sqrt(_x: Float)`, `sqrt(2.0)`) and read as `_x` in the body. Only that one slot, so it is not positional in general. A bare `_` keeps its one meaning, a value thrown away (user, 2026-11-27). Note: in Rust and Python `_x` means "unused", which agents may read into it.
- **`self`** is never named (method calls use dot syntax). **Spreads** give no names (the spread record's type says which fields fill which parameters).
- **`mut` and `cap` stay written**, on the value: `grow(items: mut list)`, punned `grow(mut items)`, the same text as the declaration `fn grow(mut items: List[T])`.
- **Function values** are called by position: a function type `(I64, I64) -> I64` has no parameter names, so `f := add; f(1, 2)` and a lambda passed to `map` take positional arguments. (Function types with names would be an extension.)
- Stored as core in parameter order; the printer writes the names and leaves out punned ones, so renaming a parameter renames it at every call site with no edit.
- A parameter's name is part of the interface: renaming it is a breaking change, and `rename` rewrites every call site in the package (user, 2026-11-27).
- **Optional arguments can be left out**: a parameter with a default (`timeout: Duration = seconds(30)`, `limit: Option[I64] = None`) is omitted at calls that don't need it; no `None` written (user, 2026-11-27; see "Parameter defaults").
- **An impl may add parameters to a trait's method only if they have defaults**, so a call through the trait still fits. A capability can't have a default (that would be authority from nowhere), so a method that may need capabilities takes them as a trait argument chosen per impl (`Component[E, O, C]` with `C` the exact record of capabilities, `{}` for none). Where that record type is `{}`, the argument is left out at calls (sugar for passing `{}`), so pure impls show no capabilities anywhere (user, 2026-11-27).
### `Option` and `Result` as unions
Proposed (user, 2026-11-27; to settle in the spec, section 6). `None` is a type with one value; `Option[T]` is `T | None`, written `T?`, so a value needs no `Some` (`find(...) -> User?`, return `user` or `None`). `Result[T, E]` is `T | E` with `E: Error`, and a function lists the errors it can return as a union: `fn load(...) -> Config | NotFound | Denied | BadToml`. `x?` returns `x`'s error members from the function (each must be in its declared union) and leaves the rest; `x except NotFound { … }` handles one member and the type narrows to the others. No wrapper enums and no conversions between error types, as in Zig's error sets and Roc's tag unions. A parameter `p?: T` is `p: T? = None`, so it can be left out; there is no `p!: T` (an error type has to be named).
Known costs, to decide in the spec:
- **Flattening**: `T?` where `T` is itself `U?` is just `U?`, so `dict.get(k)` on a `Dict[K, V?]` can't tell a missing key from a stored `None` (Kotlin's `T?` has this). Either generic code that must tell them apart uses a separate type (`Missing`), or a type parameter can be bounded as never `None`.
- **Overlap of success and error**: `T | E` needs `T` and `E` disjoint; with `E: Error`, the success type must not implement `Error` (checked where the union is formed, including in generic code).
- **Run-time tags**: every member of a union needs a tag. The lab's unions already have one per member type, unique in the program; a pointer type's `None` can be the null pointer.
- **Long error lists**: an alias names a group (`type ConfigError = NotFound | Denied | BadToml`), and a tool can show or write the union a body actually returns.

### Parameter defaults
Only record fields have defaults. A parameter default would let a spread or a call leave that parameter out. Open: whether a default is filled in where it is stored or where it is called, as for record fields. (Claude, 2026-11-25)

### Operators through special methods
The syntax for ranges, slices and steps (`a..b`, `xs[a..b]`, `0..n by 2`) could desugar to special methods (`@range`, `@slice`, `@step`) that std traits implement. The same mechanism would give `==` and `<` on user types, so an ORM condition reads `Todo.done == false`. Open: which operators are special, and how the prover sees a user's `==`. (user, 2026-11-21)

### Indexing dicts: `d[k]` and `d[k] = v`
`at` has no `Dict` version. `d[k]` would require the key to be present, an obligation for `verify` as an index in range is. (Claude, 2026-11-20)

### A filter written on a value: `xs where it > 0`
The refinement syntax could filter a list. It reads like a refinement type, which may confuse. (user, 2026-11-20)

### More patterns in lambdas
Lambdas take patterns as parameters, but not an explicit tuple pattern `((i, x)) -> …`, a nested one `(acc, (k, v)) -> …`, or typed parameters where a pair is expected. A bare record pattern `{ x, y }` (without the type's name) is not a pattern anywhere. Each is a syntax change to ask about. (agent report, 2026-11-25)

### Defaults for refined aliases
`type Port = Int where … = 8080` would give a default to every field or option of that type. The user set it aside. (user, 2026-11-20)

### Structural records: a refinement that implies the bound's
A nominal record's refined field must have the same written type as the structural bound's. Accepting a refinement that implies it would need the solver at check time. (Claude, 2026-11-26)

### One form for builtin calls in printed code
`len(xs)` and `xs.len()` are both accepted. The printer could pick one form (the method form in chains) so code reads the same everywhere. (Claude, 2026-11-20)


### Capabilities marked in types
A capability parameter is marked with a keyword, the way `mut` marks a parameter changed in place: `fn main(cap console: Console, cap fs: Files)`. A reader sees at once which parameters reach the outside world. Clearer than a sigil (`*Console` reads as a pointer, `@Console` as an annotation), and agents already know the `mut` pattern. Call sites are marked too, as `mut` arguments are (`greet(cap console)`), so a reader sees a capability being handed on. That is the point (user): every call that hands on a capability, and so may have effects, is visibly marked. It is function colouring by a keyword, written at every signature and call site on the path, as `async` is; the threading refactor (REBUILD.md L5) is only an edit aid that writes the parameters and marks for you. To settle in the spec, sections 1 and 6: whether `cap` is also required on fields and locals that hold a capability. (user, 2026-11-27)
### Assurance levels
Per package or module, like SPARK's levels: default (unproven checks run, a broken one panics); checked (a function with unproven contracts gives callers a `Result` with a contract error); proven (the build refuses any unproven contract or partial operation); bounded (termination, stack depth and memory proven too). Open: names, and whether "bounded" is within reach with counted, immutable values. (user, 2026-11-22)

### Two solvers, each for its strengths
Z3 and cvc5 are both wired in. A query could go to cvc5 when it holds strings or large Unicode regex classes, to Z3 otherwise, or to both with the first definite answer taken. This pairs with full-Unicode regex proofs, which Z3 can't decide. (user, 2026-11-20)

### Cost of a function
A function can terminate and still take exponential time. A cost class per function (constant, linear in a list, ...) would also let `build` flag a contract that walks the whole data through a helper, not only through `all`, `map` and the like. Nothing is designed. (user, 2026-11-20)

### Verification fast enough to run with every check
Two tiers: a fast pass next to the type check with a small budget per query, then a slow pass in the background with full limits and fuzzing. A long-running verification server would keep the tree, the call graph and the verdicts, and re-verify on each change. Open: what an agent sees while the slow pass runs, and whether `edit` waits for the fast pass. (user, 2026-11-20)

### More loop invariants derived from the code
Not derived yet: a prefix fact for a list only pushed to (`take(xs, len(xs₀)) == xs₀`), frame facts for dicts (keys not inserted keep their entries), modular facts of induction variables (`i % 2 == i₀ % 2`), steps by a variable amount, and relevance by data dependence instead of by name. Each removes some written invariants. (Claude, 2026-11-25)

### Quantifiers over a dict's entries
`all(d.items(), f)` is modelled only at the keys the code reads, and `all(d, f)` on a `Dict` or `Set` itself has no overload. A real quantifier over the entries would let helpers over dicts become SMT definitions stated once per query, which examples/tasks needs. Mutually recursive helpers could be seen through the same way (`define-funs-rec`). (Claude, 2026-11-25)

### Fuzzing code that reaches `extern`
`verify` never runs code that reaches an `extern` function, so it stays unproven or skipped. Options: run only pure C, allow C only inside capabilities, or run such code in a child process. (user, 2026-11-26)

### Solver-made inputs when few random ones pass
Today the solver is asked for inputs only when every random one fails a `requires`. Asking it when the acceptance rate is low, and mutating its models, would help types the solver models that the program has no builder for. (Claude, 2026-11-25)

### State graphs, cheaper
`view --graph` is off: on examples/tasks it took minutes and gigabytes with most moves unknown. A cheaper design would ask fewer questions per function, give a cheap answer when the solver can't settle one, and bound memory. A run's graph could also show `mut` parameters if the trace printed them. (user, 2026-11-26)

### Laws under interleavings
A law says what one worker sees alone. Laws over interleaved calls on a `shared` trait would say what holds when workers race. (Claude, 2026-11-25)

### `assumed` for dependencies
A dependency's contracts could be taken as assumed, so a package is verified against them without verifying the dependency. (roadmap, 2026-11-22)

### Lemmas inside traits and impls
Lemmas are top-level only. Lemmas in `impl` and trait blocks, and a `calc` step with its own `by`, would let proofs sit next to what they are about. (Claude, 2026-11-26)

### Revisit the `decreases` defaults
Picked without asking: one clause per loop or function (a tuple for several), stored as the body's first statement, an unproven clause makes the verdict "may not terminate", and the runtime check covers only direct self-calls (mutual recursion has none). Each is easy to change. (Claude, 2026-11-25)

## Memory and speed

### Self-compile in under a second
`check` after a one-function edit is under a second; `build` after one is about 4 s. Left: keep resolver errors, lint and unused-definition results per definition; decode lazily or hold the trees in a long-running process; keep each definition's expanded and typed tree by its key; make `unique` and `kept_by` incremental over the call graph; reuse each function's IR; link packages with C through lld; compile std's library in parts. (user, 2026-11-24)

### Names as symbols throughout the tree
Names in the tree are strings. Symbols (integers into one table) would make comparing and hashing names cheap, but the change touches about 3000 sites and gains a few percent now. Worth doing once the tree is stable. (user, 2026-11-24)

### The stored tree without sugar
Store the program desugared and let only the printer put sugar back. Fewer node kinds for every pass and nothing desugared twice. Each piece of sugar needs a shape of its own so the printer gives back what was written; sugar that depends on other definitions (`derive`, `comptime`) stays as written. (user, 2026-11-24)

### Codegen through LLVM's builder
Codegen writes IR text that is then read back. Calling the builder from codegen's emit sites, with operands as handles, would save the string building (about 0.3 s on compiler/). (Claude, 2026-11-24)

### Fully in-place functions
After Koka's `fip`: a marked function is checked to run without allocating, each constructor reusing memory an input gave up. Then no counting in hot loops and bounded memory, as properties the checker or `verify` proves. (user, 2026-11-21)

### Less counting
Ideas not built: values of types whose values are all constants (enums) as immortal; a dict lookup that returns a borrowed slot instead of an `Option`; arenas for decoding and compile-time values; string interpolation as one runtime call; tracking fields in the static uniqueness analysis; moving a variable at its last use (needs liveness). (Claude, 2026-11-24)

### Lanes for growing lists
After Goose: a list grown in a loop gets its own bump area, so it stays at the top while the loop's other objects go elsewhere. Large lists outside arenas are also not handled. (coordinator, 2026-11-26)


### Lists sized from the loop that fills them
A list made empty before a loop that pushes onto it in most iterations is made with room for the loop's count (`len(xs)` of a `for` over `xs`, `b - a` of a range), so it is never grown. Pushes under a branch still count: room for the count is at most one oversized block, freed or shrunk when the list is done. Growth itself stays lazy: a buffer grows only when a push finds it full (as the lab does: `list_push_owned` checks `used == cap` before writing), so filling a list to its capacity and emptying it never reallocates. (user, 2026-11-27)

### Flat and interned representations by derive
A sum type keeps its meaning in code (`type MyTree = Atom | Children(List[MyTree])`, built with constructors, taken apart with `match`, recursion as usual), and a derive picks how it is stored:
- `derive Flat`: the nodes of one tree in arrays, one entry per node in postorder (a tag array, a payload array, children as index ranges into a child array). A value of `MyTree` is a handle: the tree's arrays (counted once, as one block) and a node index. `match t { Children(cs) => … }` reads the tag at the index and gives `cs` as a list of handles into the same arrays; constructors append to the arrays of the tree being built, children first, which is the order a bottom-up build makes them anyway. Costs: a subtree is never changed in place (a change copies the path to the root, or rebuilds); keeping one small subtree keeps the whole tree's arrays alive.
- `derive Interned`: each distinct value stored once in a table; a constructor looks its value up before making it, so equal values are the same index and `==` is one comparison. For names, types and prover terms; diff, merge and the prover then compare subtrees in one step.
- Both are representation, not type: the derive generates the constructors, the pattern readers (the views `match` uses) and `Eq`, and proves them against the plain sum's (`match` on a flat value gives what it would on the plain one), so code and proofs never see the difference.
- Parallel: a frozen flat tree is read by any task without counting. Built in parallel, each task fills its own arrays and a join concatenates them, shifting the indices; an interned table is sharded by hash, or each task interns locally and a merge renumbers.
- Known in other systems as data-oriented ASTs (Zig's compiler, Carbon), struct-of-arrays layouts, and hash-consing. Without this, `derive Tree` gives only the traversal interface (fold, rewrite, find over the plain layout). (user, 2026-11-27; L6 for the derive, L7 for the layouts)
### Unboxed fixed-size arrays in the language
A built-in `Array` with unboxed elements (`Array[U8]` as bytes), written in place, indices proven in bounds. Open: whether its size is known at compile time (`Array[U8, 64]`), and how it relates to `std.array`. (user, 2026-11-24)

### The runtime optimized with the program
The runtime is a native object linked into each program, so its calls are never inlined. LTO of the runtime into the program would need bitcode parts and LTO at each link. (Claude, 2026-11-25)

### A larger stack for native `main`
The JIT runs `main` on a larger stack; a native binary could too, so recursion depth doesn't depend on how a program is run. (Claude, 2026-11-23)

## Concurrency

### A deterministic scheduler for tests
`strata test` could run tasks on a seeded scheduler, try several orders and replay a failing one, as loom and shuttle do, chosen by the test runner's `RunMode`. (user, 2026-11-22)

### Blocking `extern c` calls on a scheduler
A blocked C call holds a whole OS thread. Go hands that thread's other tasks to a new one; Strata could do the same. (Claude, 2026-11-22)

### Supervision beyond one task
Memory budgets per supervised task (running out as that task's panic), restart helpers in std (one-for-one, all-for-one), supervised scopes, and cancelling every blocking I/O call, not only `read_line`. (user, 2026-11-22)


### Shared state across tasks
Values are never shared mutably; a task that owns state serves it to others (an actor). Sketch:

```strata
type CacheRequest =
  | Get { key: String, reply: Sender[Option[Bytes]] }
  | Put { key: String, value: Bytes }

/// A cache shared by any number of tasks; copies of it are cheap (a sender).
type Cache = { requests: Sender[CacheRequest] }

fn serve_cache(requests: Receiver[CacheRequest]) {
  mut entries: Dict[String, Bytes] = dict()
  for r in requests {          // ends when the last Cache is dropped
    match r {
      Get { key, reply } => reply.send(entries.get(key))
      Put { key, value } => entries[key] = value
    }
  }
}

impl Cache {
  fn get(self, key: String) -> Option[Bytes] {
    (reply, answer) := channel()
    self.requests.send(Get { key, reply })
    answer.receive() ?? None
  }
  fn put(self, key: String, value: Bytes) { self.requests.send(Put { key, value }) }
}

fn start_cache(s: Scope) -> Cache {
  (requests, inbox) := channel()
  s.spawn(serve_cache(inbox))
  Cache { requests }
}
```

- **A shared capability** is the same shape: `impl Db for Pool` where `Pool` is a handle to the task that owns the connections; tasks receive `db: @Db` (a copy of the handle), and the trait hides that a message is sent. A test passes a `mock Db` instead.
- **Read-mostly state** (configuration, route tables, a game world's static map): the owner publishes frozen snapshots on a watch channel (it keeps only the latest value); readers take the current snapshot and read it without counting or copying.
- **std could give the generic form**: `actor(initial, (state, request) -> state)` returning a handle, with `ask` for requests that reply, so most programs write only the state type and the update.
- Cost: one message hop per access on the green scheduler; batched requests or snapshots where that is too much. (user asked for the sketch, 2026-11-27; REBUILD.md L3)
## Tools

### Structural search and rewrite: `strata search`
A pattern is Strata code with holes (`?x`, `...`, `(?x: T)` for a typed hole), with `--where` conditions on types, regexes and modules, `--text` over strings, comments and docs, and `--rewrite` that previews and applies an edit. Saved rules in strata.toml could run in `check` as lint. Names in a pattern mean what they resolve to. (user, 2026-11-24)

### Fault locations
Runtime faults could print each frame as a location (`DEF@HASH/PATH`) shown as code, with `view --last-fault`, `--trace` saying where each call came from, and refutations naming the failing call's place. Needs paths carried through desugaring and instantiation. (user, 2026-11-24)

### Undo and rebase on trees
Undo a change as a three-way merge with the roles turned (`tree.merge(after, now, before)`), per log entry or per node change. Rebase a branch by replaying each commit with the tree merge, conflicts as holes, `strata rebase --continue` once filled. The user said later, not now. (user, 2026-11-26)

### Better tree merges and history
Pair definitions by stable id instead of similarity, so a rename on one side and an edit on the other merge node by node. Keep both sides' items when both add to an unordered list (methods, fields). Show a conflict's three sides apart and pick one with `strata resolve --ours/--theirs NAME`. A `revert` that keeps later changes (three-way), and `history` that follows `strata move`. (user, 2026-11-26)

### Live edits that change types
Live edits swap function bodies only. Left: edits that change types, with values converted (a record gaining a field with a default); changes to `main` taking effect at its loop's next pass; edits sent straight to the running process instead of through the files; watching dependencies; per-function verify so a swap waits only for the changed functions' proofs. (user, 2026-11-24)

### References by hash and a names table
Unison's model: definitions stored by hash, references by hash, names in a separate table. Renames become free, merges of new code can't conflict, and several agents can add to one module at once. Open: how it relates to module files, and how a hole keeps what it replaced. (user, 2026-11-20)

### Agents working side by side
`--json` on every command, trace queries, and scoped workspaces with leases so several agents work on different slices at once. The CLI is the agent API. (roadmap, 2026-11-20)

### A Strata IDE
The viewer as a desktop app speaking ACP, so agents run inside it, with a Strata MCP server giving them structural tools (slices, check, edit, rename), and live marks of where each agent is working. Idea stage. (user, 2026-11-20)

### REPL: the package compiled once
A line compiles the package functions it reaches. A package library loaded once would make lines faster, if a line's redefinition can reach the library's callers. Also: tasks kept between lines, and output shown as it is printed. (user, 2026-11-25)

### Docs: checked examples and links
Compile and run the examples in doc comments; link a signature to the types it names; print links between pages as `strata doc PAGE#ANCHOR`; full-text search over the pages; pages for core's builtins. (user, 2026-11-25)

### Tour: jump, wrap, HTML
Jump to a definition by name in `strata tour`, wrap long lines instead of cutting them, and render a tour as HTML. (user, 2026-11-26)

### Faster decoding of module files
A built-in string table shared by every file (`Int`, `List`, `self`, …) with fixed indices, and a flat tree (nodes in arrays, children as indices) freed in one go. (user, 2026-11-20)

### Viewer hints
Holding a key could show hints in the viewer: a type expanded into its members, parameter names at a call. (user, 2026-11-21)

## Standard library

### Web pages and apps
Typed attributes (`href` taking a `Url` that excludes `javascript:`, `class` from a list); keyed lists in the client's morph; forms read into a typed record with per-field errors; typed route parameters checked against the handler, `*` wildcards, static files; an app and routes in one program; sessions kept through the ORM; Elm-style commands for slow work in `update`, run in tasks; the site's, terminal's and VS Code's highlighters knowing markup. Open: can `view` use capabilities, and whole page or diff per message. (user, 2026-11-20)

### Apps in the browser
A WebAssembly build of a web app, so the same code runs client-side. (user, 2026-11-20)


### UI components with their own state
The Elm architecture keeps one model for the whole app, which grows unwieldy. Components with local state, as in React, SwiftUI, Flutter and Iced: a component is a type with its own state, an `update` from its events, and a `view`; the runtime keeps each component's state at its place in the view tree (by position, or by a key in lists), so its parent never holds it.

```strata
type Button = { label: String }                 // what the parent passes in
type ButtonState = { pressed: Bool }            // private to the module
type ButtonEvent = | Down | Up

impl Component for Button {
  type State = ButtonState
  type Event = ButtonEvent
  fn initial(self) -> ButtonState { ButtonState { pressed: false } }
  fn update(self, state: ButtonState, e: ButtonEvent) -> (ButtonState, Option[Clicked]) {
    match e { Down => ({ pressed: true }, None), Up => ({ pressed: false }, Some(Clicked)) }
  }
  fn view(self, state: ButtonState) -> Html[ButtonEvent] {
    button(color: if state.pressed { dark } else { light }, on_down: Down, on_up: Up, self.label)
  }
}
```

- Encapsulation: the state type isn't public, so a parent sees only what goes in (the props, `Button`) and what comes out (`Clicked`), as an object's private fields and public methods would give.
- State stays a value: `update` returns the new state; the runtime stores it. Nothing is mutated behind the program's back, so updates stay provable (`ensures` on `update`) and replayable.
- When `update` runs: on events, not every frame. A `view` attaches each handler to its component's place in the tree (`on_down: Down`), so the runtime knows which component an event belongs to. Events are queued and handled one at a time on the UI's loop. For each event the runtime:
  1. calls that component's `update` with its stored state;
  2. stores the new state;
  3. if the state changed (equality on immutable values is cheap), calls that component's `view` again, and its children's only where their props changed;
  4. diffs the new tree against the old one and patches the screen.
  An event the component sends out (`Clicked`) is delivered to its parent's `update` the same way. Async work (a request, a timer) is a task that posts an event back when it is done. Per-frame work (a game, an animation) subscribes to a `Tick(dt)` event, which only subscribed components receive. Finer-grained updates later, as in Solid or Svelte: record which state each part of a view reads and change only those nodes, with no diff.
- Nothing global is threaded into components. The runtime owns the event sources, and only it holds capabilities (`run_app(cap screen, cap net, App {})`). A component describes, as data, what it wants:
  - the handlers in its `view`;
  - `subscriptions(self, state) -> List[Sub[Event]]` (`every_frame(Tick)`, `key_down(Pressed)`), read again after each update;
  - commands returned from `update` (`Fetch { url, done: Loaded }`), which the runtime runs with its own capabilities and whose results come back as events.
  So `update` and `view` stay pure: provable, and tested by calling them, with no mocks. A child is placed with `mount(child, on_out: f)`, which maps the child's output events to the parent's events.
- **Types inside an impl.** The sketch uses associated types (`type State = ButtonState` in an `impl`), which the lab doesn't have. Either the spec adds them (section 6), or the trait takes them as arguments, as operator traits already do (`impl Component[ButtonState, ButtonMessage, Clicked] for Button`). Associated types read better and let `C.Out` be named; tried in the lab first.
- **Messages.** A component's own message type (Elm's `Msg`) is what its view's handlers send to its own `update`; its `Out` is the message it sends up. Unlike Elm, the parent doesn't wrap and forward its children's messages: the runtime routes each message straight to the instance whose view made it, and only `Out` goes up, through `mount`'s `on_out`.
- **The runtime keeps the whole instance tree.** Each mounted component is an instance (its props, its state, its children); a node of the screen's tree records the path of the instance that rendered it (child indices from the root, or keys in lists) and its handler's index. A click is routed down that path; the target's `update` runs, its `Out` is mapped by the parent's `on_out` and handed to the parent's `update`, and so on up. Typed with no `Any`: an instance is a trait value `Live[P]`, where `P` is its parent's message type.
- **Sending nothing**: a component that never sends anything up has `update` return `None`, the variant type, which is a subtype of every `Option[T]` (variants are types), so it fits the trait's `-> Option[O]` with no `Never`; `mount` then needs no `on_out`. `None` is a value, not `()`: the only value it can send.
- **Props and state are one**: every field is state; a parent's constructor may set any of them, the others take their defaults. When the parent renders again, the fields its constructor writes take the new values and the rest are kept. Which fields a parent may not set is the question of private fields, separate (user, 2026-11-27).
- **State in `self`, changed in place** (user, 2026-11-27; replaces the Elm shape above): a component is a record whose fields have defaults (`type CounterPage = { count: I64 = 0, panel_open: Bool = false }`), so no `initial`. `fn update(mut self, event: E) -> Option[O]` changes the fields in place and maybe sends one event up; `fn view(self) -> Html[E]`. Strata's `mut` has no aliasing, so this keeps Elm's safety (nothing else sees the change) with React's ease. Open: when the parent renders again with new field values, which fields are its props (taken from the new value) and which are state (kept): marked fields, or "fields the parent's constructor wrote are props".
- **I/O in components** (user, 2026-11-27): a component may take capabilities in `update`, so it can fetch without sending a message all the way to the runtime. The trait can't know which ones, so `update` takes one extra argument whose type each component chooses: `fn update(self, cap world: W, state, message)`, with `W` a record of capabilities (`{ net: Net }`, or `{}` for a button). The runtime passes the app's record of capabilities, which must have every field some mounted component asks for (a structural bound, checked at `mount`). Long requests run in a task that posts a message back, so the UI loop never waits.
- **Testing with do-nothing capabilities** (user, 2026-11-27; the default for every `mock`, see "Mocks do nothing by default"): a test passes capabilities that do nothing and hold no state (`noop Net`): methods returning `()` do nothing, `Option` gives `None`, `Result` an error variant named for it (`Unavailable`), other results a value from `Default`, or a compile error naming the methods that need a choice. A test could take them all at once (`test "x" with noop { … }`). The lab's `mock T = {}` with every method left out comes close (its missing methods are generated); what those return isn't recorded, so the spec states the rule.
- Shared state (the signed-in user, a cart): one or two stores passed down as context, each an ordinary state and update, which any component may read and send events to. (user, 2026-11-27)
### `std.http` and `std.net`
Streaming bodies (server-sent events), a client connection pool, `expect: 100-continue`, timeouts beyond a caller's `race`, binary bodies. (Claude, 2026-11-26)

### ORM: relations and keys
Joins and relations (a project's to-dos), a `Key[T]` over several columns, `WeakKey[T]` without `REFERENCES` for history that outlives its row (the user's call, later), and a second model of each key column's values so `verify` can require a delete to leave no reference behind. (user, 2026-11-25)

### Mocks do nothing by default
(user, 2026-11-27) `mock T` with no methods written gives a value that does nothing and holds no state: `()` results do nothing, `Option` gives `None`, `Result` an error variant, other results a `Default` value or a compile error naming the methods that need a choice. Anything more is asked for: a method written in the mock, data in its record, a simulated model (today's `mock Db` for a trait with models simulates them by default; under this rule it would be opt-in), random answers. A test can take do-nothing mocks for every capability at once (`test "x" with mocks { … }`).

### Mocks: recording and inline mocks
Recording a mock's calls (open: per value or per type), inline mocks (`mock Db { find(id) => None }`, the rest generated), and std's stand-ins as `pub mock`s. (user, 2026-11-21)

### `std.array`
`exp`, `log` and the trigonometric functions, `concatenate`, masks, `cumsum`, `linspace`, `solve`, and operators once special methods exist. (user, 2026-11-26)

### `std.toml` writer that keeps layout
A writer that keeps comments and layout, so `strata.toml` can be edited in place. (Claude, 2026-11-22)

### String functions in Strata
`trim`, case mappings, `is_digit`/`is_alpha`/`is_space` and number parsing are still C. Moving them to Strata needs the compiled prelude to reach std.unicode's tables. (user, 2026-11-22)

## Interop

### Foreign objects by handle
Values cross `extern python` and friends as JSON, which is slow for large data. Opaque handles to foreign objects (`extern python type Model`) or a binary path would keep NumPy arrays and models on their side. (user, 2026-11-21)

### More from `derive C for f`
Static libraries, Linux `.so`, `mut` parameters, calls from several host threads, wrappers that check a function's `requires` and answer an error, and a trait giving a file's head once. (user, 2026-11-25)

### `derive Rust for f`
Calling Strata from Rust, the other direction of `extern rust`. (user, 2026-11-25)

### Linking Rust with lld
Built executables with `extern rust` are linked by clang. Linking with lld would need a libSystem stub with what Rust's std calls. (Claude, 2026-11-26)

### The host layer as `extern c`
The `host_*` I/O layer as `extern c` declarations over the runtime, instead of cases per builtin. (Claude, 2026-11-21)

### A pool of foreign workers
`extern python` and `extern ts` use one worker per program with calls serialized. A pool would let tasks call at once. (Claude, 2026-11-25)
