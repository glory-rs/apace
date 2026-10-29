# Releasing Apace

`release.json` is the explicit dependency order and namespace allowlist. Versions remain
the original fork versions, including platform/support packages with independent versions.

1. Update source and Cargo.lock, run CI and `python scripts/release.py`.
2. Commit all changes and tag the exact clean commit as `apace-v0.3.0-beta.4`.
3. Push the tag. Release reuses the full CI workflow before publishing dependency-first.
4. `CARGO_REGISTRY_TOKEN` must allow publishing and creation of these crate names.
5. Confirm registry versions and GitHub release artifacts before updating consumers.

Namespaced tags preserve inherited upstream tags without moving their historical references.
Already-published versions are never overwritten. Cross-target packages require the successful
platform jobs of the same Actions release run before host verification may be skipped.

On Windows, all Cargo homes, targets and compiler temporary files belong under D:/BuildCache.
The daily/manual sync workflow updates only `upstream-main`, never maintained `main`.
