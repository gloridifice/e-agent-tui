//! Build-time UI assets owned by the frontend library.

/// Mermaid renderer module used by the terminal presentation pipeline.
pub const GROK_MERMAID_WASM: &[u8] = include_bytes!("../assets/grok-mermaid.wasm");

/// Upstream license text shipped with the embedded Mermaid renderer.
pub const GROK_MERMAID_LICENSE: &str = include_str!("../assets/LICENSE.grok-mermaid");
