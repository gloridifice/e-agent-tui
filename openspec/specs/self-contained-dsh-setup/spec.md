# self-contained-dsh-setup Specification

## Purpose
TBD - created by archiving change add-self-contained-setup-command. Update Purpose after archive.
## Requirements
### Requirement: The executable contains the bridge runtime
The build system SHALL embed the bridge package manifest, canonical protocol contract, and every production JavaScript module under `bridge/src` in the `dshe` executable, and SHALL generate a deterministic content digest that changes when an embedded path or its bytes change. The runtime setup command MUST NOT require a repository checkout, `tools/mount-bridge.ps1`, or loose bridge source files.

#### Scenario: Setup runs outside the repository
- **WHEN** an installed `dshe` executable is run with `setup` after the source checkout has been moved or deleted
- **THEN** it can materialize the complete bridge runtime from bytes embedded in that executable

#### Scenario: A bridge module is added
- **WHEN** a new production `.js` file is added directly under `bridge/src` and the client is rebuilt
- **THEN** the generated embedded bundle includes the file and its bundle digest reflects the addition

### Requirement: The setup command is explicit and self-contained
The CLI SHALL reserve `dshe setup` as the only setup command. It SHALL execute setup without acquiring or spawning the DSH service, reading the bridge token, connecting WebSocket, or initializing the terminal UI. The obsolete spelling `dshe install` SHALL NOT perform setup and MUST direct the user to `dshe setup`.

#### Scenario: User invokes setup
- **WHEN** the user runs `dshe setup`
- **THEN** the setup operation runs and exits without entering the TUI startup path

#### Scenario: User invokes the obsolete command name
- **WHEN** the user runs `dshe install`
- **THEN** the command exits unsuccessfully with an English message that explicitly says to run `dshe setup`

### Requirement: DSH home resolution is consistent
Setup and normal startup SHALL treat an unset, empty, or whitespace-only `DSH_HOME` as the platform user home joined with `.dsh`, SHALL honor a non-empty custom `DSH_HOME`, and SHALL pass the resolved value to any DSH plugin child process.

#### Scenario: DSH_HOME is unset
- **WHEN** the user runs `dshe setup` without `DSH_HOME`
- **THEN** setup operates on the `dshe` profile below the user's default `.dsh` directory and the child DSH command uses the same directory

#### Scenario: DSH_HOME is empty
- **WHEN** `DSH_HOME` contains only empty or whitespace content
- **THEN** setup uses the same default `.dsh` directory as if the variable were unset

#### Scenario: DSH_HOME is custom
- **WHEN** `DSH_HOME` contains a non-empty custom path
- **THEN** both Rust filesystem operations and the child DSH command use that custom path

### Requirement: Setup provisions the dedicated profile idempotently
`dshe setup` SHALL stage and install the embedded bridge at `%DSH_HOME%\profiles\dshe\packages\dsh-tui-bridge`, create the dedicated profile when absent, and otherwise modify only the registrations required by the bridge. It SHALL ensure the workspace dependency, `packages/*` workspace entry, and `tui-bridge` patch insertion exist exactly once, then SHALL run the equivalent of `dsh plugin --profile dshe install` with inherited output. Unrelated valid profile configuration MUST be preserved.

#### Scenario: Fresh profile setup
- **WHEN** the dedicated `dshe` profile does not exist and setup prerequisites are available
- **THEN** setup creates the dedicated base/web profile, writes the embedded bridge, registers it, installs its dependencies, and completes successfully

#### Scenario: Existing customized profile setup
- **WHEN** the dedicated profile already contains unrelated dependencies, workspace settings, bundles, or patch entries
- **THEN** setup preserves those entries while adding or correcting only the bridge-owned registrations

#### Scenario: Setup is repeated
- **WHEN** the user runs `dshe setup` more than once with the same executable
- **THEN** the resulting bridge files and profile registrations remain equivalent and no duplicate workspace or patch entries are added

#### Scenario: Existing bridge is updated
- **WHEN** setup runs from an executable whose embedded bridge differs from the currently materialized package
- **THEN** the installation-owned bridge directory is replaced with the embedded runtime before dependency installation and validation

### Requirement: Setup records only validated success
After the DSH plugin command succeeds, setup SHALL validate the materialized bridge, profile dependency, workspace entry, patch insertion, and installed workspace package or link. It SHALL then atomically write a versioned setup record containing the dedicated profile identity, embedded bridge digest, and wire protocol version. It MUST NOT create or update that success record for a failed or unvalidated setup.

#### Scenario: Plugin installation succeeds
- **WHEN** bridge extraction, profile mutation, the DSH plugin command, and post-install validation all succeed
- **THEN** setup atomically records the current embedded bridge as ready and tells the user to restart any running DSH service before running `dshe`

#### Scenario: Plugin installation fails
- **WHEN** the DSH plugin child process cannot start or exits unsuccessfully
- **THEN** setup exits unsuccessfully without recording the attempted bundle as ready and tells the user how to address the command failure and rerun `dshe setup`

#### Scenario: Post-install validation fails
- **WHEN** the plugin command reports success but a required profile registration or installed package path is missing
- **THEN** setup exits unsuccessfully without recording success and identifies the failed validation and the next repair action

### Requirement: TUI startup requires current setup
Every CLI path that starts the TUI SHALL classify setup as ready, missing, outdated, or damaged before acquiring DSH, reading the token, connecting WebSocket, or initializing the terminal. Only ready setup SHALL proceed. Setup is ready only when the setup record is supported, its embedded bridge digest matches the current executable, and required profile/install structures remain present.

#### Scenario: Setup has never completed
- **WHEN** a user runs `dshe` without a valid setup record
- **THEN** the process exits unsuccessfully before any launcher or terminal side effect and tells the user to run `dshe setup` and then `dshe` again

#### Scenario: Embedded bridge is newer or different
- **WHEN** the setup record's bridge digest differs from the digest embedded in the running executable
- **THEN** startup exits before launcher side effects and tells the user to run `dshe setup`, restart any running DSH service, and run `dshe` again

#### Scenario: Installed setup is damaged
- **WHEN** the setup record matches the executable but a required bridge file, profile registration, or installed workspace package/link is absent
- **THEN** startup exits before launcher side effects, identifies setup as incomplete, and tells the user to run `dshe setup` to repair it

#### Scenario: Setup is ready
- **WHEN** the record matches the executable and all required setup structures are present
- **THEN** normal launcher acquisition and TUI startup may proceed unchanged

#### Scenario: Legacy script-mounted profile has no record
- **WHEN** bridge files installed by the former mount script exist but no successful setup record exists
- **THEN** startup treats setup as missing and directs the user through the one-time `dshe setup` migration

### Requirement: Setup diagnostics are English and actionable
Every failure message produced by CLI setup routing, setup execution, setup readiness checks, or setup-related launcher guidance SHALL be in English. Each message MUST identify the failed condition and state a concrete next action, including the exact command when a command can resolve the condition. Raw operating-system errors, paths, commands, and exit codes MAY be included as supporting details but MUST NOT be the only guidance.

#### Scenario: Required launch tools are unavailable
- **WHEN** neither global `dsh` nor the supported `npx` fallback can be invoked
- **THEN** setup identifies the missing prerequisite and tells the user how to install DSH before rerunning `dshe setup`

#### Scenario: A profile file cannot be parsed
- **WHEN** setup cannot safely update an existing profile file because it is malformed
- **THEN** the error names the file, explains that it could not be parsed, and tells the user to repair or restore that file before rerunning `dshe setup`

#### Scenario: A profile file cannot be written
- **WHEN** setup cannot create or replace a required file
- **THEN** the error names the path, describes the write failure, and tells the user to check access or permissions before rerunning `dshe setup`

#### Scenario: DSH plugin command fails
- **WHEN** `dsh plugin --profile dshe install` or its fallback exits unsuccessfully
- **THEN** the error identifies the attempted command and exit result and tells the user to review the preceding package-manager output, fix the reported issue, and rerun `dshe setup`

#### Scenario: Existing launcher guidance is reached
- **WHEN** a launcher failure still determines that the dedicated bridge/profile needs repair
- **THEN** its English error directs the user to `dshe setup` rather than `tools\mount-bridge.ps1`

