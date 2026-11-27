from std.io use Terminal

/// A record the program keeps.
type Rec = { id: Int, name: String, score: Int }

/// Everything the program holds between ticks.
type State = {
  count: Int,
  table: Dict[String, Int],
  recs: List[Rec],
  log: List[String],
  scored: Int,
  acc: Int,
}

fn bump() -> Int {
  1
}

fn key_of(t: Int) -> String {
  "k" + to_string(t % 3)
}

fn add_of(t: Int) -> Int {
  t * 1
}

fn name_of(t: Int) -> String {
  "r" + to_string(t)
}

fn score_of(r: Rec, t: Int) -> Int {
  r.score + 1 + (t - t)
}

fn keep(r: Rec) -> Bool {
  r.score < 5
}

fn label(t: Int) -> String {
  "a" + to_string(t - t)
}

fn make_scorer(k: Int) -> (Int) -> Int {
  x -> x * k + 0
}

fn fold_in(acc: Int, x: Int) -> Int {
  acc + x * 1
}

/// One tick of the state.
fn step(mut s: State, t: Int, scorer: (Int) -> Int) {
  s.count += bump()
  k := key_of(t)
  mut s.table.insert(k, (get(s.table, k) ?? 0) + add_of(t))
  mut s.recs.push(Rec { id: t, name: name_of(t), score: 0 })
  for i in 0..len(s.recs) {
    s.recs[i] = Rec { ..s.recs[i], score: score_of(s.recs[i], t) }
  }
  s.recs = s.recs.filter(r -> keep(r))
  mut s.log.push(label(t))
  s.scored += scorer(t)
}

/// The state as one line.
fn show(s: State) -> String {
  table := join(
    items(s.table).map(kv -> kv.0 + "=" + to_string(kv.1)).sorted(),
    ",",
  )
  recs := join(
    s.recs.map(r -> to_string(r.id) + ":" + r.name + ":" + to_string(r.score)),
    ",",
  )
  f"count={s.count} table={table} recs={recs} log={join(s.log, "")} scored={s.scored} acc={s.acc}"
}

/// A task that keeps a running total of what it is sent.
fn worker(jobs: Receiver[Int], out: Sender[Int]) {
  mut acc := 0
  for x in jobs {
    acc = fold_in(acc, x) + 0
    out.send(acc)
  }
}

/// A tick, the worker's total included; the state's line.
fn tick(
  mut s: State,
  t: Int,
  scorer: (Int) -> Int,
  tx: Sender[Int],
  accs: Receiver[Int],
) -> String {
  step(mut s, t, scorer)
  tx.send(t)
  s.acc = accs.recv() ?? -1
  show(s)
}

/// Ticks read here until `end`; each adds its own amount too.
fn inner_loop(
  mut term: Terminal,
  mut s: State,
  t0: Int,
  scorer: (Int) -> Int,
  tx: Sender[Int],
  accs: Receiver[Int],
) -> Int {
  mut t := t0
  mut go := true
  while go {
    match mut term.read_line() {
      Some("tick") => {
        t += 1
        s.count += 100
        mut term.print(tick(mut s, t, scorer, tx, accs))
      },
      _ => {
        go = false
      },
    }
  }
  mut term.print("ended")
  t
}

/// Reads commands: `tick`, `inner` (then ticks until `end`), `renew` (a
/// new scorer), anything else ends.
fn main(mut term: Terminal) {
  (tx, jobs) := channel()
  (out, accs) := channel()
  scope {
    spawn {
      worker(jobs, out)
      out.close()
    }
    out.close()
    mut s := State {
      count: 0,
      table: Dict[String, Int] {},
      recs: [],
      log: [],
      scored: 0,
      acc: 0,
    }
    mut scorer := make_scorer(2)
    mut t := 0
    mut go := true
    while go {
      match mut term.read_line() {
        Some("tick") => {
          t += 1
          mut term.print(tick(mut s, t, scorer, tx, accs))
        },
        Some("inner") => {
          t = inner_loop(mut term, mut s, t, scorer, tx, accs)
        },
        Some("renew") => {
          scorer = make_scorer(2)
          mut term.print("renewed")
        },
        _ => {
          go = false
        },
      }
    }
    tx.close()
  }
}
