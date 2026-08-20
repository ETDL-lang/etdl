# Example 3: a future domain library building on `std.events`

This proves the architecture supports a chain like:

```
std.events (built-in)
     |
acme.signals (optional — resolved from --library-path, never embedded)
     |
device-monitor.etdl (a user document)
```

`acme.signals` is **not** part of this repository's standard library and
is **not** a real product — it exists only in this example directory, as a
worked proof that a *future* domain library can `dependsOn: std.events`
and compose its generic identities, without this repository implementing
that domain.

- `acme.signals/lib.etdl` declares `dependsOn: [{name: std.events, version: "1.0"}]`
  and provides one gate, `MonitoringEventOccurred`, built as
  `OR(std.events.SignalReceived, std.events.StateChanged)` — both
  probability-free, generic identities.
- `device-monitor.etdl` imports `acme.signals` (not `std.events` directly —
  the dependency resolves transitively) and supplies concrete
  probabilities for the two generic identities two layers down.

## Running it

```bash
etdl validate device-monitor.etdl --library-path .
etdl analyze device-monitor.etdl --library-path .
```

`--library-path .` tells the resolver to look for `<name>/lib.etdl` in
this directory — `acme.signals/lib.etdl` is found there; `std.events` is
still resolved from the embedded built-in registry (never from a search
path, even though this directory also happens to be searched — see the
namespace-shadowing rule in `docs/reference/standard-library.md`).

## Output (captured verbatim)

```text
$ etdl analyze device-monitor.etdl --library-path .
document: device-monitor.etdl
event trees: 1
fault trees: 1
  Monitoring: topEvent probability = 0.370000
```

`0.370000` is `1 - (1 - 0.3) * (1 - 0.1)` — the OR of the two overridden
generic identities, exactly as declared.

## The point

Nothing about `std.events`/`std.logic` needed to change to support this.
`acme.signals` is ordinary ETDL, resolved by the same mechanism as any
other optional library, and its own qualified references into
`std.events` are resolved by the same transitive-closure splicing pass
that already handles `std.logic`'s own placeholder signals. A real future
domain (safety, security, a device-monitoring product) could be built the
same way, incrementally, without this task attempting to build it.
