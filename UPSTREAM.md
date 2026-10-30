# Upstream provenance

Upstream: https://github.com/DioxusLabs/blitz

Maintenance repository: https://github.com/glory-rs/apace

The initial renamed release retains upstream version 0.3.0-beta.4 and all previous Glory maintenance fixes. Original licensing and copyright notices remain in the repository. Names in preserved upstream changelogs describe the historical upstream releases.

Only `upstream-main` is synchronized automatically. Never reset or auto-merge the maintained `main` branch to upstream.

## Reviewed integration on 2026-10-30

Merged all 57 upstream commits through `e5929f9e` in three batches: layout and
scroll-overflow adaptation; Stylo/Dioxus, clipboard and runtime integration;
then DOM invalidation, failed stylesheet handling and CSS painting updates.

The maintained resource limits, cancellation and image generations, snapshot
upgrades, layout retirement, focus ancestry, form states, placeholders, top
layers and bounded native IME remain covered by regression tests. Retirement
keeps surviving paint order usable before resolve. New construction flags use
bits distinct from the maintained top-layer flags. Placeholder painting uses
upstream's brush override without sharing editable text or geometry.

Takia is pinned to `c79a14b54ea632462a15624acf9e7daf792bcd13` and Wingy to
`b80608744193df808d71dd9eaf9ff02627351f03`. Push those dependency commits before
pushing this integration. Stylo matches upstream's fixed Git revision
`78734bf554bb40af9dc11c0eedbe724f6055697c` and Dioxus is `0.8.0-alpha.3`.
No new registry release is implied. The existing publication validator still
rejects Git dependencies; normal source CI and release-tag checks are separate.

Validation on Windows/Rust 1.98:

- Workspace tests and doctests: 687 passed, 3 existing ignored tests. Boa's
  executable documentation needs an 8 MiB Windows main-thread stack, configured
  with `RUSTDOCFLAGS=-C link-arg=/STACK:8388608` in source CI.
- Maintained minimal DOM tests: 178 passed; parallel construction plus writing
  modes: 181 passed. Dedicated top-layer, indeterminate-checkbox and placeholder
  painting regressions all passed.
- Workspace Clippy with warnings denied, formatting and release-tool unit tests.
- WPT at `d4b721171c10aaae5f84c1b1486f716d975a0123`: 33 selected text-clip,
  text-fill, blending and isolation cases; 22 passed, 10 failed, 1 skipped,
  no crash or timeout. Every status matches a namespace-adapted `e5929f9e`
  upstream source baseline using the same maintained dependency revisions.
  Existing failures cover inline/table/fragmentation/scaled/text-decoration
  clipping and blending; this is not full CSS conformance acceptance.

AppKit/iOS native runtime checks still require their Apple CI runners.
