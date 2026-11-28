fn main() {
  (atx, arx) := channel()
  (btx, brx) := channel()
  scope {
    spawn {
      x := brx.recv() ?? 0
      atx.send(x) except { () }
    }
    spawn {
      y := arx.recv() ?? 0
      btx.send(y) except { () }
    }
  }
}
