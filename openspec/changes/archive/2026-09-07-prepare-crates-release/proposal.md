## Why

The three Rust packages cannot currently be built from their crates.io archives because their manifests and compile-time resources assume the repository workspace layout. A reproducible lockstep release workflow is needed before the documented `cargo install e-dsh` and `cargo install e-pi` commands can work.

## What Changes

- Give `e-tui`, `e-dsh`, and `e-pi` one inherited workspace version, starting at `0.0.1`.
- Declare publishable versioned path dependencies so local development and crates.io resolution use the same package relationship.
- Make every package archive self-contained, including the default key map and the bridge runtime embedded by `e-dsh`.
- Add complete crates.io package metadata and package-specific README files.
- Add a lockstep `cargo-release` configuration that updates package and dependency versions, publishes in dependency order, and creates one project tag.
- Add scoped validation for generated release assets and packaged-crate builds.

## Capabilities

### New Capabilities
- `lockstep-crate-release`: Defines synchronized versioning, package self-containment, validation, and publication ordering for the Rust workspace.

### Modified Capabilities

None.

## Impact

The root workspace manifest, all three crate manifests, compile-time asset locations, release tooling, package documentation, and development/release documentation are affected. The runtime protocol and user interaction behavior are unchanged.
