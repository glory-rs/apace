# Glory maintenance branch

Upstream: [DioxusLabs/blitz](https://github.com/DioxusLabs/blitz), integrated
through `e5929f9e` on 2026-10-30. The Apace namespace and maintained patches are
retained. `Cargo.toml` pins the coordinated Takia and Wingy maintenance commits;
see `UPSTREAM.md` for integration batches, validation and publication limits.

The maintenance patch carries bounded resource admission/cache/completions,
cancellation and document close, stylesheet snapshot correctness, incremental
inline reconstruction, focus-within state, password display/selection and IME
purpose, and text-wrap-mode damage propagation. Existing upstream unsafe code
and licenses remain; the transferred additions use safe Rust. Direct regressions
are in apace-dom; integration/geometry/window tests live in glory-rs/glory.
Linux/FreeBSD test-only Fontique enables fontconfig-dlopen, matching the adapter
without requiring target development headers. Runtime Fontconfig is still needed.

Run `cargo test --locked -p apace-dom --no-default-features --features
system-fonts,complex-scripts --lib`. Cargo.lock pins this maintenance test
environment. It is independent of consumer lockfiles. Keep build/cache output
outside this checkout; Glory uses D:/BuildCache/glory-semantic-ui.
