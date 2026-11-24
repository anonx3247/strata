from std.process use Launcher, Commands
from std.json use Json

type Point = { x: Int, y: Int }

derive Json for Point

fn main(mut launcher: Launcher) {
  print(mut launcher.run("echo", ["hi", "there"], ""))
  print(mut launcher.run("no-such-program-x", [], ""))
  p := Point { x: 1, y: 2 }
  print(p.to_json())
}
