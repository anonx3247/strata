fn greet(name: String) -> String {
    "hello, " + name
}

fn main() {
    s := greet("Ada")
    print(s)
    t := "hello, Ada"
    print(f"{s}! {1 + 2} {2.5} {true} {s == t}")
    print("quote \" and backslash \\ and é")
    print("")
}
