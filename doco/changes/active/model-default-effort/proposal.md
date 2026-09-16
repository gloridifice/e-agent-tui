<!-- doco:change mode=proposal-only -->
# Per-model default effort in e

## Purpose

Let users persist a preferred reasoning effort for an exact provider/model route in e, without changing Pi's configuration. This extends the existing model command and picker rather than adding a backend setting.

## Scope and acceptance

The new command is:

```text
/model <model-id> set-default-effort <effort>
```

- It resolves the same canonical or unique bare model reference as model selection. It validates and normalizes the effort against that target model's declared efforts, not the current model's. Invalid, ambiguous, unsupported, or extra arguments remain local errors.
- Completion covers model references, `set-default-effort`, and the target model's supported efforts. Existing model and effort completion/navigation remain intact.
- Store exact provider, model, and effort ids in `model_default_efforts`, an array in e's shared `config.toml`, empty in the embedded defaults. Reject malformed/duplicate route entries. Use the existing adapter-owned config persistence effect and error reporting; no Pi config, bridge protocol, or backend request is involved in setting a default.
- Setting a default changes only the local preference. Subsequent direct or picker model selection (including letter shortcuts and deferred drafts) uses a supported stored default. Marked temporary prompts capture it at admission and retain it while queued. Explicit effort selection, session resume, and temporary route restoration retain their authoritative effort. Missing or unsupported saved defaults do not fabricate supported efforts or block ordinary selection.
- The model menu displays the stored effort id immediately after the model name, before an optional letter mark, using the existing subdued activity-label tone (umber in the default Ferra theme). Keep suffix width accounting Unicode-safe and perform no render-time I/O.
- Preserve unrelated uncommitted work, including current compaction/reload command and configuration changes. No effort-level projection changes, default-model setting, clear-default command, or automatic startup/session-resume override is included.

### Verification

Scoped tests cover config round trips and validation, route-specific command parsing/completion, persistence effects, picker selection, explicit effort precedence, and queued temporary model restoration. Check adapter config persistence using a temporary directory. Run formatting and compilation checks proportionate to the shared frontend changes; no real user configuration may be modified by tests.

### Current-document impact

Document the new e-only persisted preference in configuration/storage, its selection semantics in interaction/sessions, and its menu annotation in presentation. Add concise command usage to the user README and update command-local hints/translations. Existing keys and detailed help navigation remain unchanged.

## Result

Implemented; the change remains active, not completed or archived.

The model command and target-specific completion persist exact-route preferences through e's existing config effect. The model menu annotates saved efforts, and direct/menu/marked selections apply supported defaults without replacing explicit or restored efforts. The preference schema stays a leaf; catalog policy validates defaults against model capabilities. Current configuration, interaction, and presentation contracts and the README are synchronized.

Verification passed:

- `cargo test -p e-tui model_default`: 9 feature regressions.
- `cargo test -p e-tui model`: 48 tests; `effort`: 21; `config::tests`: 11; `i18n::tests`: 4.
- `cargo test -p e-pi config::tests`: 1 temporary-directory persistence test.
- `cargo test -p e-dsh -p e-pi --test architecture`: 18 tests.
- `cargo fmt --all --check`, `git diff --check`, and `doco check model-default-effort`.
- `cargo clippy --workspace --all-targets`: completed with existing warnings outside this feature; the new-code warning was corrected.

No live terminal or real user configuration was used for validation. No implementation scope is deferred; unrelated concurrent changes were preserved.
