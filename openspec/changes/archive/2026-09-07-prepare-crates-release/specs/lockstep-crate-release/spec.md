## ADDED Requirements

### Requirement: Synchronized workspace version
The Rust workspace SHALL declare one inherited package version for `e-tui`, `e-dsh`, and `e-pi`, beginning at `0.0.1`. The release workflow MUST update the versioned internal dependency requirement and lockfile together with that package version.

#### Scenario: Preparing a later release
- **WHEN** a maintainer requests a workspace version increment through the release tool
- **THEN** all three packages, the published `e-tui` dependency requirement, and `Cargo.lock` describe the same release version

### Requirement: Self-contained package archives
Each Rust package archive SHALL contain every source and compile-time resource needed to verify it without access to the repository layout outside the extracted package.

#### Scenario: Verifying package archives
- **WHEN** Cargo packages and verifies each workspace member
- **THEN** `e-tui`, `e-dsh`, and `e-pi` build successfully from their extracted package directories

### Requirement: Canonical generated release assets
The top-level `bridge/` directory SHALL remain authoritative for the embedded DSH bridge. A deterministic synchronization command MUST generate the package-local bridge mirror, and check mode MUST reject missing, stale, or extra mirrored files.

#### Scenario: Bridge source changes without synchronization
- **WHEN** an authoritative production bridge file differs from the package-local mirror
- **THEN** the generated-asset check fails with an actionable synchronization command

### Requirement: Ordered guarded publication
The release workflow SHALL provide a non-publishing dry run by default and an explicit execution mode that publishes `e-tui` before its dependent adapters and creates one shared project version tag.

#### Scenario: Release dry run
- **WHEN** a maintainer runs the release command without explicit execution
- **THEN** package, version, dependency, commit, tag, and publication operations are reported without uploading crates

#### Scenario: Executed lockstep release
- **WHEN** a maintainer executes an approved release from the permitted branch with valid registry credentials
- **THEN** all three packages are published in dependency order under one version and one project tag

### Requirement: Published package metadata
Each package SHALL provide its license, repository, homepage, description, and package-specific README in crates.io metadata.

#### Scenario: Inspecting package metadata
- **WHEN** Cargo constructs any of the three package archives
- **THEN** the archive metadata identifies the project and includes documentation appropriate to that package
