// Placeholder for the Patch laws (branch `lab-types-as-values` builds
// `Patch[T]`): stated here over one record, `User`, with its patch written
// by hand. Stating them for every `T` needs `Patch[T]` and `patched` as
// functions of a record's fields, which the logic doesn't have: a type
// function's result would have to be a datatype made per `T`, with its
// laws proven by a tactic per `T` or by parametricity over field lists.

type User = { name: String, age: Int }

type Change[T] = | Keep | SetTo(T)

type UserPatch = { name: Change[String], age: Change[Int] }

fn change[T](c: Change[T], old: T) -> T {
  match c {
    Keep => old,
    SetTo(v) => v,
  }
}

/// `x.patched(p)`: a new value.
fn patched(u: User, p: UserPatch) -> User = User { name: change(p.name, u.name), age: change(p.age, u.age) }

/// `apply(mut x, p)`, field by field, as a function of the old value (the
/// logic has no `mut`).
fn apply(u: User, p: UserPatch) -> User {
  named := match p.name {
    Keep => u,
    SetTo(n) => { ..u, name: n },
  }
  match p.age {
    Keep => named,
    SetTo(a) => { ..named, age: a },
  }
}

lemma patched_empty(u: User) ensures patched(u, UserPatch { name: Keep, age: Keep }) == u {
  ?proof
}

lemma patched_set_age(u: User, a: Int) ensures patched(u, UserPatch { name: Keep, age: SetTo(a) }).age == a {
  ?proof
}

lemma patched_set_keeps_name(u: User, a: Int) ensures patched(u, UserPatch { name: Keep, age: SetTo(a) }).name == u.name {
  ?proof
}

lemma apply_is_patched(u: User, p: UserPatch) ensures apply(u, p) == patched(u, p) {
  ?proof
}
