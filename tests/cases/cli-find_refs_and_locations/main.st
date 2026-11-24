use jobs

fn drain(w: jobs.Worker) -> Int {
  jobs.run_worker(w) + jobs.run_worker(w)
}

fn main() {
  w := jobs.Worker { queue: [jobs.Task { id: 1, attempts: 0 }] }
  print(drain(w))
  print(jobs.area(jobs.Circle { r: 1.0 }))
  t := jobs.retried(jobs.Task { id: 2, attempts: 1 })
  print(t.attempts)
}
