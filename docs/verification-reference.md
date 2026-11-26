# Verification reference

This page covers what `strata verify` reports and how to tune it. It adds detail to [verification](verification.md).

## Results

`strata verify` turns every contract into proof obligations and gives each function one of these results:

- **proven** (`✓`): the SMT solver proved every check. `--checks` lists them.
- **fuzzed** (`~`): some checks couldn't be proven, so the function was run on generated inputs and nothing failed. Each unproven check gives the reason, for example *depends on `fold` is not modelled*.
- **failed** (`✗`): a counterexample. A counterexample from the solver is compiled and run first, and reported only if it fails. Otherwise the fuzzer looks for one.
- **incomplete** (`○`): the function still has a hole.
- **assumed** (`≈`): trusted, not checked. This covers code that calls the system: a capability's own code (`capability ...`, such as std's `Terminal`, `Disk` and `Process`, or a program's own), C reached through `extern c`, and Python functions (`extern python`, never run, with contracts checked at run time). It also covers functions whose only unproven checks are `assumed` contracts (below).

The contracts of assumed code are taken as true where it is called, and the surrounding code is checked as usual. A function that calls C or takes a capability is proven from those contracts. Compiled code still checks a C function's `ensures` at each call, since nothing proves it. Only running differs: code that takes a capability isn't fuzzed, because only `strata run` provides one, and checks the solver doesn't prove are reported as skipped, with the reason.

Derived code is checked too. `derive Json for Signup` generates `Signup.to_json` and the other methods, plus copies of `std.json`'s helpers specialized for `Signup`'s fields (`std.json.encode_as.for.Signup.4`). These are listed and checked with it, so a fault that only some field types can cause is reported there. An example is an `Int` field that may exceed what JSON holds exactly, with `integers = "proven"` under `[json]`.

## What is checked

The checks are exactly those that compiled code makes at run time:

- a callee's parameter refinements and `requires`
- the refinements of constructed values and assigned fields
- annotated bindings
- `assert`
- division by zero
- the function's own result type and `ensures`

Calls are checked against the callee's contract, never its body, which is how modules stay independent. A function passed by name (`lines.map(line_total)`) is checked against one arbitrary element of the list.

**`old` in `ensures`.** `old e` is `e` evaluated when the function started, for `mut` parameters and `mut self`. Elsewhere in the clause, they mean their values at exit.

- `verify` proves the clause at each exit (the end and every early `return`), with the parameters' values there and `old` read over the values on entry.
- At a call seen through the callee's contract, the `mut` arguments come back as new values of their types, related to the values before the call by the `ensures`, and those values are written back.
- A method known only by its contract (a trait method called on a type parameter's value) returns the same result for the same arguments. So `c.total()` before and after `mut c.incr()` are related by `incr`'s contract.
- In compiled code, an unproven `ensures` with `old` evaluates each `old e` on entry (`codegen.keep_olds`), keeps it until the check at the exit, and shows it in the fault next to the final value: `(before: len(xs) = 1, after: len(xs) = 1)`.
- In the tree, it is `Old(Expr)`, expression tag 40.

**Assumed contracts.** `assumed requires e`, `assumed ensures e` and a loop's `assumed invariant e` are verified exactly like the same clauses without `assumed`. Their checks are reported with `(assumed: not checked when the program runs)`, including at calls to a callee with an assumed `requires` or `ensures`.

- A function whose only unproven checks belong to assumed contracts, none refuted, is reported **assumed** (`≈`, `main.assumed_verdict`), naming the contracts. `--require proven` accepts it like other assumed code. Its verdict is cached, its proofs are recorded, and the contracts it did prove are still dropped from the compiled code.
- The code generator leaves assumed contracts out whatever `verify` found (`codegen.assumed_compiled`). An assumed `requires` becomes `true`, so the other clauses keep their numbers. `--check-assumed` (`run`, `test`) compiles them in, and verify's fuzz drivers always check them.
- `build` prints one warning list covering each unproven assumed contract and each written contract kept as a run-time check that walks a list (`walks_list`: `all`, `any`, `sum`, `fold`, `filter`, `map`, `count`, `sorted`, a comprehension). A call to a function that walks a list isn't detected.
- `assumed` is a keyword only before `requires`, `ensures` and `invariant`.
- `strata verify --assumed` lists what proofs trust, by package: the program's modules, the std modules it reaches, and core. It shows each `assumed` clause and every contract of an `extern c` or `extern python` function. It runs nothing.
- The runtime's C functions (core.runtime) carry `assumed` contracts saying what the C does: the trusted base under every proof. `strata --file core verify 'core.runtime.*'` fuzzes each against its contracts through a wrapper with the same parameters and contracts that calls it (`main.runtime_wrappers`, a list's element type made `Int`), 200 cases each, and never proves them. A function with a `Ptr` parameter or result, or with no `ensures`, isn't fuzzed. A contract calls another runtime function where it can (`strata_int_add`'s `it - b == a`), so the fuzzer checks the C against the C. 57 of the 196 declarations have contracts, and 50 are fuzzed. Where the C implements a builtin that `smt` models, the contracts say what that model says, or less (`strata_str_byte_slice` is the clamping and length bounds of `byte_slice`'s model).
- In the tree, it is `Assumed(Expr)` around the clause's condition, expression tag 41.

**Refinements and contracts make the same checks.** A parameter's refinement is checked where a `requires` is (at each call, and assumed in the body), and a result's where an `ensures` is. That is why `strata check` asks for single-value facts as refinements and keeps contracts for relations: moving a fact changes no verdict. A `requires` that names one parameter, an `ensures` that names only `it` or one element of a tuple result, and a refinement that names another parameter are warnings, which `strata fix` rewrites. Named results (`-> (next: Q, added: Natural)`) are read in `ensures` as the tuple's elements (`next` is `it.0`) and reported by name.

## How the encoding works

The prover lives in `compiler/smt`. It executes the body symbolically, with values as SMT terms.

**Datatypes.** Records, sums, tuples and unions are SMT datatypes (`declare-datatypes`). A parameter's outer level is its fields. A part that is itself structured is a single term of its type's datatype, opened through the datatype's selectors the first time a field is read or a pattern looks inside. A value of a large recursive type, such as a syntax tree, therefore costs only what the function reads of it. The solver knows the datatype axioms by itself: equal values have equal parts, different constructors make different values, and a value is never one of its own parts (`t != t.left` is proven). A type's refinements are a predicate on the term, defined for each term the code opens. A list inside a datatype value is a term of a list sort, with its length and items as functions of the term.

**Lists.** A list is known by its length and by how its elements were produced: a checked type, a literal, `range`, `filter`, `map`, `+`, `push`, or a part of another list. Elements are tracked position by position where possible. `all(xs, p)` is proven on an arbitrary element and, once known, gives `p` for every element the code reads.

**Unions.** A value of a union (`Square | Circle`) is a tag saying which member it holds, plus a value for each member. A member value passed where the union is expected, or branches returning different members, are combined into one. A `match` arm narrows it to its member.

**Control flow.** Branches are merged with `ite`. A loop keeps, for the variables it changes, only their declared types and its invariants, inferred or written. After a `while`, it also knows the condition is false, unless a `break` exited.

**Queries.** Each check becomes the question "can its negation be true under what is known here?", sent to the solver (Z3 or cvc5) as SMT-LIB text. `unsat` means proven. `sat` comes with a model, which becomes the counterexample. Debug flags:

- `verify --smt` prints the queries.
- `verify --obligations` prints each obligation with the solver's answer (`proven`, `refuted` or `unknown`), without running anything.
- `verify --termination` prints each function's termination verdict and whether contracts may see through it.
- `verify --fuzz` prints the fuzzer's verdict alone, with no solver.

**Dicts and sets.** A dict is an SMT array from each key to an `Option` of its value, and a set is an array from each item to a `Bool`. Each has its size as a function of the array.

- `get`, `contains`, `len`, `insert`, `remove`, literals, and set `union`, `intersection` and `difference` are array terms (`select`, `store`, a constant array for `{}`).
- The size changes by one when an `insert` adds a new key or a `remove` deletes an existing one. These are facts about each operation, with no quantifier.
- Two dicts are equal when they have the same entries, so `b == old b` holds after a function that puts back what it took out.
- A `for` over a dict's entries or a set's items reads each from the array.
- A value read from a dict is valid for the dict's value type (a `Dict[Int, Loc]` whose `Loc` has `price > 0`), because each operation carries the dict's validity to the next.
- `keys`, `values`, `set(xs)` and `dict(pairs)` of a non-literal list are known only by their sizes.
- A counterexample shows an input dict or set as a literal of its first entries (`Set[String] { "x", "" }`).

**Unmodelled values.** Anything not modelled becomes an unknown value. This includes floats, positions after `filter` and `sort`, most higher-order builtins, and unions whose members can share a shape (`List[Int] | List[String]`) or include a type parameter. An unknown value can only make a proof fail, never make a false proof succeed, and the reason is kept for the report. `Int` is a mathematical integer, as at run time.

**Strings.** An SMT string is a sequence of code points, so `len(s.code_points())` is its exact length. String positions are byte offsets. `byte_len(s)` is a function of the string known to lie between its number of code points and four times that, and equal to it on an ASCII string. `byte_slice`, `index_of` and a match's position are exact on ASCII strings and bounded on others, so refinements on lengths and positions stay provable.

- Modelled exactly: `+`, `contains`, `starts_with`, `ends_with`, `chars` (its length, and each element one character), `replace` with a non-empty pattern, `to_string` of an `Int`, `Bool` or `String`, and `is_digit`.
- `replace` with an empty pattern is unknown, because it puts the replacement around every character, which SMT's `replace_all` doesn't do.
- `parse_int` is exact for digits with an optional `-`, and for strings with no digit (`None`). Other forms (whitespace, `+`, `_`) are unknown.
- `trim` keeps only a length bound.
- `to_upper`, `to_lower`, `is_alpha`, `split`, `lines`, `repeat` and `reverse` are unknown.
- `chr` checks that its argument is a character, and `ord` that its argument is one character.

**Regular expressions.** `matches(s, "regex")` becomes a regular-expression constraint with `std.regex`'s semantics; `std.regex.smt_model` builds the prover's model. Strata regexes are Unicode regexes: `\d` is any decimal digit, `\w` and `\s` are Unicode classes, `\p{..}` works, `.` is any character except `\n`, and `(?i)` is Unicode case folding.

- The pattern is parsed by `std.regex` and translated exactly: literals, classes, groups, alternation, repetition, and `^` and `$` at the very start or end.
- Word boundaries, multi-line anchors and anchors elsewhere are unknown.
- Z3 can't decide classes with hundreds of ranges (`\w`, `\p{L}`, Unicode `\d`), so such a class is bounded from both sides: `matches` is true on a subset of its ranges and false outside a superset.
- When one string is matched against two patterns, the relation between their languages (inclusion or disjointness) is computed exactly from the patterns' automata and added as a fact. So `\d{3}` implies `\d+`, and `[a-z]+` implies `\w+`.

**Strings past U+2FFFF.** SMT-LIB strings hold the code points U+0000 to U+2FFFF; both Z3 and cvc5 stop there. Strata strings go up to U+10FFFF, so a proof about strings covers only strings whose characters are all at most U+2FFFF. A string literal with a later character (planes 3 to 16: rare CJK, tags, private use) is unknown. The cross-check, which runs proven functions on generated inputs, covers the rest.

**Helpers in contracts.** A contract can call an ordinary function, as in `type Digit = Int where small(it)` with `fn small(n: Int) -> Bool { 0 <= n && n < 10 }`. Inside a contract, a helper that terminates and can't fail is seen through: its body is used, not only its contract. A large body (2000 bytes of SMT or more), or one that calls a recursive helper (below), is stated once in each query that uses it, as an SMT definition (`define-fun`) that each use applies; a helper whose `ensures it == e` says what it gives is defined by `e`. A helper that calls a recursive one is seen through too. A small body is used at each call, as is one over lists, dicts or sets (what is known of their elements is stated where the code reads them, which a definition would hide) and one that isn't a single term of its parameters (it makes values the solver knows nothing about, such as a dict's entries, `set` or `filter`). So `fn zero() -> Digit { 0 }` is proven and `fn ten() -> Digit { 10 }` is refuted. A helper that may not terminate is never seen through, because unfolding it could let the solver prove false things. A helper marked `terminates` is seen through, and the proof records that it relied on the mark.


**Spec functions.** A function of the program's own (not core's or std's) that runtime code never calls, only contracts and proofs, that a lemma names, and whose body quantifies or makes a set (`all`, `any`, `count`, `filter`, `set`, a comprehension) is a spec function, found without a keyword: examples/tasks' `tasks_kept`, `new_pending`, `all_locked`. Outside a proof, a call of one is a symbol of its arguments: two calls on equal arguments are equal, in the function's queries, a lemma's and an `assert … by`'s, so a lemma's `ensures tasks_kept(a, b)` is the `tasks_kept(a, b)` a loop invariant states. Its body is seen through only in a proof: a lemma, a law, and an `assert … by`'s condition and steps, where the body's quantifiers stay in small queries. In a function's own queries what is known of it comes from the lemmas its proofs call. A helper no lemma names, and a small helper with no quantifier (`holds`, `after_attempt`), is seen through as before: the proofs written without lemmas need that. Any function seen through its contract is also one function of its arguments: equal arguments, equal results.

A written loop `invariant` sees through helpers the same way (`invariant head_first(steps, orders)`).

A function given to `all`, `any`, `filter` and the like is read as a contract would read it, so the helpers it calls are seen through. A call of the function being checked is the exception: in the function's own code it is known by its contract, as a recursive call made directly is (`if ms.all(m -> refn(types, m, lit, fuel - 1) == Plain)` in `compiler/contracts`). Seen through, its body would be seen through again at each element the function is applied to, nested.

A recursive helper that calls only itself, terminates and can't otherwise fail (`fn count(t: Tree) -> Natural`, `find` down a search tree) becomes an SMT function of its arguments inside a contract. Its body is stated once in each query that uses it, as an SMT definition whose recursive calls are applications of the function. At each use, the application is said equal to that definition's, and so are the recursive calls inside it, two levels deep, each with the helper's contract. The same arguments therefore give the same result wherever it is called. A contract about the helper is then proven using the helper's own contract at its recursive calls. For example, `ensures count(it.0) == count(t) + if it.1 { 1 } else { 0 }` is proven for an AVL tree's insertion, rotations included. std's `SortedDict` and `SortedSet` state their sizes and what `insert` and `remove` find this way.

**Lists in contracts.**

- Elements are known by position where the list's construction allows it: a parameter, a literal, a concatenation, a slice.
- At a loop's head, a list the loop changes is any valid list of its declared type (`mut q: List[Resting]`). Each element taken (`o := q[i]`) is therefore a valid `Resting`, and facts about its fields (`o.remaining - min(r, o.shown) >= 0`) are proven.
- Two lists the code compares (`invariant n == 0 ==> q == orders`), once known equal, agree at the positions the code reads from either. The facts are made only at those positions, with no quantifier.
- `all` and `any` nest. `all(0..len(ms), i -> … any(0..i, j -> …))` is proven for one call whose result is appended to a conversation of that type. Several such calls are not yet cheap (see IDEAS.md).
- `sum(xs)`, `sum(xs.map(f))` and `fold(xs, init, (acc, x) -> acc + e)` are modelled by how the list was produced: a literal's elements, the two parts of a concatenation (so `sum(xs + [x]) == sum(xs) + x`), and the positions a slice takes from its source (`sum(xs[..k]) + sum(xs[k..]) == sum(xs)`). Any other list has its own prefix-sum function, 0 at 0, and a slice's sum is the difference of two of its values.
- Bounds on the elements carry over to a sum: at least 0 when no element is negative, at least the length when each is at least 1, and at most (or at least) the length times `m` when a fact about every element (`all(xs, x -> x <= m)`) gives one.
- `fold` with any operator other than `+` is not modelled.

## Termination

`verify` also reports whether each function always finishes.

**terminates**, when shown automatically:

- No recursion, and only `for` loops.
- A `while a < b` loop (also `<=`, `>`, `>=`, `!=`) whose bound `b - a` the solver proves decreases on every pass that doesn't end the loop. Candidate bounds come from the condition's comparisons (joined by `&&` or `||`), then from the comparisons the body tests (`if` and inner `while` conditions over variables known before the loop), up to 4. Each is tried alone, then in pairs compared lexicographically, which finds loops where one quantity decreases while the ones before it stay the same. Examples: `(n - i, m - j)` for `while i < n { if j < m { j += 1 } else { i += 1; j = 0 } }`, and `(len(a) - i, len(b) - j)` for a merge's `while i < len(a) && j < len(b)`.
- A `while` or `loop` with a `decreases` clause whose measure the solver proves decreases on every pass (below).
- Recursion where some argument is a smaller piece of a parameter, taken apart by a `match` (`[_, ..rest] => 1 + length(rest)`).
- Recursion with a measure: an integer expression of the parameters that the solver proves is non-negative at each recursive call and smaller for the call's arguments. These candidates are tried automatically:
  - each integer parameter (`fact(n - 1)`, `gcd(b, a % b)`)
  - the length of each list or string parameter (`total(drop(xs, 1))`)
  - for each comparison in the body, the distance between its sides (`100 - n` in `up(n + 1)` under `n <= 100`, `hi - lo` in a binary search)
  - pairs of these, compared lexicographically (`(m, n)` for Ackermann's function)

  A function with a `decreases` clause has that measure as its only candidate.

In every case, everything the function calls must terminate too.

**may not terminate**, with the reason: a `loop` without a `decreases` clause, other recursion, mutual recursion, a `decreases` measure not shown to decrease, or a call to such a function.

**assumed**: the function is marked `terminates`, after its contract: `fn f(x: Int) -> Int terminates { … }`. Its callers terminate only if it does.

**`decreases` clauses.** When no candidate works, the measure can be written: `decreases e` on a `while` or `loop`, on its own line between the header and the body (like `invariant`), or on a recursive function, next to `requires` and `ensures`.

- `e` is an `Int` expression or a tuple of them compared lexicographically. With `decreases (left, len(book.orders))`, each pass either lowers `left`, or keeps it and lowers the other.
- The deciding component must be at least 0 before the pass or call and lower after it, and the components before it must satisfy the same.
- A loop or function has at most one clause. A `for` loop takes none, since it always ends.
- It is a check like any other. `verify` proves it at the end of each pass (`decreases `(a, b)` after a pass of `while …``), including at each `continue` that doesn't exit, and at each recursive call (`decreases `(m, n)` at a call of `ack``), using the parameters' entry values and the call's arguments. The termination verdict uses it as the only candidate (`terminates: `while …` ends: its `decreases (a, b)` measure goes down on every pass`).
- When not proven, it is checked at run time, like `invariant`. A loop's measure is saved at the start of a pass and compared at the next (`decreases failed: `k` did not go down in a pass of `while i > 0` (before: 3, after: 3)`). A recursive function's measure is computed on entry and compared with the arguments' measure at each recursive call. The code generator's functions are `decreases_start`, `decreases_pass`, `decreases_entry` and `decreases_call` in `compiler/codegen`. A solver counterexample is therefore run and confirmed, and a loop with a wrong measure stops with a fault instead of running forever. The verdict then says `may not terminate: … its `decreases …` was not shown to go down on every pass (it is checked when the program runs)`.
- In the tree, a loop's clause is a `Decreases` statement among the first statements of its body, with its `invariant`s. A function's clause is the first statement of its body. Both are printed where they were written.

`verify NAME` analyses termination only for what the selected functions reach, since a function's verdict depends only on what it calls. Checking one function therefore doesn't pay for every loop in the program and std.

## Policies

A package's `strata.toml` states what `verify` requires. Each rule is a list of modules: `"*"` for all of them, `"main"` for the root module, and a module includes the modules below it.

```toml
[verify]
proven = ["core"]        # every check proven, not just fuzzed
terminates = ["*"]       # every function shown (or marked) to terminate
no_recursion = ["core"]  # no recursion at all, even recursion that terminates
```

- Recursion is allowed unless `no_recursion` forbids it.
- A run that breaks a rule fails and lists each violation.
- `proven` accepts assumed code, and the run lists everything it accepted that way.
- `verify --require proven,terminates,no-recursion` applies the same rules to everything checked, in a package or not.
- Loading the package fails if a rule names a module it doesn't have.

## Testing what the solver can't prove

**Cross-check.** A proven function that terminates is still run on a few generated inputs: `--cross-check N`, 20 by default, 0 to disable. If one fails, the prover was wrong, and `verify` reports a failure *found by the cross-check*, flagged as a prover bug. Each run has a step budget. A run that exceeds it, or overflows the stack, proves nothing either way and is set aside, since proven code can still be far too costly to run (Ackermann's function, for example).

**Inputs built by the program.** A strong `requires` rejects most random inputs: of 800 random order books, 12 met `requires book_ok(b)`. Under a `requires`, an argument whose type is one of the program's records or sums is therefore usually built the way the program builds one.

- In three cases out of four, the fuzz driver calls a builder: one of the program's functions that returns the type without taking one (`empty_book()`). It then calls a stepper up to 10 times, or not at all. A stepper takes the value `mut` (`new_order(mut b, c)`) or takes it and returns a new one (`insert(l, p, o) -> Ladder`). Arguments are generated. The fourth case is generated randomly.
- A step whose `requires` doesn't hold on its arguments is skipped.
- When some builders or steppers have an `ensures`, only those are used, since a function that merely hands back a value from a random input rarely preserves an invariant.
- Never used: the function being fuzzed, generic functions, `extern c` functions, std's and core's functions, and functions with a parameter for which no value can be generated (a capability).
- The built value is still checked against the `requires` when the function is called, so a builder or stepper that breaks the invariant only costs a discarded case.
- A fault while building (a builder's own `requires` unmet, a bug in a stepper) happens in a separate frame (`strata_fuzz_build`) and discards the case. The stepper's own verdict reports its bugs.
- The count stays accurate: "200 cases, 69 discarded by `requires`" includes both kinds of discard.
- On the order book, the engine's handlers went from 12 cases (788 discarded) to 200 (69 discarded, nearly all random ones), and the ladder's from 200 cases (278 discarded) to 200 (36 discarded).
- If the driver can't be compiled with the builders (for example, a stepper that doesn't compile), it is compiled without them.

**Inputs from the solver.** Random values rarely satisfy some types, such as a list of consecutive integers, or a record whose indices must stay within its own list. When the fuzzer can't generate inputs ("could not generate a value", or every input broke the `requires`), the solver generates them. It looks for values that satisfy the parameters' types and the `requires`, with lists of at most 6 elements. Up to 20 of them, each different from the previous ones, are run as the fuzzer's cases. A failure is reported as *found by the fuzzer, on inputs from the solver*:

```text
✗ last                     Ensures violation (found by the fuzzer, on inputs from the solver)
    input:   [-1, 0, 1, 2]
```

**Order of work.** The solver runs first and the fuzzer second, so every result is the same from run to run. The cross-check can therefore detect a prover bug.

## Caching

**Verdicts.** Each check's verdict is kept between runs in the cache (`~/.cache/strata/verify-strata/checks/`, moved by `STRATA_CACHE`), one file per check. Each file is written whole and renamed into place, so concurrent runs can share them safely. The key includes:

- the check's name
- the closure hash of its definition, covering everything it depends on, read from the module files' headers
- the number of cases and of cross-checked cases, and the seed
- the solver's name and version
- std's, core's and the prelude's hashes
- the `strata` binary (its SHA-256), so a rebuilt compiler checks again

A changed definition is checked again, along with what depends on it, and nothing else is. A cached verdict is printed like a fresh one, followed by `(cached)`, and `--require` and `[verify]` apply to it the same way. `--no-cache` checks everything again and stores the new verdicts.

A verdict where the solver gave up is cached, because the solver's resource limit is deterministic and it would give up again. Each query asks the solver why it gave up. A verdict where the solver timed out (the 20-second backstop, which a busy machine may hit), gave no answer, or gave no reason is not cached.

**Proofs.** A proof is also stored by what it relies on (`verify-strata/proofs/`): the check's own code, the interface (signature and contract) of each callee seen only through its contract, the code of each callee seen through, and everything else it reaches through code. The number of cases is not part of the key. When a callee's body changes but its contract doesn't, its callers' proofs are kept and only their cross-checks run again, since the cross-check runs the callees. `--proofs FILE` stores the record elsewhere. `build` verifies into a record of its own, started empty, and keeps it as the program's record afterwards: what it leaves out is what verifying these sources proves, never what earlier runs (a selection, other settings) left in the record. A definition not verified in a run keeps its record while its code is unchanged. `compiler proofs FILE` prints the current proofs, in the form `codegen --proofs` reads, including the entry checks every caller proved (`entry F argument P`, `entry F requires I`). Those are the checks that `build` and `run` leave out.

**Loop findings.** Termination's findings for each function's loops are stored the same way (`verify-strata/loops/`). That includes std's own functions (a loop's bound, a recursion's measure), keyed by the code involved. Functions generated from std for a program (`std.json.encode_as.for.Signup.4`) belong to the program and are analysed every time.

**Status.** Each definition's status (the worst verdict among its checks) is stored in `verify-strata/status/` by closure hash, for viewers. `compiler verdicts FILE` lists them. `strata diff` reads each definition's latest verdict from `STRATA_VERDICTS` if set, otherwise from `<git common dir>/strata/verdicts` (shared by every worktree), otherwise from the cache.

Fuzz drivers are compiled only for definitions with a check to run.

## The verifier

`strata verify` runs the verifier in `compiler/main` (`compiler verify`) on a script (`--file`) or on the package around the current folder.

- The program is checked first, as `strata check` checks it. If it has errors, they are printed and nothing is verified.
- A selection (`verify NAME`, `prefix*`, `module:m`, `slice:NAME`, a module's name, or a method's: `memory.MemQueue.claim` checks only that method of its implementation) chooses which definitions are checked, and the package's `[verify]` rules apply.
- It checks functions, a type's inherent methods, implementations' methods (through wrappers that call them), laws, and the copies a `derive` generates.
- Proofs come from `compiler/smt`, and fuzz drivers from `compiler/codegen`, run in the compiler's own JIT.
- `build` verifies first (`compiler verify --quiet`), printing one line on stderr about the checks not verified before.
- A call to an `extern c` function is assumed through its contract.
- It loads only the std definitions the program reaches, and builds and loads a package's `[native]` C for the code it runs.

**Parallelism.** Functions are checked on all cores, with at most one solver process per core. A function's obligations are solved in one solver script, each between `push` and `pop`.

- Z3 stays running, one process per core for the whole run, and each script is sent after `(reset)` with its limits set again. Starting Z3 takes longer than most queries, so it is paid once per core. A script already answered in the run isn't sent again.
- A solver that is a script, or that doesn't answer a command as it reads it, gets one process per script instead. `STRATA_SOLVER_POOL=0` forces this.
- `STRATA_SOLVER_LOG=FILE` writes one line per script: its time, flags, size, hash and first answer.
- Termination's loop bounds and recursion measures are found the same way.
- Each query bounds the string lengths it uses below 2^64. Without that bound, Z3 tries to build a string of 10^308 characters to refute `len(s) < 10^308`.

**Loop-invariant searches.** `verify` and `build` encode with `Methods.search` 0: each loop gets its derived invariants (`smt.loop_facts`) and its written ones, and a loop whose search is left out is noted (`Vc.skipped`, `Vc.skipped_loops`: the checks made before it and up to the end of its body, its head and the variables it changes). A check not proven in or after such a loop gets a hint naming the loop and the variable, and `strata fix NAME` (`main.loop_hint`); termination's "no bound was shown to go down" gets one too (`termination.analyze_with`). Verdicts, proofs and termination findings are cached under keys that say so (`loops derived`).

The search runs in `verify --find-invariants BY` (`main.find_invariants`), which `strata edit` (the changed functions and implementations with loops, `main.loop_functions`) and `strata fix` (`main.fix_loops`) run through `main.search_invariants`, from Strata's home, with no check of the program, no fuzzing and no report. The chosen functions with loops are encoded with search 0, then a function with a check failing after a skipped loop is encoded again with the candidates naming the failing checks' words (`search` 1), then with all of them (`search` 2) (`main.solved_together`); termination encodes a function whose loop isn't shown to end again with the search (`Analysis.found_loops`). Each loop's inference (`smt.while_loop`) records what it found and what its search cost (`LoopFound`, `searched`): Z3's resource units, from `(get-info :rlimit)` after each search query, and the time.

- Every invariant a search found (`found_by_search`) is written into the program as `invariant` clauses (`main.write_found`, `main.with_found`; a function's loops and an implementation's methods'). This is one edit: the changed module files are re-encoded, and each changed definition is written, parsed and resolved again, and kept only if it produces the same tree. The exit code says whether anything was written (0, else 3).
- A value before the loop (`$entry_x`) is named in code by `main.entry_forms`. It is the expression `x` was set to just before the loop, when that expression uses only variables that neither the loop nor the statements since change (`value_before`). Otherwise it is a binding `start_x := x` added just before the loop.
- When the package was opened from its folder, the edit is logged (`"by":"edit"` or `"by":"fix"`) and committed (`--log`).
- Every search query is a job flagged `budget` in `c/solver.c`. The run's budget (default 2,000M units, `search_budget`; `solver_budget`) is charged the units each script actually used; an answer from the run's memo costs nothing. Once the budget is used up, a search is answered `budget` without running, and the loops left are reported (`search_notes`).
- A loop with written invariants asks its inference questions as `probe`s, 0.5M units each, stopping at the first unanswered one.

## Solvers

`z3` is found on the `PATH`. `STRATA_SOLVER` names another solver binary, for example `STRATA_SOLVER=cvc5`. A solver whose `--version` names cvc5 is run with cvc5's flags, and any other is run like Z3. Both get the same SMT-LIB queries. Without a solver, everything is fuzzed.

Each query has a deterministic resource limit instead of a timeout (`rlimit` for Z3, `--rlimit-per` for cvc5), with a 20-second time limit only as a backstop, so results can be cached by content hash. A proof gets 2M Z3 units, a tenth of what a model search gets (inferring loop invariants, generating inputs). Every proof in std, the examples and the compiler takes under 0.5M. A query that exceeds the limit is reported as "the solver gave up", and the fuzzer looks for a counterexample. The solver's version is part of the cache key.

The test suite passes with either solver. cvc5 has no Homebrew formula; install it with `brew tap cvc5/cvc5 && brew install cvc5`, or from its releases.
