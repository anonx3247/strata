---
title: Types holding a Real are refined only by their other parts
packages: compiler
---

# Why
After #575, `strata --file std verify --require proven` reported 13 more
functions as not proven. All of them build a `JsonValue` or `TomlValue`
array from a list made in a loop (`Array(out)`). `Number(Real)` made those
types refined through `Real`'s part `den: Positive`. That meant proving every
list element valid, and verify doesn't track the elements of such a list.

# Background
`refined` decides whether checking a value against a type involves a
refinement. It uses the set of types that reach one, found once
(`refined_types`).

```strata smt.refined
```

# Intuition
Outside core.real a `Real` is SMT's real. Every `Real` is valid there:
core.real keeps `den > 0` (proven), and the checker refuses a `Real` built
from its parts. So a type holding a `Real` is refined there only through its
other parts. Inside core.real, where a `Real` is its record, the full set
still applies.

# The change
```strata-diff smt.refined smt.real_free_types smt.refined_types_skipping
```

```strata-also
smt.Methods smt.methods_of termination.context: the second set, computed once
smt.refined_walk smt.refined_types: the same rule when walking, the old set kept
```
