# Example 2: why `std.units` belongs in stdlib — and why it isn't built yet

There is no runnable `.etdl` file for this example, because there is
nothing to run: `std.units` is **not implemented** in this version (see
`docs/reference/standard-library.md`'s "Module reference" for the full
reasoning). This document demonstrates the limitation directly instead of
hiding it behind a working-looking example.

## The problem, concretely

ETDL's core language has no unit-of-measure primitive. A basic event's
`mission_time` and a fault tree's various time-flavored fields are all raw
`f64` — nothing in the type system tracks whether a given number means
seconds, minutes, or hours. Nothing stops this:

```yaml
basicEvents:
  ComponentA:
    description: "..."
    probability: 0.01
    missionTime: 24        # the author meant 24 HOURS
  ComponentB:
    description: "..."
    probability: 0.01
    missionTime: 1440       # the author meant 1440 MINUTES (= 24 hours)
```

Both basic events describe "a 24-hour mission," but nothing in ETDL knows
that — `missionTime: 24` and `missionTime: 1440` are just two different
numbers to the compiler, the exponential-failure-model evaluator, and any
analysis that consumes them. If one engineer writes hours and another
writes minutes in the same document (or the same fault tree, copy-pasted
from two different sources), the mistake is silent: both parse, both
validate, both compile, and the resulting probabilities are simply wrong
by a factor of 60, with no diagnostic anywhere.

## Why this belongs in `std.units`, not a reliability supplement

The mistake above has nothing to do with reliability specifically — the
same silent-unit-confusion problem exists for *any* numeric quantity in
*any* domain that measures things over time, sizes, or counts (a
security domain measuring session timeouts, a performance domain
measuring request latencies, a future domain nobody has written yet).
Fixing it once, generically, at the `std.units` layer would benefit every
domain; fixing it inside the reliability supplement alone would only ever
protect reliability-specific fields, leaving the exact same silent-mistake
shape open everywhere else.

## Why it isn't implemented in this version

Per this task's explicit instruction ("if unit checking is not currently
supported by ETDL, document the limitation instead of implementing unsafe
implicit behavior"): making `std.units` merely provide named numeric
*constants* (`HOUR_IN_SECONDS: 3600`, say) — without ETDL also gaining a
real unit *type* that the compiler checks — would not fix the example
above. A document could import `std.units.HOUR_IN_SECONDS` and *still*
write `missionTime: 24` by mistake right next to it; nothing would
connect the constant to the field. Shipping the constants without the
checking would look like a units feature while providing none of the
safety a units feature exists for — exactly the "unsafe implicit
behavior" this task says not to build. So neither half is implemented:
not a fake constants-only module, and not a real unit-checked type
(which is a substantial core-language change on its own, requiring
changes to parsing, validation, and every numeric field in the AST — well
beyond what a standard-library task should force through).

## What would need to change

A real fix needs, at minimum:
1. A core `Unit` concept in `etdl-parser::ast` (e.g. an enum of supported
   units per quantity kind — time, count, size), attached to the numeric
   fields that need it (`missionTime`, and any future domain-specific
   rate/duration fields).
2. Validation that rejects unit mismatches at the point of use (a gate
   combining basic events with different declared mission-time units, for
   instance) rather than silently truncating or converting.
3. Only *then* would `std.units` have something real to re-export:
   documented, named unit values the type system actually enforces.

This is proposed as a future, dedicated language-design task — not
something to bolt onto the standard-library mechanism as a side effect of
proving `std.events`/`std.logic` work.
