# Bugs to fix

Bugs to fix in Strata, newest review 2026-11-27 at v0.5.0 (origin/main 5f039e07), found by reading the code; remove an entry in the PR that fixes it.

## Verification (soundness)

- [ ] **Header `code_hash`, `interface_hash`, `deps` and `name` are never checked, but verify keys its cache on them.** A module header's `code_hash`, `interface_hash`, `deps` and `name` are never checked against the content, and hashes are trusted by default, but verify keys its cached verdicts on them.
  Where: binary.entry_with (compiler.binary.st:2243), binary.read_header_of; main.keys_over/closure_keys (compiler.main.st:7763–7810); binary.read, decode_module, decode_entry (`checked: false`). Source: review front-end. Confidence: likely.
- [ ] **Function values have no identity: calls with different function arguments are equated.** A function-typed parameter is `VFn { ret }` (`shape`: `FnType { ret, .. } => VFn { ret }`). It carries nothing else. In `call_key`, an argument `Var(n)` bound to a local is keyed by `debug(v)` (`is_fn` is false for a bound name).
  Where: smt.call_key (rendered .st ~9252), smt.same_call, smt.call (SeeThrough branch, memo reuse of lists), Val.VFn. Source: review prover. Confidence: likely.
- [ ] **Function-typed parameters: results assumed valid, lambda arguments never checked.** `call_value` on a `VFn { ret }` makes `res := shape(ret)` and then `assume(check_type(ret, res, []))`. The callee therefore assumes a refined result (`(Int) -> Nat`).
  Where: smt.call_value, smt.check_type_at (`UnitType | FnType { .. } => "true"`), smt.refined_by (`UnitType | FnType { .. } => false`), smt.call_contract. Source: review prover. Confidence: likely.
- [ ] **An unfolded recursive definition is only as sound as termination's verdict.** For a function in `Methods.unfold`, `define_fn` declares `fn:KEY` and defines `def:KEY` as the body with recursive calls as `fn:KEY` applications. `rec_use` then asserts `(= (fn:KEY args) (def:KEY args))` at each use, two levels deep.
  Where: smt.rec_use, smt.define_fn (rec branch), smt.rec_call, smt.call (`contains(vc.methods.unfold, name)`). Source: review prover. Confidence: likely.
- [ ] **Scalar merges are not shared: term text doubles at each conditional update.** `share` is applied to list lengths, arrays, tags and datatype terms (`merge` around 6289-6334, `share_deep`), but not to scalars. After `if c1 { x = x + 1 }`, x is `(ite c1 (+ x0 1) x0)`, with x0 twice.
  Where: smt.merge (`(VInt(x), VInt(y)) => VInt(ite(c, x, y))`, same for Bool and String), smt.branches_narrowed, smt.assign. Source: review prover. Confidence: likely.
- [ ] **A long-running `verify_file` once reported a refuted edit as proven.** Repeated calls in one process returned "proven" for an edit with a counterexample. Cause not found; `run --live` uses a process per edit to avoid it.
  Where: verify_file. Source: ledger V2. Confidence: possible.
- [ ] **Fuzz harness crashes on a function taking `mut Book` and returning a value.** SIGSEGV in the harness.
  Where: fuzzer. Source: ledger V23 (STRATA-ISSUES 0.3.4). Confidence: likely.
- [ ] **A cached verdict can go stale when a callee's body changes a caller's errors.** A function's key holds its callees' interfaces only (`"interface " + e.interface` for any `fn` not in `run`). Some of a caller's errors depend on a callee's body.
  Where: compiler.checked.keys_of; main.loaded_check; check.type_errors_with (`on_type`, `calls_on_type`). Source: review checker. Confidence: likely.
- [ ] **`ensures it == e` replaces the body as the function's definition.** `body := match defining_ensures(vc, f) { Some(e) => eval(mut vc, e), None => eval(mut vc, f.body) }`. Contracts see through the function as `e`, whether or not that ensures is proven.
  Where: smt.defining_ensures, smt.define_fn. Source: review prover. Confidence: sure.
- [ ] **Strings with code points past U+2FFFF are modelled wrongly.** Strings and regex classes are modelled only up to U+2FFFF, while the model calls itself exact, so code points above it are modelled wrongly.
  Where: smt.builtin (`chr`: `(=> (<= x 196607) (= c (str.from_code x)))`, `ord`: `str.to_code`), smt.max_char, std.regex.smt_ranges/max_smt_char; std.regex.smt_ranges, std.regex.max_smt_char; std.regex.smt_ranges, to_smt, smt_model. Source: review prover; review std. Confidence: likely.
- [ ] **Structural bounds modelled as closed records.** For `[T: { id: Int, .. }]`, `T` is inserted into `vc.types` as a record with only those fields, with the claim "`f` can only read them (it makes no value of `T` and compares none)".
  Where: smt.rows_as_records. Source: review prover. Confidence: possible.
- [ ] **`ForPat` arms are taken as never matching.** An unexpanded `ForPat` arm gets the condition `false`. Its body runs on a dead path, so every check in it is proven vacuously, and the "some arm matches" obligation leaves it out.
  Where: smt.pattern (`ForPat { .. } => "false"`), main.verified_decls (`if derives { expand_in(...) } else { sweet0 }`). Source: review prover. Confidence: possible.
- [ ] **Unresolved method calls bound by a unique name suffix over all functions.** When `method_call` finds nothing, any single function anywhere in core, std and the program whose qualified name ends with `.name` is called with the receiver as an argument.
  Where: smt.call (fallback `keys(vc.fns).filter(k -> ends_with(k, "." + name))`). Source: review prover. Confidence: possible.
- [ ] **Loop havoc may miss a variable changed through `mut`.** Only `MutExpr(MethodCall { recv })` and the arguments `passed_mut` finds from a declaration count as changes. `method_params` takes the first trait declaring a method of that name (`[d, ..] => Some(d.params)`).
  Where: smt.changed_vars, smt.passed_mut, smt.method_params. Source: review prover. Confidence: possible.
- [ ] **Results that silently answer "true" become proven checks that codegen removes.** A goal of `"true"` is recorded as "proven … without a query" (`vc.given`) and listed as a proof. In `check_type_at`, a type name missing from `vc.types` (a key mismatch: qualified vs bare names, an alias) gives `"true"`, and so does a `RowType`.
  Where: smt.check_type_at (`RowType(_) => "true"`, `_ => match get(vc.types, name) { None => "true", ...}`), smt.oblige (`g == "true"` goes to `vc.given`), main.proof_lines, codegen.proven. Source: review prover. Confidence: possible.
- [ ] **Two transparency analyses with different rules.** `methods_of` sets `Methods.seen` from `smt.transparency`. Termination's encodings run with that. `verify` then replaces it with termination's `seen_through`. The two `cannot_fail` differ. smt's treats every trait method call as `ContractOnly`.
  Where: smt.transparency + smt.cannot_fail + smt.callee_seen vs termination.cannot_fail + termination.seen_through. Source: review prover. Confidence: sure.
- [ ] **Recursive calls in `spawn`/`scope`/`race`/`await`/`select`/`comptime` are not looked at.** The arm that recurses lists `Interp | IfExpr | Binary | Unary | ListExpr | TupleExpr | CtorExpr | FieldOf | IsExpr | Return | Try | MutExpr | Fallback`.
  Where: termination.struct_expr (the last arm, `_ => ()`). Source: review prover. Confidence: sure.
- [ ] **`self.field.m()` assumed smaller in an implementation of `m`.** The edge is dropped, not checked. The call dispatches to the field type's implementation, which can rebuild a value of the original type around its own `self` and call `m` again.
  Where: termination.calls_in (`smaller := st.own != "" && trait_node(name) == st.own && self_field(recv)`). Source: review prover. Confidence: likely.
- [ ] **Termination proofs assume the function's own `ensures`, which may be unproven.** Measure and loop-bound obligations are asked over the function's facts. Those include the callee contracts assumed at each call, the function's own `ensures` at its recursive calls among them.
  Where: smt.call_contract (a recursive call assumes `f.post`), smt.measure_check / termination.loop_answered (obligations over `vc.facts`). Source: review prover. Confidence: likely.
- [ ] **A `for` over an adapter of a value whose type termination can't read stays unknown.** `termination.expr_type` reads types from bindings in scope, calls of the program's functions, builtins whose overload the arguments' types settle, fields, tuples, lists, and `match`/`if` values whose branches agree. It reads no type from a block with statements, a lambda parameter without a written type, a builtin result whose overload or type arguments aren't settled (`get(d, k)` with `d` a `Dict` literal), or a `match` with a branch it can't type. 28 compiler/ functions are unknown this way (`codegen.builtin_call`'s `drop(args, 1)`, `smt.shape`'s `rc.fields ?? []`), 20 compiler/ and 2 std for a `for` over a value the encoder doesn't model (`check.union_conflicts`, `std.orm.Query.all`).
  Where: termination.expr_type, termination.source_of. Source: termination-modelling PR. Confidence: sure.
- [ ] **A method call on a receiver of unknown type reaches every type's own method of that name.** Sound but imprecise: `d.members.filter(..).map(..)` in std.json.tags_of reached `std.array.Array.map`; it is proven now only because `Array.map` is. The checker's resolution (`check.mark_method_calls`, #626) tags a call only as a trait's or "other", not with the receiver's type, so it can't narrow these.
  Where: termination.calls_in, termination.recv_head. Source: termination-modelling PR. Confidence: sure.
- [ ] **Mutual recursion always unknown; tree recursion not proven to terminate (known).** Recorded in ~/dev/strata-bench/STRATA-ISSUES.md ("Tree recursion isn't proven to terminate. 52 and 68 functions respectively are reported 'may not terminate'"). The code has no measure search for components larger than one.
  Where: termination.analyze_with (`Some(other) => Err("it is recursive through ...")`). Source: review prover; ledger V7. Confidence: sure.
- [ ] **A cached proof skips the lemma termination gate.** The proof key (`interface_keys`) holds callees' interface hashes for ContractOnly callees, not their termination status. A lemma calling `g` stays keyed the same when `g`'s body changes and `g` stops terminating.
  Where: main.verify_program (`Some(p) => Some(lawful(cross_checked(p, cc, t.once, fz)))` for `proved`; the gate `(Some(f), Some(Unknown(why))) => if f.lemma && x0 is VProven { VIncomplete(...) }` only in the `check_fn` branch). Source: review prover. Confidence: likely.
- [ ] **Checks matched to assumed clauses by text prefix and substring.** For any assumed clause with text `t`, all four forms (`ensures t`, `requires t`, ``invariant `t` ``, `invariant failed: t`) are tried against the check. Matches are `starts_with(line, k + " ")`, `starts_with(line, k + ":")` or `contains(line, k + " (")`.
  Where: main.assumed_note, main.assumed_verdict, main.rules_broken (`VAssumed(_) => { accepted = true }`). Source: review prover. Confidence: likely.
- [ ] **Stack overflow with a quantified refinement on a list element type.** STRATA-ISSUES.md #4b: "The verifier overflows its stack in `smt.element_at → instance → subst_val` when a list element type's refinement contains `all(...)`". Not re-run, and no PR claims a fix.
  Where: smt.element_at → smt.instance → smt.subst_val. Source: review prover. Confidence: sure.
- [ ] **Counterexample replay crashes.** STRATA-ISSUES.md #4c: "crash (signal 11) in: strata_verify_replay_0" on nested, mutually recursive types with `Natural`-refined fields. `build` fails too.
  Where: main.replay (strata_verify_replay_0). Source: review prover. Confidence: sure.
- [ ] **`build` and `verify` disagree on what is proven.** STRATA-ISSUES.md (ob-verify): "`strata build` lists 35 clauses as 'not proven' that `strata verify` reports as proven".
  Where: verify paths (see "Three different ways to turn solver output into verdicts"). Source: review prover; ledger V22. Confidence: sure.
- [ ] **Refined fields of recursive types make the solver reject checks.** STRATA-ISSUES.md (sql arms): "the solver itself reported this as its own bug". Not reproduced 2026-11-28 with `type Expr = | Lit(Int where it >= 0) | Add { l: Expr, r: Expr, n: Int where it > 0 }` (proven); the sql arms' own type is still to try.
  Where: smt sorts / datatype declarations. Source: review prover; ledger V8, V9. Confidence: sure.
- [ ] **A literal in an arm that can't be taken fails a `where it` result refinement at compile time.** `fn h(v: Int) -> Bool where it { match Some(v) { Some(s) => s == v, None => false } }` is rejected: "refinement violated at compile time in the value `h` returns: false does not satisfy `Bool where it`". The `None => false` arm is never taken.
  Where: check (compile-time refinement of a function's result). Source: smt-datatypes. Confidence: sure (reproduced).
- [ ] **smt finds core.real by function name.** `real_record` takes a function for one of core.real's when its last name part starts with `real_` or it holds `Real.`, so a program's own `real_…` function reads `Real` as a record (REBUILD rule 12).
  Where: smt.real_record, smt.builtin_key. Source: smt-datatypes. Confidence: sure (by reading).
- [ ] **`core.rt.hash_slot` divides by a table size it doesn't require to be positive.** `hash_slot(c, 0)` divides by zero; its callers (`large_find`, `large_put`, `large_grow`) read the size from memory (`load_i64`), so a `requires m > 0` would be a check in the allocator that no proof can remove.
  Where: core.rt.hash_slot. Source: smt-datatypes (refuted obligations of #631). Confidence: sure.
- [ ] **`Int`'s `Div` and `Rem` impls divide by zero.** `Int.div(0, 0)` and `Int.rem(0, 0)` fault; the traits have no precondition to say the divisor isn't zero, and a trait-wide `requires` would be wrong for `Float`.
  Where: core.ops (impl Div for Int, impl Rem for Int). Source: smt-datatypes (refuted obligations of #631). Confidence: sure.
- [ ] **A selector of several core names finds only the first.** `strata --file core verify 'core.gen.shift_out,core.float.two_to'` says "`core.float.two_to` names no definition and no module"; each name alone is found.
  Where: the verify selector (main). Source: smt-datatypes. Confidence: sure (reproduced).
- [ ] **`verify` loses the value under `x!` for capability or trait calls in std.** When `x` is a call on a capability or a trait inside std, the facts about `x!` are lost.
  Where: verify. Source: ledger V3 (#574). Confidence: likely.
- [ ] **`verify` of a nine-arm type match over `std.agent.Chunk` goes past 8 GB.** Memory explodes in seconds.
  Where: verify. Source: ledger V4 (#574). Confidence: likely.
- [ ] **LLVM pass-registry errors loop during parallel fuzzing.** `verify` of `codegen.computed_value` prints "Required pass not found" without end; 84 functions at once printed "Pass 'Function Alias Analysis Results' is not initialized" in a loop (84 MB log). `jit_fuzz_load` initializes the native target per load.
  Where: compiler/c/jit.c jit_fuzz_load. Source: ledger V5 (#532, #596). Confidence: likely.
- [ ] **`len(s) == 0` doesn't make a set empty.** Set size is not tied to members, which gave a false counterexample.
  Where: smt (sets). Source: ledger V10. Confidence: likely.
- [ ] **Callees in other modules are not analysed for termination in a by-name run.** A verify run naming some functions sees their callees in other modules only through their contracts.
  Where: verify (by name). Source: ledger V12. Confidence: possible.
- [ ] **`verify` can't run functions reading thread-local storage.** The JIT sets up no TLS (`_tlv_bootstrap_error`); `arena_put` on main stops this way.
  Where: JIT fuzzing. Source: ledger V24. Confidence: likely.
- [ ] **The std termination cache keeps old findings after an edit.** The cache is keyed by name and binary, so std edited without a rebuild keeps stale findings for the changed function.
  Where: termination cache. Source: ledger V31. Confidence: likely.
- [ ] **Float fields compared as reflexive datatype terms.** A record with a `Float` field is a datatype term over an uninterpreted `Float` sort. `p == p` is `"true"` and `(= p p)` holds. If run-time equality compares floats IEEE-wise, a NaN field makes `p == p` false.
  Where: smt.sort_type (`"Float" | "_" => Some(opaque_sort(...))`), smt.eq (`VData` with `x == y` gives `"true"`, else `(= x y)`). Source: review prover. Confidence: possible.
- [ ] **Trait invariants looked up by short name.** `vc.methods.traits.filter(x -> x.name == name || x.name == last)`, then the first match. For a parameter typed `b.Db`, another module's `a.Db` invariants can be assumed of it.
  Where: smt.trait_invariants. Source: review prover. Confidence: possible.
- [ ] **Argument refinements checked with no bindings.** A parameter refinement that names an earlier parameter (`i: Int where it < n`) is evaluated with `n` unbound. It can't be proven at any call. The result check passes `binds`, but this one doesn't.
  Where: smt.call_contract (`check_type(mut vc, subst(p.ty, sub), a, [])`). Source: review prover. Confidence: likely.
- [ ] **Scratch files and job dedup keyed by a 64-bit polynomial hash.** `h = h * 131 + byte`, wrapping at 64 bits. Two different scripts with the same hash share one `.smt2` path and one `.out` file, and `run_scripts` runs only the first.
  Where: smt.text_hash, smt.run_script, smt.run_scripts (non-pooled path, `seen` by `text_hash(flags + script)`). Source: review prover. Confidence: sure.
- [ ] **`cannot_fail` accepts calls of non-`Var` callees and doesn't look at `let` annotations.** `Call { callee: FieldOf … }` (a stored function) passes as "cannot fail". A `let x: Nat = e` (a run-time check when not proven) isn't considered. Such functions become transparent and are seen through as total.
  Where: termination.cannot_fail (`_ => true`), smt.cannot_fail (`_ => SeeThrough`). Source: review prover. Confidence: possible.
- [ ] **The lemma gate only fires for `Unknown`.** `Assumed(names)` (a callee marked `terminates`) and `None` (not analysed) pass. An induction then rests on a `terminates` mark, and the verdict says only "proven".
  Where: main.verify_program (lemma gate). Source: review prover. Confidence: sure.
- [ ] **Loops stepping an `I64` counter are reported "may not terminate" unless the counter's lower bound is shown.** The bound `inner - j` of an `I64` counter wraps when `j` may be negative, so it is shown to go down only with `j >= 0` in the condition or an invariant (`std.array.gather` needed `invariant j >= 0`). Nothing derives that bound from the counter starting at 0 and only going up.
  Where: termination. Source: ledger V6. Confidence: likely.
- [ ] **A function returning `Set[TaskId]` isn't seen through.** `list_set` doesn't restate the elements' refinements.
  Where: smt. Source: ledger V11. Confidence: likely.
- [ ] **Refined records pushed to a list in a loop are not proven element by element.** Fixed by #612 only for types holding a `Real`.
  Where: smt. Source: ledger V13 (#612). Confidence: likely.
- [ ] **`old` inside a trait `ensures` lambda makes the method "skipped".** The method is not verified.
  Where: verify. Source: ledger V14 (#529). Confidence: likely.
- [ ] **`verify` reports an insert naming a missing row as "not proven" instead of refuted.** `sqlite.memory()` doesn't carry its empty model through `create`; a `delete` of a referenced row is refused only at run time.
  Where: std.sqlite models. Source: ledger V27. Confidence: likely.
- [ ] **A refinement evaluated where a value is made makes no obligation for the checks inside it, unless its type is in a signature.** The checks inside a refinement nested in a parameter's, result's or `let` annotation's type are obligations (at any depth: elements, fields, payloads, union members, function types). A type met only where a value is built in a body (`Box { h: xs }` with `Box` in no signature) has its refinement checked there, and the checks inside it are neither proven nor reported.
  Where: smt.own_parts (called from smt.function and smt.annotated), smt.check_type_at (refinements evaluated through `pred`). Source: nested-refinements PR. Confidence: sure.
- [ ] **A contract's function value passed to `find` leaves its checks unknown.** In a function's own contract, the checks of a function value passed to `map`, `filter`, `count`, `fold` or a function of the program are obligations for any element or argument. `find` applies its function itself, at a nested level, so it reports one unknown obligation, "the checks inside `…`". `fold`'s accumulator is an unknown value, so checks on it are unknown.
  Where: smt.builtin (`find`), smt.call_exprs (the `find` exception), smt.fn_arg_run (`fold`). Source: nested-refinements PR. Confidence: sure.
- [ ] **Two sites with the same text can't be told apart, so a check can be left out where it wasn't proven.** A site is a hash of the expression's printed text (`text.check_site`). When the prover evaluates one of two same-text sites twice and skips the other, codegen counts two proofs at that site and leaves both checks out. The occurrence count only guards against a site skipped entirely. Node ids would close it.
  Where: text.check_site, codegen.proven, smt (`Vc.here`, `given_sites`). Source: PR #633. Confidence: likely.
- [ ] **A bound trait's methods in another module are not modelled.** A call of one is "`get` is not modelled", so an impl's or method's bound can't change a verdict the prover reaches.
  Where: smt (methods of a bound's trait). Source: PR #629. Confidence: likely.
- [ ] **Fuzzing crashes when a program method is named like a built-in.** A `--file` program with `type A = { x: Int }`, `impl A { fn max(self) -> Int }` and an unproven `fn t(k: Float) -> Float ensures it >= 1.0 = max(k, 1.0)` crashes in `jit_fuzz_case` (signal 10/11).
  Where: fuzzer (`jit_fuzz_case`). Source: PR #670. Confidence: sure (on its base too).

## Type checker

- [ ] **A concrete value isn't taken as a trait value in `+` and `==`.** With `xs: List[Out]`, `xs + [Tally { n: 7 }]` gives "`+` operands: expected `List[Out]`, found `List[Tally]`"; with `one: Shape`, `one == Square { side: 1.0 }` gives "`==` operands: expected `Shape`, found `Square`". A `let` with the trait type takes both. Codegen builds the original forms, but `one == Square { side: 1.0 }` gave `false` (the golden recorded it), where `one == same` with `let same: Shape = Square { side: 1.0 }` gives `true`. `codegen-constructs` (trait_places) and `codegen-programs-4` (methods_and_traits) go through a `let` until this is fixed.
  Where: compiler.check, binary operators' operand types (no wanted type from the other operand). Source: build-fails-on-errors PR. Confidence: sure.
- [ ] **Integer patterns at or past 2^62 are called unreachable.** `check.int_max()` is `4611686018427387903` and `int_min()` its negation: the ranges `check_coverage` works with end there, so `match n { 4611686018427387904 => …, _ => … }` gives "unreachable match arm `4611686018427387904`: the arms above already cover it", though `Int` has no bound. `build-bigint` builds with `--ignore-check-errors` until this is fixed.
  Where: compiler.check.int_max/int_min, ints_not, cover_of (`ast.IntPat(n) => CInts([(n, n)])`), check_coverage; codegen has its own pair. Source: build-fails-on-errors PR. Confidence: sure.
- [ ] **In std.array, a decimal literal beside `max` or `min` called as a function is a `Real`.** `max(k, 1.0)` and `min(k, 1.0)` with `k: Float`, in a function of std.array, give the literal the type `Real`, so smt reads it as an unknown value ("a value of an unexpected shape"). The same function in std.time, or in a `--file` program with types that have methods `max` and `min`, types it as a `Float`; `k.max(1.0)` is a `Float` in std.array too. std.array's three array types have methods `max` and `min`, and `Array.scale` keeps `float(1)` for this reason.
  Where: not found; to look at: check.infer_call (the built-in branch), check.dec_unmarked (a literal with no recorded type becomes a `Real`). Source: nested-refinements PR. Confidence: sure of the behaviour, not of the place.
- [ ] **Trait bounds are checked only for a few traits.** `if t.derivable is None && !t.capability && len(t.params) == 0 { checked.insert }`, and blanket-implemented traits are then removed. `infer_call` checks bounds only `if contains(i.methods.checked, b.1)`.
  Where: compiler.check.methods_of (`checked`, ~1989), infer_call (5732), own_trait_of. Source: review checker. Confidence: sure.
- [ ] **The type tables are rebuilt by nine entry points, and they disagree.** `type_errors_with` builds `types_of(code + all)` and `with_code(Methods { ..methods_of(all), local, own_traits, noallocs }, methods_of(code))`.
  Where: compiler.check.type_errors_with, instantiated, typed, lambdas_typed, holes_of, mark_field_reads, own_methods_resolved, spreads_resolved, field_typing. Source: review checker. Confidence: sure.
- [ ] **Matches on a local variable or a field of sum type are never checked.** `coverage_checked` excludes every `TNamed` except `Tuple`, saying "`exhaustive` checks the others". `exhaustive.scrutinee_type` knows only a parameter never rebound (`Var(x) => if contains(unsafe, x) … else get(params, x)`), a call returning `Bool`, and comparisons.
  Where: compiler.check.coverage_checked (7961), check.infer_match, exhaustive.scrutinee_type (501). Source: review checker. Confidence: sure.
- [ ] **A sum match with any nested pattern is not checked.** `pattern_variants` gives `None` for `Circle(0)` or `Some(Ok(x))`, which sets `partial`, and then `if partial || undecided { return }`. `exhaustive` says it skips "constructors partly covered by nested patterns".
  Where: compiler.check.variants_covered (`partial = true`), pattern_variants; exhaustive.covered/understood. Source: review checker. Confidence: sure.
- [ ] **An impl's method signatures are never compared with its trait's.** `declarations` checks only that the names match (`missing method`, `is not a method of trait`). `check` checks each method's body against its own written signature.
  Where: compiler.declarations.declaration_errors_in (ImplDef arm); compiler.check.decl_type_errors (ImplDef arm). Source: review checker. Confidence: likely.
- [ ] **The checker doesn't type `ensures` clauses beyond their shape.** `ensures n == "x"` on an `Int` passes `check`.
  Where: checker (ensures). Source: ledger C1 (#462). Confidence: sure.
- [ ] **`stringify(x.to_json())` type-checks and crashes.** `to_json` returns a `Result`, yet the call passes `check`, then crashes with signal 10/11 in `std.json.stringify`.
  Where: checker (calls). Source: ledger C3 (#497). Confidence: likely.
- [ ] **Generic type arguments are covariant everywhere, including contravariant positions.** `ok = ok && fits(mut i, was[k], gas[k])` for every argument of every named type. The comment justifies this by value semantics ("values are never shared").
  Where: compiler.check.fits, the `(TNamed, TNamed)` arm. Source: review checker. Confidence: likely.
- [ ] **A field every member of a union (or every variant) has takes the last one's type.** Both loops do `out = Some(x)` for each member and never compare. The comments ("with the same type", and the IDEAS entry "a field all have, with one type, gets that type") claim a check the code doesn't make.
  Where: compiler.check field access on `TUnion` (3455-3480), variants_field (9093). Source: review checker. Confidence: sure.
- [ ] **`solve_pending` skips obligations on generic and compound types.** `TNamed { name, args } => if len(args) == 0 && program_type(i, name) && …`. `Box[Int]`, `List[Foo]`, `Option[Foo]`, unions and functions fall into `_ => ()` and are accepted.
  Where: compiler.check.solve_pending (9641). Source: review checker. Confidence: sure.
- [ ] **Trait obligations are not tracked in packages.** `ast.TraitDef(t) => if d.module == "" { own_traits.insert }`, with the comment "In a package, names are qualified in ways the obligations don't follow." In a package, `own_trait_of` always gives `None`, so a trait method used as a function (`price(x)`, `xs.map(price)`) is `TAny`, and `true.price()` makes no obligation.
  Where: compiler.check.type_errors_with (`own_traits`), own_trait_of, trait_fn_of. Source: review checker. Confidence: sure.
- [ ] **Own methods and trait methods share one table key.** The own `impl Pt { fn area }` and the trait's `impl Area for Pt { fn area }` both write `of["Pt.area"]`, and whichever comes later in `decls` wins. The comment "a type's own method comes before a trait's of the same name, as calls find them" holds only for `own_recv`/`trait_recv`, not for `of`.
  Where: compiler.check.methods_of (`of.insert(q + "." + f.name, …)`). Source: review checker. Confidence: likely.
- [ ] **Generic impls whose parameter names differ from the type's are ignored.** `usable := get(types.named, q) ?? ["?"] == im.type_params && (len(im.type_params) == 0 || own_params)`. For `type Box[T]` with `impl[U] Box[U]`, or a specialised `impl Show for Box[Int]`, `usable` is false, so the methods never enter `of`, `self_methods` or `mut_self`.
  Where: compiler.check.methods_of (`usable`). Source: review checker. Confidence: sure.
- [ ] **Closure and `extern` calls with the wrong number of arguments pass.** `infer_call` skips argument checking when the counts differ, saying "reported by `expression_errors`". But `errors_in` only reports when `get(c.arity, n)` is `Some`, and returns `None` when `contains(env, n)` (a local closure).
  Where: compiler.check.infer_call (`if len(params) == len(args)`), errors_in (Call arm, ~944), expression_errors_in. Source: review checker. Confidence: sure.
- [ ] **Any arm with a guard turns off the missing-value check.** One undecidable arm anywhere disables the whole exhaustiveness check for ints, strings, lists, tuples and unions. IDEAS ("Match coverage with the SMT solver") says verify then obliges "some arm matches".
  Where: compiler.check.check_coverage (`if rows.any(r -> r.any(c -> has_cond(c))) { return }`). Source: review checker. Confidence: sure.
- [ ] **`missing` gives "nothing missing" past depth 12.** `None` means exhaustive. Depth grows by one per column and per constructor field, so a tuple of 13 elements, or a list pattern nested past 12 levels, is accepted as covered whatever the arms are.
  Where: compiler.check.missing (`if depth > 12 { return None }`), useful (`depth > 12 … return true`). Source: review checker. Confidence: sure.
- [ ] **Float coverage ignores NaN.** The atoms are intervals from -inf to +inf and points between them. No atom stands for NaN (the file has no mention of NaN). `match x { x where x < 0.0 => …, x where x >= 0.0 => … }` is judged exhaustive, but NaN fails both comparisons and reaches `no match arm`.
  Where: compiler.check.atoms (TFloat), sample, real_holds. Source: review checker. Confidence: likely.
- [ ] **Known: methods of std types and built-ins have no facts, so their matches are covered by no one.** IDEAS.md line 200: "methods of std's types the checker doesn't see (`SortedDict.get`) and built-ins (`get`, `xs.first()`) have no facts, so matches on them are covered by neither the checker nor `exhaustive`".
  Where: compiler.check.variants_covered / narrowed_at. Source: review checker; ledger C20. Confidence: sure.
- [ ] **`declarations` finds traits by short name, so imported traits are not checked.** An `impl std.json.Json for T` (the trait named in full, as check's comment says another module writes it) looks up `"std.json.Json"` and finds nothing, so neither missing nor extra methods are reported.
  Where: compiler.declarations.declaration_errors_in (`trait_methods.insert(t.name, …)`, `get(trait_methods, im.trait_name)`). Source: review checker. Confidence: likely.
- [ ] **Known: a trait and a type with the same name in one module pass `check`.** IDEAS.md lines 2384 and 2528 (std-net): codegen then can't find the type's constructor. Not claimed fixed. The code is consistent with it: `lower` checks `types.traits` before `types.named`.
  Where: compiler.check / compiler.declarations (name tables). Source: review checker; ledger C8. Confidence: sure.
- [ ] **Known: a generic `List` overload and a `String` overload let a call that fits neither pass.** IDEAS.md line ~2535: "`xs["a"]` on a `List[Int]` … fixed on branch `std-math`, #573, not on main". Assumed still present at v0.5.0.
  Where: compiler.check.builtin_call / fitting_builtins. Source: review checker; ledger C2 (fixed only on unmerged #573). Confidence: sure.
- [ ] **Known: core is never type-checked.** IDEAS.md line ~2558: "`strata --file core check` only decodes core's files; it never type-checks core's code". Not claimed fixed.
  Where: main / check entry for `--file core`. Source: review checker; ledger C18. Confidence: sure.
- [ ] **`mut` markers on method arguments are never checked.** `params_of` collects `FnDef` parameters only, and method calls never reach `check_arg`. `check` checks only the receiver's `mut self` marker and, through `mut_params`, exactness for functions only (it is keyed by function name).
  Where: compiler.mutability.walk (`MethodCall { recv, args, .. } => { walk_arg; walk_args }`), params_of; compiler.check.infer_method. Source: review checker. Confidence: likely.
- [ ] **`noalloc` trusts every receiver it can't type.** `TList(_) | TAny | TVar(_) => ()` for loops, `TAny` for method receivers, and `_ => None` for arithmetic.
  Where: compiler.check.noalloc_typed, allocating_arith. Source: review checker. Confidence: sure.
- [ ] **A `T: Json` bound on a generated call is not checked.** Calls of `std.python.argument[T: Json]` pass with types that are not `Json`.
  Where: checker (trait bounds). Source: ledger C4 (#495). Confidence: likely.
- [ ] **`check` accepts a call of a deleted function by its qualified name.** `model.unique` still checks after `unique` is deleted.
  Where: checker. Source: ledger C5 (#529). Confidence: likely.
- [ ] **A pattern of a removed variant passes `check`.** `ast.Spread(_)` was accepted by `check` and refused only by codegen.
  Where: checker (patterns). Source: ledger C13 (#562). Confidence: possible.
- [ ] **A refined record accepts a `()` for a field.** With a record refined, `Box { table: b.table.insert(k, e), … }` passed `check`, which the unrefined type refuses.
  Where: checker (records). Source: ledger C14 (#486). Confidence: likely.
- [ ] **A two-parameter lambda where a one-argument function is expected is not reported.** `xs.map((a, b) -> a)` on a `List[Int]` passes `check`.
  Where: checker (lambdas). Source: ledger C15. Confidence: likely.
- [ ] **A type nothing fixes stays `_`, and `==`/hashing skip it.** A `_` part of a value is compared as `true`; there is no error for a value with a `_` part that is made.
  Where: checker / codegen. Source: ledger C16 (#563, #565 partial). Confidence: likely.
- [ ] **Names of builtins break models and methods.** A trait model named `keys` is never read through the model; a user `fn insert` breaks `mut self.all.insert(..)` in an impl; `Set[KeyValue] {}` (a set of a union) doesn't type-check.
  Where: checker. Source: ledger C19. Confidence: likely.
- [ ] **A `comptime` parameter is `TAny`.** `fn_inferred` gives it `TAny` ("worked on while compiling: not modelled"), so a union asked for of it passes unchecked. Three comparisons in std: `std.orm.key_value` (twice) and `impl std.sqlite.Sql for Option[T]`, the last of the 18 that #628 let through as unknown. Typing it needs dependent parameters: `comptime t: Type` typed as its declared type, `x: t` as an opaque type named by `t`, `match comptime t { Int => … }` arms substituting the arm's type for `t` in the environment (`Option[y]` binding `y` as a type parameter), and a call instantiating `t` in the later parameters' types with the type its argument names (`key_value(y, v)`).
  Where: compiler.check.fn_inferred (`p.compile_time`). Source: checker-narrowing. Confidence: sure.
- [ ] **A coverage message names a list of functions `_`.** `match xs { Some(_) => 1, None => 0 }` on a `List[(Int) -> Int]` reports "non-exhaustive match on `_`: `[]` is not covered" (and both arms unreachable) besides the pattern errors.
  Where: compiler.check.cover_of / check_coverage (the scrutinee's type text). Source: checker-silent. Confidence: sure.
- [ ] **Unknown types and wrong type-argument counts lower to `TAny`.** `Some(ps) => if len(ps) == len(args) {…} else { TAny }`, `None => TAny`, and `ast.RowType(_) => TAny`. A misspelled or imported-but-unmodelled type, or `Box[Int, Int]`, makes every value of it unchecked. A generic alias with the wrong number of arguments too.
  Where: compiler.check.lower. Source: review checker. Confidence: sure.
- [ ] **The "can't be passed as a value" rule only sees generic functions among `decls`.** `on_type` is built from `decls` only. A generic function in `lib` that calls `T.empty()` and is passed as a value is never reported. That includes a definition of the same package that the check cache (main.loaded_check) put in `lib` because it was already checked.
  Where: compiler.check.type_errors_with (`on_type`). Source: review checker. Confidence: sure.
- [ ] **Constructor patterns match by short name across sums.** In a union of two sums from different modules that each have a variant `Empty`, a pattern `a.Empty` also covers `b.Empty`'s atom, so the missing `b.Empty` is not reported.
  Where: compiler.check.covers (`last_part(c) == last_part(n)`). Source: review checker. Confidence: possible.
- [ ] **Known: a non-exhaustive error is reported against the wrong function on a rejected edit.** IDEAS.md "Known gaps" (line ~2532): "On a rejected edit, non-exhaustive-match errors are reported against the wrong functions (e.g. `lint.refined_by` …)". No PR claims a fix.
  Where: compiler.exhaustive / main error labelling. Source: review checker; ledger C11. Confidence: sure.
- [ ] **Known: what is left of a mixed union isn't narrowed after its variants are matched.** IDEAS.md "Known gaps" line ~2529 (std-net). Not claimed fixed.
  Where: compiler.check.arm_members / arm_variants. Source: review checker; ledger C9. Confidence: sure.
- [ ] **Known: `List[B[M]] =>` refused for a local generic record.** IDEAS.md line ~2530 (std-pages): "unknown constructor `M` in pattern". Not claimed fixed.
  Where: compiler.check.cover_of / declarations.pattern_errors. Source: review checker; ledger C10. Confidence: sure.
- [ ] **Calls to imported or same-named functions skip the `mut` rules.** A name defined twice is left out ("A name defined twice is left out"), and lookups go by the name as written. When `get(w.fns, n)` is `None` the arguments are only walked: neither a missing `mut` nor a needless one is reported.
  Where: compiler.mutability.params_of, walk_call. Source: review checker. Confidence: likely.
- [ ] **A `spawn` inside a lambda passes the `scope` rule.** `Lambda { … } => Ctx { ..c, in_spawn: false }` keeps `floor: true` from an enclosing `scope`. A lambda made inside a `scope` that spawns, returned from the scope and called later, starts a task outside any scope.
  Where: compiler.capabilities.walk (Lambda arm keeps `floor`). Source: review checker. Confidence: possible.
- [ ] **`unused.members` types field reads with the second, weaker inference.** `field_typing` builds `types_of(all)` and `methods_of(all)` with no core code traits (see the table finding), so more receivers are `TAny`. That counts as "read of every record" (`None => true`): conservative.
  Where: compiler.unused.members, check.field_typing / field_reads. Source: review checker. Confidence: sure.
- [ ] **The lint rewrite renames `it` by substitution.** A refinement conjunct that holds a lambda with its own `it`, or that names a variable called like the parameter, would be captured or renamed wrongly when it is moved into a `requires`.
  Where: compiler.lint.fn_shapes (`renamed(c, "it", p.name)`). Source: review checker. Confidence: possible.
- [ ] **Some errors are missed once an import fails.** compiler/'s checker misses errors such as `unknown constructor in pattern` after a failed import.
  Where: checker. Source: ledger C7. Confidence: likely.
- [ ] **Errors are labelled by short name.** `text.inline` is reported as `markdown.inline`, `parse.binary` as `check.binary`, `resolve.decl_kind` as `parse.decl_kind`.
  Where: checker (error labels). Source: ledger C12 (#562). Confidence: likely.
- [ ] **`check` of a `--file` program doesn't type `s.graphemes()`.** std's impl is not in the tables there.
  Where: checker (file mode). Source: ledger C17 (#453). Confidence: likely.
- [ ] **A match the syntax check can't type is found only by `verify`.** `match p.op` errors surface only in `verify`.
  Where: checker. Source: ledger C21 (#416). Confidence: likely.
- [ ] **`build --no-verify` prints no warnings.** Warnings are lost when verification is skipped.
  Where: build. Source: ledger C24 (#459). Confidence: possible.
- [ ] **A union of type parameters given a union before they are fixed is refused.** `fn same[T, E](v: T | E, t: T, e: E)` called as `same(x, 0, "")` with `x: Int | String` is "a value of `Int` fits more than one member of `_ | _`": the arguments are checked in order, and when `v` is, `T` and `E` are still open. With `v` last it checks.
  Where: check.members_fit / open_member (arguments unified in order in the call's inference). Source: generic-unions PR. Confidence: sure.
- [ ] **`fits` compares nested types without fuel.** After its `TFn` arm, `fits` recurses with no fuel. `unify`'s fuel is reached only on speculative paths (overload search, joins), whose error is dropped.
  Where: check.fits, check.unify. Source: PR #628. Confidence: likely.
- [ ] **Trait methods are keyed by the trait's short name.** Two traits with one short name in different modules can't be told apart.
  Where: check (trait method tables). Source: PR #626. Confidence: likely.
- [ ] **A record's fields have no `pub` of their own.** Outside its module, a `pub` record shows every field and a private record none.
  Where: check.closed_fields, check.Types.private. Source: PR #649. Confidence: sure.

## Front end

- [ ] **`"\r"` is not an escape.** `"\r"` in a string literal is a backslash and an `r`, not a carriage return; `\0`, `\u{..}` and `\x` are missing too. Code comparing with `"\r"` (`lex.debug_char`, `parse.debug_quoted`, std.json's decoder and `quote`) is dead or wrong, so std.json decodes the escape `\r` as backslash + `r`.
  Where: lex.escaped (compiler.lex.st:537); lex.debug_char (compiler.lex.st:557); std.json.read_string (std.json.st:1235); std.json.quote (std.json.st:365). Source: review front-end; review std; ledger F1, F2, S3. Confidence: sure.
- [ ] **Inherent and trait methods go into the module's global namespace, marked public.** Inherent methods: `ImplDef(i) => if i.trait_name == "" { … mq := qualified(m, f.name); mut table.insert(mq, Item { module: m, public: true, … })`.
  Where: resolve.names_of (compiler.resolve.st:1517–1550), resolve.scope_of. Source: review front-end. Confidence: likely.
- [ ] **Refinements written as type arguments are hoisted to root-level aliases keyed by their printed text.** `text := type_text(a); if !contains(all, text) { … made.push((base, pred)) }`.
  Where: desugar.inline_refinements (compiler.desugar.st:1431), desugar.unrefined_type. Source: review front-end. Confidence: likely.
- [ ] **Lexing is quadratic: `text_of` copies the tail of the source for every token.** `join(take(drop(lx.chars, a), b - a), "")`. `drop` copies everything after `a`, and it is called for every identifier, number and comment. `symbols()` also rebuilds a 47-element list and runs `find` with `starts_with` for every symbol token.
  Where: lex.text_of (compiler.lex.st:358), lex.lex. Source: review front-end. Confidence: likely.
- [ ] **Comment placement is quadratic.** Each call scans all of `p.comments`: `for (i, c) in enumerate(p.comments)`.
  Where: parse.leading, parse.trailing, parse.blank_line_before, parse.use_comment. Source: review front-end. Confidence: sure.
- [ ] **`:=` doesn't refuse refutable patterns.** IDEAS.md:1051: "`:=` doesn't refuse refutable patterns at all today: `Some(z) := q` checks". A bare record pattern `{ x, y }` is not a pattern anywhere. The code still has no check.
  Where: parse.statement → binding_pattern (no refutability check, unlike param_pattern). Source: review front-end. Confidence: sure.
- [ ] **`select`/`race` arm comments are dropped.** The parser stores `SelectArm.comments` (leading and trailing, compiler.parse.st:2496/2520), but the printer never calls `comments_text(a.comments, …)` for them, unlike match arms (text.st:839).
  Where: text.inline, the `Select(arms) | Race(arms)` case (compiler.text.st:881–904). Source: review front-end. Confidence: sure.
- [ ] **A negative literal as a receiver prints as a negation of the call.** The parser folds `-5` into `IntLit(-5)` / `FloatLit(-f)`. So `(-5).abs()` is `MethodCall { recv: IntLit(-5) }`. `prec(IntLit(-5))` falls to `_ => 12`, so it prints `-5.abs()`, which reparses as `Neg(5.abs())` and evaluates to -5 instead of 5.
  Where: text.prec (compiler.text.st:1110), parse.unary. Source: review front-end. Confidence: sure.
- [ ] **`derive T for X { model m = e }` prints as `derive T for X`, losing the models.** `if i.derived && len(i.methods) == 0 && i.clauses == [] { return impl_head(i) }`. `models` is not checked, but parse.impl_rest accepts `model` lines in a derive block.
  Where: text.impl_decl_text (compiler.text.st:2066), impl_interface_text (1706). Source: review front-end. Confidence: sure.
- [ ] **Markup is recognised by call shape, so user code is shown as markup.** `MethodCall { recv: Var("web"), name, args } => Some((name, args))`, and likewise `Call { callee: Var("web.…") }`. Any value named `web` with methods `element`/`children`/`attr` (a local, a parameter, a record) prints as `<…>`, which reparses as `std.web.element(…)`.
  Where: text.web_fn (compiler.text.st:504), text.expr_text → markup_view. Source: review front-end. Confidence: likely.
- [ ] **Printing is exponential in nesting depth for long expressions.** `expr_text` always computes `one := inline(e, d)`. When the result is too long it prints the children again in broken form, each of which again does `inline` and then maybe broken.
  Where: text.expr_text, text.wrap, text.prec → text.shorthand. Source: review front-end. Confidence: likely.
- [ ] **Two sum types in one module with the same variant name collide silently.** `cq := qualified(m, v.name); mut table.insert(cq, item)`. No error is given; the last one wins in the table, in `children`, and in `store.graph_from`'s `ctor_types`.
  Where: resolve.names_of (compiler.resolve.st:1504–1509); derive.defs_of `ix.ctors` likewise. Source: review front-end. Confidence: likely.
- [ ] **Names made by desugaring are not hygienic.** IDEAS.md:2006: "a local or a function of that name takes them over… a top-level `fn at(…)` is called by `xs[i]` instead of the built-in." Desugar still emits plain `Var("at")` and the other names.
  Where: desugar.unsugared_node and sliced (`call("at"…)`, `range`, `slice`, `len`, `set_at`, `dict`, `set`, `map`, `filter`, `fold`). Source: review front-end; ledger F3. Confidence: sure.
- [ ] **Foreign wrappers are recognised by the shape of their body, so a user function can be taken as assumed.** `Call { callee: Var(n), .. } => … if n == "std." + w + ".decoded" || n == "std." + w + ".finished"`. The doc says "`verify` takes it as assumed and never runs it".
  Where: desugar.foreign_call, desugar.python_call (compiler.desugar.st:1780–1801). Source: review front-end. Confidence: likely.
- [ ] **Mock methods are computed from the old state for every `self` in an `ensures`.** `value: self_as(before(…ch.1…), "$old_self")` replaces every `self`, not only the ones under `old`, with the pre-call receiver. For `ensures self.n == len(self.items) && self.items == old(self.items) + [x]` the mock sets `n` from the old `items`, which breaks the trait's own `ensures`.
  Where: desugar.mock_of (compiler.desugar.st:296–306). Source: review front-end. Confidence: likely.
- [ ] **`value_uses` rewrites locals that share a derived function's name.** `Var(name) => match get(values, name)`. There is no scope check. Root-module functions are stored unqualified, so in a script a local or parameter named like a function that has `derive Tool for search` (here `search`) is replaced with `search.Tool.value()` or `$unbound.search`.
  Where: derive.value_use (compiler.derive.st:2871). Source: review front-end. Confidence: likely.
- [ ] **Method dispatch takes the first impl in declaration order, not the static one.** `for i in w.impls { if type_head(i.for_type) == h { for f in i.methods { if f.name == m { return Some(…) } } } }`.
  Where: eval.method_for (compiler.eval.st:1672), eval.method_typed. Source: review front-end. Confidence: likely.
- [ ] **Two definitions with the same header name share an entry.** Header names come from `decl_name`: `impl Foo`, `impl Show for Pt`, `test "x"`.
  Where: binary.encode_module_kept (`match get(kept, name)`), desugar.decl_name. Source: review front-end. Confidence: likely.
- [ ] **Script `edit` accepts a `comptime` that `check` refuses.** `strata --file F edit` of a script stores a `comptime` that `check` then refuses.
  Where: edit. Source: ledger F8. Confidence: likely.
- [ ] **Columns are in characters, except after a string, where they move by bytes; multi-line strings don't reset the column.** `lx.col += 2 + byte_len(s)`. The module doc says columns count characters. Inside a string `\n` bumps `lx.line` but never resets `lx.col`, and an escape counts as one character instead of two.
  Where: lex.string (compiler.lex.st:530). Source: review front-end. Confidence: sure.
- [ ] **The markup heuristic misreads keyword-led operands.** Only `return`, `in` and `assert` are excluded: `Ident(s) => !(s == "return" || s == "in" || s == "assert")`. So `else <p/>`, `await <x/>`, `spawn <p/>`, `=> … else <b>` all lex `<` as a comparison.
  Where: lex.ends_operand (compiler.lex.st:737). Source: review front-end. Confidence: likely.
- [ ] **Doc lines are trimmed, so indented doc examples lose their indentation.** Recorded in IDEAS.md:404 ("a `///` doc's indented example loses its indentation when `edit` reads it (doc lines are trimmed)").
  Where: lex.lex: `DocTok(trim(byte_slice(t, 3, …)))`. Source: review front-end; ledger T3. Confidence: sure.
- [ ] **Pattern parameters become `$pN` with `let`s at the top of the body, so `decreases` is refused.** `"a function with a parameter written as a pattern can't have a decreases clause yet"`. Recorded in IDEAS.md:1051. The `$p{len(out)}` rename is information passed by naming, and `text.fn_view` and `text.lambda_view` have to reverse it by matching `Let { value: Var(v) }` shapes.
  Where: parse.fn_rest (compiler.parse.st:3159), parse.params_bound. Source: review front-end. Confidence: sure.
- [ ] **Huge format widths silently become 0, and the overflow check is dead.** `width := … parse_int(w) ?? 0`, then `if w != "" && width > 18446744073709551615`. When `parse_int` fails the width is already 0, so the error is never given. The same applies to precision.
  Where: parse.parse_spec (compiler.parse.st:2130–2150). Source: review front-end. Confidence: likely.
- [ ] **The capability and impl "needs a body" checks are dead.** Both call `fn_rest(mut p, false)`, which sets `has_body := !optional_body || …`, so `has_body` is always true. Their `if !has_body` messages never fire: a missing body fails earlier with "expected `{`".
  Where: parse.capability_decl (3503), parse.impl_rest (3471). Source: review front-end. Confidence: sure.
- [ ] **A `Set`/`Dict` comprehension in an `if`/`for`/`while`/`match` head prints unparseable code.** It covers `DictExpr`/`SetExpr` but not `Comp { kind: SetComp | DictComp }`, which print as `Set { … }` / `Dict { … }`. In a head the parser has `no_struct` set and fails with "put the `Set { … }` in parentheses here" (parse.collection).
  Where: text.brace_on_spine (compiler.text.st:257). Source: review front-end. Confidence: sure.
- [ ] **`shared capability trait` prints a combination the parser refuses.** The printer prints both `shared ` and `capability ` before `trait`. The parser accepts only `shared trait` or `capability trait` (decl_kind:812/819). A derivable trait also loses its `shared`/`capability` flags when printed.
  Where: text.trait_text (compiler.text.st:79–93). Source: review front-end. Confidence: sure.
- [ ] **Type parameters are not locals in expressions.** `T.empty()` is `MethodCall { recv: Var("T") }`. `module_path` returns None, then `expr(Var("T"))` runs `name(r, "T")`, which consults `r.visible` but not `r.tparams` (only `ty()` checks tparams).
  Where: resolve.expr, the `Var(x)` case (compiler.resolve.st:735). Source: review front-end. Confidence: possible.
- [ ] **Any global (non-local) name equal to a module path is read as that module.** `if contains(r.modules, path) { Some(path) }`. `r.modules` holds every module of the program and std, not only the imported ones. A root-level value or function sharing a module's name turns `x.f` into a call of `x.f`, which then fails with "module doesn't use".
  Where: resolve.module_path (compiler.resolve.st:1366). Source: review front-end. Confidence: possible.
- [ ] **`bind` silently drops a binding when no scope is open.** `match get(r.locals, n - 1) { Some(s) => …, None => () }`. Patterns resolved outside an open scope, such as a `relocated` definition's patterns before any `open`, bind nothing.
  Where: resolve.bind (compiler.resolve.st:1241). Source: review front-end. Confidence: possible.
- [ ] **Use before import is accepted by `edit`.** `edit` and `check` accept a definition using a type or constructor stored before its import was added; codegen catches it later.
  Where: resolve (names that resolve to nothing "stay as written, and the checker reports it"); compiler.declarations / check.lower (`None => TAny`). Source: review front-end; review checker; ledger C6. Confidence: likely.
- [ ] **`modelled` keys traits by short name too, so same-named traits in two modules collide.** `mut out.insert(t.name, m)` for every trait. `modelled_of("a.Db")`, when it misses, looks up `"Db"`, which may be another module's `Db`.
  Where: desugar.modelled (compiler.desugar.st:168), desugar.modelled_of (falls back to the last part). Source: review front-end. Confidence: likely.
- [ ] **Mock types may be generated twice across incremental desugars.** `used` is collected from `decls` (the new batch) only. `mock_of` makes `$mock_T` and its impl every time a batch uses `mock T`. Nothing checks `context` for an existing `$mock_T`, unlike `anon_types`, which checks `have`.
  Where: desugar.with_models (compiler.desugar.st:107–149). Source: review front-end. Confidence: possible.
- [ ] **Named results are substituted without regard to shadowing.** Every `Var(n)` whose name is a result name becomes `it.k`, including a lambda parameter of that name inside the `ensures`. `self_as` and `fn_models`' `self_as(…, "it")` have the same capture problem with a lambda parameter named `it`.
  Where: desugar.results_named (compiler.desugar.st:931). Source: review front-end. Confidence: possible.
- [ ] **Field-by-field update rewriting may pass through states that break a record's refinement.** `x = T { ..x, a: v, b: w }` becomes `x.a = $upd_a; x.b = $upd_b`. For `type T = { a: Int, b: Int } where it.a <= it.b`, the intermediate `x` (new `a`, old `b`) may break the refinement.
  Where: desugar.updates_assigned (compiler.desugar.st:962). Source: review front-end. Confidence: possible.
- [ ] **Functions are recognised by short name: `compile_error`, `implements`.** `if short(n) == "compile_error" && len(args) == 1` and `short(n) == "implements" && len(args) == 2`. Any user function with that short name, in any module, called inside derived code is hijacked.
  Where: derive.expr (compiler.derive.st:1295, 1302). Source: review front-end. Confidence: sure.
- [ ] **`comptime` outside functions, tests and impl methods is never folded.** Only `FnDef`, `TestDef` and non-derived `ImplDef` method bodies are checked. A `comptime e` in a field default, a type refinement, a trait's default method or a contract stays a `Comptime` node, which `eval` and the other passes treat as plain evaluation.
  Where: derive.holds_comptime (compiler.derive.st:2933). Source: review front-end. Confidence: possible.
- [ ] **`MutExpr` is ignored, and built-ins that change values are triggered by method name alone.** `xs.sort()` without `mut` on a list is treated as a changing call and assigned back. The receiver expression is evaluated twice: once by `eval(recv)` and again inside the `changes_as` block.
  Where: eval.eval (`MutExpr(x) => eval(mut st, w, x)`), eval.method_call (`if changing(name) …`). Source: review front-end. Confidence: likely.
- [ ] **`entry_strings` swallows errors.** `match strings(mut r) { _ => () }`. A corrupt table gives a partial list with no error.
  Where: binary.entry_strings (compiler.binary.st:2862–2868). Source: review front-end. Confidence: sure.
- [ ] **The 10000-level depth limit may still overflow the native stack.** Decoding recurses `expr → exprs → expr`, several frames per level. Whether 10000 levels fit on the stack isn't established; it is a guard, not a proof.
  Where: binary.max_depth (10000), binary.deeper. Source: review front-end. Confidence: possible.
- [ ] **`without_notes` keeps comments inside `assert … by` proofs, `calc` steps and derive clauses.** `AssertBy { cond, proof }` and `Calc(xs)` are not walked, so `Comment` statements inside a proof block stay. Editing a comment there, or on a derive clause, changes `code_hash`/`interface_hash`, which invalidates verify results and recompiles.
  Where: ast.bare_stmt (`_ => s`), ast.without_notes (`ImplDef(ImplDecl { ..i, … })` keeps `clauses` with their comments; `DeriveFnDef(x) => DeriveFnDef(x)`). Source: review front-end. Confidence: sure.
- [ ] **`calc_steps` leaves the first step's `_` as a variable.** `(Binary { op, left: Var("_"), right }, Some(p))` only matches after a step. A first step `_ <= c`, or a step after a non-binary step, keeps `Var("_")`, which later reads as an unknown variable instead of an error.
  Where: ast.calc_steps (compiler.ast.st:770-ish). Source: review front-end. Confidence: sure.
- [ ] **`tests_it` ignores shadowing of `it`.** `IsExpr { value: Var("it"), .. } => true`, even inside a lambda whose parameter is named `it`. `check.narrowed_result` would then narrow on the wrong fact.
  Where: walk.tests_it. Source: review front-end. Confidence: possible.
- [ ] **`type Answer = Some(Int) | None` says `unknown type None`.** language-reference.md says it is refused as `Option[Int]`; the message is wrong.
  Where: resolver / checker. Source: ledger F5. Confidence: likely.
- [ ] **`let`/`for` patterns naming a removed variant still reject the edit.** Patterns outside `match` keep a removed variant and block the edit.
  Where: edit. Source: ledger F7. Confidence: likely.
- [ ] **The tree-sitter grammar doesn't know `decreases`.** Editors mis-highlight or mis-parse a `decreases` clause.
  Where: tree-sitter grammar. Source: ledger F10 (#461). Confidence: possible.
- [ ] **A `use` after `fn main` in the root module is not seen.** A `use util` stored after `fn main` (an `--as-given` save) gives "unknown variable `util`". It works when the `use` comes first.
  Where: resolve (root module's imports). Source: PR #651. Confidence: sure.
- [ ] **A private std name imported while std is read as headers is "unknown variable".** With std read as precompiled headers (`edit` without `--as-given`), `from std.json use hex_digit` gives "unknown variable `hex_digit`": the header lacks the name, and `use_errors_from` drops "module `std.…` has no …". `strata check` names it as private.
  Where: resolve.use_errors_from, std headers. Source: PR #651. Confidence: sure.

## Code generation

- [ ] **(memory) `extern c` `mut String` parameter: the buffer is freed before it is read back.** For `p.mutable` with `ct.kind == "str"`, the `strata_cstr` copy goes in the out slot and is also pushed to `frees`. After the call, `for c in frees { … strata_cstr_free(c) }` runs before `for (l, ct, slot, old) in outs { raw := load slot; from_c(raw …) }`.
  Where: codegen.foreign_call (st:21127-21195). Source: review codegen. Confidence: sure.
- [ ] **A union with two members of the same head reads under the wrong tag.** Widening into `List[Int] | List[String]` reads the list under the wrong tag (wrong results).
  Where: codegen (unions). Source: ledger G3 (#577). Confidence: likely.
- [ ] **Refinements inside a generic record's type arguments are not checked at run time.** A `Box[Cents]` holding a negative value doesn't fault.
  Where: codegen (run-time checks). Source: ledger G5 (#570). Confidence: likely.
- [ ] **A union holding a tuple returned by a call prints its first element.** `let u: Int | (Int, Int) = dm(8, 3)` prints `2`.
  Where: codegen (unions). Source: ledger G9 (#542). Confidence: likely.
- [ ] **A `for i in a..b` counter adds 1 to the raw word, also when `a` is a big integer.** The counting path takes `lo` and `hi` as `Int` words, with no test that they are small. A big `Int` is a handle outside [-2^62, 2^62) (runtime.c `big_at`), so `add 1` turns it into another handle.
  Where: codegen.for_loop (st:15315, 15405 `{next} = add i64 {i}, 1`). Source: review codegen. Confidence: likely.
- [ ] **A Float's hash is its bits, but `==` is `fcmp oeq`.** `0.0 == -0.0` holds, but their hashes differ. So a `Set[Float]` can hold both, and a `Dict` lookup of `-0.0` can miss the key `0.0`.
  Where: codegen.hash_helper (st:8118 `TBool | TFloat => to_slot` = `bitcast double to i64`), equal (6820 `fcmp oeq`). Source: review codegen. Confidence: likely.
- [ ] **`?` passes the failure through without converting it to the function's result type.** The fail path is `give_result(g, V { ..v, ty: better(g.ret, v.ty) })`. `better` keeps `g.ret`'s type but does no representation change. When the function's error type is a union (`Result[T, A | B]`) and `v`'s is a member `A`, a boxed member would need `to_union`.
  Where: codegen.try_want (st:17091). Source: review codegen. Confidence: possible.
- [ ] **No type checks and no verifier on the fast IR path.** `value(r, t)` returns a local of whatever type it has, never compared with `t`, so `store i64 %p` with `%p` a `ptr` builds an invalid instruction. LLParser would reject it (known bug #6 shows codegen does make such IR).
  Where: ir.c value (st:429), instruction, read_module; no `LLVMVerifyModule` anywhere in compiler/c. Source: review codegen. Confidence: sure.
- [ ] **The pattern `Ok(())` doesn't compile.** `Ok(_)` does.
  Where: codegen (patterns). Source: ledger G1 (#574). Confidence: likely.
- [ ] **A `_ =>` arm of a union match doesn't narrow in compiled code.** `c.x` there is "not compiled yet".
  Where: codegen (match). Source: ledger G2 (#577). Confidence: likely.
- [ ] **`Shop.line(s, x)` (own `self` method called on the type) doesn't compile.** Codegen reports "`Shop` without its fields".
  Where: codegen (methods). Source: ledger G4 (#570). Confidence: likely.
- [ ] **A `match` giving a `U64` with a literal `0` in one arm crashed at run time.** Not reduced.
  Where: codegen. Source: ledger G6. Confidence: possible.
- [ ] **A list holding an `Option[Int]` and an `Option[Float]` makes invalid IR.** LLVM refuses a `switch` with duplicate cases.
  Where: codegen. Source: ledger G7. Confidence: likely.
- [ ] **`x := setting(k) ?? d` with `k` unset compiles to an undefined value.** LLVM reports "use of undefined value %v.x".
  Where: codegen (`??`). Source: ledger G8 (#495). Confidence: likely.
- [ ] **A lambda with no expected type gets `()` as its parameter type.** `r := R { f: Some(x -> x) }` called in a generic function makes IR that LLVM rejects.
  Where: codegen (lambdas). Source: ledger G10 (#450). Confidence: likely.
- [ ] **`containers` in `build-copies` is quadratic though the case says linear.** The case times out under load.
  Where: codegen (copies). Source: ledger P1. Confidence: likely.
- [ ] **Long list literals need about 5 GB at 20,000 elements.** `cli-long_list_literals_compile_in_linear_time` passes on output but not memory.
  Where: codegen (list literals). Source: ledger P2. Confidence: likely.
- [ ] **The temporary file name does not include the PID.** Two processes writing the same entry write the same temporary file. One can rename it into place while the other has truncated and is rewriting it, so a reader sees a torn entry.
  Where: cache.write_entry (`tmp := path + ".tmp-" + sha256_hex(path + text)`), write_entry_bytes (`… + to_string(len(bytes))`). Source: review codegen. Confidence: likely.
- [ ] **Cancellation checks match methods by name.** `Fixed.take` checks for cancellation because `Line.take` waits.
  Where: codegen (cancellation). Source: ledger G13 (#435). Confidence: likely.
- [ ] **Equal `SortedDict`s can compare unequal.** Two with the same entries can have different tree shapes, and `==` compares shapes.
  Where: std.collections / codegen ==. Source: ledger G20 (#448). Confidence: likely.
- [ ] **Remaining codegen gaps.** `[]` in a record literal, a list of a union of records inside a run-time `ensures`, the built-in `level_or_empty` on a Dict, the constructor `Cancel` in `strata_run_call`.
  Where: codegen. Source: ledger G21 (#455 partial). Confidence: possible.
- [ ] **Cold and warm builds of the same compiler differ by a few bytes.** `compile_part` reads parts with LLVM's IR parser, not c/ir.c's; one `eval` function's alignment differs.
  Where: codegen.compile_part. Source: ledger G26 (#434, #427). Confidence: likely.
- [ ] **`f(mut v, g(v))` keeps `v`'s reference until `f` returns, so `f`'s first change copies.** Also `f(mut v, h(mut v))` and `f(mut v, len(v.d))`. 20,000 dict inserts took 0.07 s with the argument bound first, 2.3-6.6 s nested. smt binds these arguments first; main has a few dozen such calls.
  Where: codegen (argument evaluation for `mut` arguments). Source: PR #607. Confidence: likely.
- [ ] **`xs[i] = v` copies `xs` when `v` is read from another list.** 100,000 assignments took 26 s; the same loop with a dict takes 0.07 s.
  Where: codegen (element assignment). Source: PR #613. Confidence: likely.
- [ ] **Memory verdicts fall back to the worst case in five places.** Calls of a local closure keep every argument (`is_it(x)` in `codegen.hint_subject`). Operators other than comparisons keep their operands even when the result is only read, because an operator's `impl` isn't looked up. Built-ins that take a function (`map`, `filter`) count as storing their arguments. `stack_let` and `stack_arg` use roots' `kept` and run without types, so a kept field path also keeps a record off the stack. `builtin_head` counts the program's functions of that short name for a `String` receiver, so a std function named `split` makes `s.split(..)` keep `s`. An `Interp` with one part may give back its part, so `f"{p.name}"` stays kept.
  Where: codegen.kept_by, codegen.escapes, codegen.stack_let, unique.head_of, unique.builtin_head. Source: PR #654, PR #662, PR #669. Confidence: sure.
- [ ] **`mut_method` trusts `exit` of every short-name candidate for an unresolved receiver.** That is sound while every `impl` method is registered, and only while built-ins and mocks keep a `mut self` held once.
  Where: unique.mut_method. Source: PR #647. Confidence: possible.

## Runtime and core

- [ ] **A test's start forgets the senders its thread holds, so a later `close` of one faults.** `strata_run_begin` calls `forget_senders()`, which drops this thread's `held` entries without closing them (`senders` stays up). A `close` of such a sender after the test began fails `unhold_sender` and faults with "`close` of a sender this task doesn't hold, or has closed already", and the channel's receivers never see it end.
  Where: runtime.c forget_senders, strata_run_begin, strata_channel_close. Source: PR #621 (runtime-uaf). Confidence: likely.
- [ ] **Per-thread free lists only go back when the thread ends (producer/consumer grows without bound).** A freeing thread pushes onto its own `heap` list. `refill` only takes from the shared list, and `strata_memory_thread_end` gives lists back at thread end. A consumer that frees what a producer allocated hoards the blocks, and the producer keeps carving new chunks.
  Where: core.rt.strata_free (threaded branch), refill, strata_memory_thread_end. Source: review core-runtime; ledger R3. Confidence: sure.
- [ ] **(memory) `join` is not synchronized: double `pthread_join` and a wrong `live_tasks`.** `static void join(Task *t) { if (t->joined) return; pthread_join(t->thread, NULL); t->joined = 1; if (__atomic_sub_fetch(&live_tasks, 1, …) == 0) set_threaded(0); …}`.
  Where: runtime.c join (1535), strata_await (1621), strata_scope_end, strata_task_share. Source: review core-runtime. Confidence: likely.
- [ ] **(memory) `str_trim` makes a negative-length string on non-UTF-8 input (memory corruption).** The forward loop steps by the lead byte's length (`a += char_size(s, a)`) and decodes past the end. The backward loop steps back over continuation bytes and can end before `a`.
  Where: core.str.str_trim (core.str.st:158), str_part, str_alloc. Source: review core-runtime. Confidence: sure.
- [ ] **Host strings are never checked to be UTF-8.** Each builds the `String` with `str_from(buf, len)` and no check. IDEAS.md "Known gaps": "`Files.read` (`host_read_file`) and `Console.read_line` don't check that the bytes are UTF-8 … it should be an `FsError`".
  Where: runtime.c strata_host_read_file, strata_host_read_line, strata_host_read_stdin, strata_host_env_var, strata_host_args, strata_host_run (output); std_net's `net_text` uses `utf8_prefix`. Source: review core-runtime; ledger R1. Confidence: sure.
- [ ] **`Taken` is a `FiniteIterator` over any iterator, but its `next` need not end.** The `take` doc says "Finite even when this iterator is not", and the `FiniteIterator` doc says "`verify` proves that a `for` loop over one ends".
  Where: core.iter `impl[I: Iterator[T], T] FiniteIterator[T] for Taken[I, T]` (core.iter.st:124); `Filtered.next`'s `loop`. Source: review core-runtime. Confidence: likely.
- [ ] **The JIT never frees modules.** `jit_fuzz_load` keeps up to 4096 (`static LLVMOrcLLJITRef fuzz_jits[4096]`), so a long-lived process grows with every `check`, then fails with "too many modules compiled".
  Where: compiler/c/jit.c. Source: ledger R2. Confidence: sure.
- [ ] **(memory) A crossed task handle can outlive its `Task`.** `release_scope` frees each `Task` at scope end (`free(t)`). A handle sent on a channel (`strata_task_share`) to a thread outside the scope can be awaited after that, which dereferences freed memory.
  Where: runtime.c release_scope, strata_await, strata_task_share. Source: review core-runtime. Confidence: possible.
- [ ] **`send` after this thread closed its sender still succeeds.** The only check is `if (c->senders == 0)`. core.channel says "A `send` after the channel has ended fails, and so does a second `close` of one sender". A send after the thread's own `close` passes while other copies are open, because `holds_sender` isn't checked.
  Where: runtime.c strata_channel_send. Source: review core-runtime. Confidence: sure.
- [ ] **An unbuffered send in `select` doesn't wait for its receiver, and can strand a value.** `(c->cap == 0 && c->waiting > c->len)` then `push_item` and return. `strata_channel_send` instead waits `while (c->cap == 0 && c->taken < mine …)`. A receiving `select` raises `waiting` on every channel it lists.
  Where: runtime.c strata_select (kind 1). Source: review core-runtime. Confidence: sure.
- [ ] **Every program linked with std polls for cancellation after each runtime call that may panic.** Recorded in IDEAS.md lab-fleet bugs: "Every program linked with std counts as supervising, so it polls for cancellation after every runtime call that may panic".
  Where: runtime.c soft_fault / strata_cancel_any (and codegen's cancel points). Source: review core-runtime; ledger G12. Confidence: sure.
- [ ] **Every big `Int` has the same hash.** `return v < -(1<<62) || v > (1<<62)-1 ? 0x5bd1e995 : v;`. A `Set`/`Dict` of big integers (U64 hashes and IDs past 2^62, `1 << 70` keys) puts every key on one linear-probing run, so each insert or lookup is O(n).
  Where: runtime.c strata_int_hash. Source: review core-runtime. Confidence: sure.
- [ ] **A big integer literal is re-parsed on every evaluation.** `int_lit(s)` builds the number from its decimal digits each time it is called. IDEAS.md lab-fleet bugs: "An integer literal of 2^62 or more is re-parsed from its digits each time it is evaluated; `to_u64`'s precondition holds one, so every `n.to_u64()` allocates".
  Where: core.int.int_lit. Source: review core-runtime; ledger G11. Confidence: sure.
- [ ] **(memory) `str_at`, `str_slice` and `str_case` read past the end on a truncated sequence.** `str_part(s, i, char_size(s, i))` copies a lead byte's full length even past `str_size`. `str_slice` does `j += char_size(s, j)` with no `j <= n` clamp, then `str_part(s, i, j - i)`.
  Where: core.str.str_at (281), str_slice (323), char_code, str_case (550), str_is. Source: review core-runtime. Confidence: sure.
- [ ] **Code-point counting disagrees across functions on invalid UTF-8.** `str_len` and prelude `str_chars` count non-continuation bytes. `char_offset` (used by `str_at`, `str_slice`) jumps by the lead byte's stated length. `CodePoints.next` uses `min(n, remaining)` bytes and doesn't check that they are continuation bytes.
  Where: core.str.str_len vs char_offset; prelude str_chars; core.text CodePoints.next vs CodePoints.len. Source: review core-runtime. Confidence: sure.
- [ ] **`CodePoints.next` gives negative or out-of-range code points.** `c = c * 64 + (byte_at(self.text, self.at + k) - 128)` doesn't check that the byte is in 0x80..0xBF, so `"\xC0\x00"` gives -128. Leads 0xF8..0xFF give values past 0x10FFFF, and surrogates and overlong forms are accepted. core.str's `char_code` masks with `& 63` instead, so the two decoders disagree.
  Where: core.text CodePoints.next. Source: review core-runtime. Confidence: sure.
- [ ] **Host paths and process arguments silently drop everything after a NUL.** `memcpy(c, s->bytes, len); c[len] = 0;` has no NUL check, while `strata_cstr` (FFI) faults on an embedded NUL. A path such as `"secret\0.txt"` passes a `.txt` suffix check in Strata and opens `secret`.
  Where: runtime.c static `cstr` (4180, used by read_file, list_dir, env_var, …), strata_host_run (`strndup`). Source: review core-runtime. Confidence: sure.
- [ ] **(data loss) A read error during `read_file` returns a truncated string as success.** `while ((got = fread(…)) > 0) …; fclose(f); return str_from(buf, len)`. `ferror` is never checked, so an EIO in the middle of a file looks like its end. `malloc` and `realloc` results aren't checked either.
  Where: runtime.c strata_host_read_file, strata_host_read_stdin. Source: review core-runtime. Confidence: sure.
- [ ] **`range` passes `Int`s to an `I64` function: big bounds are read as raw words.** `list_range(lo: I64, hi: I64)` computes `hi - lo` on the encoded words.
  Where: core.list.list_range; codegen `call(mut g, TList(TInt), "strata_range", vs)`; core.builtins `range(a: Int, b: Int)` (no `requires`). Source: review core-runtime. Confidence: likely.
- [ ] **`float_from_bits` makes infinite `Float`s.** `ord` is true for infinities, so `Some(inf)`. core.types says "An operation whose result is not finite fails". Recorded in IDEAS.md "Known gaps": "`float_from_bits` gives infinities (only a NaN is `None`), though a `Float` is never infinite".
  Where: codegen `("float_from_bits", TInt, 1)` (`fcmp ord`); core.builtins `float_from_bits` ("`None` for a NaN"). Source: review core-runtime; ledger G14. Confidence: sure.
- [ ] **`len` on `Taken` and `Filtered` runs a copy, with the side effects of a real run.** `for _ in self { n += 1 }` on a copy. For a `Receiver`, the copy shares the channel, so `rx.take(3).len()` consumes up to 3 messages. A `Mapped` or `Filtered` closure runs twice.
  Where: core.iter Taken.len ("Counted by running a copy"), Filtered.len, FiniteIterator.get/at. Source: review core-runtime. Confidence: likely.
- [ ] **"A builtin without `requires` always returns" is false for several builtins.** The header says "A builtin without `requires` always returns, so contracts see through the functions that call it". `matches` says itself that it fails on an invalid pattern, with no `requires`.
  Where: core.builtins header; `matches` ("An invalid pattern fails"), `sum(xs: List[Float])` (prelude `sum_float`: `total += x` faults on overflow), `range` (big bounds, out of memory), `repeat`, `float_from_bits` (see above). Source: review core-runtime. Confidence: sure.
- [ ] **Hashing and comparing `-0.0` and `0.0` disagree in tables.** `0.0` and `-0.0` compare equal but hash differently. A table of up to `TBL_SMALL` (8) entries is searched in order and finds `-0.0` as `0.0`. A larger one probes by hash and doesn't, so `set([...])` can hold both and `contains` depends on the set's size.
  Where: codegen hash for `TFloat` (`to_slot`: the bits) and runtime.c tbl_search; core.builtins `compare` ("Numbers by value"). Source: review core-runtime. Confidence: likely.
- [ ] **`std.io.printed(console)` on a test's `Console` parameter crashed.** Signal 11 in `std.io.printed`.
  Where: std.io / runtime. Source: ledger R4. Confidence: possible.
- [ ] **Arena tags wrap after 8191 arenas.** `store_i64(d, 32, rem_unsigned(n, 8191) + 1 << 48)`. `arena_made` compares only the tag. Kept arenas live for ever (finding above), so arena number k+8191 has the same tag as a live kept arena.
  Where: core.rt.strata_arena_begin, arena_made. Source: review core-runtime. Confidence: likely.
- [ ] **(memory) `hash_slot` can give a negative slot.** `h := (c >> 4) * 40503` wraps. For `h == INT64_MIN`, `0 - h` is still negative, so `% m` is negative and `large_put`/`large_find` index before the table. This only matters with `--memory-sites`, and the address needed is improbable.
  Where: core.rt.hash_slot. Source: review core-runtime. Confidence: possible.
- [ ] **A cancelled unbuffered `send` leaves its value in the channel.** The value is pushed first, then `while (c->cap == 0 && c->taken < mine && !strata_cancelled()) WAIT(…)`. On cancellation it returns with the item still queued, so a "cancelled" send can still be received.
  Where: runtime.c strata_channel_send. Source: review core-runtime. Confidence: sure.
- [ ] **Timed waits use the wall clock.** `clock_gettime(CLOCK_REALTIME, &now)` feeds `pthread_cond_timedwait`. A wall-clock step (NTP, a sleep/resume) makes sleeps and `select` timers wake early or very late.
  Where: runtime.c wait_until. Source: review core-runtime. Confidence: sure.
- [ ] **(memory) `stack_leave` with an empty stack reads before the counts array.** With `r == 0`, `load_i64(calls, 32776 + (r - 1) * 8)` reads a frame slot and stores depth -1. From then on every stack text is garbage. This needs an unbalanced enter/leave (e.g. after `test_run` resets depth to 0 while an outer frame later leaves).
  Where: core.fault.stack_leave (core.fault.st:45). Source: review core-runtime. Confidence: possible.
- [ ] **`test_run` doesn't restore the outer run's capture flag and leaks a failed test's arenas.** It sets `store_i64(st, 80, 1)`, then `store_i64(st, 80, 0)` instead of restoring the old value, so a nested run (compile-time code inside a test) turns capturing off for the outer one.
  Where: core.run.test_run. Source: review core-runtime. Confidence: sure.
- [ ] **`strata_run_begin` forgets the thread's held senders without restoring them.** `nheld = 0` without saving the old list. After a test on a thread that held senders (a JIT host running compile-time code), that thread can no longer `close` them.
  Where: runtime.c strata_run_begin (`forget_senders()`). Source: review core-runtime. Confidence: possible.
- [ ] **`ord`'s `requires` and its runtime check differ on invalid UTF-8.** For `"\x80"`, `chars` gives one element, so the precondition holds, but `str_ord` sees `char_size` 2 ≠ 1 and faults. Code that verify proved safe faults anyway.
  Where: core.builtins `ord` (`requires len(chars(s)) == 1`) vs core.str.str_ord (`char_size(s, 0) == str_size(s)`). Source: review core-runtime. Confidence: sure.
- [ ] **Float show and parse in C depend on the locale.** `snprintf("%.*e")` and `strtod` follow `LC_NUMERIC`. `shortest` copies every byte except `.` as a digit, so a C library loaded via FFI that calls `setlocale` changes `to_string(1.5)` and `parse_float("1.5")`.
  Where: runtime.c shortest, strata_show_float, parse_float_trimmed (strtod), strata_show_float_fixed (snprintf). Source: review core-runtime. Confidence: possible.
- [ ] **`float_fixed` precision is cast to `int`.** `snprintf(NULL, 0, "%.*f", (int)p, x)`: a `p` past `INT_MAX` wraps, and a negative `p` means "default 6". core.builtins/fmt has no `requires p >= 0`.
  Where: runtime.c strata_show_float_fixed. Source: review core-runtime. Confidence: likely.
- [ ] **Float `%` can return `|b|` itself.** `r + |b|` for a tiny negative `fmod` rounds to `|b|`: `-1e-20 % 1.0` gives `1.0`, not a value below `|b|`. A zero `b` gives NaN with `r < 0.0` false, so NaN is returned (probably caught by the compiled `strata_finite`).
  Where: core.float.float_mod. Source: review core-runtime. Confidence: sure.
- [ ] **Table entries are limited to 2^32.** The entry index has 32 bits, with no check when a table grows past that.
  Where: runtime.c tbl_place / tbl_search (`tag << 32 | (e + 1)`, `(v & 0xFFFFFFFF) - 1`). Source: review core-runtime. Confidence: sure.
- [ ] **`strata_host_run` leaks `argv` on early errors and is macOS-only.** `return NULL` after `cloexec_pipe` fails skips `free(argv[i])`. It uses `posix_spawn_file_actions_addinherit_np` and `addchdir_np` (Apple or very recent glibc), and `F_SETNOSIGPIPE` (macOS).
  Where: runtime.c strata_host_run. Source: review core-runtime. Confidence: sure.
- [ ] **Recursion depth depends on how a program runs.** A native binary's `main` runs on the default stack; the JIT uses a larger one.
  Where: runtime (stack). Source: ledger R6. Confidence: likely.
- [ ] **`_ := e` compiles through the refutable `let`.** It emits an `Int` drop on an `I64`.
  Where: codegen / desugar. Source: ledger R8. Confidence: likely.

## std

- [ ] **JSON with CRLF line endings is refused.** `contains([" ", "\n", "\r", "\t"], get(cs, i) ?? "")`. Here `"\r"` is two characters, so a carriage return (one code point) is never whitespace. `{\r\n"a": 1}` fails with "expected a key".
  Where: std.json.skip_space (std.json.st:1211). Source: review std. Confidence: sure.
- [ ] **`matches()` recompiles its pattern on every call and never uses the DFA.** Codegen emits `call std.regex.match_code(text, pattern)`, which runs `compile(parse(pattern)!)!` and then `is_full_match` (the Pike VM) every time. The doc of `matcher` says "Compiled code builds it while compiling, for a literal pattern", but nothing in codegen calls `matcher` or `run_matcher`.
  Where: std.regex.match_code / full_match; compiler.codegen.regex_matches. Source: review std. Confidence: sure.
- [ ] **Property predicates decode a whole table on every call.** `is_alphabetic(c)` for `c >= 128` runs `contains_code(decode(unicode_tables.bool_property("Alphabetic") ?? ""), c)`. `decode` splits a long string of decimal numbers and parses each one, so one character test costs thousands of `parse_int` calls.
  Where: std.unicode.is_alphabetic, is_white_space, is_alphanumeric, perl_word, perl_space, perl_digit. Source: review std. Confidence: sure.
- [ ] **No timeouts and no connection limit.** `serve` spawns `answer(conn, handler)` for every accepted connection with no bound. The reads (`next_line`, `exact`) wait forever. A client that opens connections and sends nothing (slowloris) holds a task, which is an OS thread (IDEAS, Concurrency step 1), per connection.
  Where: std.http.serve, serve_connection, read_request. Source: review std. Confidence: sure.
- [ ] **`javascript:` URLs, `srcdoc` and `<script>` are allowed.** `AttrName` refuses only names matching `[oO][nN].*`. `href(url)` is `attr("href", url)` with no check. Recorded in IDEAS.md (Web UIs step 1: "Not yet: `javascript:` URLs in `href` are not rejected"), with no fix since.
  Where: std.web.href, src, attr, AttrName, TagName, element. Source: review std; ledger S5. Confidence: sure.
- [ ] **`Real` columns are TEXT, so `CHECK`s and queries compare text.** A field `price: Real where 0 <= it && it <= 1000` gets the column `"price" TEXT … CHECK (0 <= "price" AND "price" <= 1000)`. In SQLite, comparing a TEXT-affinity column with an unaffiliated literal applies TEXT affinity to the literal, so this compares `'5' <= '1000'`, which is false.
  Where: std.orm.sql_type (`Real => "TEXT"`), column_sql, check_sql, Compare for KeyPath. Source: review std. Confidence: likely.
- [ ] **No depth limit.** `[[[[…` recurses once per bracket. Deep input from the network ends the program with a stack overflow, which is a fault, not an `Err` ("errors are results").
  Where: std.json.read_value (recursive). Source: review std. Confidence: likely.
- [ ] **`encode` is quadratic for non-ASCII text.** `out = out + [192 + n / 64] + [128 + n % 64]`. This is reassignment rather than `push`, so each non-ASCII character copies the list built so far. It is used by `std.net.encode_bytes` (`write_bytes`, `read_bytes` defaults), `Files.read_bytes` and std.http, so sending a 1 MB non-ASCII body is O(n²).
  Where: std.utf8.encode. Source: review std. Confidence: likely.
- [ ] **Repeated headers are joined with commas.** `joined := … old + ", " + v`. `Set-Cookie` values can't be joined this way, because `Expires=Mon, 01 …` already holds a comma. On the way out, a `Dict` can't hold two `Set-Cookie` headers at all, so a server can't set two cookies.
  Where: std.http.read_headers; Request/Response `headers: Dict[String, String]`. Source: review std. Confidence: sure.
- [ ] **A client `HEAD` request misreads the response.** `read_response` decides whether there is a body from the status alone (`status < 200 || status == 204 || status == 304`). For `HEAD`, it then waits for `content-length` bytes that never come.
  Where: std.http.read_response / exchange. Source: review std. Confidence: likely.
- [ ] **The path is decoded before it is split.** `/files/a%2Fb` becomes `/files/a/b` before `segments` splits it, so `:name` can never hold a `/`, and `..%2F..` turns into real `..` segments. `segments` also drops empty and `..` segments as given, with no normalisation.
  Where: std.http.read_request (`path: percent_decode(path, false)`), Router.handle, match_path. Source: review std. Confidence: sure.
- [ ] **The client sends the URL target unchecked.** Header values are checked for CR/LF, but `target` is not. A URL holding a space, CR or LF (`http://h/a\r\nX: y`) is written into the request line.
  Where: std.http.parse_url, request_head. Source: review std. Confidence: sure.
- [ ] **Bodies are UTF-8 text only, and have no streaming limit.** Recorded in IDEAS.md (std.http, "bodies are UTF-8 text. Not built: streaming bodies…, binary bodies"). So no images, uploads or gzip. `Body.rest()` reads with no `body_limit`.
  Where: std.http (Request.body, Response.body: String), Body.rest. Source: review std. Confidence: sure.
- [ ] **Foreign keys are on only after `Schema.create`.** `PRAGMA foreign_keys = ON` is sent only by `create`, and the pragma lasts for one connection. A program that opens an existing database file and doesn't call `create` has no `REFERENCES` enforcement.
  Where: std.sqlite.Sqlite.open; std.orm.Schema.create. Source: review std. Confidence: sure.
- [ ] **Invalid UTF-8 or NUL in TEXT columns.** SQLite TEXT may hold any bytes, including embedded NULs and invalid UTF-8 written by another program. They come back through an `extern c` `String` (a C string), so they are cut at the first NUL.
  Where: std.sqlite.Statement.value (`sqlite3_column_text`). Source: review std; ledger R1 (sqlite part). Confidence: possible.
- [ ] **Non-ASCII string literals in refinements are mangled.** `c` is a byte from `byte_at`, and `chr(195)` is `Ã`. So `where it != "é"` becomes `CHECK ("x" <> 'Ã©')`, and `==` against such a literal makes SQLite refuse every valid row.
  Where: std.orm.tokens (`text = Some(t + chr(c))`). Source: review std. Confidence: sure.
- [ ] **A dropped stream is taken as a complete reply.** When `read_event` returns `None` (end of stream), `finished(s.reading.content, s.reading.stop)` is called. With `stop == ""` (no `message_delta` arrived, because the connection broke mid-reply), it returns `Ok(Message {…})`.
  Where: std.agent.step (None branch), finished. Source: review std. Confidence: likely.
- [ ] **The tool schema disagrees with std.json's encoding.** A tuple-shaped union member (`Pixels(Int)`) goes through `_ => schema_of(m, depth)`, then `shaped` → Tuple → `{"type": "array", "prefixItems": …}`. std.json reads it only as `{"type": "Pixels", "value": 3}`, so a model that follows the schema sends input that `decode_as` refuses.
  Where: std.agent.member_schema, shaped. Source: review std. Confidence: likely.
- [ ] **`MemFiles` behaves unlike `Disk`.** `make_dir` is `Ok(())` and records nothing, so `exists(dir)` and `is_dir(dir)` are false after it, and `list(dir)` on an empty folder is `NotFound`. `remove` of a folder is `NotFound`.
  Where: std.fs.MemFiles. Source: review std. Confidence: sure.
- [ ] **A bad exit code reads as success.** `code := parse_int(parts[0]) ?? 0`, and a negative code is also forced to 0. If the host's text isn't a number, the program appears to have succeeded. `parts[0..2]` are indexed with no length check, which faults if the host returns fewer.
  Where: std.process.Launcher.run_with. Source: review std. Confidence: sure.
- [ ] **`SortedSet`/`SortedDict` check an O(n) `count` on every change.** The `ensures count(self.root) == count(old self.root) + …` clauses walk the whole tree. STRATA-ISSUES.md (v034 order book) records "`std.collections.SortedSet` costs O(n) per operation… keeps an O(n) `count` check on every insert and remove".
  Where: std.collections.SortedDict.insert, remove, SortedSet.insert, remove, put, cut, balance, node, join. Source: review std; ledger S6. Confidence: sure.
- [ ] **The derived JSON decoder faults on an integer too large for a `Float` field.** It faults "too large for a Float" instead of returning an error (`std.json.decode_as.for.Summary.10`).
  Where: std.json derive. Source: ledger S1 (#601). Confidence: likely.
- [ ] **A `Bool` option defaulting to `true` can't be turned off.** `std.cli` maps `Bool` to `FlagOpt` and has no `--no-x`.
  Where: std.cli. Source: ledger S4. Confidence: sure.
- [ ] **A dependency's `[native]` C isn't linked into programs that use it.** Only the program's own package's and std's native code is linked.
  Where: build (linking). Source: ledger S7. Confidence: likely.
- [ ] **`derive Python`/TS wrappers don't check a function's `requires`.** Bad arguments fault the host process.
  Where: std derive Python/TS. Source: ledger S11. Confidence: likely.
- [ ] **The number grammar is lenient.** It takes any run of `-+0123456789.eE`, then calls `parse_int` or `Real.parse`. `parse_int("+5")` is `Some(5)` (STRATA-ISSUES P2), and `Real.parse` accepts `.5` (its own doc says so).
  Where: std.json.read_number. Source: review std. Confidence: likely.
- [ ] **Union members are told apart by type-name strings.** `if name == "Int" || name == "Float" { ["number"] } … else { ["object " + name] }`. `Real`, `I32`, `U8` and the other sized ints fall into "object Real" and similar, so `Int | Real` is not reported as overlapping, although both are written as numbers.
  Where: std.json.kinds_of. Source: review std. Confidence: likely.
- [ ] **Decoding a `Distinct` type skips its refinement.** Other arms call `t.check(…)`; this one doesn't. If a `distinct` type can carry its own `where`, it is not checked when read.
  Where: std.json.decode_as `Distinct[y] => Ok(t(decode_as(y, j, path, keys)!))`. Source: review std. Confidence: possible.
- [ ] **No byte-order mark.** A leading U+FEFF is read as a key and fails ("expected a key"). Editors on Windows write one.
  Where: std.toml.parse. Source: review std. Confidence: likely.
- [ ] **`normalize` drops non-ASCII characters.** `else if n < 128 { out += c }` drops other characters without a word, so `\p{Grëek}` looks up `grek`.
  Where: std.unicode.normalize. Source: review std. Confidence: sure.
- [ ] **A server's `HEAD` response sends `content-length: 0`.** `Response { ..r, body: "" }`, and then `write_response` writes `content-length: {byte_len(r.body)}`, which is 0. A HEAD response should carry the length GET would send.
  Where: std.http.serve_connection, write_response. Source: review std. Confidence: sure.
- [ ] **`Body.read_line` is quadratic.** Every call scans `pending` from the start, then copies the rest (`self.pending[i + 1..]`). `more` does `self.pending += bytes.map(…)`. A long SSE stream with many lines per read re-scans the buffer each time.
  Where: std.http.Body.read_line, newline_in. Source: review std. Confidence: sure.
- [ ] **`!` binds differently in SQL.** In Strata `!it.a == it.b` is `(!a) == b`. In SQL, `NOT "a" = "b"` is `NOT (a = b)`, because NOT binds less tightly than `=`. The CHECK can then refuse rows the refinement accepts.
  Where: std.orm.operator (`"!" => "NOT"`). Source: review std. Confidence: likely.
- [ ] **`Cond.not` and NULL.** `NOT ("x" < ?)` is NULL, so false, for a NULL `x`. `x.lt(v).not()` therefore doesn't select rows where `x` is `None`, though in Strata terms "not less than" holds for them.
  Where: std.orm.Cond.not, compared. Source: review std. Confidence: sure.
- [ ] **A streamed `pause_turn` is unbounded.** IDEAS.md says "streamed, with no limit", while `Claude.response` stops after 8. A server that keeps pausing loops forever.
  Where: std.agent.step. Source: review std. Confidence: sure.
- [ ] **`read_event` trims too much.** `trim(byte_slice(line, 5, …))`. SSE removes exactly one leading space after `data:`, and nothing at the end. Harmless for JSON payloads, but wrong for SSE in general.
  Where: std.agent.read_event. Source: review std. Confidence: sure.
- [ ] **Generated Python docstrings can break.** `"""` inside a doc is escaped, but a doc line ending in `"` gives `"""text""""`, which ends the docstring early and leaves a stray `"`. The generated `.py` doesn't parse.
  Where: std.ffi.py_doc. Source: review std. Confidence: likely.
- [ ] **`plain` stops escapes at the wrong place.** An escape is skipped until the first character with case (`to_lower(c) != to_upper(c)`), which includes non-ASCII letters. OSC sequences (`ESC ] 8 ;; url ESC \`, hyperlinks) stop at the first letter of the URL and leave the rest in the output.
  Where: std.term.plain. Source: review std. Confidence: sure.
- [ ] **The ORM doesn't refuse a `REAL` key column.**
  Where: std.orm. Source: ledger S12. Confidence: likely.

## Tools

- [ ] **A new module with many loops spends 10-20 s of its edit on termination's loop-bound searches.** `verify_inputs` now builds verify's inputs from what the selection reaches (`reached_decls`), so an edit of one function with a loop in compiler/ spends about 1 s there, not 3-8 s. For new code nothing is cached, and `analyze_with` searches a bound for each loop the changed functions reach: a copy of lab-kernel's `kernel_tactics` (84 definitions, 22 loop bounds, 36 measures) spends 12-21 s in `verify: termination`, out of 34-45 s. Analysing only what the targets call, not the targets themselves, didn't help there: the loop functions call each other.
  Where: main.verify_program (`analyze_with`), termination's loop bounds. Source: verify-inputs PR. Confidence: sure (`STRATA_TIMINGS=1`: `verify: termination: loop bounds`).
- [ ] **(data loss) `abort` ignores write errors, deletes the backup, and says everything is restored.** `restore_files` uses `mut disk.remove(…) !! ()` and `mut disk.write_bytes(path, bytes) !! ()`. Then `abort_command` runs `mut disk.remove(pending.file_of(root)) !! ()` and prints "every file is as it was before it".
  Where: main.abort_command, main.restore_files. Source: review tools. Confidence: sure.
- [ ] **`strata update` is a test-internal command: it takes a DIR, not names, and always exits 0.** `low_level` makes `update` always the internal step (`… | "update" | … => true`). `update_deps` reads `dir := get(args, 0) ?? "."` and `names := drop(args, 1)`. Errors only `print("error: " + e)`, and `code` stays 0.
  Where: main.command_line (`"update" => update_deps(…)`), main.low_level, main.update_deps. Source: review tools. Confidence: sure.
- [ ] **(data loss) `upgrade` ignores STRATA_HOME: outside the repo it re-stores every reference with no std or core.** `lib := std_imports(disk, "std", decls)` and `core := read_core(disk)`. `read_core` reads `core_files()`, which are `"core/types.strata"` and so on, relative to the current folder.
  Where: main.upgrade_command. Source: review tools. Confidence: likely.
- [ ] **(data loss) The log is written after the files, and a failed append still leaves the change on disk.** `apply_edit` runs `write_modules`, then `disk.append(root + "/" + log_file(), …)` and returns `Err` on failure, which gives exit 1 with the files changed and nothing logged.
  Where: main.apply_edit, main.apply_change, main.done_command. Source: review tools. Confidence: sure.
- [ ] **`git_commit` stages and commits everything under the package folder.** `["add", "-A", "--", ".", ":(exclude).strata"]`, then `["commit", "-q", "-m", message, "--", "."]`. Unrelated work-in-progress files, untracked notes, `.env` files, the strata.lock rewritten by dependency resolution, and C or doc files in the package folder are all committed under the edit's message.
  Where: main.git_commit. Source: review tools. Confidence: sure.
- [ ] **`apply_change` ignores the pending file's write failure and the log's.** `mut disk.write(pending.file_of(o.root), pending.pending_json(saved)) !! ()` and `mut disk.append(…log…) !! ()`. A rename during a pending change can lose its rename record, so the `done` entry has no `renames` and `sync` can't follow it.
  Where: main.apply_change. Source: review tools. Confidence: sure.
- [ ] **`revert` is not logged.** It writes the working tree only (checked, in one step, refused during a pending change), so `strata.log` has no entry for it and `sync` can't follow it; a log entry needs `revert_command` to get a clock.
  Where: main.revert_command. Source: tools-dataloss-edits PR. Confidence: sure.
- [ ] **`holeify` still matches fields and trait methods by short name.** `FieldOf { name }` is a use of any changed record's field of that name, and a method call one of any changed trait method of that name, so an unrelated `x.name` or `x.m()` in a broken definition can become a hole (written; `abort` puts it back).
  Where: pending.uses. Source: tools-dataloss-edits PR. Confidence: sure.
- [ ] **Renames reach patterns only inside `match` arms.** `patterns_renamed` rewrites only `MatchExpr` arms. Patterns in `Let` (destructuring `Point { x, y } := p`), `For`, `CompFor`, lambda parameter patterns (IDEAS "Patterns as parameters") and `CondPat` are not renamed.
  Where: refactor.patterns_renamed, refactor.substituted. Source: review tools. Confidence: likely.
- [ ] **(data loss) A malformed pending.json makes `abort` delete module files, or forget the change.** `parse_pending` returns `Some` for any JSON object. A missing `before` gives `[]`, and a bad entry gives `("", "")`. `unhex` maps a bad digit to 0 (`index_of(…) ?? 0`).
  Where: pending.parse_pending, main.restore_files. Source: review tools. Confidence: likely.
- [ ] **merge-file stores core files qualified, breaking core.** Neither path has the `of_core` case `edit_file` has (`codebase.merge_parsed`).
  Where: main.resolve_command → main.encode_at; main.merge_file_command → kept_decls (`stored(others + mine, std, core)`). Source: review tools; ledger T7. Confidence: sure.
- [ ] **`sync` checks every package in the repository, core included, with a reduced check.** It runs `discover` and then, for every package and script under the git top, `check_errors_of(program, std_imports(…), core, values)`. That includes `compiler/`, `core/` (checked like any package, though core "checks only as the runtime's compilation does"), `std/` and every example.
  Where: main.merged_checks. Source: review tools. Confidence: likely.
- [ ] **Git URLs and revs from strata.toml reach git without `--`.** `["clone", "--bare", "--quiet", url, repo]` and `["--git-dir", repo, "fetch", "--quiet", …, url, "+refs/heads/*:refs/heads/*"]` have no `--`. `rev-parse --verify --quiet asked^{commit}` uses a `rev` value taken from the manifest.
  Where: deps.bare_repo, deps.checkout, deps.fetch_git. Source: review tools. Confidence: possible.
- [ ] **Two dependencies with the same name: the first wins silently.** `if len(path) > 0 && !any(out, f -> f.name == at.name) { mut out.push(at) }`. A diamond in which two packages depend on different versions (or URLs) of `util` keeps whichever came first, with no conflict reported, and both lock pins are written.
  Where: deps.visit, deps.fetched_visit. Source: review tools. Confidence: sure.
- [ ] **`strata parse FILE` exits 0 when the file doesn't parse.** `main.front_command` ends with `parse_one(disk, file)` then `0`; the package branch returns 0 too.
  Where: main.front_command. Source: ledger T1 (#577). Confidence: sure.
- [ ] **Selectors split on `,`, so impls with a comma in their name can't be named.** `impl Json for Dict[String, V]` can't be selected in `view` or `verify` (`views.select` splits on `,`).
  Where: views.select (compiler.views.st:46, 641); main.st:22846. Source: ledger T2 (#448). Confidence: sure.
- [ ] **Round-tripping `core/builtins` through a non-core file loses overloads.**
  Where: edit / view. Source: ledger T8. Confidence: likely.
- [ ] **`strata upgrade` writes core's files without their dependency lists.** Fixed only on unmerged #562.
  Where: upgrade. Source: ledger T10. Confidence: likely.
- [ ] **(data loss) `--file std edit std.regex` rewrote all std files and lost the module name.** It also auto-committed; a file-mode edit stores references comptime's codegen can't compile. Recorded against an old seed.
  Where: edit (file mode). Source: ledger T12 (#421). Confidence: possible.
- [ ] **Adding an `all` overload for `Dict` by `edit` replaced the list one.** `edit` replaces a definition by name, so a new overload overwrote the existing one.
  Where: edit (overloads). Source: ledger V15 (#488, #529). Confidence: likely.
- [ ] **`strata init` in `.` names the package `.`.** `name := match last_slash(dir) { … None => dir }`. Called with `process.current_dir()` this is fine, but a relative `dir` without a slash (`"."`, `"foo"`) gives `name = "."` or the bare argument.
  Where: codebase.init. Source: review tools. Confidence: possible.
- [ ] **`strata --file F add MISSING` creates an empty module and exits 0.** `text := if cmd == "add" { disk.read(a.module ?? "") !! "" }`. A missing or unreadable FILE gives `""`, which parses to `[]`. If `path` exists, `new == old` and it prints "no changes", exit 0.
  Where: main.edit_file. Source: review tools. Confidence: sure.
- [ ] **Exit codes are inconsistent, and `fail_with`'s doc says 1 but it returns 2.** `/// … Returns exit code 1.` but the body ends in `2`. An edit rejected by the checker exits 1 (`Err(_) => 1`), a parse error in the same command exits 2 (`fail_with`), and a failed log append exits 1.
  Where: main.fail_with, main.edit_command, main.apply_edit. Source: review tools. Confidence: sure.
- [ ] **`piped_input` with `--rm` ignores slow piped input.** `if console.interactive() || driver_stdin_idle().to_int() == 1 { return if removing { Ok("") } … }`. `driver_stdin_idle` is "silent for 100 ms". `slow_producer | strata edit --rm X` then applies only the removal and drops the piped definitions without an error.
  Where: main.piped_input. Source: review tools. Confidence: possible.
- [ ] **`auto_commit = false` is found by substring, not by reading the TOML.** `index_of(manifest, "auto_commit = false") is Some`. `auto_commit=false` or extra spacing doesn't match. A comment containing the text does match, which turns commits off.
  Where: main.git_commit. Source: review tools. Confidence: sure.
- [ ] **A failed git commit is reported but the command exits 0.** `mut console.eprint(f"the change is made, but git commit failed: …")`, and no result is returned, so `edit`, `rename` and `done` all exit 0.
  Where: main.git_commit (returns `()`), callers apply_edit, apply_change, done_command. Source: review tools. Confidence: sure.
- [ ] **`rename` accepts keywords and builtin names as the new name.** `is_name` accepts any run of letters, digits and `_`. `strata rename f match` produces `fn match(...)`, which `view` prints and `parse` can't read back. Whether the checker catches it depends on resolve.
  Where: refactor.is_name. Source: review tools. Confidence: possible.
- [ ] **`move` leaves a type's derives and impls behind.** Only the definition itself moves (`Decl { ..y, module: target }`). The `derive Json for Point` and `impl Show for Point` declarations stay in the old module, and nothing says so.
  Where: refactor.move_def, refactor.moved_names. Source: review tools. Confidence: possible.
- [ ] **`done --keep-holes` can produce duplicate hole names.** Holes are numbered from `len(p.holes) + 1` within a pending change and renamed `pending_k` → `todo_k` at `done`. A second pending change starts again at `pending_1`, so the next `done --keep-holes` makes a second `?todo_1` next to the first.
  Where: pending.keep_holes, main.apply_edit (`first`). Source: review tools. Confidence: likely.
- [ ] **merge-file's scratch folder is named by string lengths.** `tmp := (TMPDIR ?? "/tmp") + "/strata-merge-" + f"{byte_len(path)}-{byte_len(o)}"`. Two merges with paths of the same length (any two concurrent repositories or worktrees) share the folder, and `line_merge` writes `ours`/`base`/`theirs` there and then runs `remove_all(tmp)`.
  Where: main.merge_file_command (`tmp`), merge.line_merge. Source: review tools. Confidence: sure.
- [ ] **Rename detection in merge is a similarity guess, and its errors are swallowed.** A definition deleted on one side and a similar one added (`sim >= 0.6 && followed`) is treated as a rename and applied to the base and the other side. `o = refactor.rename_all(o, …) !! o` hides a failure on the base.
  Where: merge.renames, merge.merge. Source: review tools. Confidence: possible.
- [ ] **`sync` ignores git merge's exit code.** `ran := mut launcher.run_with("git", […, "merge", "--no-ff", "--no-commit", target], …)`, then `match ran { Err(e) => …, Ok(_) => () }`.
  Where: main.sync_command. Source: review tools. Confidence: sure.
- [ ] **`setup-git` quotes the executable with single quotes.** `driver := f"'{exe}' merge-file %O %A %B %P"`. A path containing `'` breaks the driver. `setup-git` exits 0 when not in a git repository (`Err(_) => "."`), and it only prints a hint about `.gitattributes`.
  Where: main.setup_git_command, main.sync_command. Source: review tools. Confidence: sure.
- [ ] **Cache folders for git dependencies race between processes.** There is no lock. Two strata runs (parallel `tools/check` jobs) can both `git clone --bare` into the same `repo`. A clone that was interrupted leaves a `repo` folder that `disk.exists(repo)` takes as present.
  Where: deps.bare_repo, deps.checkout. Source: review tools. Confidence: likely.
- [ ] **`module_files` stops at depth 64 without a word.** `for _ in 0..64 { … }`. Deeper module files are left out of every package operation, including `upgrade` and `restore_files`, which deletes module files missing from `before`.
  Where: deps.module_files. Source: review tools. Confidence: sure.
- [ ] **(data loss) `upgrade` returns at the first write error after rewriting part of the tree.** `return fail_with(…)` inside the loop, after earlier files were already renamed. Its temp name `target + "-tmp"` (`x.strata-tmp`) is the name `codebase.write_file` uses, so a concurrent edit collides with it.
  Where: main.upgrade_command. Source: review tools. Confidence: sure.
- [ ] **(data loss) `pack` overwrites any path, including a module file of the package.** `disk.write_bytes(target, bytes)` with no check that `target` isn't one of the package's own module files. `strata pack main.strata` in a package replaces its root module with a bundle of the whole program.
  Where: main.pack_command. Source: review tools. Confidence: likely.
- [ ] **The shipped std library is preferred without checking it matches std's sources.** When `home/std/names`, `std.o` and `libstd.dylib` exist, they are returned at once (`return Some(shipped)`), before any key. In a checkout where a release's precompiled library was copied into `std/`, edits to std are ignored by JIT runs.
  Where: main.std_library. Source: review tools. Confidence: possible.
- [ ] **Unknown long options are accepted by the global parser.** `else if starts_with(a, "--") { mut flags.insert(a, …) }`. Any typo is kept as a flag and ignored by commands that only test `given(line, …)`, for example `strata check --with-dep`.
  Where: driver.read_command_line, `strata_command` users. Source: review tools. Confidence: likely.
- [ ] **(data loss) `skill install` overwrites the files without asking.** `disk.write(path, text)` replaces `SKILL.md` in every agent's skills folder, so a user's edits are lost. The skill text is hard-coded strings (its own copy of the CLI's usage), which can drift from `driver.usage_text`.
  Where: skill.skill_command. Source: review tools. Confidence: sure.
- [ ] **`edit` can't delete a detached `//` section banner.**
  Where: edit. Source: ledger T4. Confidence: likely.
- [ ] **`from m use f, g` with `--rm 'use m'` is refused.** `edit` reports "both given and removed".
  Where: edit. Source: ledger T5. Confidence: likely.
- [ ] **`strata diff` and `tour --check` report untouched `noalloc` functions as changed.** About 35 functions in untouched core modules show "signature changed".
  Where: diff. Source: ledger T9. Confidence: likely.
- [ ] **The merge driver re-encodes a definition with different bytes.** `smt.infer_invariants` came out with the same code and different bytes.
  Where: merge-file. Source: ledger T11. Confidence: likely.
- [ ] **An edit turns a compatibly broken use in an erroring definition into a hole.**
  Where: edit (pending changes). Source: ledger T15. Confidence: likely.
- [ ] **A run killed by SIGKILL leaves its `smt/PID` folder.**
  Where: verify scratch. Source: ledger T23. Confidence: likely.
- [ ] **Calls in contracts are not marked before a rename.** `mark_method_calls` doesn't infer calls in `requires`, `ensures`, invariants and models, so a method rename there follows the "no other method of that name" rule. `merge.follow_rename` and `diff` call `rename_all` without marking calls at all. A missed call makes the post-rename check fail, so nothing is changed silently.
  Where: check.mark_method_calls, rename.rename_all, merge.follow_rename, diff. Source: PR #626. Confidence: sure.
- [ ] **Read-only commands skip a module they can't decode.** lint, unused, `codegen_with` and settings go through `package_decls` / `module_decls`, print the error naming the file, and go on without that module.
  Where: main.package_decls, main.module_decls and their callers. Source: PR #626. Confidence: sure.
- [ ] **An edit is accepted when the codebase already has a module-level error.** An error such as "`helper` is both a function and a method of `util.T`" hides the edit's own privacy error.
  Where: edit (check of the edited package). Source: PR #651. Confidence: sure.
- [ ] **Adding a definition that calls existing ones moves them below it.** `edit shop` with `fn c() = a() + 1` puts the callees, with the module's top comment, below the new definition. `strata new` does the same.
  Where: edit (order of stored definitions). Source: PR #641. Confidence: sure.
- [ ] **Inserting an `else if` shows edits to every later branch.** The tree nests the chain, so diff pairs branches by position. Lining chains up as lists would fix it.
  Where: tree.changes, diff. Source: PR #630. Confidence: sure.
- [ ] **`tree.changes` compares variant and method ids.** Only the definition's own id is skipped, so versions from the log report variants as `edited` in `--json`'s `nodes`. `hunks` hides them because they print the same.
  Where: tree.changes. Source: PR #630. Confidence: sure.
- [ ] **`strata tour` says "nothing else needs re-checking" for every body change.** A body a contract may see through or unfold changes its callers' proofs; `edit` says so.
  Where: diff (tour's change labels). Source: PR #663. Confidence: sure.
- [ ] **`verify 'module:main'` matches nothing for the root module.** `cd compiler && strata verify 'module:main' --termination` prints "error: `module:main` names no definition and no module" and exits 0.
  Where: views (selectors). Source: PR #665. Confidence: sure.
- [ ] **A selection error in `verify` exits 0.** A selector that names nothing prints its error, then `verify` exits 0.
  Where: main.verify_program (selection). Source: PR #665. Confidence: sure.
- [ ] **The claim lint is tested by examples only.** `compiler/claims` has `test` blocks and no contract. In a sample of 25 flags, 3 were false positives; rule lists and term definitions are the usual kind.
  Where: claims. Source: PR #622. Confidence: sure.
- [ ] **`strata edit`'s own commits carry no attribution trailer.** The auto-commit's message can't take `Co-Authored-By` or other trailers.
  Where: main.git_commit. Source: PR #646. Confidence: sure.
- [ ] **Nothing checks that compiler/ builds with the latest release.** Code that needs a newer checker builds on main's own build but fails stage 1 of `tools/bootstrap.sh` from the release (#668's `??` after `is Some`, rewritten in #672). Build one stage with the latest release before merging a change to compiler/.
  Where: tools/bootstrap.sh. Source: PR #668, PR #672. Confidence: sure.
- [ ] **A whole-std `verify --require proven` takes more than 300 s.**
  Where: verify. Source: PR #627. Confidence: sure.
- [ ] **Plain `verify` spends 6-9 s on compiler/ checking the program first.** It runs before the selection is used.
  Where: main.verify_program (`verify: check the program`). Source: PR #667. Confidence: sure.
- [ ] **`docs/std` is behind std.** Regenerating it rewrites 25 pages.
  Where: docs/std. Source: PR #623. Confidence: possible.

## Test cases failing on main

- [ ] **`verify --termination` of a whole compiler/ goes past 300 s.** Per module (`verify 'M.*' --termination`) `main`, `markdown` and `tour` go past 300 s each, on 8a5c58aa too.
  Where: termination (per-run analysis of the whole package). Source: termination-modelling PR. Confidence: sure.

- [ ] **std fails `--require proven`.** `std.orm.Schema.create` and `std.orm.Key.load` fail the policy on main.
  Where: std.orm. Source: ledger tests table (#513, #530). Confidence: sure (checked on ecf3c860).
- [ ] **`build-std_array` fails.** Its `verify` proves `chain` with 13 checks and `mean_of` with 6, where the case wants 6 and 3. On 61492e1d too.
  Where: tests/cases (verify check counts). Source: build-fails-on-errors PR. Confidence: sure.
- [ ] **`build-copies` times out under load.** Its `containers` step is quadratic (see Code generation).
  Where: tests/cases. Source: ledger tests table (#492, #513, #573, #609). Confidence: likely.
- [ ] **`smt-a_call_written_twice_is_one_list`, `smt-long_terms_read_through_a_let` and `smt-the_same_value_is_one_term` hit their 60 s limit.** On ecf3c860 too, run 4 at a time.
  Where: tests/cases, the SMT terms work (#616). Source: main-cases PR. Confidence: sure.
- [ ] **`cli-smt_proves_sums_over_lists` fails.** `bounded` (`ensures it <= 10 * len(xs)`, a sum of elements under 10) is not proven: "a possible counterexample it could not express". On ecf3c860 too.
  Where: compiler/smt, sums over lists. Source: main-cases PR. Confidence: sure.
- [ ] **`cli-structural_records` fails.** A structural record's refined field is lost: `total_of(x: { total: Cents, .. }) -> Cents` gets the counterexample `x = { total: -1 }`, and `bigger` proves 2 checks instead of 3. On ecf3c860 too.
  Where: compiler/smt, refinements of structural record fields. Source: main-cases PR. Confidence: sure.

## From the old IDEAS.md, to reconcile

Problems IDEAS.md recorded before it was split (origin/main 1799da47), not yet checked against the entries above or against the code. Lines marked (issue) are design problems for ISSUES.md. Move each into its section, or drop it if fixed.


## Verify and the prover
- [Open questions] (issue) `extern_reach` is over-approximate by method name: `std.orm.read_column.for.Todo` is "not run" because `Row.value` is taken for `Statement.value`; typed method calls in std's code would remove it.
- [Open questions] (issue) examples/tasks needed proof-shaped code: an opaque predicate over a value merged at a branch join compares arrays (timeouts); an `assert` with no `by` sees a helper only by its contract; a call made only by its contract is a new value at each use; refinement validity is stated per term, not once per type; encode time dominates.
- [Open questions] Generic callee type arguments come only from a `let` annotation: `mut q := empty_queue(policy)` stays at the generic sort for the prover (also under "A script the solver rejects").
- [Open questions] An `extern c` result can't be refined (`-> I32 where it >= 0` "doesn't cross to C").
- [Open questions] `frequencies`' loop doesn't carry "every count is Positive" (a dict's values through a loop) nor `set(ws)` facts.
- [Open questions] `wc` has an open `?unreachable` hole in derived code (`std.cli.value.for.Options`).
- [Open questions] cvc5 still runs as a process per script.
- [Open questions for the user] Graph moves needing unique ids stay unknown (`set` modelled by size); more than one list or dict on a path isn't followed.
- [Open questions for the user] Trait-level `assumed invariant` is not built.
- [Open questions for the user] A dict's size isn't derived from a set of distinct members.
- [Open questions for the user] The checker doesn't type an `ensures` beyond its shape: `ensures n == "x"` on an `Int` passes `check`.
- [Proven ensures narrow the result] std types' methods (`SortedDict.get`) and built-ins (`get`, `first`) have no facts, so matches on them are covered by neither checker nor `exhaustive`; a field read of a variable bound from an undecided call is an error; a trait's default method isn't narrowed.
- [std.regex.samples matches its pattern] The law is unprovable: the invariant fails when the predicate's receiver is a sum-typed value (element facts dropped), and `matcher`/`run_matcher` aren't shown to terminate. WIP on branch `regex-samples-law` (714f5aa).
- [Written proofs] `excluding` with `List` keys of a generic key type and `unique(xs + [x])` aren't modelled; the level-0 map says `fn` for a lemma; `view` of a module hides lemmas with no marker.
- [String search with memchr] Encode path left: `piece`'s key lookups per line (index terms by key), `cone_in`, `keys_in`, `word_spans`, `command_heads`.
- [A script the solver rejects] Termination's and the invariant search's rejected queries count as not proven but their message isn't shown.
- [A for keeps its ensures] List-model limits: `ensures it == xs.map(f)` (whole-list equality) and counting through `filter` aren't proven; `ensures` that call functions leave a `for` unindexed (query size).
- [Keys of tables] A trait model named `keys` is read as the builtin; the solver has no sets of `List`s nor of unions holding `Float`; contracts nest only 4 deep; an insert of a missing row is "possible counterexample", not confirmed; a concrete `Connection` doesn't carry its empty model through `create` and inserts; `delete`/`all` contracts aren't found (two traits declare those names).
- [Shared laws] A caller relying on the model across calls gets "not proven", no counterexample.
- [Trait models] Generic traits' mocks; models of std's traits when a program is desugared without std.
- [Frames, key sets and uniqueness] (issue) A loop's queries carry the whole function's context (2-4 MB, no cone across a pass), so loop proofs over dict models time out.
- [Frames, key sets and uniqueness] `old` inside a lambda of a trait `ensures` makes the method "skipped: `old` outside an `ensures`".
- [Frames, key sets and uniqueness] `all(d, f)`/`any(d, f)` overloads on `Dict`/`Set` not added.
- [Helpers in contracts as SMT definitions] Mutually recursive helpers aren't seen through.
- [Nested quantifiers by polarity] A witness moved by a concatenation in front (`a + b`) isn't found; `drop_first` is neither proven nor refuted; helper calls in a body (not a contract) are known by contract only.
- [Facts about every element, per instance] With `all` facts and sets made of lists stated at the elements and keys the code reads (smt `all_of`, `list_set`), 17 functions proven with `STRATA_QUANTIFIERS=forall` are reported as needing a lemma. Equality of sets made of lists, where z3 compares at a key no term names (examples/tasks `memory.ids_none`, `id_set_step`, `ids_step`, `got_step`, `done_step`, `frame_ids`, `frame_got`, `outside_ids`, `outside_done`); an `all` whose element fact is an `any` (`new.taken`, `memory.listed_push`); facts over slices and sets of ids through `with` and `unique` (`memory.some_new_here`, `some_new_kept`, `settled_insert`, `unlocked_insert`, `locked_insert`, `unique_push`), instances not traced.
- [Dicts and sets in the solver] `keys`/`values` known by length only; `map_values` not modelled; a dict nested in a datatype isn't shown in a counterexample.
- [Verification: SMT] `to_upper`/`to_lower`/`is_alpha`, `trim`, `split`, `lines`, `repeat`, `reverse` left unknown to the prover.
- [Tool fixes] A match on what the checker can't type from syntax (`match p.op`) is found only by `verify`; tests compiled bare aren't run.
- [Termination and helpers] A lambda's call of another recursive helper is seen through at each element.
- [Proven checks leave the compiled code] Not yet for trait methods (compiled names differ from verify's labels).
- [Loops and invariants] A non-empty list can't be shown as the solver's counterexample; nested quantified facts with strings time out; `lex.lex` and `main.verify_file` searches exceed 2M.
- [Values made as they are read] A refinement calling a function with a `requires` is evaluated without the call site's path condition.
- [Values as SMT datatypes] Values don't keep their facts through loops and `mut` calls (refreshed to their type's guarantees).
- [Match coverage with the SMT solver] No obligation per arm that it can match (unreachable arms the checker can't see).
- [Unions, variants as types] List length conditions are only "empty" or "not empty".
- [Failures] A list from a dict gets a new length at each use, so `if len(xs) > 0 { xs[0] }` on it isn't proven.
- [Running while verifying] `verify_file` called repeatedly in one process once reported a counterexample as proven; cause not found.
- [Known gaps] `verify` behind the old Rust one: no solver-made inputs when random ones rarely pass; generic functions not fuzzed.
- [Known gaps] A verify run naming only some functions doesn't analyse their callees' termination in other modules.
- [Known gaps] `len(s) == 0` doesn't make a set empty in the encoding; a function returning `Set[TaskId]` isn't seen through (`list_set` drops element refinements).
- [Known gaps] Lab-fleet verify bugs: value under `x!` lost for a capability call inside std; a nine-arm type match over `std.agent.Chunk` went past 8 GB; `verify` of `codegen.computed_value` and of many functions at once loops on LLVM pass-registry errors; fuzzing `std.array` passes 300 s (string concatenation in fuzz cases); `I64` counter loops "may not terminate".
- [Known gaps] `markdown.inline` and `main.doc_command` not verified (time limit).
- [Known limitations] (issue) Prover coverage: positions lost through `filter` and `sort`; floats, dicts and sets partly modelled.
- [Known limitations] (issue) Verification is slow and little is cached (see the speed idea).

## Checker, codegen and runtime
- [Iterators and FiniteIterator] A nested comprehension whose inner source is an iterator needs a lazy `flat_map`; a user's own `it.next()` loop isn't shown to end.
- [Builtins viewable] `mut console.print(n)` with `n: Int` passes `check` and crashes when run.
- [Builtins viewable] `reverse` and `repeat` go through `join` (O(n log n)); `range(a, b)` outside a `for` builds the list.
- [Web UIs] A `_ =>` arm of a union match doesn't narrow in compiled code; widening a union keeps tags (`List[Int] | List[String]` read under the wrong tag); the checker refuses `List[B[M]] =>` for a local generic record; a handler number can be stale if the page changes before its turn.
- [Web UIs] A user module named `web` with `element`/`children` is shown as markup.
- [strata repl] A function named `at` captured indexing (probably fixed by desugar-hygiene #617); `process.args()` under `--allow process` is the REPL's command line.
- [Keys of tables] A user function named `insert` breaks `mut self.all.insert(..)` in an impl; `Set[KeyValue] {}` doesn't type-check; `with(s, ("p", 7))` on a `Set[(String, KeyValue)]` crashed in a cross-check.
- [Spreading a record] `eval` and `comptime` code don't write spreads out; no spreading into built-ins.
- [Structural records] Compiled code doesn't check refinements inside a generic record's type arguments (`Box[Cents]`).
- [Unused values are errors] `build --no-verify` prints no warnings; checker errors labelled with the wrong module (`binary.stmt` for `eval.stmt`).
- [Unused definitions] Methods of an inherent `impl` never called aren't reported.
- [Integers and floats] A literal in a `Set {}`/`Dict {}` literal or a generic call's argument without a hint stays an `Int`.
- [Foreign functions] `x := setting(k) ?? d` with `k` unset compiles to an undefined LLVM value; blobs don't cross `extern c`; a dependency's `[native]` C isn't linked.
- [Derives written on the declaration] `stringify(x.to_json())` type-checks though `to_json` returns a `Result`, and crashes.
- [Derives on functions] `verify` of examples/wasm reports `std.json.decode_as.for.Summary.10` refuted (decoder crashes on an integer too large for a Float); a release can't target wasm.
- [Derived traits, comptime] Reading the i-th element when `i` is known only at compile time; "more than one `List` field" not reported; `setting(..)` reads no strata.toml.
- [The strata command in Strata] The checker misses some errors once an import fails; verifying `codegen.read_slot` crashes (raw reads at arbitrary addresses).
- [Long-term goal: self-hosting] (old notes, may be stale) A list of `Option[Int]` and `Option[Float]` makes LLVM refuse the IR; `verify` stack-overflowed on nested closures over lists; `strata --file F edit` accepts a `comptime` `check` refuses; regex captures and `find` not supported by the VM.
- [Concurrency] O(threads) look per blocking wait; `compiler/check` doesn't look inside `select`/`race`; `close` on every path not checked; `Task` not in written types.
- [Error handling, strings and mutation] In-place updates not for paths deeper than one field, sums with several variants, or a `mut` field.
- [Mutation at the call site] (issue) A `mut` argument is decided to be passed by reference without overlap, but each goes through its own slot, copied in and out.
- [Memory in high-performance code] `_ := e` compiles through the refutable `let`; `compiler check core/rt.strata` reports core.rt's statics as unknown variables; the call stack and fault raising are still in C (open question).
- [Memory: scoped allocation from roots] A miscompile found with regions forced on is not fixed.
- [Memory: static uniqueness] Body-walking functions crashed the verifier at main b5ab99e1 (`smt.pass_walk`).
- [A mut variable read by another argument] `mut v.m(…)` reading `v` in another argument still finds it shared.
- [Desugared code calls the built-ins] compiler/smt's loop rewriting names (`$for_xs`, `take`, `len`) and check's/codegen's internal `to_list`/`real_of` aren't hygienic.
- [A flaky dup of a freed object] `unique.proven_sites`' arena workaround can now go.
- [Memory: small values] A union holding a tuple returned by a call prints its first element.
- [Memory: fixes by per-site counters] The empty list literal `[]` is allocated each time.
- [Memory: per-site counters] core.rt's and runtime.c's own operations, arena blocks, dups through trait tables aren't counted by site.
- [Memory: where memory goes] Live objects by type, not just by site.
- [std.collections] A lambda in a record field with no type wanted at all gets `()` parameters and LLVM rejects the IR.
- [Floats in the prover] The section's "Left, in order" list is not carried here: check it in git.
- [Known gaps] `float_from_bits` gives infinities; `Files.read`/`Console.read_line` don't check UTF-8; `d[k]` not accepted; a `_` type left in codegen makes `==` and hashing skip that part.
- [Known gaps] Lab-fleet checker bugs: `"\r"` is a backslash and `r`; a trait and a type with one name pass check but codegen fails; mixed-union narrowing on verify's path; unknown types in a definition stored before its import aren't reported; non-exhaustive errors on the wrong function after a rejected edit; errors labelled by short name; a `List` and a `String` overload let `xs["a"]` pass (fixed on std-math only).
- [Known gaps] Lab-fleet codegen bugs: `Ok(())` pattern doesn't compile; `Shop.line(s, x)` doesn't compile; a `U64` match with literal `0` crashed; `parallel.map` with allocating functions crashed (cause found, fixed); producer/consumer programs grow without bound (per-thread free lists); every std-linked program counts as supervising; integer literals >= 2^62 re-parsed each time; the JIT never frees modules.
- [Known gaps] Messages: an unknown error for an unused/typo `from std.io use Console`; `type Answer = Some(Int) | None` says "unknown type None".

## Tools
- [Removing or adding variants in a pending change] `let`/`for` patterns naming a removed variant still reject the edit.
- [Pending changes] One pending change per package; a use the edit broke compatibly is also turned into a hole.
- [Storage: the tree, in binary] The header hashes the stored bytes, the log and verdicts the written form; a module shown alone can't tell apart names from two `*` imports.
- [Running while verifying] Hot swap misses lambdas, `main`, inlined functions and generic instances only the proven module makes.
- [Speed] `strata --file std edit std.regex` auto-commits and rewrites all std files with the module name lost; `--file std edit` stores std modules under local names; the open-holes count after an edit covers only changed definitions; `desugar(lib)` runs twice per check; core.float's `float_show`/`float_fixed`/`float_parse` are unused; cancel points: methods by name (`Fixed.take` checks because `Line.take` waits), supervised programs keep every check.
- [Generated docs] Pages of modules that are gone aren't removed; `std.regex` has no module doc.
- [PR tours] A fold inside an open left-out change toggles only when first on screen; a diff fold holds one level; doc diffs show as one line.
- [Docs in the terminal] `///` lines lose leading spaces on `edit` (indented examples become prose); a colored span wrapped keeps its color on the indentation; code in a heading isn't colored; core modules list no builtins.
- [Builtins viewable] `refs` doesn't find a program's uses of a builtin.
- [Web UIs] `strata parse FILE` exits 0 when the file doesn't parse.
- [view in reading order] Header dependency lists go stale; `views.depth_first`'s worklist isn't shown to terminate.
- [strata diff in seconds] About 2 s of a diff is outside it (std, hashing the tree, git); log-mode verdict lookups encode every definition.
- [Diff and three-way merge on trees] `strata resolve core/rt.strata` stores core calls qualified, so core.rt's build drops every function.
- [Written proofs] Round-trips need `--proofs`; CLAUDE.md's round-trip line doesn't say so.
- [Keys of tables] Header deps of an unchanged module are kept stale on re-encode; `strata --file std view MODULE` names no module.
- [Structural records] Highlight queries lack a capture for `rest`; VS Code's grammar isn't updated for records or spreads.
- [Defaults for record fields] A `Bool` option defaulting to `true` can't be turned off (`--no-x`).
- [Web UIs] `javascript:` URLs in `href` aren't rejected.
- [Doc comments on union members] A type inlined into a union keeps its doc out.
- [Git, diffs and review] A refused rename follow leaves references to the old name for `sync`'s check.
- [Packages, modules and dependencies] (issue) Dependencies by git URL: fetching, `strata.lock` and `strata update` were not built per this file (check against compiler/deps).
- [Packages] `run` subprocesses: no standard input, environment changes, streaming output or timeout.
- [A release with nothing to install] The compiler finds its home from the current folder or `STRATA_HOME`; a killed run leaves `smt/PID`.
- [The tools' output] `std.io.printed(console)` in a test crashed with signal 11.
- [Networking] Two tasks using copies of one connection aren't synchronized; a root-module local named like a rewritten function is rewritten too.
- [Known gaps] `--json` missing on `view`, `render`, `interface`, `status`, `done`, `test`, `run`.
- [Known gaps] Lab-fleet tool bugs: `strata --file core check` never type-checks core; round-tripping `core/builtins` through a non-core file loses overloads; `diff`/`tour --check` report ~35 `noalloc` core functions as changed; `upgrade` writes core without dependency lists (fixed on stored-core only); the merge driver re-encoded `smt.infer_invariants` with different bytes; `edit` refuses `from m use f` with `--rm 'use m'`; `edit` can't delete a detached `//` banner; selectors split on `,` so `impl Json for Dict[String, V]` can't be named; `parse.debug_quoted` replaces `"\r"` with itself; `declarations.Names.open` is always false; unused prelude dict/set functions.
- [Known gaps] Speed and memory: `view --graph` past 4.5 GB; long list literals need 5 GB at 20,000 elements; `containers` in `build-copies` quadratic; `std.json.parse` makes one-character strings (65% of peak on 8 MB); `check` of compiler keeps 112 MB read by `read_bytes`.
- [Known gaps] Failing cases on main: `cli-modules_contracts_and_files` (view order).
- The format trials' "Found, not fixed" items (stored core, stable ids, operators, the trio's merge) are listed in REBUILD.md's appendix, "The format trials as recorded in the lab": to reconcile here.
