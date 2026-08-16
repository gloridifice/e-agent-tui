//! render_demo — print a sample markdown document through the renderer so
//! the box-drawing output can be eyeballed.

use dsh_tui::config::Theme;
use dsh_tui::render::render_markdown;

fn main() {
    let md = "\
# 标题一

一段 **粗体**、*斜体* 和 `行内代码`，还有[链接](https://example.com)。

## 表格

| 名称 | 值 | 说明 |
|---|---|------|
| foo | 1 | 中文内容 |
| bar | 2 | ok |

### 代码

```rust
fn main() {
    println!(\"hello\");
}
```

#### 列表

- 项目一
- 项目二
  - 嵌套项目

1. 第一
2. 第二

> 引用一行
> 引用二行

---

```mermaid
graph TD
    A[开始] --> B{判断}
    B -->|是| C[执行]
    B -->|否| D[结束]
```

**结束**
";
    let theme = Theme::ferra();
    let mut next = 0;
    let mut units = std::collections::HashMap::new();
    let options = dsh_tui::render::RenderOptions::default();
    let lines = render_markdown(md, &theme, &mut next, &options, &mut units);
    for (i, r) in lines.iter().enumerate() {
        let plain: String = r.line.spans.iter().map(|s| s.content.as_ref()).collect();
        let atom = if r.atomic { " ⚛" } else { "" };
        println!("{i:3} |{plain}|{atom}");
    }
}
