# /// script
# requires-python = ">=3.11"
# dependencies = ["prompt-toolkit>=3.0.48,<4"]
# ///
"""Read-only history timeline sketch. All events and billing values are synthetic."""

from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass
from datetime import datetime, timedelta
from decimal import Decimal
from pathlib import Path
import tomllib

from prompt_toolkit import Application
from prompt_toolkit.data_structures import Point
from prompt_toolkit.filters import Condition
from prompt_toolkit.formatted_text.utils import split_lines
from prompt_toolkit.key_binding import KeyBindings
from prompt_toolkit.layout import (
    ConditionalContainer,
    HSplit,
    Layout,
    ScrollOffsets,
    VSplit,
    Window,
)
from prompt_toolkit.layout.controls import FormattedTextControl, UIContent, UIControl
from prompt_toolkit.layout.margins import ScrollbarMargin
from prompt_toolkit.mouse_events import MouseEvent, MouseEventType
from prompt_toolkit.output import ColorDepth
from prompt_toolkit.styles import Style


SECONDS_PER_ROW = 5
CANVAS_WIDTH = 110
EPOCH = datetime(2026, 9, 18, 14, 30)
MODELS = (
    "anthropic/claude-sonnet-4.5",
    "openai/gpt-5.2",
    "deepseek/deepseek-v3.2",
)
MODEL_COLORS = dict(zip(MODELS, ("coral", "rose", "sage")))
KIND_COLORS = {
    "USER": "blush",
    "THINK": "bark",
    "ASSISTANT": "coral",
    "TOOL": "sage",
    "ANSWER": "rose",
    "AGENT STOP": "sage",
    "MODEL": "honey",
}


@dataclass(frozen=True)
class Usage:
    model: str
    input: int
    output: int
    cache_read: int
    cache_write: int
    cost: Decimal | None

    @property
    def tokens(self) -> int:
        return self.input + self.output + self.cache_read + self.cache_write


@dataclass(frozen=True)
class Event:
    seconds: int
    kind: str
    model: str | None
    detail: str
    usage: Usage | None = None


@dataclass(frozen=True)
class Turn:
    number: int
    title: str
    events: tuple[Event, ...]

    @property
    def complete(self) -> bool:
        return self.events[-1].kind == "AGENT STOP"

    @property
    def usage(self) -> tuple[Usage, ...]:
        # Billing is attached once to each assistant response, not each block.
        return tuple(event.usage for event in self.events if event.usage is not None)


def token_total(usages: tuple[Usage, ...]) -> int:
    return sum(usage.tokens for usage in usages)


def cost_label(usages: tuple[Usage, ...]) -> str:
    known = [usage.cost for usage in usages if usage.cost is not None]
    missing = len(known) != len(usages)
    if missing and not known:
        return "unknown"
    amount = sum(known, Decimal(0))
    return f"${amount:.4f}" + (" + ?" if missing else "")


def demo_turns() -> tuple[Turn, ...]:
    recipes = (
        (0, 85, 0, 0, "Inspect the history pipeline"),
        (110, 70, 1, 1, "Locate the turn boundaries"),
        (210, 115, 2, 2, "Review usage metadata"),
        (350, 95, 0, 1, "Hand off to another model within one turn"),
        (470, 80, 1, 1, "Sketch the timeline layout"),
        (610, 105, 0, 0, "Check a longer tool sequence"),
        (750, 65, 2, 2, "Summarize the proposed interface"),
        (865, 45, 1, 1, "An unfinished turn with unknown pricing"),
    )
    turns = []
    for index, (start, duration, first, last, title) in enumerate(recipes, 1):
        model, final_model = MODELS[first], MODELS[last]
        open_turn = index == len(recipes)
        request = Usage(
            model, 1500 + index * 370, 260 + index * 41, 4000 + index * 650, 0,
            None if open_turn else Decimal("0.0137") * index,
        )
        answer = Usage(
            final_model, 800 + index * 190, 490 + index * 87,
            6100 + index * 510, 400 if last == 0 else 0,
            Decimal("0.0183") * index,
        )
        events = [
            Event(start, "USER", None, title),
            Event(start + 5, "THINK", model, "Reasoning block; no separate usage charge"),
            Event(start + 17, "ASSISTANT", model, "One model response requesting tools", request),
            Event(start + 20, "TOOL", model, "read: inspect relevant source"),
            Event(start + 22, "TOOL", model, "search: locate matching symbols"),
            Event(start + 35, "TOOL", model, "Tool result received; not a model response"),
        ]
        if first != last:
            events.append(Event(start + 40, "MODEL", final_model, "Model changes; the turn does not end"))
        events.append(Event(start + 45, "THINK", final_model, "Reasoning continues inside the same turn"))
        if not open_turn:
            events.extend((
                Event(start + duration - 7, "ANSWER", final_model, "Final assistant response; wait for agent stop", answer),
                Event(start + duration, "AGENT STOP", final_model, "Agent stopped working; close this turn"),
            ))
        turns.append(Turn(index, title, tuple(events)))
    return tuple(turns)


def field(text: str, width: int, right: bool = False) -> str:
    clipped = text if len(text) <= width else text[:width - 1] + "…"
    return clipped.rjust(width) if right else clipped.ljust(width)


def fragment(text: str, color: str = "mist", bold: bool = False) -> tuple[str, str]:
    return (f"class:{color}" + (" bold" if bold else ""), text)


def clock(seconds: int) -> str:
    return (EPOCH + timedelta(seconds=seconds)).strftime("%H:%M:%S")


class Timeline(UIControl):
    def __init__(self, view: HistoryPrototype) -> None:
        self.view = view

    def is_focusable(self) -> bool:
        return True

    def create_content(self, width: int, height: int) -> UIContent:
        self.view.body_height = height
        header = self.view.header_rows
        return UIContent(
            get_line=lambda row: header[row] if row < len(header) else self.view.timeline_row(row - len(header)),
            line_count=len(header) + self.view.row_count,
            cursor_position=Point(x=0, y=len(header) + self.view.selected),
            show_cursor=False,
        )

    def mouse_handler(self, event: MouseEvent):
        if event.event_type == MouseEventType.SCROLL_UP:
            self.view.move(-3)
        elif event.event_type == MouseEventType.SCROLL_DOWN:
            self.view.move(3)
        elif event.event_type == MouseEventType.MOUSE_UP:
            self.view.select(event.position.y - len(self.view.header_rows))
        else:
            return NotImplemented
        return None


class HistoryPrototype:
    def __init__(self) -> None:
        self.turns = demo_turns()
        self.usages = tuple(usage for turn in self.turns for usage in turn.usage)
        self.by_model = {
            model: tuple(usage for usage in self.usages if usage.model == model)
            for model in MODELS
        }
        self.buckets: dict[int, list[tuple[Turn, Event]]] = defaultdict(list)
        for turn in self.turns:
            for event in turn.events:
                self.buckets[event.seconds // SECONDS_PER_ROW].append((turn, event))
        self.row_count = max(self.buckets) + 1
        self.collision = 0
        self.body_height = 12
        self.show_models = True
        self.header_rows = self.build_header()
        self.selected = -len(self.header_rows)
        self.app: Application | None = None
        self.timeline_window: Window | None = None

    def select(self, row: int) -> None:
        self.selected = max(-len(self.header_rows), min(self.row_count - 1, row))
        self.collision = 0
        if self.app is not None:
            self.app.invalidate()

    def move(self, delta: int) -> None:
        self.select(self.selected + delta)
        if self.timeline_window is not None:
            last_top = max(0, len(self.header_rows) + self.row_count - self.body_height)
            self.timeline_window.vertical_scroll = max(
                0, min(last_top, self.timeline_window.vertical_scroll + delta),
            )

    def jump(self, direction: int, turns_only: bool = False) -> None:
        rows = (
            [turn.events[0].seconds // SECONDS_PER_ROW for turn in self.turns]
            if turns_only else sorted(self.buckets)
        )
        candidates = [row for row in rows if (row - self.selected) * direction > 0]
        if candidates:
            self.select(min(candidates) if direction > 0 else max(candidates))
            if turns_only and self.timeline_window is not None:
                self.timeline_window.vertical_scroll = max(0, len(self.header_rows) + self.selected - 1)

    def build_header(self):
        sections = [self.summary()]
        if self.show_models:
            sections.append(self.model_summary())
        sections.append([fragment("  " + "─" * 106, "umber")])
        return [line for section in sections for line in split_lines(section)]

    def summary(self):
        completed = sum(turn.complete for turn in self.turns)
        count = sum(len(turn.events) for turn in self.turns)
        return [
            fragment("\n  " + field("TOTAL TOKENS", 25) + field("TOTAL COST / USD", 30)
                     + field("TURNS", 22) + "EVENTS", "bark"),
            fragment("\n  " + field(f"{token_total(self.usages):,}", 25), "blush", True),
            fragment(field(cost_label(self.usages), 30), "coral", True),
            fragment(field(f"{completed} closed / 1 open", 22), "sage"),
            fragment(f"{count}  (not turns)", "mist"),
        ]

    def model_summary(self):
        lines = [fragment("\n  " + field("MODEL / observed usage", 42)
                          + field("TOKENS", 17, True) + field("COST / USD", 23, True), "bark")]
        for model, usage in self.by_model.items():
            lines.extend((
                fragment("\n  " + field(model, 42), MODEL_COLORS[model]),
                fragment(field(f"{token_total(usage):,}", 17, True), "mist"),
                fragment(field(cost_label(usage), 23, True), "blush"),
            ))
        return lines

    def timeline_row(self, row: int):
        entries = self.buckets.get(row, [])
        stamp = clock(row * SECONDS_PER_ROW) if entries or row % 6 == 0 else ""
        if not entries:
            active = any(
                turn.events[0].seconds // SECONDS_PER_ROW < row
                < turn.events[-1].seconds // SECONDS_PER_ROW
                for turn in self.turns
            )
            lines = [fragment("  " + field(stamp, 10) + " " * 32, "bark"),
                     fragment(" │ " if active else " ┆ ", "umber")]
        else:
            turn, event = entries[self.collision % len(entries)] if row == self.selected else entries[-1]
            models = list(dict.fromkeys(item.model for _, item in entries if item.model))
            model = models[0] if len(models) == 1 else "mixed models" if models else "—"
            kinds = list(dict.fromkeys(item.kind for _, item in entries))
            label = " + ".join(kinds)
            if len(entries) > len(kinds):
                label += f" ×{len(entries)}"
            if row == self.selected:
                stamp = clock(event.seconds)
                model = event.model or "—"
                label = event.kind
                if len(entries) > 1:
                    label += f" {self.collision % len(entries) + 1}/{len(entries)}"
            color = KIND_COLORS[event.kind]
            node = "◆" if event.kind in ("USER", "AGENT STOP") else "●" if event.kind == "ANSWER" else "○"
            lines = [
                fragment("  " + field(stamp, 10), "bark"),
                fragment(field(model, 32), MODEL_COLORS.get(model, "bark")),
                fragment(f" {node} ", color),
                fragment(field(label, 23), color, event.kind in ("USER", "AGENT STOP")),
            ]
            if any(item.kind == "AGENT STOP" for _, item in entries):
                lines.append(fragment(
                    f"T{turn.number:02}  {token_total(turn.usage):>7,} tok  {cost_label(turn.usage)}", "blush", True,
                ))
            elif not turn.complete and event is turn.events[-1]:
                lines.append(fragment(
                    f"T{turn.number:02}  {token_total(turn.usage):,} tok  {cost_label(turn.usage)}  (open)", "honey",
                ))
            elif any(item.kind == "USER" for _, item in entries):
                lines.append(fragment(f"T{turn.number:02}  begin", "bark"))
        return lines

    def run(self) -> None:
        palette_path = Path(__file__).resolve().parents[2] / "crates/e-tui/assets/themes/ferra.toml"
        palette = tomllib.loads(palette_path.read_text(encoding="utf-8"))["colors"]
        styles = {name: color for name, color in palette.items()}
        styles.update({
            "": f"bg:{palette['night']} {palette['mist']}",
            "base": f"bg:{palette['night']} {palette['mist']}",
            "scrollbar.background": f"bg:{palette['night']} {palette['umber']}",
            "scrollbar.button": f"bg:{palette['umber']}",
        })
        bindings = KeyBindings()

        def bind(keys, action):
            for key in keys:
                bindings.add(key)(lambda event, action=action: action())

        bind(("q", "escape", "c-c"), lambda: self.app.exit())
        bind(("j", "down"), lambda: self.move(1))
        bind(("k", "up"), lambda: self.move(-1))
        bind(("pagedown", "c-d"), lambda: self.move(max(1, self.body_height - 2)))
        bind(("pageup", "c-u"), lambda: self.move(-max(1, self.body_height - 2)))
        bind(("g", "home"), lambda: self.select(-len(self.header_rows)))
        bind(("G", "end"), lambda: self.select(self.row_count - 1))
        bind(("n",), lambda: self.jump(1, turns_only=True))
        bind(("p",), lambda: self.jump(-1, turns_only=True))
        bind(("tab",), lambda: self.jump(1))
        bind(("s-tab",), lambda: self.jump(-1))

        @bindings.add("enter")
        def cycle(event):
            self.collision += 1

        @bindings.add("s")
        def toggle_stats(event):
            old_height = len(self.header_rows)
            self.show_models = not self.show_models
            self.header_rows = self.build_header()
            delta = len(self.header_rows) - old_height
            if self.selected < 0:
                self.select(min(-1, self.selected - delta))
            if self.timeline_window.vertical_scroll:
                self.timeline_window.vertical_scroll = max(0, self.timeline_window.vertical_scroll + delta)

        def text_window(content, height, style=""):
            return Window(FormattedTextControl(content), height=height, style=style,
                          always_hide_cursor=True, wrap_lines=False, char=" ")

        timeline = Window(
            Timeline(self),
            width=CANVAS_WIDTH,
            right_margins=[ScrollbarMargin()],
            scroll_offsets=ScrollOffsets(top=0, bottom=0),
            always_hide_cursor=True,
            char=" ",
        )
        self.timeline_window = timeline
        @Condition
        def enough_space():
            if self.app is None:
                return True
            size = self.app.output.get_size()
            return size.columns >= CANVAS_WIDTH and size.rows >= 26

        warning = text_window([
            fragment("\n  HISTORY / TIMELINE  ·  FERRA\n", "coral", True),
            fragment("  Resize to at least 112 columns × 26 rows.\n", "blush"),
            fragment("  The canvas and 5s/row scale stay fixed.  q: quit", "bark"),
        ], 4)
        root = HSplit([
            ConditionalContainer(VSplit([Window(char=" "), timeline, Window(char=" ")]), enough_space),
            ConditionalContainer(HSplit([warning, Window(char=" ")]), ~enough_space),
        ], style="class:base")
        self.app = Application(
            layout=Layout(root, focused_element=timeline),
            key_bindings=bindings,
            style=Style.from_dict(styles),
            color_depth=ColorDepth.TRUE_COLOR,
            full_screen=True,
            mouse_support=True,
            refresh_interval=0.5,
        )
        self.app.run()


if __name__ == "__main__":
    HistoryPrototype().run()
