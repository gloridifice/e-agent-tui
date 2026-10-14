# Tips catalog — zh-CN draft

Status: Candidate UI strings for rotating-tips review; not a current contract
or a production catalog.

Chinese strings below are proposed localized UI content. Backticks mark Bark
keywords, not visible Markdown. Ordinary text uses Umber. Render `Tips` followed
by exactly one ASCII space, then the content; do not render the IDs or ownership
labels. The line sits right-aligned inside the composer's bottom rule.

Shortcut examples use Windows defaults. Production must resolve each shortcut
from the effective key mapping; disabled bindings make the corresponding tip
ineligible. Commands requiring idle admission explicitly say so in the content.

Ownership: 24 shared candidates, 3 dshe candidates, 8 pie candidates. Candidates
with unavailable prerequisites are omitted, not replaced with advice for another
backend. These counts do not imply an initial release commitment.

## Shared — registered by e-tui

1. **shared.help**
   按 `Ctrl+H` 查看当前快捷键帮助。

2. **shared.reading-copy**
   按 `Ctrl+R` 进入阅读模式，用 `y` 复制当前内容块。

3. **shared.preview**
   按 `Ctrl+P` 打开 `Preview`，查看完整内容。

4. **shared.copy-answer**
   输入 `/copy`，复制最近一次已完成回复的 Markdown 原文。
   Eligibility: a completed assistant Markdown answer exists.

5. **shared.send-asap**
   智能体工作时按 `Enter` 可以发送补充消息，无需等待本轮会话结束。
   Eligibility: no active compaction or temporary-route admission barrier.

6. **shared.send-after-turn**
   智能体工作时，按 `Ctrl+Enter` 发送消息，会在本轮会话结束后发送。

7. **shared.newline**
   按 `Shift+Enter` 换行，按 `Enter` 发送。

8. **shared.cancel-queue**
   有排队消息时，`Esc` 优先撤回消息；队列空后才中断任务。
   Eligibility: pending prompts exist and no local shell command is running.

9. **shared.history-views**
   输入 `/history` 查看执行耗时，在历史页按 `Tab` 切换时间线。
   Eligibility: an attached session has execution-history support.

10. **shared.history-copy**
    输入 `/history copy-10`，复制耗时最长的十项操作。
    Eligibility: an attached session has execution-history support.

11. **shared.command-completion**
    输入 `/` 查找命令，用 `Tab` 补全，`Esc` 取消候选。

12. **shared.path-completion**
    输入 `@` 补全路径；文件名支持子串匹配，用 `Tab` 接受候选。

13. **shared.local-shell**
    输入 `!` 执行本地命令，输出不会发送给模型。
    Eligibility: no local shell command is already running.

14. **shared.skill-message**
    用 `/skill:名称 消息` 调用技能，同时附上你的需求。
    Eligibility: the skill catalog is nonempty.

15. **shared.model-effort**
    按 `Ctrl+L` 选择模型，按 `Ctrl+E` 调整思考强度。
    Eligibility: model selection and current-model effort selection are available.

16. **shared.model-marks**
    在模型菜单用 `Shift+字母` 标记模型，之后按该字母快速切换。
    Eligibility: model selection is available; reserved binding chords cannot be marks.

17. **shared.temporary-model**
    用 `//字母 消息` 临时选择已标记模型，任务结束后恢复原模型。
    Eligibility: at least one effective mark resolves to an available model.

18. **shared.model-default-effort**
    用 `/model 模型 set-default-effort 强度` 保存该模型的默认思考强度。
    Eligibility: at least one available model exposes selectable efforts.

19. **shared.link-copy**
    按 `Ctrl+Y`，再按回复中的标记字符，复制对应链接或路径。
    Eligibility: the latest completed answer has active validated quick-link tags.

20. **shared.mouse-copy**
    用鼠标选中文字即可复制；阅读模式的 `y` 可复制完整内容块。

21. **shared.settings-reload**
    输入 `/settings` 调整前端设置，`/reload` 重载配置和后端资源。

22. **shared.config-path**
    输入 `/econfig` 查看前端配置文件的位置。

23. **shared.new-resume**
    空闲后用 `/new` 开新会话，用 `/resume` 续接历史会话。

24. **shared.compaction-queue**
    `压缩期间`发送的普通消息会排队，等当前任务结束后再发送。
    Eligibility: a live compaction is active.

## dshe only — registered by e-dsh

1. **dshe.login-proxy**
   空闲后在 `/login` 的 `Proxy` 页面管理自定义模型端点。
   Eligibility: DSH proxy-management support is available.

2. **dshe.new-mode**
   空闲后用 `/new 模式` 按 DSH 预设新建会话，可用模式会出现在补全中。
   Eligibility: the adapter supplied at least one selectable new-session mode.

3. **dshe.compaction-route**
   `/compact set-model 模型` 指定 DSH 手动和自动压缩使用的模型。
   Eligibility: compaction-model routing is available.

## pie only — registered by e-pi

1. **pie.fork**
   空闲后用 `/fork` 从较早的用户消息分叉，原输入会回到编辑栏。
   Eligibility: fork is available and prior user messages exist.

2. **pie.clone**
   空闲后用 `/clone` 复制当前会话分支，不会创建 Git 分支。
   Eligibility: clone is available for an attached session.

3. **pie.native-auth**
   空闲后用 `/login 提供商` 登录，用 `/logout` 移除已保存的凭据。
   Eligibility: native authentication and logout are available.

4. **pie.mcp**
   用 `/mcp` 管理 MCP 服务和工具；工作时保存配置后，空闲再 `/reload`。
   Eligibility: the native MCP manager is available, not merely an identically named extension command.

5. **pie.resume-tree**
   在 `/resume` 中，`(fork)` 标记表示从父会话分叉出的历史会话。
   Eligibility: adapter-supplied session ancestry is available.

6. **pie.resume-cli**
   退出 pie 后，终端会显示当前已保存会话的 `恢复命令`。
   Eligibility: the current session is saved and can produce a recovery command.

7. **pie.error-recovery**
   可在 `/settings` 关闭 `错误自动重试`，取消等待中的追加重试。
   Eligibility: e's fallback recovery is enabled; this does not disable native Pi retry.

8. **pie.auto-compaction**
   Pi 的`自动压缩`始终使用原生默认模型；指定压缩模型只影响手动压缩。
   Eligibility: native compaction is supported.

## Held candidates — prompt-macro work is not part of this delivery

These are shared candidates, not backend-private tips. Enable only after the
in-progress prompt-macro workflow is accepted for delivery.

1. **shared.prompt-macro**
   用 `/prompt-macro 名称 提示词` 定义宏，输入精确名称即可执行。

2. **shared.prompt-macro-route**
   提示词宏可指定 `--model` 和 `--effort`，执行后恢复原模型和强度。

## Registration proposal

See [implement.md](implement.md) for the proposed public registration boundary,
localized styled segments, prerequisites and rotation.
No registry or production integration has been implemented in this phase.

## Content verification sources

- Shared interaction: [interaction contract](../../../../specs/interaction-and-sessions.md),
  [default bindings](../../../../../crates/e-tui/assets/default_key_mapping.toml),
  [command declarations](../../../../../crates/e-tui/src/command_catalog.rs), and
  [current Chinese help strings](../../../../../crates/e-tui/locales/zh-CN.yml).
- dshe: [DSH integration](../../../../guides/dsh-integration.md), the current interaction
  contract, and [bridge compaction](../../../../../bridge/src/compaction.js).
- pie: [development workflows](../../../../guides/development.md), the current interaction
  and runtime contracts, and [adapter command discovery](../../../../../crates/e-pi/src/adapter/mod.rs).
