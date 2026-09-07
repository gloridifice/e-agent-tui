## 1. Workspace Release Configuration

- [x] 1.1 Centralize package metadata and version `0.0.1` in the workspace manifest
- [x] 1.2 Make all package manifests inherit workspace metadata and use a versioned workspace `e-tui` dependency
- [x] 1.3 Configure cargo-release for shared versions, dependency updates, publication, and one project tag

## 2. Package Self-Containment

- [x] 2.1 Move the default key mapping authority into the `e-tui` package and update compile-time/documentation references
- [x] 2.2 Add deterministic bridge release-asset synchronization with stale and extra-file detection
- [x] 2.3 Make `e-dsh` build exclusively from its package-local generated bridge assets
- [x] 2.4 Add package-specific README files and controlled package contents

## 3. Validation and Documentation

- [x] 3.1 Add generated-asset and archive verification commands to the maintained testing/release workflow
- [x] 3.2 Generate current bridge release assets and verify all three package archives independently
- [x] 3.3 Validate the OpenSpec change and record all tasks complete
