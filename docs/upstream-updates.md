# Selected upstream updates

Apace keeps its own crate names, resource lifecycle, input handling, and
coordinated Takia/Wingy dependencies. Changes from
[DioxusLabs/blitz](https://github.com/DioxusLabs/blitz) are reviewed and ported
selectively. The renamed repository no longer schedules writes to an
`upstream-main` branch. Historical mirror refs are reference material only.

The original-name fork [glory-rs/blitz](https://github.com/glory-rs/blitz)
tracks upstream source separately from the maintained Apace codebase. Use
that fork or the official repository for read-only comparisons.

The 2026-11-09 review used upstream commit
`808ded9348289cac0a3b4940e69da21d5b918443` and common ancestor
`e5929f9e2af42f8fdc10cea315b502d3937b1350`, focusing on the previous 30 days.

| Upstream commit | Selected fix | Apace adaptation |
| --- | --- | --- |
| `c58ecda5898f352ef02c0a76291a0b4ea5e7d474` | Resolve Windows absolute asset paths as URL paths | Keep Dioxus/file URL semantics and upstream regression tests. |
| `554d73a273065e19d7e1d0e83970b37da7fbdf8c` | Skip URLs that cannot be resolved | Guard form submission and stylesheet reload; retain Apace's existing bounded image lifecycle and failure tracking. |
| `9a93d470dd6d30e2cbf7bd57e7419282e20535e2` | Decode external and imported CSS from bytes | Keep response URL checks, abort signals, import depth limits and resource admission accounting; use the existing published Stylo API. |
| `b0e009c187d3ab0ae54b9a057d67b25fd34c57d0` | Removing a link's href before its stylesheet loads must not panic | Preserve unrelated special element data and port upstream regression coverage. |
| `b6bb45c550a9490afa6c43468c77535c6d76856d` | Match stylesheet link relations without ASCII case sensitivity | Preserve Apace's resource loading path. |
| `9b9471f2fa1cd5f487088c33ad292581c73da3db` | Decode non-UTF-8 page bytes without panicking | Port to Apace launch/readme/example entry points. |
| `c888b09d92165df42a2e165d51b74a8e234ac14a` | Compile cache and multipart networking together | Forward the optional middleware multipart feature and check the combination in CI. |

The review leaves the large CDP/DevTools/code-editor additions, Git-pinned
Stylo/anyrender upgrades, new Taffy APIs, and overlapping input/painting/layout
changes for separate work. Those changes require architecture or rendering
validation beyond this compatibility and resource-loading update. No upstream
merge replaces Apace's maintained control states or resource limits.
