## Context

The workspace contains one shared frontend library and two executable adapters. All three manifests currently declare independent `0.0.3` versions, adapter dependencies use repository-only paths, and compile-time includes reach outside package roots. Cargo can build the checkout but cannot verify the resulting crates.io archives. The root license and README also do not currently populate package metadata.

## Goals / Non-Goals

**Goals:**

- Establish `0.0.1` as the first synchronized public version.
- Keep all package versions and the internal dependency requirement synchronized automatically.
- Make each `.crate` archive independently buildable.
- Preserve `bridge/` as the canonical Node bridge source while embedding a deterministic generated mirror in `e-dsh`.
- Provide a repeatable dry-run and publish workflow.

**Non-Goals:**

- Automatically infer semantic version levels from commit messages.
- Publish during this implementation task.
- Change runtime behavior, wire semantics, or DSH deployment behavior.
- Produce platform binaries through GitHub Releases.

## Decisions

### Inherit one workspace package version

All members inherit `[workspace.package].version`. The workspace declares the versioned local `e-tui` dependency once under `[workspace.dependencies]`, and both adapters inherit it. Cargo cannot interpolate the package version into a dependency requirement, so the release tool remains responsible for updating that second value.

Alternative: retain three explicit package versions. This is rejected because it permits accidental skew and creates unnecessary release edits.

### Use cargo-release for lockstep updates

`[workspace.metadata.release]` enables `shared-version`, dependency upgrades, consolidated commits, and a shared `v{{version}}` tag. A dry run remains the default; publication requires `--execute`. The first release publishes the already-declared `0.0.1`, while later releases select `patch`, `minor`, or an explicit version.

Alternative: release-plz. It is optimized for independently calculated package versions and adds complexity when strict lockstep is required.

### Package only self-contained compile inputs

The key mapping authority moves into `crates/e-tui/assets`, next to the other frontend defaults. Documentation links directly to that authority.

A new deterministic sync tool mirrors the production bridge package files into `crates/e-dsh/assets/bridge`. `e-dsh/build.rs` consumes only this package-local mirror. The top-level `bridge/` remains authoritative, and check mode fails when the mirror is missing, stale, or contains extra files.

Alternative: let `build.rs` use the checkout bridge when available and fall back to a mirror. This is rejected because checkout and crates.io builds could embed different inputs.

### Validate archives, not only the checkout

Release checks run the generated-asset check and `cargo package` verification for all three members. Package-specific README files describe only the relevant crate and installation prerequisites.

## Risks / Trade-offs

- [Generated bridge mirror can become stale] → Make staleness checkable with one command, include it in bridge tests/release documentation, and have builds consume the mirror consistently.
- [Exact internal version requirements require all packages to be republished together] → This is intentional while the frontend contract is unstable and lockstep versioning is required.
- [crates.io publication is irreversible] → Keep publication out of routine validation, require a clean tree and cargo-release dry run before `--execute`.
- [Moving the key-map authority changes documentation paths] → Update all Current documentation and compile-time includes in the same change.

## Migration Plan

1. Introduce workspace inheritance and set the synchronized version to `0.0.1`.
2. Move package resources inside their owning crate and generate the bridge mirror.
3. Add package metadata, per-crate README files, and release configuration.
4. Verify generated assets and all package archives from clean package extraction directories.
5. Perform the first crates.io publication separately after credentials and ownership are configured.

Rollback before publication is a normal Git revert. After publication, a broken version must be yanked and replaced with a new synchronized patch version.

## Open Questions

None.
