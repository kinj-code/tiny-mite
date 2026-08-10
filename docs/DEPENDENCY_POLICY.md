# Tiny Mite — Dependency Policy

## Principles

1. **Minimize the dependency graph.** Every new crate must justify its inclusion.
2. **Evaluate footprint.** Check binary size, compile time, and transitive deps before adding.
3. **Check license compatibility.** Prefer MIT, Apache-2.0, BSD-2/3-Clause. Copyleft licenses require explicit approval.
4. **Prefer pure Rust.** Avoid FFI/bindings dependencies unless necessary for core functionality (e.g., llama.cpp).
5. **Pin versions carefully.** Workspace dependencies use minimum-compatible semver. Lockfile committed for reproducibility.
6. **Security is continuous.** Dependencies are audited in CI via `cargo-deny` on every push. Known vulnerabilities block merge.
7. **Offline-first compatible.** No dependency must silently phone home. Avoid telemetry-including crates.
8. **Maintenance matters.** Prefer actively maintained crates with responsive maintainers.

## Crate evaluation checklist

For every new dependency, answer:

- [ ] Is this functionality already available in the standard library or an existing workspace crate?
- [ ] What is the license?
- [ ] How many transitive dependencies does it pull in?
- [ ] Does it use `unsafe`? If so, is the safety invariant documented and audited?
- [ ] Is it actively maintained (recent commits, responsive issues)?
- [ ] Does it support the Rust edition and minimum version we target?
- [ ] Does it work offline without phoning home?
- [ ] How large is the compiled artifact impact?

## Banned dependency categories

- Crates that execute arbitrary code at build time without explicit opt-in.
- Crates that embed remote network calls in normal operation without configuration.
- Abandoned crates with known security vulnerabilities and no fork.
- Crates with licenses incompatible with MIT/Apache-2.0 dual licensing without explicit approval.

## Currently approved dependencies

See `Cargo.toml` workspace dependencies and `deny.toml` for the current allowlist.

## Adding a dependency

1. Propose the addition with justification in the PR description.
2. CI will automatically check licenses, bans, and advisories via `cargo-deny`.
3. If `cargo-deny` rejects the crate, either find an alternative or escalate for review.