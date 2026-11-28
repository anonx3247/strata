# Laws checked by the dependent kernel

The dependent kernel (`compiler/kernel_dependent`) is a type checker for a small type theory. It has Π types, universes `Type : Type1 : …`, equality with its eliminator `J`, inductive types (mutual too), `Int` literals that compute, and built-in rules. A law is a definition whose type is its statement, and the kernel checks the proof term. Everything that builds the terms is untrusted.

There are two ways to give it laws:

1. `strata kernel check --dependent FILE…` takes laws written as in docs/kernel-laws.md, with the same statements, steps and goal files. The HOL kernel proves each law, and its proof is replayed as a term for the dependent kernel to check.
2. `strata kernel dependent FILE` takes laws for every record (`lemma l[T: Record]`). These are stated once and proven once, then hold at each record of the module with no proof of their own.

## 1. Laws as in docs/kernel-laws.md

Write the module exactly as docs/kernel-laws.md says: the same statements, the same proof steps (`match`, `if`, `simplify(…)`, `unfold`, `assert … by`, `calc`, `witness`, `arithmetic()`, `auto()`, …), and induction as a recursive call. Then run:

```
strata kernel check --dependent tests/kernel/laws/lists.strata
```

The usual report comes first. Then there is one line for each proven law:

```
dependent: proven append_nil (11.3 ms, a term of 10383 nodes)
dependent: 4 checked (61.3 ms, the base 1.4 ms)
```

A line `dependent: failed NAME: …` means the term didn't check. Report it as a bug: every HOL proof should replay. The goal files are the HOL kernel's: tests/kernel/goals/*.strata and *.kp, which docs/kernel-laws.md and docs/kernel-tactics.md describe.

How the replay works:
- **Statements.** A statement `p` is the type `holds p` (`p == true`). Equality, `forall` and `exists` are the kernel's `eqv`, `all` and `some`, each with an intro and an elim rule.
- **Steps.** Each step of the HOL proof becomes a lemma applied: `hol.trans`, `hol.cong`, `hol.mp`, … Definitions, `if`, β and literal arithmetic hold by computation.
- **Datatypes.** Datatypes become inductive types. Their facts (injectivity, distinctness, induction) are proven once and used by name.
- **Integers.** The integers' axioms are the HOL kernel's 16, built in.

Cost: about 10× the HOL kernel's time, because the terms are large (300 to 800k nodes).

## 2. Laws for every record

```
from std.typing use Type, Field, Record

type Change[T] = | Keep | Set(T)

trait Patchable[P] {
  fn patched(self, p: P) -> Self
  fn apply(mut self, p: P)
}

type User = { name: String, age: Int }

impl[T: Record] Patchable[Patch[T]] for T {
  fn patched(self, p: Patch[T]) -> T {
    T.make(f -> match p.(f.name) { Keep => self.(f.name), Set(v) => v })
  }
  fn apply(mut self, p: Patch[T]) {
    comptime for f in T.fields {
      match p.(f.name) { Keep => (), Set(v) => { self.(f.name) = v } }
    }
  }
}

type Patch[T: Record] = Record {
  fields: [Field { name: f.name, type: Change[f.type], default: Keep } for f in T.fields],
}

lemma patched_empty[T: Record](x: T)
  ensures x.patched(Patch[T] {}) == x {
  simplify()
}
```

Store the module with `strata --file F.strata edit --as-given < text`. Then run `strata kernel dependent F.strata`.

How each part translates:
- **A record type `T`** becomes its field names `F` (one constructor per field), each field's type `ty f`, a decision procedure for comparing field names, and the field list `T.fields`. A law may assume that `T.fields` has every field. At a record, the kernel checks that by computation.
- **`x.(f.name)`** is `x` at the field `f`. A record is a function from its fields to their values.
- **`T.make(f -> e)`** is the record whose field `f` is `e`.
- **`comptime for f in T.fields { … }`** in a `mut self` method folds over the field list, changing `self` field by field.
- **A type function** whose body is `[Field { name: f.name, type: E, default: K } for f in T.fields]` makes, for each field, the type `E` with `f.type` as that field's type, and `K` as the default (`Patch[T] {}`).
- **A statement** is an equation (`a == b`), or `T.fields.all(f -> p)`, which says `p` for every field.
- **The module's sum types** (`Change`) become inductive types. `match` on one is its eliminator.

The steps:

| Step | Does |
|---|---|
| `simplify()` | Takes the law's parameters. On an equation between records it compares them field by field (`funext`). Then it closes the goal if both sides are the same once definitions are unfolded. |
| `fields()` | An equation between records becomes one about an arbitrary field `g` (`funext`). |
| `simplify(applied_at)` | On a field `g` of `apply`'s result: rewrites with `applied_at` at the goal's `p`, `x` and `g`, then with `T.fields` having `g`, then closes by unfolding. |

`applied_at` is a library lemma. It is proven when the module's `apply` is translated, by induction over the field list:

```
apply(x, p) at g == if T.fields has g { patched(x, p) at g } else { x at g }
```

The output gives, for each law, `proven for every record NAME (time, a term of N nodes)`, or `failed NAME: why`, or `open NAME` with its goal. It then lists each law's instance at each record: `instance User.patched_empty : …`.

The goal file is tests/kernel/goals/patch_every_record.strata. It has the three laws (empty patch, each field, `apply` = `patched`) with `?proof` bodies. tests/kernel/dependent/patch_every_record.strata has them proven.

What doesn't translate yet:
- a record with a field of another record of the module;
- any other expression in a method or a statement (arithmetic, lists);
- steps other than the three above.
