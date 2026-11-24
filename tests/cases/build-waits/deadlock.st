fn main() {
  (atx, arx) := channel()
  (btx, brx) := channel()
  scope {
    spawn {
      x := brx.recv() ?? 0
      atx.send(x)
      atx.close()
    }
    spawn {
      y := arx.recv() ?? 0
      btx.send(y)
      btx.close()
    }
  }
  atx.close()
  btx.close()
}
