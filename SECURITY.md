# Security Policy

ETDL is a toolchain for reliability modeling. Its security posture matters because
it parses untrusted documents (`.etdl`, AsyncAPI YAML/JSON) and generates code.

## Supported versions

| Version | Supported |
|---|---|
| 0.1.x | current development; security fixes land on main and are released in the next published version |
| Older | not supported |

## Reporting a vulnerability

**Do not open a public issue for a vulnerability.** Report privately to the
maintainers via GitHub Security Advisories:

- Compiler: https://github.com/ETDL-lang/etdl/security/advisories/new
- Specification: https://github.com/ETDL-lang/etdl-specification/security/advisories/new

Include:

- affected crate/version,
- a minimal reproducer (untrusted input that triggers the issue),
- expected vs observed behavior,
- (if known) impact.

We aim to acknowledge reports within 5 business days and to ship a fix in the
next release.

## Known hardening measures

### Parser / compiler
- ECEL `[index]` parsing saturates on overflow; it cannot panic on untrusted
  input.
- Recursive traversals (DAG checks, codegen) are bounded by the document; deeply
  nested flat chains are a remaining robustness item (tracked).
- Unknown non-`x-` fields are rejected; `x-*` fields are preserved.
- Fault-tree math is overflow-proof (f64 binomial/factorial) and cannot panic.

### Filesystem / imports
- AsyncAPI local imports are confined to the project root: a `..` segment is
  rejected (no path traversal).
- Remote (`http(s)://`) imports are disabled in the reference implementation.
- The WASM validator has **no filesystem access**; callers pass file contents.

### Runtime
- Chaos injection is disabled by default and ignored in production.
- Retry never panics (timeout/exhaustion return errors).
- W3C traceparent IDs come from OS randomness.

### WASM
- No `unsafe` code; no filesystem/network access from within the sandbox.

## Dependency review

`cargo audit` runs in CI. Critical advisories block the build.

## Denial-of-service

Resource limits for untrusted AsyncAPI documents (size, `$ref` depth) and very
large fault/event trees are on the roadmap; see `READINESS_AUDIT.md` P2 items.

## Disclosure process

1. Reporter submits via Security Advisory.
2. Maintainer triages (P0 = crash/remote code execution; P1 = data corruption;
   P2 = robustness).
3. Fix lands on `main`; advisory is published; affected versions are noted.
4. Releases are tagged and crates published.
