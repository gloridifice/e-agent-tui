# declarative-config-overlay Specification

## Purpose
TBD - created by archiving change remediate-architecture-audit. Update Purpose after archive.
## Requirements
### Requirement: 持久化配置只有一个 Rust 字段 schema
所有持久化配置字段 MUST 只在可直接 `Deserialize` 的 `Config` schema 中声明一次；系统 MUST 不再维护字段平行的 `CompleteConfig`、`PartialConfig`、`into_config` 或手写 apply 清单。

#### Scenario: 新增配置字段
- **WHEN** 开发者在 `Config` 与嵌入默认 TOML 中加入一个字段
- **THEN** loader 可继承和覆盖该字段，而无需在第二个持久化结构或字段复制函数中再次声明

### Requirement: 默认 TOML 仍是默认值唯一来源
`client/assets/default_config.toml` MUST 保持所有配置默认值的唯一来源；Rust 字段 attribute 或 `Config::default` MUST 不复制这些业务默认值。

#### Scenario: 用户文件缺少新字段
- **WHEN** 旧用户配置不包含当前 schema 的一个或多个字段
- **THEN** loader 从嵌入默认 TOML 继承这些字段的值

#### Scenario: 无用户配置
- **WHEN** 用户配置文件不存在
- **THEN** loader 返回完整解析后的嵌入默认配置并解析所选主题

### Requirement: 用户配置按值覆盖后严格反序列化
Loader MUST 先解析嵌入默认 TOML 和用户 TOML、只接受默认 schema 中存在的用户键、递归执行值级覆盖，再对合并结果进行一次严格 `Config` 反序列化。

#### Scenario: 合法部分覆盖
- **WHEN** 用户文件只覆盖 `theme` 和 `page_max_width`
- **THEN** 这两个值使用用户设置，其余字段使用嵌入默认值

#### Scenario: 旧版未知字段
- **WHEN** 用户文件含已废弃或未知字段
- **THEN** loader 忽略该字段并保留其他合法覆盖，不因未知字段使整个配置失效

#### Scenario: 已知字段类型错误
- **WHEN** 用户把数值字段写成无法解析的类型
- **THEN** loader 走明确且有测试的安全回退/诊断路径，不产生部分未初始化 Config

### Requirement: 运行时派生字段不参与持久化 schema
解析后的 theme cache 等运行时派生状态 MUST 从持久化反序列化中跳过，并在 Config 加载、reload 和主题切换后显式解析更新。

#### Scenario: Reload 配置
- **WHEN** 用户执行 `/reload`
- **THEN** loader 重新合并配置并刷新 `resolved_theme`，渲染期间不读取磁盘

### Requirement: Settings 元数据只描述可见 UI
`settings::SettingDef` SHALL 只拥有标签、说明、控件类型和显式可编辑行为；它 MUST 不充当第二份完整持久化 schema。

#### Scenario: 非 UI 兼容字段
- **WHEN** Config 含仅用于旧文件兼容且不再改变交互的字段
- **THEN** 该字段可被 loader 接受，而无需在 settings 页面创建伪设置项

### Requirement: Reveal configuration is persisted through the canonical schema
The embedded default TOML and the directly deserializable `Config` schema SHALL define `background_color`, `message_chars_per_second`, and `preview_lines_per_second` exactly once as persisted values. Their defaults SHALL be `"#000000"`, `120`, and `30` respectively. Existing user files that omit them SHALL inherit these defaults through the existing known-key overlay. The obsolete `preview_chars_per_second` key SHALL be ignored as an unknown key and SHALL NOT be numerically converted to rows per second.

#### Scenario: Existing config predates reveal settings
- **WHEN** a valid user config omits all current reveal fields
- **THEN** loading succeeds with `background_color = "#000000"`, `message_chars_per_second = 120`, and `preview_lines_per_second = 30`

#### Scenario: User overrides reveal settings
- **WHEN** a user config supplies valid values for one or more current reveal fields
- **THEN** those values override the embedded defaults and all unrelated values continue to inherit or override through the existing recursive overlay

#### Scenario: User file contains the obsolete Preview character rate
- **WHEN** a user config contains `preview_chars_per_second` but not `preview_lines_per_second`
- **THEN** the obsolete key is ignored and Preview uses the default line rate of 30

### Requirement: Reveal settings are editable and validated
The `/settings` Input Page SHALL expose editable rows for the fade background/reference color, transcript maximum grapheme reveal speed, and Preview maximum display-row reveal speed. Confirmed values SHALL save and apply immediately. `background_color` SHALL accept exactly a six-digit `#RRGGBB` value, case-insensitively; both speed values SHALL accept whole units-per-second values from 0 through 1024. Zero SHALL disable pacing for its lane and expose complete content immediately. Invalid edits SHALL NOT replace the last valid configured value.

#### Scenario: User changes the fade background
- **WHEN** the user confirms `#1a2B3c` in the background-color row
- **THEN** the canonical persisted value becomes a valid normalized hex color and active faded groups are repainted against it

#### Scenario: User enters an invalid color
- **WHEN** the user confirms a value that is not exactly `#RRGGBB`
- **THEN** the settings page retains the previous valid `background_color` and does not persist the invalid value

#### Scenario: User changes transcript speed
- **WHEN** the user confirms a whole value from 0 through 1024 for the transcript reveal row
- **THEN** `message_chars_per_second` uses and persists that grapheme rate immediately

#### Scenario: User changes Preview speed
- **WHEN** the user confirms a whole value from 0 through 1024 for the Preview reveal row
- **THEN** `preview_lines_per_second` uses and persists that wrapped-row rate immediately

#### Scenario: User enters an out-of-range speed
- **WHEN** the user confirms a value above 1024, a negative value, or a non-whole value
- **THEN** the settings page retains the previous valid speed and does not persist the invalid value

