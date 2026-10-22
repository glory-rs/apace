# Glory maintenance branch

Upstream: DioxusLabs/blitz, commit `92b11f1bd8ab7975411328b0406a95a09445b4e7`
(0.3.0-beta.4). Package names and the upstream workspace structure are retained.
Taffy is pinned to the corresponding glory-rs/taffy maintenance commit in the
workspace manifest. Glory consumes this repository by immutable Git revision.

The maintenance patch carries bounded resource admission/cache/completions,
cancellation and document close, stylesheet snapshot correctness, incremental
inline reconstruction, focus-within state, password display/selection and IME
purpose, and text-wrap-mode damage propagation. Existing upstream unsafe code
and licenses remain; the transferred additions use safe Rust. Direct regressions
are in blitz-dom; integration/geometry/window tests live in glory-rs/glory.

Run `cargo test --locked -p blitz-dom --no-default-features --features
system-fonts,complex-scripts --lib`. Cargo.lock pins this maintenance test
environment. It is independent of consumer lockfiles. Keep build/cache output
outside this checkout; Glory uses D:/BuildCache/glory-semantic-ui.
