## 1. Profile runtime and migration

- [x] 1.1 Change the dedicated-profile constant and all profile-targeted runtime diagnostics to `e` while preserving `dshe` product identifiers.
- [x] 1.2 Migrate a legacy `profiles/dshe` directory to `profiles/e` during setup only when the current directory is absent, with actionable failure reporting.
- [x] 1.3 Update focused setup, launcher, and bridge-I/O tests for the new name and migration behavior.

## 2. Development tooling and documentation

- [x] 2.1 Make the development bridge mount default and profile-targeted smoke examples use `e`.
- [x] 2.2 Update project instructions and maintained architecture documentation for the dedicated `e` profile and migration procedure.

## 3. Verification

- [x] 3.1 Run focused Rust tests and formatting checks for the changed modules.
- [x] 3.2 Verify `dsh --profile e --dump-config` after migration/setup without changing unrelated profile configuration.
