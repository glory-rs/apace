# Apace

A modular HTML/CSS document, layout, paint and window host. This maintained fork includes bounded resources, image observations/revocation, top layers, incremental retirement and native control/IME fixes.

Apace is maintained by Glory and is based on [Blitz](https://github.com/DioxusLabs/blitz).
The first renamed release preserves the fork's version `0.3.0-beta.4` and existing maintenance patches.
Original authors, licenses and copyright notices are retained. Upstream projects using the
original library are not represented as users of this renamed fork.

[![CI](https://github.com/glory-rs/apace/actions/workflows/ci.yml/badge.svg)](https://github.com/glory-rs/apace/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/apace.svg)](https://crates.io/crates/apace)
[![docs.rs](https://img.shields.io/docsrs/apace)](https://docs.rs/apace)

## Dependency

```toml
[dependencies]
apace = "=0.3.0-beta.4"
```

The published family includes `apace`, `apace-dom`, `apace-traits`, `apace-paint`, `apace-html`, `apace-net`, `apace-shell`, `stylo_takia` and the namespaced support crates. Additional Dioxus integrations and browser/WPT tools remain development-only.

## Branches and releases

- `main` contains this project's maintained implementation, namespace and release tooling.
- `upstream-main` mirrors the upstream default branch exactly. Its scheduled/manual sync never updates `main`.
- Upstream changes are reviewed before integration into maintained `main`.
- Release tags use `apace-v0.3.0-beta.4`, preserving inherited upstream tags.

See [UPSTREAM.md](UPSTREAM.md) for provenance and [RELEASING.md](RELEASING.md) for
the explicit dependency order, checks and publication procedure. Supported platform
checks are compilation/test evidence and do not imply unperformed device or OS IME acceptance.
