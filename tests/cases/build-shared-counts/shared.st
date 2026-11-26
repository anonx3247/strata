// Many tasks sharing nested values (lists of records of strings, big
// integers, unions, closures, trait values) while the main thread keeps
// counting them too: passed on channels, returned through `await` (a
// handle awaited on two threads), held by tasks a race cancels or that
// panic. What crosses to another thread is marked shared and counted
// atomically, the rest plainly; every object is freed once.

type Person = { name: String, tags: List[String], born: Int }

type Kind = Lead { since: Int } | Member | Guest(String)

type Team = { name: String, people: List[Person], kinds: List[Kind], lead: Option[Person] }

type Note = { who: Person, text: String, n: Int }

trait Labelled {
  fn label(self) -> String
}

impl Labelled for Person {
  fn label(self) -> String {
    self.name + "!"
  }
}

fn team(t: Int) -> Team {
  mut people := []
  mut kinds := []
  for i in 0..20 {
    mut people.push(
      Person {
        name: f"p{t}-{i}",
        tags: [f"t{i % 3}", f"team {t}", "x"],
        born: 1000000000000000000000 + i,
      },
    )
    mut kinds.push(
      match i % 3 {
        0 => Lead { since: 1000000000000000000000 + t },
        1 => Member,
        _ => Guest(f"guest {i}"),
      },
    )
  }
  Team { name: f"team {t}", people, kinds, lead: get(people, 0) }
}

/// Copies and drops parts of the shared teams over and over: each copy and
/// drop a count change on a shared object.
fn churn(teams: List[Team], id: Int, rounds: Int) -> List[Person] {
  mut kept := []
  mut n := 0
  for r in 0..rounds {
    for t in teams {
      for (i, p) in enumerate(t.people) {
        ys := [p, p]
        zs := ys + [p]
        k := t.kinds[i]
        n += len(zs[0].tags) + byte_len(p.name) + byte_len(f"{k}")
        if (r + id + i) % 7 == 0 {
          mut kept.push(p)
        }
      }
    }
  }
  kept
}

fn produce(tx: Sender[Note], teams: List[Team], id: Int) {
  for t in teams {
    for p in t.people {
      tx.send(Note { who: p, text: f"from {id}", n: len(p.tags) })
    }
  }
}

/// Churns, then waits on `rx` (nothing ever comes): a race cancels it there.
fn churn_and_wait(teams: List[Team], rx: Receiver[String]) -> String {
  kept := churn(teams, 1, 2)
  got := rx.recv() ?? "ended"
  got + kept[0].name
}

fn fails(teams: List[Team], k: Int) -> Int {
  ps := teams[0].people
  ys := [ps[k], ps[k]]
  len(ys) / (k - k)
}

fn round(teams: List[Team], j: Int) -> String {
  suffix := f" of round {j}"
  tag := (p: Person) -> p.name + suffix
  mut labels: List[Labelled] = []
  first := teams[0]
  for p in first.people {
    let l: Labelled = p
    mut labels.push(l)
  }
  (tx, rx) := channel(8)
  (quiet_tx, quiet_rx) := channel()
  (htx, hrx) := channel()
  (kept, notes, twice, raced, failed, mine) := scope {
    mut hs := []
    for id in 0..8 {
      mut hs.push(spawn churn(teams, id, 20))
    }
    for id in 0..4 {
      spawn {
        produce(tx, teams, id)
        tx.close()
      }
    }
    tx.close()
    // tasks holding a closure and trait values that capture shared parts
    c := spawn sum(teams[1].people.map(p -> byte_len(tag(p))))
    d := spawn sum(labels.map(l -> byte_len(l.label())))
    // a handle sent to another task: its result held on two threads
    h := spawn churn(teams, 7, 2)
    w := spawn {
      match hrx.recv() {
        Some(k) => len(await k),
        None => 0,
      }
    }
    htx.send(h)
    htx.close()
    // the main thread counts the shared teams meanwhile
    mut mine := 0
    for t in teams {
      for p in t.people {
        qs := [p, p, p]
        mine += len(qs[1].tags)
      }
    }
    mut notes := []
    for note in rx {
      mut notes.push(note)
    }
    raced := race {
      churn_and_wait(teams, quiet_rx) as a => a,
      { len(teams) } as b => f"{b} teams",
    }
    r := scope {
      x := supervised spawn fails(teams, 3)
      await x
    }
    failed := match r {
      Ok(_) => "no panic",
      Err(_) => "panicked",
    }
    (await hs, notes, (len(await h), await w, await c, await d), raced, failed, mine)
  }
  quiet_tx.close()
  people := sum(kept.map(k -> len(k)))
  f"{people} kept, {len(notes)} notes ({notes[0].who.name}), {twice}, {raced}, {failed}, {mine}{suffix}"
}

fn main() {
  mut teams := []
  for t in 0..6 {
    mut teams.push(team(t))
  }
  for j in 0..3 {
    print(round(teams, j))
    // held once again: changed in place, then shared anew
    mut teams.push(team(10 + j))
  }
  print(len(teams))
}
