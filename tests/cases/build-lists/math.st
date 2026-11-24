// A tour of `strata verify` on small number functions: refinements,
// recursion, loops and lists. Run `strata --file examples/math.strata
// verify --checks` to see what is proven, what is only fuzzed, what may
// not terminate, and the two bugs at the end with their counterexamples.

/// A decimal digit.
type Digit = Int
  where 0 <= it && it <= 9

// ───────────────────────── recursion ─────────────────────────

/// n! is never 0. Terminates: `n` goes down on every call and is never
/// negative there.
fn factorial(n: Natural) -> Positive {
  if n == 0 {
    1
  } else {
    n * factorial(n - 1)
  }
}

/// Greatest common divisor (Euclid). Terminates: `b` goes down.
fn gcd(a: Natural, b: Natural) -> Natural
  ensures a > 0 || b > 0 ==> it > 0 {
  if b == 0 {
    a
  } else {
    gcd(b, a % b)
  }
}

/// b to the power e, by squaring. Terminates: `e` halves or drops by one.
fn power(b: Int, e: Natural) -> Int
  ensures e == 0 ==> it == 1 {
  if e == 0 {
    1
  } else if e % 2 == 0 {
    power(b * b, e / 2)
  } else {
    b * power(b, e - 1)
  }
}

/// The sum of a number's decimal digits.
fn digit_sum(n: Natural) -> Natural
  ensures n < 10 ==> it == n {
  if n < 10 {
    n
  } else {
    n % 10 + digit_sum(n / 10)
  }
}

/// Ackermann's function: it terminates, though not because one argument
/// goes down: the pair (m, n) does, compared in order.
fn ackermann(m: Natural, n: Natural) -> Positive {
  if m == 0 {
    n + 1
  } else if n == 0 {
    ackermann(m - 1, 1)
  } else {
    ackermann(m - 1, ackermann(m, n - 1))
  }
}

// ───────────────────────── loops ─────────────────────────

/// 1 + 2 + ... + n. Every assignment keeps `total` a `Natural`.
fn triangle(n: Natural) -> Natural {
  mut total: Natural = 0
  for i in 1..=n {
    total += i
  }
  total
}

/// The integer square root. The loop ends (its bound goes down), and
/// `ensures` needs a loop invariant, which the verifier infers: `r * r <= n`.
fn isqrt(n: Natural) -> Natural
  ensures it * it <= n {
  mut r: Natural = 0
  while (r + 1) * (r + 1) <= n {
    r += 1
  }
  r
}

/// 1 + 2 + ... + n again, with its closed form. No inferred invariant says
/// it, so the loop states it: `invariant` is proven on entry and after
/// every pass, and then known at the loop's head.
fn triangle_closed(n: Natural) -> Natural
  ensures 2 * it == n * (n + 1) {
  mut total: Natural = 0
  mut i: Natural = 0
  while i < n
    invariant 2 * total == i * (i + 1) {
    i += 1
    total += i
  }
  total
}

/// Steps until the Collatz sequence reaches 1. Whether it always does is an
/// open problem, and the verifier says so: "may not terminate".
fn collatz_steps(n: Positive) -> Natural {
  mut x: Positive = n
  mut steps: Natural = 0
  while x != 1 {
    if x % 2 == 0 {
      x /= 2
    } else {
      x = 3 * x + 1
    }
    steps += 1
  }
  steps
}

// ───────────────────────── lists ─────────────────────────

/// Squares of the positive numbers: `filter` keeps the positives, `map`
/// keeps them positive.
fn positive_squares(xs: List[Int]) -> List[Positive] {
  [x * x for x in xs where x > 0]
}

/// No division by zero: every `d` in `2..n` is at least 2.
fn is_prime(n: Int) -> Bool
  ensures it ==> n >= 2 {
  n >= 2 && all(2..n, d -> n % d != 0)
}

/// Every number `filter` keeps passed `is_prime`, so it is at least 2.
fn primes(limit: Natural) -> List[Int where it >= 2] {
  (0..=limit).filter(is_prime)
}

/// What a caller knows: every element is positive, so the first one is.
fn first_or_one(xs: List[Int]) -> Positive
  requires all(xs, x -> x > 0) {
  match xs {
    [x, ..] => x,
    [] => 1,
  }
}

// ───────────────────────── two bugs ─────────────────────────

/// Wrong: an empty list has no average (division by zero).
fn average(xs: List[Int]) -> Int {
  sum(xs) / len(xs)
}

/// Wrong: a digit plus one isn't always a digit.
fn next_digit(d: Digit) -> Digit {
  d + 1
}

fn main() {
  print([
    factorial(5),
    gcd(12, 18),
    power(2, 10),
    digit_sum(1234),
    ackermann(2, 3),
    triangle(10),
    isqrt(17),
    collatz_steps(6),
  ])
  print(positive_squares([-2, 3, 0, 4]))
  print(primes(20))
  print(first_or_one([5, 1]))
  print(average([]))
}
