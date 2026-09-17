# History timeline prototype

A standalone Python TUI for reviewing the proposed history view. It reads the
repository's Ferra palette and uses only synthetic in-memory events and costs.
It does not read or write actual history, call a model, or change the Rust UI.

## Run

From the repository root, with Python 3.11+:

```sh
uv run tools/prototypes/history_timeline.py
```

Alternatively, install the single dependency in your Python environment:

```sh
python -m pip install "prompt-toolkit>=3.0.48,<4"
python tools/prototypes/history_timeline.py
```

Use a true-color terminal at least **112 columns × 26 rows**; 132 × 42 is
recommended (the Windows backend reserves a terminal column). The canvas stays
110 columns wide. Resizing changes the viewport,
not the time scale. Smaller terminals show a resize notice. Statistics and the
timeline share one scrollable page; nothing is sticky. There is no title banner,
timeline legend or column heading, bottom detail panel, or shortcut bar. Both
the terminal cursor and the row highlight are hidden.

## Interaction

| Input | Action |
| --- | --- |
| Wheel, Up/Down, j/k | Move through page rows |
| PgUp/PgDn, Ctrl-U/Ctrl-D | Move one viewport |
| Home/End, g/G | Top/bottom of the page |
| Tab / Shift-Tab | Next/previous occupied slot |
| n / p | Next/previous user turn |
| Click | Select a slot |
| Enter | Cycle events sharing the selected slot |
| s | Toggle per-model totals |
| q, Escape, Ctrl-C | Exit |

## Reading the sketch

- A **turn** starts at `USER` and ends only at `AGENT STOP`. Reasoning, tool
  requests, tool results, and answers are events inside that turn. An answer or
  a model switch alone does not end it.
- Each row represents **5 seconds**, including idle time. Events in the same
  slot share a row; Enter cycles their exact timestamps and types in place,
  without stretching the axis. There is no zoom or automatic gap compression.
- The left column shows the event's model context (the invoking model for tool
  events); user events have no model. The middle shows event types. The right
  shows whole-turn totals at `AGENT STOP`, or an explicitly open subtotal at
  the last observed event of the unfinished turn.
- Global and per-model totals include all observed usage, including the open
  turn. Usage is attached once per model response, never copied onto tool or
  reasoning blocks. A mixed-model turn contributes to both models' totals.
- For this fixture, input, output, cache-read, and cache-write tokens are
  **disjoint** categories. Real adapters would need to normalize their native
  usage before applying this sum. Costs are synthetic USD amounts, not a price
  catalog or an estimate of a real bill. `+ ?` marks a known subtotal with
  missing prices; entirely unknown costs remain `unknown`, not zero.

The sample includes seven closed turns, an unfinished turn, simultaneous tool
blocks, an intra-turn model switch, and unavailable pricing. Production history
schema changes, adapter collection, migrations, and the real history page are
intentionally deferred pending feedback on the prototype.
