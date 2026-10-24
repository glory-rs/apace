# Document top layers

`BaseDocument::set_top_layer(NodeId, Option<TopLayerConfig>)` installs or closes
one live viewport layout/paint root. DOM parentage, selector matching and
inherited style remain unchanged. Open entries preserve order on update;
close/reopen appends. The 1024-entry budget is checked before any mutation.
Missing/generationally retired identities, disconnected/non-element/root targets
and nonfinite backdrop components are rejected. Detachment/drop retires entries;
a connected move retains the entry and follows its new style ancestry.

The engine sets `:popover-open` and `:modal` state. Host code remains responsible
for finite positioning, initial/return focus, modal focus scopes, inertness and
dismissal proposals. This API alone does not advertise an input/focus driver.
Computed `display:none` on an ancestor suppresses the box and its backdrop while
retaining only its current lease. Reattachment never resurrects a retired lease.

Each root is removed from ordinary formatting/paint children and laid out
against the viewport using Taffy's root out-of-flow path. Separate transform and
stacking passes retain its own styles while escaping ancestor transforms,
opacity and clipping. Painting appends ordered backdrops/roots after the
document. Hit testing visits those roots in reverse order; modal background hits
resolve to their layer rather than the underlying document. Client geometry,
page hit coordinates, IME absolute coordinates, viewport scroll and vertical
writing-mode conversion share the same root boundary.

Acceptance commands (keep build/cache paths on the caller's configured drive):

```text
cargo test --locked -p blitz-dom --no-default-features --features system-fonts,complex-scripts --lib
cargo test --locked -p blitz-dom --no-default-features --features system-fonts,complex-scripts,parallel-construct,writing-mode --lib
cargo test --locked -p blitz-paint --no-default-features --test top_layers
```

The focused DOM tests cover ancestry/selector/font inheritance, flow exclusion,
transform/clip escape, nested order, update/reopen, modal hit catch, viewport
scroll/resize, hidden recovery, connected move, detach/drop/generation reuse,
finite admission and the live entry budget. The CPU raster test verifies actual
pixels above a document node with an enormous z-index, ancestor clip/opacity/
transform escape, nested roots, exactly one half-alpha paint/backdrop and close
cleanup. Existing engine tests remain part of the acceptance commands.
