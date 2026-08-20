# Example 3: how a future safety domain could consume the same tree

Not implemented — this is a documented sketch, per the task's own
instruction not to build the safety domain now. It exists to show that
`etdl-tree-core` genuinely does not need to change for a second domain to
consume it.

## The same structure, a different interpretation

Take `generic.etdl`'s `operational-monitoring` tree verbatim — the exact
same YAML, unmodified:

```yaml
x-tree-event:
  trees:
    - id: "operational-monitoring"
      root: "AnyConditionObserved"
      nodes:
        ConditionA: { kind: leaf, description: "..." }
        ConditionB: { kind: leaf, description: "..." }
        AnyConditionObserved:
          kind: gate
          gate: OR
          children: ["ConditionA", "ConditionB"]
```

`etdl-reliability::tree_adapter::evaluate_assuming_independence` reads this
as "combine two probabilities under an OR, assuming independence" and
produces a `Probability`. A **safety** domain consuming the identical
`Tree` value could instead:

- Interpret `OR` as "a hazardous state exists if any precondition holds" —
  a boolean, never computing a probability at all (a legitimate,
  simpler interpretation this same structure supports).
- Attach a **severity** (not a probability) to each leaf and propagate the
  *maximum* severity through gates rather than combining probabilities —
  a completely different evaluation function over the same `Tree` and
  `TreeNode` types, requiring zero changes to `etdl-tree-core`.
- Reject the tree outright if any leaf lacks a required safety
  classification in its `metadata` map (`TreeNode::metadata` already
  supports arbitrary key/value pairs for exactly this kind of
  domain-specific annotation, without the tree supplement needing to know
  what `"safety_class"` means).

## What would actually need to be built (not now)

A `safety_adapter` module, structurally identical in shape to
`etdl-reliability::tree_adapter` (a pure function from `&Tree` plus
caller-supplied domain data to a domain-specific result), living in a
future safety crate that depends on `etdl-tree-core` — never the reverse,
and never touching `etdl-reliability` either. Exactly the same dependency
shape this task's reliability adapter already demonstrates:

```text
etdl-tree-core                      etdl-tree-core
      |                                   |
etdl-reliability::tree_adapter     future-safety::tree_adapter
      |                                   |
Probability (via std.probability)   Severity (a new, safety-owned type)
```

Nothing about `Tree`, `TreeNode`, `GateKind`, validation, or traversal
needs to change to support this — that is the acceptance criterion this
sketch exists to demonstrate, not to build.
