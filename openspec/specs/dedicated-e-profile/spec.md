# dedicated-e-profile Specification

## Purpose
TBD - created by archiving change rename-dsh-profile-to-e. Update Purpose after archive.

## Requirements

### Requirement: The product uses the dedicated e profile
The `dshe` launcher and setup command SHALL use `e` as the dedicated DSH profile name. A fresh setup SHALL provision the bridge below `%DSH_HOME%\profiles\e`, invoke the DSH plugin installer with `--profile e`, and record `e` as the setup-record profile value.

#### Scenario: Fresh setup
- **WHEN** `%DSH_HOME%\profiles\e` and `%DSH_HOME%\profiles\dshe` are both absent and the user runs `dshe setup`
- **THEN** setup creates and installs the bridge in `%DSH_HOME%\profiles\e` and subsequent launcher commands target `dsh --profile e`

### Requirement: Existing legacy profile migrates without configuration loss
Before provisioning the current profile, setup SHALL rename `%DSH_HOME%\profiles\dshe` to `%DSH_HOME%\profiles\e` when the legacy directory exists and the current directory does not. It SHALL then complete ordinary setup against the renamed directory, preserving valid existing profile files and unrelated dependencies and patch entries.

#### Scenario: Legacy profile is the only dedicated profile
- **WHEN** `%DSH_HOME%\profiles\dshe` exists, `%DSH_HOME%\profiles\e` does not exist, and the user runs `dshe setup`
- **THEN** setup moves the legacy directory to `%DSH_HOME%\profiles\e`, preserves its unrelated configuration, and writes a successful setup record whose profile is `e`

#### Scenario: Current profile already exists
- **WHEN** both `%DSH_HOME%\profiles\dshe` and `%DSH_HOME%\profiles\e` exist and the user runs `dshe setup`
- **THEN** setup SHALL leave `%DSH_HOME%\profiles\dshe` untouched and provision only `%DSH_HOME%\profiles\e`

### Requirement: Profile-targeted guidance names e
Operator diagnostics, development mount defaults, compatibility-smoke examples, and maintained architecture documentation SHALL name `e` whenever referring to the dedicated DSH profile. They SHALL continue to use `dshe` for the executable, setup command, configuration directory, and setup-record filename.

#### Scenario: Bridge connection repair guidance
- **WHEN** the client reports that its bridge route is unavailable
- **THEN** the guidance names `dsh --profile e`, `mount-bridge.ps1 -Profile e`, and `dsh plugin --profile e install` as the profile-targeted repair commands
