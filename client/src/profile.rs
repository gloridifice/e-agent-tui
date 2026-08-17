//! Startup profiling: Tracy zones (feature `tracy`) plus env-gated phase
//! timers. See `../docs/tracy.md` for the integration plan.
//!
//! - `DSH_TUI_TRACY=1` + a running Tracy profiler → zones appear in Tracy.
//! - `DSH_TUI_TIMING=1` → per-phase millisecond timings print to stderr,
//!   usable headless and without the profiler.

use std::time::Instant;

/// Env-gated phase timer: with `DSH_TUI_TIMING=1` every `mark` prints the
/// elapsed time since the previous mark to stderr.
pub struct PhaseTimers {
    enabled: bool,
    last: Instant,
}

impl PhaseTimers {
    pub fn new() -> Self {
        Self {
            enabled: std::env::var("DSH_TUI_TIMING").as_deref() == Ok("1"),
            last: Instant::now(),
        }
    }

    pub fn mark(&mut self, name: &'static str) {
        if !self.enabled {
            return;
        }
        let now = Instant::now();
        let ms = now.duration_since(self.last).as_secs_f64() * 1000.0;
        self.last = now;
        eprintln!("[dshe timing] {name:>34}: {ms:10.2} ms");
    }
}

/// Start the Tracy client when `DSH_TUI_TRACY=1`. The returned handle keeps
/// the client alive for the process lifetime. Without the feature this is a
/// no-op.
#[cfg(feature = "tracy")]
pub fn start_tracy() -> Option<tracy_client::Client> {
    if std::env::var("DSH_TUI_TRACY").as_deref() != Ok("1") {
        return None;
    }
    let client = tracy_client::Client::start();
    eprintln!("[dshe] tracy client started — connect the profiler");
    Some(client)
}

#[cfg(not(feature = "tracy"))]
pub fn start_tracy() -> Option<()> {
    None
}

/// One Tracy zone: `let _z = e::tracy_zone!("name");`.
///
/// Safe without the profiler: it returns `None` (a no-op) unless the Tracy
/// client is actually running — `span!` itself panics without a client, so
/// this macro guards the call and requires a string literal (the underlying
/// macros bake the name into a static).
#[cfg(feature = "tracy")]
#[macro_export]
macro_rules! tracy_zone {
    ($name:literal) => {{
        match tracy_client::Client::running() {
            Some(client) => Some(client.span(tracy_client::span_location!($name), 0)),
            None => None,
        }
    }};
}

#[cfg(not(feature = "tracy"))]
#[macro_export]
macro_rules! tracy_zone {
    ($name:literal) => {{
        let _ = $name;
        None::<$crate::profile::NoopSpan>
    }};
}

/// Drop-only stand-in so `let _z = e::tracy_zone!(..)` compiles without
/// the feature (non-Copy: explicit `drop(_z)` stays meaningful).
#[cfg(not(feature = "tracy"))]
#[must_use]
pub struct NoopSpan;
