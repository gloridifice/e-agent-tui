# CJK-Aware Greedy Line Wrapping — Design

> Status: Historical
> Authority: Non-normative. This document preserves the completed research and implementation record; current wrapping invariants live in the client architecture and source.

Implementation status at completion: **implemented** — `unicode-linebreak` 0.1.5 (crates.io) is integrated into
`crates/e-tui/src/wrap.rs` (words split at UAX #14 break opportunities, whitespace handling
unchanged, over-wide words fall back to grapheme splitting). This document remains the research and
design record.

Scope: the Rust client only (`crates/e-tui/src/wrap.rs`, shared by transcript layout, Preview,
content cards, and the input bar). No bridge/protocol impact.

## 1. Problem

`word_wrap_ranges` treats every non-whitespace run as an atomic "word". Line break opportunities
exist **only** at whitespace runs. CJK text (CJK ideographs, kana, Hangul) is written without
spaces, so a CJK run never breaks until it overflows the row width, at which point the grapheme
fallback splits it per grapheme with no typographic rules. Consequences:

- **Kinsoku violations** (禁則処理): closing/ending punctuation (`，。、」』）！？：；／…`) and small
  kana (`ぁっゃ`) can start a row; opening brackets (`（「『`) can end a row.
- **Torn Latin words**: `世界abc` at width 5 wraps as `世界a` / `bc` even though `abc` fits whole
  on the next row.

> Note: Hangul jamo of one syllable form a single grapheme cluster (UAX #29), so the old wrapper
> already never split a syllable block; UAX #14's LB26 is therefore redundant here. The behavior
> this change adds is at the *grapheme* level: ideographs, kana, and Hangul syllables become
> independent break opportunities, with kinsoku gluing around punctuation.

Verified current outputs (`wrap_text`):

| input | width | current output | defect |
|---|---|---|---|
| `你好，世界` | 4 | `你好` / `，世` / `界` | fullwidth comma starts a row |
| `ああっあ` | 4 | `ああ` / `っあ` | small kana `っ` starts a row |
| `世界abc` | 5 | `世界a` / `bc` | Latin word torn across rows |
| `한` (jamo) | 6 | `하` / `ᆫ` | Hangul syllable block split |

## 2. How typesetting engines do it (research)

> Note: web search was unavailable in this session (search API credentials failed), so the findings
> below were verified directly against the local clone `target/unicode-linebreak` — its
> `LineBreak-15.0.0.txt` data and the rule set inlined in `gen-tables/src/main.rs` (rules
> LB1–LB31).

The common foundation of every real engine (Chromium/Blink, WebKit, Gecko, Pango, Qt, HarfBuzz
consumers, ICU `BreakIterator`) is the **Unicode Line Breaking Algorithm (UAX #14)**:

1. **LB1** — assign every code point a Line_Break class. Relevant classes (verified from
   `LineBreak-15.0.0.txt`):

   | class | meaning | examples (verified) |
   |---|---|---|
   | `ID` | ideographic | `4E00..9FFF` CJK ideographs, `3042` あ, most kana |
   | `CJ` | conditional Japanese starter | `3041` ぁ, `3063` っ, `30A1` ァ, `30C3` ッ, `30FC` ー |
   | `H2`/`H3` | Hangul syllables | `AC00` 가 … |
   | `JL`/`JV`/`JT` | Hangul jamo | `1100..115F` ᄀ… |
   | `CL` | closing punctuation | `3001..3002` 、。, `FF0C` ，, `FF0E` ．, `FF09` ）, `300D` 」 |
   | `OP` | opening punctuation | `300C` 「, `FF08` （ |
   | `EX` | exclamation/question | `FF01` ！, `FF1F` ？ |
   | `NS` | non-starter | `FF1A..FF1B` ：；, `3005` 々, `FF9E..FF9F` ゛゜ |
   | `IS` | infix separator | `002E` ., `003A..003B` :; |
   | `SY` | symbol | `002F` / |
   | `IN` | inseparable | `2026` … |
   | `SP` / `GL` | space / no-break space | `0020`, `00A0` NBSP |
   | `AL` | alphabetic | Latin letters |
   | `ZWJ` / `EM` | zero-width joiner / emoji modifier | `200D`, `1F3FB..1F3FF` |
   | `SA` | complex context | Thai `0E01..0E30` … |

2. **LB2–LB31** — decide per adjacent pair whether a break is Allowed, Forbidden, or Mandatory.
   The rules that produce the CJK typographic behavior (verified in the clone's rule table):

   - **LB31** — break everywhere not otherwise forbidden ⇒ `ID ÷ ID` (汉字之间可换行), kana, Hangul
     syllables, and mixed `ID ÷ AL` / `AL ÷ ID` boundaries.
   - **LB13** — `× CL / CP / EX / IS / SY`: no break before closing punctuation (行头禁则 for
     `，。、」』）！？：；／…`).
   - **LB14** — `OP ×`: no break after opening brackets (行末禁则 for `（「『…`).
   - **LB16** — `(CL|CP) × NS`; **LB21** — `× NS`: no break before non-starters (small kana, 々,
     ー; the default tailoring resolves `CJ → NS`).
   - **LB22** — `× IN`: no break before ellipsis.
   - **LB23–LB25** — numeric contexts: `1.5` and currency expressions stay together.
   - **LB26/LB27** — Korean syllable blocks stay together (`JL × JV × JT`, …).
   - **LB28** — `AL × AL`: Latin words do not break inside.
   - **LB30b** + explicit ZWJ handling — emoji base+modifier and ZWJ sequences do not split.
   - **LB18** — `SP ÷`: break after spaces; **LB7** — no break before spaces.

3. The engine then performs **greedy first-fit** over the break opportunities (CSS line breaking is
   greedy; Knuth–Plass justification is optional on top). CSS Text Level 3 maps this to
   `word-break: normal` + `line-break: normal` — the correct baseline for a terminal.

Japanese 禁則処理 (行头禁则 / 行末禁则) is thus **encoded in the LB rules themselves**, not a
separate pass. Engines additionally do dictionary segmentation for Thai/Lao (`SA`/`CB`) and
hyphenation for Western words; both are deliberately out of scope here (the clone tailors
`AI/SG/XX/SA → AL`, which is fine for a terminal).

## 3. Candidate library: `unicode-linebreak` 0.1.5

Facts verified from the local clone (`Cargo.toml`, `src/lib.rs`, `src/shared.rs`):

- A UAX #14 implementation, conforming to **Unicode 15.0.0** (`UNICODE_VERSION` constant).
- **Apache-2.0**, `no_std`, **zero runtime dependencies**, `rust-version = 1.56`, edition 2021 —
  compatible with this workspace and the existing `unicode-segmentation` / `unicode-width` style of
  dependency.
- API:
  - `linebreaks(s) -> impl Iterator<Item = (usize, BreakOpportunity)>` — byte index of the
    character *after* the break, tagged `Allowed` or `Mandatory` (a trailing `Mandatory` at end of
    text is always emitted).
  - `break_property(codepoint) -> BreakClass`, `split_at_safe(&str)` (for incremental wrapping).
- Data: compact trie tables generated from `LineBreak.txt` by `gen-tables` (a two-level property
  trie plus a ~45×45 state pair table). Published artifact is on the order of tens of KiB; exact
  size to be confirmed when building.
- Tailors applied: `AI/SG/XX/SA → AL`, `CJ → NS` — i.e. the UAX #14 *default* tailoring.
- Caveats:
  - The git clone at `target/unicode-linebreak` does **not** ship the generated `src/tables.rs`
    (it is produced by `gen-tables`, which requires `hashbrown`/`regex`); the crates.io published
    artifact includes it. `target/` is a build directory and is cleaned by `cargo clean`, so it
    must not be used as a path dependency as-is.
  - Break opportunities are reported at **char** boundaries; our tokens are **grapheme** clusters,
    so opportunities must be filtered to grapheme boundaries (one cheap check).
  - No hyphenation, no Thai dictionary — acceptable for this terminal (see §2).

## 4. Integration design (recommended)

Keep the existing architecture of `wrap.rs` untouched — tokens, `RowBuilder`, the deferred-
whitespace mechanism, the over-wide grapheme fallback, byte-range rows, and
`wrap_text_chunks`'s char/byte offset accounting. Change only the granularity of a "word":

1. **Tokenize** exactly as today (grapheme-based; consecutive whitespace → one space token,
   consecutive non-whitespace → one word token).
2. Run one `linebreaks(text)` pass per wrap call. For each word token, collect the internal break
   opportunities: byte offsets inside the token where UAX #14 reports `Allowed` **and** the offset
   is a grapheme boundary.
3. **Split word tokens at those opportunities during tokenize** — a word becomes a sequence of
   *segments* (maximal runs with no allowed internal break):
   - a CJK run splits into single-ideograph segments (`你好` → `你`, `好`);
   - `好，` stays glued (`好 × ，` per LB13);
   - `「` glues to the following character (`OP ×` per LB14);
   - a Latin word stays one segment (`AL × AL` per LB28);
   - a Hangul syllable block stays one segment (LB26).
4. The existing greedy fill then consumes whole segments. When the next segment does not fit, the
   row breaks before it — the current word-break path, now with finer granularity. The whitespace
   path (defer, then consume the run at a break) is **unchanged**: UAX #14's `SP ÷` (LB18)
   coincides with it, so all existing whitespace tests keep passing untouched.
5. A segment wider than the row still falls back to the existing grapheme splitting (preserves the
   "never exceed width" invariant; over-wide segments are rare — a huge emoji, a long Latin word,
   or a long numeric run).
6. `Mandatory` breaks: LF/CR/FF are whitespace and never occur inside a word token; the end-of-text
   mandatory break falls outside every token. Handle defensively (treat as a segment boundary) or
   assert.

### 4.1 Why not a full UAX #14 segment model including spaces?

Treating spaces as UAX #14 segments would move trailing whitespace into rows and change the
current, tested, desirable behavior ("whitespace run consumed by the break"). Keeping the existing
whitespace path makes the change purely additive.

### 4.2 Data structures

Split word tokens into segment tokens at tokenize time (no new field on `Token`; `RowBuilder` is
unchanged). Cost: one segment per ideograph — bounded by line length, and lines are width-capped in
practice. Alternative: store `Vec<usize>` of internal break offsets on `Token` and scan on demand;
only worth it if token-count profiling ever matters (the render cache rewraps only on width /
generation changes, so the extra tokens are per-rewrap, not per-frame).

### 4.3 Performance

`linebreaks()` is one linear pass per logical line, added to the existing tokenize + width-measure
passes. Rewraps are already cached by width/generation (`docs/client.md` render-cache section);
`split_at_safe` exists if incremental wrapping is ever needed. No measurable impact expected.

### 4.4 Adjacent issues (out of scope unless wanted)

- `NBSP` (U+00A0, `GL`) is currently classified as breakable whitespace by `char::is_whitespace`
  — typographically it must never break (LB12/LB12a). Fixing it touches the whitespace classifier;
  listed as an optional follow-up.
- `WJ` (word joiner) and `ZWSP` (zero-width space) inside words become correct for free once words
  are segmented (LB8/LB11); currently `ZWSP` inside a word is mishandled.
- No `line-break: strict` / `word-break: keep-all` tailoring; the UAX #14 default is the baseline.

### 4.5 Compatibility with the paced grapheme reveal (verified after reveal landed)

The presentation-only paced reveal (`crates/e-tui/src/reveal.rs`) is fully compatible with this
change; no reveal code needs to change. Verified against the current implementation:

- `RevealSignature` indexes only the **logical (pre-wrap) lines'** grapheme sequence and
  deliberately excludes styles and line boundaries (`reveal.rs` doc comment), so it is invariant
  under reflow. Callers build it from the unwrapped lines (`ui/transcript.rs` and
  `ui/region/preview.rs`), and the existing test
  `width_only_line_reflow_does_not_change_the_logical_signature` already guards reflow-only
  changes — the UAX #14 break-point change is exactly such a change. Reveal progress therefore
  never resets and the per-grapheme pacing is unchanged.
- Pipeline order is `logical lines → apply_reveal (clip + fade) → wrap at paint time`, so the
  clipped prefix is re-wrapped with the new break rules through the existing `reveal_dirty` /
  suffix-splice path.
- Fade age depends only on `revealed − global grapheme index`, not on row boundaries; reveal
  counts graphemes, not display columns, so CJK double-width graphemes are unaffected.
- Semantic/copy/cache content stays complete — this proposal only changes display break points,
  consistent with the reveal sidecar's "presentation-only" contract.

## 5. Behavior examples (proposed output)

Greedy first-fit over UAX #14 opportunities:

| input | width | current | proposed |
|---|---|---|---|
| `你好，世界` | 4 | `你好` / `，世` / `界` | `你` / `好，` / `世界` — `，` never starts a row |
| `ああっあ` | 4 | `ああ` / `っあ` | `あ` / `あっ` / `あ` — `っ` never starts a row |
| `世界abc` | 5 | `世界a` / `bc` | `世界` / `abc` — Latin word kept whole |
| `가나` | 2 | `가` / `나` | `가` / `나` — unchanged (syllables were already separate graphemes) |
| `「你好」世界` | 6 | `「你好` / `」世界` | `「你` / `好」世` / `界` — `」` never starts a row |
| `aa  bb` | 5 | `aa` / `bb` | `aa` / `bb` — whitespace path unchanged |
| `世界abc` | 7 | `世界abc` | `世界abc` — unchanged when everything fits |

Note: with kinsoku gluing, greedy CJK wrapping can produce short rows (`你` alone above). This is
inherent to greedy first-fit over unbreakable punctuation pairs and matches browser behavior; a
lookahead/justification pass (Knuth–Plass style) would be an optional future improvement, out of
scope here.

## 6. Test plan

Model layer (`crates/e-tui/src/wrap.rs` tests):

- The §5 table as unit tests (CJK/kana/Hangul/mixed breaks).
- Kinsoku: `，。、」』）！？：；／…` never starts a row; `（「『` never ends a row; small kana
  (`っゃぁ`) and `ー` never start a row.
- Latin word adjacent to CJK stays whole when it fits on its own row.
- Hangul jamo block not split when it fits; over-wide block still falls back to grapheme splitting.
- Numeric contexts (`1.5`, `¥100`) not split internally when they fit.
- Existing whitespace tests unchanged (spaces deferred/consumed identically).
- `wrap_text_chunks` char/byte offsets across dropped CJK boundaries.
- Emoji ZWJ / combining marks still never split (extend existing tests).
- `row_count_matches_materialized_word_wraps` extended with CJK/punctuation inputs.

UI layer (per AGENTS.md, spacing changes need UI-layer regression tests, not only model tests):
add a TestBackend case in `crates/e-tui/src/ui/main_pane_tests.rs` (which already owns the
wrapping regressions, e.g. `input_bar_word_wrap_keeps_cursor_anchored_after_consumed_space`,
`preview_wraps_long_lines_to_the_pane_width`) asserting cached wrapped row content/colors for a
CJK line (e.g. `你好，世界` wrapping with the comma at the end of row 1).

Commands: `cargo test --lib wrap` plus the touched UI test module; `cargo fmt --check`. (This
sandbox has no crates.io network access; build/verify in the normal dev shell.)

## 7. Dependency decision

- **Option A (recommended)** — add `unicode-linebreak = "0.1"` to `crates/e-tui/Cargo.toml` from
  crates.io; commit the root `Cargo.lock`. Zero runtime deps, Apache-2.0, `no_std`, Unicode 15.0.0.
  Maintenance note: it is a single-maintainer crate (axelf4), stable and widely used (e.g. by
  `textwrap`); the algorithm and tables are generated from Unicode data, so drift risk is low.
- **Option B (path dependency)** — use the clone at `target/unicode-linebreak` after generating
  `src/tables.rs` via `gen-tables`. Not recommended: `target/` is a build directory, and AGENTS.md
  retires the old vendoring fallback.
- **Option C (hand-rolled subset)** — implement a small classifier + pair-rule function modeled on
  the clone (~200 lines: `ID/CL/CP/EX/NS/IS/SY/IN/BA/HY/SP/GL/ZWJ/AL/CM/NU/PR/PO/HL` classes and
  rules LB13/14/16/18/21/22/23–25/26/28/31). Zero new dependency, full control, but must be
  maintained for parity with UAX #14 and carries more review/test burden.

Recommendation: **Option A**; keep the clone only as reference material. If adding the dependency
is unacceptable, Option C is viable but more work.

**Decision (implemented): Option A — `unicode-linebreak = "0.1.5"` from crates.io**, added to
`crates/e-tui/Cargo.toml`; the root `Cargo.lock` records `unicode-linebreak v0.1.5` (no transitive
dependencies).

## 8. Docs sync (after implementation)

- `docs/client.md` (~lines 61–64): update the "Wrap scanning" paragraph — break opportunities come
  from UAX #14 (whitespace + CJK/kinsoku), replacing "whole words … separating whitespace".
- `docs/design.md`: wording check on the wrapping notes (lines ~203, 296–302); likely "word" →
  "break segment" only.
- `docs/README.md`: this proposal already added to the index.
- `ui.rs` help_overlay: no key changes → no update.
- AGENTS.md: no change (behavioral note lives in `client.md`).

## 9. Decisions and open items

1. **Greedy short-row artifacts (accepted)** — greedy first-fit over UAX #14 opportunities can leave
   short rows (e.g. `你` alone at width 4); this is browser-equivalent behavior and no lookahead is
   planned.
2. **ZWSP/WJ fixed as a bonus; NBSP remains a follow-up** — because words are now segmented by UAX
   #14, zero-width space (break after) and word joiner (no break around) inside a word behave
   correctly. NBSP (U+00A0, class `GL`) is still classified as breakable whitespace by
   `char::is_whitespace`; fixing it means teaching the whitespace classifier about non-breaking
   whitespace, tracked as a follow-up.
3. **Dependency (resolved)** — Option A: `unicode-linebreak 0.1.5` from crates.io (see §7).
