type Card = { suit: String, rank: Int }

type Level = Low | Mid(Int) | High { why: String }

fn main() {
    s := "  Hello, World  "
    t := trim(s)
    print(t)
    print(split("a,b,,c", ","))
    print(contains(t, "World"))
    print(starts_with(t, "Hell"))
    print(ends_with(t, "x"))
    print(index_of(t, "o"))
    print(index_of(t, "z"))
    print(replace(t, "l", "L"))
    print(repeat("ab", 3))
    print("héllo".reversed())
    print(chars("añb"))
    print(lines("one\ntwo\r\nthree\n"))
    print(ord("A"))
    print(chr(97))
    print(byte_len("é"))
    print(byte_at("A", 0))
    print(byte_slice("hello", 1, 3))
    print(parse_int("42"))
    print(parse_int("123456789012345678901234567890"))
    print(parse_int("x"))
    print(parse_float("2.5"))
    print(to_string(12) + debug("q"))
    print(abs(-5))
    print(abs(-2.5))
    print(min(3, 7))
    print(max(2.5, 1.5))
    print(float(7))
    print(floor(2.7))
    print(ceil(2.1))
    print(round(2.5))
    print(sqrt(16.0))
    print(checked_div(1.0, 0.0))
    print(checked_add(1.0, 2.0))
    print(checked_sqrt(-1.0))
    print(checked_floor(3.9))
    print(float_bits(1.0))
    print(float_from_bits(4607182418800017408))
    print([5, 3, 9, 1, 3].sorted())
    print(["pear", "apple", "fig"].sorted())
    print([2.5, -1.0, 0.0].sorted())
    print(([(2, "b"), (1, "z"), (2, "a")]).sorted())
    print(([High { why: "x" }, Mid(3), Low, Mid(1)]).sorted())
    print([Card { suit: "h", rank: 3 }, Card { suit: "s", rank: 1 }].sorted_by(c -> c.rank))
    ns := Set { 3, 1, 2, 3 }
    print(ns)
    print(len(ns))
    print(contains(ns, 2))
    print({
      mut c := ns
      mut c.insert(0)
      c
    })
    print({
      mut c := ns
      mut c.remove(3)
      c
    })
    print(union(ns, Set { 9, 1 }))
    print(intersection(ns, Set { 2, 3, 4 }))
    print(difference(ns, Set { 1 }))
    print(to_list(ns))
    print(set(["b", "a", "b"]))
    ages := Dict { "bo": 30, "al": 25 }
    print(ages)
    print(get(ages, "al"))
    print(get(ages, "zz"))
    print(contains(ages, "bo"))
    print({
      mut c := ages
      mut c.insert("cy", 41)
      c
    })
    print({
      mut c := ages
      mut c.insert("al", 26)
      c
    })
    print({
      mut c := ages
      mut c.remove("bo")
      c
    })
    print(keys(ages))
    print(values(ages))
    print(items(ages))
    print(map_values(ages, a -> a + 1))
    mut total := 0
    for (name, age) in ages {
        total += age
    }
    print(total)
    for n in ns {
        print(n)
    }
    let empty: Dict[String, Int] = Dict {}
    print(empty)
    print(Set[Int] {})
    print(ns == Set { 1, 2, 3 })
    print(Set { x for x in [1, 2, 2, 3] })
    pi := 3.14159
    n := 42
    w := "hi"
    print(f"[{pi:.2}] [{n:>6}] [{n:<6}] [{n:^6}] [{n:06}] [{n:+}] [{w:*>5}] [{w:.1}]")
}
