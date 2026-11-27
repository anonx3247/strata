// Tasks and channels: 4 producers make 20,000 jobs each and send them on a
// buffered channel; 4 workers receive them, compute a result record each,
// and send it on; the main thread keeps a summary. Values cross threads on
// every send.

type Job = { id: Int, text: String, weights: List[Int] }

type Done = { id: Int, score: Int, _note: String }

fn job(p: Int, i: Int) -> Job =
  Job { id: p * 100000 + i, text: f"job {p}-{i}", weights: [i % 7, i % 11, i % 13] }

fn score(j: Job) -> Done {
  mut s := byte_len(j.text)
  for w in j.weights {
    s += w * w
  }
  Done { id: j.id, score: s, _note: f"{j.text} scored {s}" }
}

fn main() {
  (jobs_tx, jobs_rx) := channel(64)
  (done_tx, done_rx) := channel(64)
  total := scope {
    for p in 0..4 {
      spawn {
        for i in 0..20000 {
          jobs_tx.send(job(p, i))
        }
        jobs_tx.close()
      }
    }
    jobs_tx.close()
    for _w in 0..4 {
      spawn {
        for j in jobs_rx {
          done_tx.send(score(j))
        }
        done_tx.close()
      }
    }
    done_tx.close()
    mut t := 0
    mut best := 0
    for d in done_rx {
      t += d.score
      if d.score > best {
        best = d.score
      }
    }
    (t, best)
  }
  print(f"{total}")
}
