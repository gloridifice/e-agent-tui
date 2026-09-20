# validate-ambiguous-and-absolute-quick-links

## Purpose
Quick-link discovery currently treats URI and absolute-path candidates alike: both bypass filesystem validation. This retains false positive absolute paths. It also consumes a relative path followed immediately by Chinese parenthetical diff statistics, such as `final-report/2_virtual_geometry_demo.html（+340/−7`, as one missing but syntactically strong path instead of considering the existing file before `（`.

The change makes ambiguous local-path tokens explicit and lets bounded adapter-owned filesystem validation select the interpretation that actually exists.

## Scope and acceptance

In scope:

- Distinguish URI, absolute filesystem, and workspace-relative candidates in the provider-neutral quick-link model.
- Treat Chinese opening wrappers as soft local-path boundaries: retain the complete token and derive progressively shorter path hypotheses rather than unconditionally splitting a potentially legal filename.
- Group hypotheses from one token, validate them together, and emit at most one selected target from each group.
- Require every absolute path and every soft-boundary group to have an existing interpretation. URI targets remain independent of filesystem state, while unambiguous strong relative candidates retain their existing missing-path policy.
- Validate native absolute paths even when they are outside the current workspace. Preserve relative-path workspace containment and symlink-escape rejection.
- Reject non-native absolute syntax and Windows UNC/device namespace paths instead of probing them. Keep filesystem work bounded, outside frontend state guards, and outside rendering.
- Preserve the selected target's exact source spelling for tags and clipboard copy. Do not replace it with a canonical path.
- Make rendered tag insertion recognize the same Chinese boundaries and resolve overlapping selected targets deterministically.
- Update the current presentation and runtime/adapter contracts for the delivered behavior.

Acceptance criteria:

- If only `final-report/2_virtual_geometry_demo.html` exists, the example above receives one tag immediately after `.html`; the longer hypothesis is not exposed.
- If the complete parenthetical path exists, it wins over its shorter existing prefix. If no hypothesis in a soft-boundary group exists, the group produces no tag.
- A native absolute file or directory receives a tag only when it exists, including outside the workspace. Missing, inaccessible, foreign-platform, UNC, and device paths do not receive tags.
- URI behavior, relative containment, stale-result rejection, the 36-tag presentation limit, copy text, and render-time no-I/O behavior remain intact.

Non-goals:

- Opening targets, checking URI reachability, finding a Git repository root, or changing the copy-link shortcut and guidance.
- Treating missing absolute paths as useful speculative links.
- Supporting network shares or Windows device namespace paths.

This intentionally changes the public quick-link contract: absolute paths no longer bypass existence validation, and ambiguous local-path tokens are resolved as one candidate group rather than independent links.

## Result
Delivered. Quick-link targets are now classified as URI, absolute path, or workspace-relative candidates; Chinese/full-width opening wrappers produce ordered local-path hypotheses that validation resolves to at most one existing target; and absolute paths require native existence, including outside the workspace, while foreign-platform, UNC, and device namespaces are rejected without probing. Relative containment, symlink-escape rejection, stale-result handling, copy text, and the 36-tag presentation bound are unchanged. The presentation and runtime/adapter contracts were synchronized.

Verification: focused `e-tui`, `e-pi`, and `e-dsh` tests, executable compile checks, and `cargo fmt --all --check` all passed. No unresolved blockers, deferrals, or limitations.
