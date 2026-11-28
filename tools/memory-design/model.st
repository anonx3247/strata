// The memory design's rules as a model: where an object lives, which pointers the rules allow, what crossing to a task and freezing make, and the laws that make freeing whole scopes safe.

/// Where an object lives. A depth is a scope's depth on the path from the
/// program's root (0) through the scopes and the tasks they started.
/// - `Static`: a constant, never freed.
/// - `Counted`: counted by task `owner` alone, freed at its last holder's
/// end. `home` is the scope the checker bounds its holders by (0 when it
/// may live to the program's end).
/// - `Arena`: made in the arena of the scope at `depth` on task `owner`,
/// freed whole when that scope ends.
/// - `Frozen`: part of the snapshot of the scope at `depth`, read by every
/// task below it, counted by none, freed whole when that scope ends.
type Place =
  | Static
  | Counted { owner: Int, home: Int }
  | Arena { owner: Int, depth: Int }
  | Frozen { depth: Int }

/// The scope whose end frees an object (-1: none does).
fn ends_with(p: Place) -> Int {
  match p {
    Static => -1,
    Counted { owner: _, home } => home,
    Arena { owner: _, depth } => depth,
    Frozen { depth } => depth,
  }
}

/// Whether an object is still there once the scope at depth `k` has
/// ended (scopes end innermost first, after the tasks they started).
fn alive_after(p: Place, k: Int) -> Bool {
  ends_with(p) < k
}

/// The task that counts an object, if one does.
fn counter(p: Place) -> Option[Int] {
  match p {
    Counted { owner, home: _ } => Some(owner),
    _ => None,
  }
}

/// The pointers the rules allow, from an object at `a` to one at `b`.
/// - Nothing points into a scope that ends before the holder's.
/// - A counted object never points into an arena: its count could keep it
/// past the arena's end, and an arena's objects are never walked.
/// - Frozen objects point only at frozen and static ones.
/// - Counted objects and arenas point only at objects of their own task,
/// besides frozen and static ones.
fn may_point(a: Place, b: Place) -> Bool {
  match b {
    Static => true,
    Frozen { depth } => match a {
      Static => false,
      _ => depth <= ends_with(a),
    },
    Counted { owner: o, home: h } => match a {
      Counted { owner, home } => owner == o && h <= home,
      Arena { owner, depth } => owner == o && h <= depth,
      _ => false,
    },
    Arena { owner: o, depth: d } => match a {
      Arena { owner, depth } => owner == o && d <= depth,
      _ => false,
    },
  }
}

/// Where a block at `p` is once it crosses to task `to` (captured by a
/// spawn, sent, or a result awaited elsewhere): a static or frozen block
/// as it is; a counted one held once by a sender that gives it up moves;
/// any other is copied, counted by `to`, bounded by `home`.
fn crossed(p: Place, to: Int, home: Int, moves: Bool) -> Place {
  match p {
    Static => Static,
    Frozen { depth } => Frozen { depth },
    Counted { owner: _, home: h } => if moves {
      Counted { owner: to, home: h }
    } else {
      Counted { owner: to, home }
    },
    Arena { owner: _, depth: _ } => Counted { owner: to, home },
  }
}

/// Where a block at `p` is once frozen into the snapshot of the scope at
/// depth `d`: a static block and one of an enclosing snapshot as they are,
/// any other copied into the snapshot.
fn frozen_place(p: Place, d: Int) -> Place {
  match p {
    Static => Static,
    Frozen { depth } => if depth <= d {
      Frozen { depth }
    } else {
      Frozen { depth: d }
    },
    _ => Frozen { depth: d },
  }
}

/// No dangling pointer: what an allowed pointer reaches is still there
/// for as long as its holder is.
lemma no_dangling(a: Place, b: Place, k: Int)
  requires may_point(a, b) && alive_after(a, k) && k >= 1
  ensures alive_after(b, k)

/// No object is counted by two tasks: whatever may point at a counted
/// object is of its counter's task, or a frozen or static object never is.
lemma one_counter(a: Place, b: Place)
  requires may_point(a, b) && counter(b) != None
  ensures counter(a) == counter(b) || match a {
    Arena { owner, depth: _ } => Some(owner) == counter(b),
    _ => false,
  }

/// A frozen object reaches only frozen and static objects.
lemma frozen_closed(d: Int, b: Place)
  requires may_point(Frozen { depth: d }, b)
  ensures match b {
    Frozen { depth: _ } | Static => true,
    _ => false,
  }

/// Nothing points into an arena from outside its task's arenas.
lemma arena_held_only_by_arenas(a: Place, o: Int, d: Int)
  requires may_point(a, Arena { owner: o, depth: d })
  ensures match a {
    Arena { owner, depth } => owner == o && depth >= d,
    _ => false,
  }

/// What crosses to a task is counted by that task alone, or not at all.
lemma crossing_counts_once(p: Place, to: Int, home: Int, moves: Bool)
  ensures counter(crossed(p, to, home, moves)) == None
    || counter(crossed(p, to, home, moves)) == Some(to)

/// An arena's object never crosses as itself: the receiver gets a copy, so
/// an arena may end while the receiver still runs.
lemma arena_never_crosses(o: Int, d: Int, to: Int, home: Int, moves: Bool)
  ensures match crossed(Arena { owner: o, depth: d }, to, home, moves) {
    Counted { owner, home: _ } => owner == to,
    _ => false,
  }

/// A frozen copy made for the scope at `d` lives no longer than that
/// scope, and a part of an enclosing snapshot no longer than its own.
lemma frozen_ends_with_scope(p: Place, d: Int)
  requires d >= 0
  ensures ends_with(frozen_place(p, d)) <= d

/// Freezing keeps pointers allowed: a snapshot's copy of an object
/// pointing at `b` points at `b`'s place in the same snapshot.
lemma freezing_keeps_pointers(a: Place, b: Place, d: Int)
  requires d >= 1 && may_point(a, b)
  ensures may_point(frozen_place(a, d), frozen_place(b, d))
    || frozen_place(a, d) == Static

/// Values never point at younger ones: an object points only at objects
/// made before it (`born` numbers each object as it is made).
fn older(holder_born: Int, held_born: Int) -> Bool {
  held_born < holder_born
}

/// An update in place of an object held once (by a variable, no object)
/// keeps the order: no object reaches it, so the updated object counts as
/// made now, after anything it is given.
lemma update_in_place_keeps_order(held_born: Int, now: Int)
  requires held_born < now
  ensures older(now, held_born)

/// No cycle: births fall strictly along every path of allowed pointers.
fn falling(bs: List[Int]) -> Bool {
  match bs {
    [] => true,
    [_] => true,
    _ => bs[1] < bs[0] && falling(drop(bs, 1)),
  }
}

/// A path whose births fall never comes back to where it started.
lemma no_cycle(bs: List[Int])
  requires len(bs) >= 2 && falling(bs)
  ensures bs[len(bs) - 1] < bs[0]
  decreases len(bs)
{
  if len(bs) > 2 {
    no_cycle(drop(bs, 1))
  }
}
