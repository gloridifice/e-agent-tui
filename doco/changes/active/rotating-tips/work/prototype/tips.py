#!/usr/bin/env python3
"""Interactive visual prototype: rotating Tips inside the composer's bottom rule.

Run:     python doco/changes/active/rotating-tips/work/prototype/tips.py
Install: python -m pip install prompt_toolkit "wcwidth>=0.6"

F1 controls on/off; F2 working/idle; F3 next tip; F4 border-status scenario;
F5 Chinese/English; F6 narrow/full width; Ctrl+C or Ctrl+Q quit.
Type freely. Enter simulates submission; Esc stops work; Ctrl+J adds a line.
Only mock data is used. No backend calls, clipboard access, or shell execution.
"""

from __future__ import annotations

import argparse
import asyncio
import math
import sys
import time
from dataclasses import dataclass

try:
    from prompt_toolkit.application import Application
    from prompt_toolkit.filters import Condition
    from prompt_toolkit.key_binding import KeyBindings
    from prompt_toolkit.layout import ConditionalContainer, HSplit, Layout, VSplit, Window
    from prompt_toolkit.layout.controls import FormattedTextControl
    from prompt_toolkit.layout.dimension import Dimension
    from prompt_toolkit.layout.processors import Processor, Transformation
    from prompt_toolkit.output import ColorDepth
    from prompt_toolkit.styles import Style
    from prompt_toolkit.widgets import TextArea
    from wcwidth import iter_graphemes, wcswidth
except ImportError:
    raise SystemExit('Install: python -m pip install prompt_toolkit "wcwidth>=0.6"')


# Ferra foregrounds; the background matches the supplied terminal screenshot.
BG = "#1e1e1e"
UMBER, BARK = "#4d424b", "#6f5d63"
STYLE = Style.from_dict({
    "": f"bg:{BG} #d1d1e0",
    "umber": UMBER,
    "bark": BARK,
    "sage": "#b1b695",
    "honey": "#f5d76e",
    "coral": "#ffa07a",
    "text-area": f"bg:{BG} #d1d1e0",
    "cursor": f"bg:#d1d1e0 {BG}",
    "selected-text": "bg:#4d424b #d1d1e0",
})
MAIN_KEY = "Cmd" if sys.platform == "darwin" else "Ctrl"
TIPS = {
    "zh": (
        f"按 `{MAIN_KEY}+R` 进入阅读模式，按 `y` 复制当前内容块。",
        f"按 `{MAIN_KEY}+Enter` 排队，等当前轮次结束后发送。",
        f"按 `{MAIN_KEY}+P` 打开 `Preview`，查看完整内容。",
        "输入 `/copy`，复制最近一次已完成回复的 Markdown 原文。",
        f"按 `{MAIN_KEY}+L` 选择模型，按 `{MAIN_KEY}+E` 调整思考强度。",
        "输入 `@` 补全路径，按 `Tab` 接受候选项。",
        "输入 `!` 执行本地命令，输出不会发送给模型。",
        f"按 `{MAIN_KEY}+H` 查看快捷键帮助。",
    ),
    "en": (
        f"Press `{MAIN_KEY}+R` to read; `y` copies the current block.",
        f"Press `{MAIN_KEY}+Enter` to queue a message after this turn.",
        f"Open `Preview` with `{MAIN_KEY}+P` to see the full content.",
        "Use `/copy` for the latest completed answer's Markdown source.",
        f"Choose a model with `{MAIN_KEY}+L`, effort with `{MAIN_KEY}+E`.",
        "Type `@` to complete a path; `Tab` accepts a candidate.",
        "Run local commands with `!`; output stays out of model context.",
        f"Press `{MAIN_KEY}+H` for keyboard help.",
    ),
}
SCENARIOS = ("normal", "shell", "macro", "status")
Fragments = list[tuple[str, str]]


def span(text: str, color: str = "bark") -> Fragments:
    return [(f"class:{color}", text)]


def width_of(parts: Fragments) -> int:
    return sum(max(0, wcswidth(text)) for _, text in parts)


def clip(parts: Fragments, width: int) -> Fragments:
    if width <= 0:
        return []
    if width_of(parts) <= width:
        return parts
    result = []
    remaining = width - 1
    for style, text in parts:
        for cluster in iter_graphemes(text):
            cells = max(0, wcswidth(cluster))
            if cells > remaining:
                return result + span("…", "umber")
            result.append((style, cluster))
            remaining -= cells
    return result


def tip_fragments(text: str) -> Fragments:
    return span("Tips ") + [
        ("class:bark" if index % 2 else "class:umber", part)
        for index, part in enumerate(text.split("`"))
    ]


def between(left: Fragments, right: Fragments, width: int) -> Fragments:
    right = clip(right, max(0, width // 2))
    left = clip(left, max(0, width - width_of(right) - 2))
    return left + span(" " * max(0, width - width_of(left + right))) + right


class Line(FormattedTextControl):
    def __init__(self, render):
        super().__init__(show_cursor=False)
        self.render = render

    def create_content(self, width, height):
        self.text = self.render(width)
        return super().create_content(width, height)


def row(render):
    return Window(Line(render), height=1, wrap_lines=False, always_hide_cursor=True)


class SoftwareCursor(Processor):
    def apply_transformation(self, ti):
        if ti.lineno != ti.document.cursor_position_row:
            return Transformation(ti.fragments)
        column = ti.source_to_display(ti.document.cursor_position_col)
        fragments = [(style, char) for style, text in ti.fragments for char in text]
        if column < len(fragments):
            style, char = fragments[column]
            fragments[column] = (style + " class:cursor", char)
        else:
            fragments.append(("class:cursor", " "))
        return Transformation(fragments)


@dataclass
class Demo:
    interval: float
    lang: str
    working: bool
    controls: bool
    narrow: bool = False
    scenario: int = 0
    tip_index: int = 0
    tip_visible: bool = False
    remaining: float = 6.0
    elapsed: float = 0.0
    prompt: str = "梳理输入区布局，保持消息区域和输入体验不变。"

    def local(self, zh: str, en: str) -> str:
        return zh if self.lang == "zh" else en

    def next_tip(self):
        self.tip_index = (self.tip_index + 1) % len(TIPS[self.lang])
        self.remaining = self.interval

    def set_working(self, working: bool):
        self.working = working
        self.remaining = self.interval

    def top_rule(self, width: int) -> Fragments:
        label = span("── ", "umber")
        label += span("e·pi", "honey" if self.working else "bark")
        label += span(" gpt-6-astra Xhigh ")
        label = clip(label, width)
        return label + span("─" * max(0, width - width_of(label)), "umber")

    def bottom_rule(self, width: int) -> Fragments:
        self.tip_visible = False
        mode = SCENARIOS[self.scenario]
        left = span("──", "umber")
        if mode in ("shell", "macro"):
            left += span(" powershell " if mode == "shell" else " macro ")
            left += span("──", "umber")
        left = clip(left, width)
        available = max(0, width - width_of(left) - 8)
        right = []
        if mode == "status":
            right = clip(span(self.local("正在切换模型，请稍候…", "Switching model, please wait…")), available)
        elif self.working and available >= 24:
            right = clip(tip_fragments(TIPS[self.lang][self.tip_index]), available)
            self.tip_visible = True
        if not right:
            return left + span("─" * max(0, width - width_of(left)), "umber")
        gap = max(0, width - width_of(left + right) - 4)
        return left + span("─" * gap + " ", "umber") + right + span(" ──", "umber")

    def activity(self, width: int) -> Fragments:
        if not self.working:
            return clip(span("  ✓ ", "sage") + span(self.local("本轮已完成", "Turn complete")), width)
        spinner = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"[int(self.elapsed * 8) % 10]
        return clip(span(f"  {spinner} ", "bark") + span("Thinking...", "umber"), width)

    def metadata(self, width: int) -> Fragments:
        state = self.local("工作中", "working") if self.working else self.local("空闲", "idle")
        text = (f"{state} · {SCENARIOS[self.scenario]} · {self.tip_index + 1}/{len(TIPS[self.lang])}"
                f" · {self.interval:g}s  |  umber {UMBER} / bark {BARK}")
        return clip(span(text), width)


async def clock(app: Application, demo: Demo):
    last = time.monotonic()
    while True:
        await asyncio.sleep(0.1)
        now = time.monotonic()
        delta, last = now - last, now
        if demo.working:
            demo.elapsed += delta
        # Hidden tips keep their reading time when they become visible again.
        if demo.working and demo.tip_visible:
            demo.remaining -= delta
            if demo.remaining <= 0:
                demo.next_tip()
        app.invalidate()


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--interval", type=float, default=6, help="visible seconds per tip (default: 6)")
    parser.add_argument("--lang", choices=("zh", "en"), default="zh")
    parser.add_argument("--idle", action="store_true", help="start idle with no Tips")
    parser.add_argument("--clean", action="store_true", help="hide prototype controls; F1 restores them")
    args = parser.parse_args()
    if not math.isfinite(args.interval) or args.interval <= 0:
        parser.error("--interval must be a positive finite number")
    demo = Demo(args.interval, args.lang, not args.idle, not args.clean, remaining=args.interval)
    keys = KeyBindings()
    editor = TextArea(
        multiline=True,
        height=Dimension(min=1, max=5),
        dont_extend_height=True,
        prompt=span("❯ "),
        input_processors=[SoftwareCursor()],
        wrap_lines=True,
    )
    editor.window.always_hide_cursor = Condition(lambda: True)

    @keys.add("f1")
    def toggle_controls(event):
        demo.controls = not demo.controls

    @keys.add("f2")
    def toggle_work(event):
        demo.set_working(not demo.working)

    @keys.add("f3")
    def next_tip(event):
        demo.next_tip()

    @keys.add("f4")
    def next_scenario(event):
        demo.scenario = (demo.scenario + 1) % len(SCENARIOS)

    @keys.add("f5")
    def switch_language(event):
        demo.lang = "en" if demo.lang == "zh" else "zh"
        demo.remaining = demo.interval

    @keys.add("f6")
    def toggle_width(event):
        demo.narrow = not demo.narrow

    @keys.add("enter")
    def submit(event):
        if editor.text.strip():
            demo.prompt = " ".join(editor.text.splitlines())
            editor.buffer.reset()
        demo.set_working(True)

    @keys.add("escape", eager=True)
    def stop_work(event):
        demo.set_working(False)

    @keys.add("c-j")
    def newline(event):
        editor.buffer.insert_text("\n")

    @keys.add("c-c")
    @keys.add("c-q")
    def quit_demo(event):
        event.app.exit()

    controls = HSplit([
        row(lambda w: clip(span("e / Tips prototype", "sage") + span("  ·  bottom rule / right aligned / no extra row"), w)),
        row(lambda w: clip(span("F1 controls  F2 work/idle  F3 next  F4 status  F5 中/EN  F6 narrow  Ctrl+Q quit"), w)),
        row(demo.metadata),
        row(lambda w: clip(span(demo.local(
            "仅视觉演示，不连接后端。F4：普通 → shell → macro → 高优先级状态。",
            "Mock only, no backend. F4: normal → shell → macro → higher-priority status.",
        ), "umber"), w)),
        Window(height=1),
    ])
    conversation = HSplit([
        row(lambda w: clip(span("❯ ") + [("", demo.prompt)], w)),
        Window(height=1),
        row(lambda w: clip([("", demo.local(
            "我会先检查输入框的上下边框，保留原有布局和状态信息。",
            "I will inspect the composer borders and preserve the existing layout and status labels.",
        ))], w)),
        Window(height=1),
        row(lambda w: clip(span("  • read ", "umber") + span("crates/e-tui/src/ui/region/composer.rs  0.2s"), w)),
        row(lambda w: clip(span("  • read ", "umber") + span("crates/e-tui/assets/themes/ferra.toml  0.1s"), w)),
    ])
    page = HSplit([
        ConditionalContainer(controls, filter=Condition(lambda: demo.controls)),
        ConditionalContainer(conversation, filter=Condition(lambda: app.output.get_size().rows >= (22 if demo.controls else 17))),
        Window(height=Dimension(min=0, weight=1)),
        row(demo.activity),
        Window(height=1),
        row(demo.top_rule),
        editor,
        row(demo.bottom_rule),
        Window(height=1),
        row(lambda w: between(span("CH84% 21%/272k $1.01"), span(f"{MAIN_KEY}+H Help"), w)),
        row(lambda w: between(span(demo.local("输入区 Tips 视觉原型", "Composer Tips prototype")), span(r"G:\MyProjects\Rust\e"), w)),
    ], width=lambda: Dimension(max=64) if demo.narrow else Dimension())
    root = VSplit([Window(width=1), page, Window(width=1)], padding=0, style=f"bg:{BG}")
    app = Application(
        layout=Layout(root, focused_element=editor),
        key_bindings=keys,
        style=STYLE,
        color_depth=ColorDepth.TRUE_COLOR,
        full_screen=True,
        mouse_support=False,
        include_default_pygments_style=False,
    )
    app.run(pre_run=lambda: app.create_background_task(clock(app, demo)))


if __name__ == "__main__":
    main()
