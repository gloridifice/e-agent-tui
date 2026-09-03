## ADDED Requirements

### Requirement: Architecture graph covers nested production modules
The Rust architecture guard SHALL recursively discover production modules under `e-dsh`, `e-pi`, and `e-tui`, assign each source file a stable fully qualified module identity, and include dependencies expressed through `crate`, `self`, and `super` paths. Test-only modules and module-containment declarations MUST NOT create production dependency edges.

#### Scenario: Nested cycle is introduced
- **WHEN** two or more nested production modules depend on one another through a strongly connected path
- **THEN** the architecture check fails and reports the fully qualified module identities in the cycle

#### Scenario: Relative imports resolve to production modules
- **WHEN** a nested module imports another discovered module through `self`, one or more `super` prefixes, a grouped import, or an absolute `crate` path
- **THEN** the architecture graph records an edge to the same canonical target module

#### Scenario: Test-only dependency is ignored
- **WHEN** a dependency exists only inside a `#[cfg(test)]` test module
- **THEN** it does not create a production graph edge or a false cycle

#### Scenario: Nested workspace graph remains acyclic
- **WHEN** the architecture check scans the complete production trees of all three Rust packages
- **THEN** it finds no multi-node strongly connected component and preserves the existing package and rendering-layer boundary assertions
