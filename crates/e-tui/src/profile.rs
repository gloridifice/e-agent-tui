//! Runtime profiling: startup phase timers, optional per-frame aggregates,
//! backend I/O counters, and Tracy zones. Executable adapters own the
//! environment or configuration switches that enable these diagnostics.

use std::{
    collections::VecDeque,
    io::{self, Write},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use ratatui::{
    backend::{Backend, ClearType, WindowSize},
    buffer::Cell,
    layout::{Position, Size},
};

/// Env-gated startup timer. Each mark prints elapsed time since the previous
/// mark, not elapsed time since process start.
pub struct PhaseTimers {
    enabled: bool,
    last: Instant,
}

impl Default for PhaseTimers {
    fn default() -> Self {
        Self::new(false)
    }
}

impl PhaseTimers {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
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
        eprintln!("[e timing] {name:>34}: {ms:10.2} ms");
    }
}

#[derive(Default)]
struct IoCounterInner {
    cells: AtomicU64,
    bytes: AtomicU64,
    draw_ns: AtomicU64,
    flush_ns: AtomicU64,
}

/// Shared counters used by a backend and its writer. Counters are reset at the
/// beginning of each frame transaction and sampled after the final flush.
#[derive(Clone, Default)]
pub struct IoCounters(Arc<IoCounterInner>);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IoSnapshot {
    pub changed_cells: u64,
    pub emitted_bytes: u64,
    pub draw_ns: u64,
    pub flush_ns: u64,
}

impl IoCounters {
    pub fn reset(&self) {
        self.0.cells.store(0, Ordering::Relaxed);
        self.0.bytes.store(0, Ordering::Relaxed);
        self.0.draw_ns.store(0, Ordering::Relaxed);
        self.0.flush_ns.store(0, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> IoSnapshot {
        IoSnapshot {
            changed_cells: self.0.cells.load(Ordering::Relaxed),
            emitted_bytes: self.0.bytes.load(Ordering::Relaxed),
            draw_ns: self.0.draw_ns.load(Ordering::Relaxed),
            flush_ns: self.0.flush_ns.load(Ordering::Relaxed),
        }
    }

    fn add_cells(&self, count: u64) {
        self.0.cells.fetch_add(count, Ordering::Relaxed);
    }

    fn add_bytes(&self, count: u64) {
        self.0.bytes.fetch_add(count, Ordering::Relaxed);
    }

    fn add_draw_time(&self, elapsed: Duration) {
        self.0.draw_ns.fetch_add(
            elapsed.as_nanos().min(u128::from(u64::MAX)) as u64,
            Ordering::Relaxed,
        );
    }

    fn add_flush_time(&self, elapsed: Duration) {
        self.0.flush_ns.fetch_add(
            elapsed.as_nanos().min(u128::from(u64::MAX)) as u64,
            Ordering::Relaxed,
        );
    }
}

/// Writer adapter that counts bytes accepted by the wrapped writer. It does
/// not force additional flushes and therefore preserves buffering semantics.
pub struct CountingWriter<W> {
    inner: W,
    counters: IoCounters,
}

impl<W> CountingWriter<W> {
    pub fn new(inner: W, counters: IoCounters) -> Self {
        Self { inner, counters }
    }

    pub fn into_inner(self) -> W {
        self.inner
    }
}

impl<W: Write> Write for CountingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buf)?;
        self.counters.add_bytes(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.inner.write_all(buf)?;
        self.counters.add_bytes(buf.len() as u64);
        Ok(())
    }
}

/// Backend adapter that counts changed cells and separates backend draw/flush
/// time. All terminal behavior is delegated unchanged to the wrapped backend.
pub struct CountingBackend<B> {
    inner: B,
    counters: IoCounters,
}

impl<B> CountingBackend<B> {
    pub fn new(inner: B, counters: IoCounters) -> Self {
        Self { inner, counters }
    }

    pub fn inner(&self) -> &B {
        &self.inner
    }

    pub fn inner_mut(&mut self) -> &mut B {
        &mut self.inner
    }
}

impl<B: Write> Write for CountingBackend<B> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.inner.write_all(buf)
    }
}

impl<B: Backend> Backend for CountingBackend<B> {
    type Error = B::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let counters = self.counters.clone();
        let started = Instant::now();
        let result = self
            .inner
            .draw(content.inspect(move |_| counters.add_cells(1)));
        self.counters.add_draw_time(started.elapsed());
        result
    }

    fn append_lines(&mut self, n: u16) -> Result<(), Self::Error> {
        self.inner.append_lines(n)
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        self.inner.get_cursor_position()
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        self.inner.set_cursor_position(position)
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> Result<Size, Self::Error> {
        self.inner.size()
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        let started = Instant::now();
        let result = self.inner.flush();
        self.counters.add_flush_time(started.elapsed());
        result
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FrameSample {
    pub scheduler_delay: Duration,
    pub update: Duration,
    pub render: Duration,
    pub draw: Duration,
    pub flush: Duration,
    pub total: Duration,
    pub changed_cells: u64,
    pub emitted_bytes: u64,
    pub cache_rebuilds: u64,
    pub cache_patches: u64,
    pub materialized_rows: u64,
    pub preview_rebuilds: u64,
    pub preview_patches: u64,
    pub preview_materialized_rows: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Distribution {
    pub count: usize,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
}

fn percentile_sorted(values: &[f64], percentile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let rank = (values.len() as f64 * percentile.clamp(0.0, 1.0))
        .ceil()
        .max(1.0) as usize;
    values[rank.saturating_sub(1).min(values.len() - 1)]
}

/// Nearest-rank percentile (`ceil(N × p)`), shared by diagnostics and release
/// benchmark fixtures so reported thresholds use one definition.
pub fn percentile(values: &mut [f64], percentile: f64) -> f64 {
    values.sort_by(f64::total_cmp);
    percentile_sorted(values, percentile)
}

fn distribution(mut values: Vec<f64>) -> Distribution {
    if values.is_empty() {
        return Distribution::default();
    }
    values.sort_by(f64::total_cmp);
    Distribution {
        count: values.len(),
        p50: percentile_sorted(&values, 0.50),
        p95: percentile_sorted(&values, 0.95),
        p99: percentile_sorted(&values, 0.99),
        max: *values.last().unwrap_or(&0.0),
    }
}

/// Bounded aggregate frame diagnostics. A disabled collector stores nothing;
/// an enabled collector keeps only the latest `capacity` samples.
pub struct FrameMetrics {
    enabled: bool,
    capacity: usize,
    report_every: usize,
    seen: usize,
    samples: VecDeque<FrameSample>,
}

impl FrameMetrics {
    pub fn new(enabled: bool, capacity: usize, report_every: usize) -> Self {
        Self {
            enabled,
            capacity: capacity.max(1),
            report_every: report_every.max(1),
            seen: 0,
            samples: VecDeque::with_capacity(capacity.clamp(1, 4096)),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn record(&mut self, sample: FrameSample) -> Option<String> {
        if !self.enabled {
            return None;
        }
        self.seen += 1;
        if self.samples.len() == self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
        self.seen
            .is_multiple_of(self.report_every)
            .then(|| self.report())
    }

    pub fn report(&self) -> String {
        let metric = |f: fn(&FrameSample) -> f64| {
            distribution(self.samples.iter().map(f).collect::<Vec<_>>())
        };
        let total = metric(|s| s.total.as_secs_f64() * 1000.0);
        let scheduler = metric(|s| s.scheduler_delay.as_secs_f64() * 1000.0);
        let render = metric(|s| s.render.as_secs_f64() * 1000.0);
        let draw = metric(|s| s.draw.as_secs_f64() * 1000.0);
        let cells = metric(|s| s.changed_cells as f64);
        let bytes = metric(|s| s.emitted_bytes as f64);
        let rebuilds: u64 = self.samples.iter().map(|s| s.cache_rebuilds).sum();
        let patches: u64 = self.samples.iter().map(|s| s.cache_patches).sum();
        let rows: u64 = self.samples.iter().map(|s| s.materialized_rows).sum();
        let preview_rebuilds: u64 = self.samples.iter().map(|s| s.preview_rebuilds).sum();
        let preview_patches: u64 = self.samples.iter().map(|s| s.preview_patches).sum();
        let preview_rows: u64 = self
            .samples
            .iter()
            .map(|s| s.preview_materialized_rows)
            .sum();
        format!(
            "[e frames] n={} total_ms p50={:.2} p95={:.2} p99={:.2} max={:.2}; scheduler_p95={:.2}; render_p95={:.2}; draw_p95={:.2}; cells_p95={:.0}; bytes_p95={:.0}; rebuilds={rebuilds}; patches={patches}; visible_rows={rows}; preview_rebuilds={preview_rebuilds}; preview_patches={preview_patches}; preview_rows={preview_rows}",
            total.count,
            total.p50,
            total.p95,
            total.p99,
            total.max,
            scheduler.p95,
            render.p95,
            draw.p95,
            cells.p95,
            bytes.p95,
        )
    }
}

/// Start the Tracy client when the owning executable enables it. The returned
/// handle keeps the client alive for the process lifetime. Without the feature
/// this is a no-op.
#[cfg(feature = "tracy")]
pub fn start_tracy(enabled: bool) -> Option<tracy_client::Client> {
    if !enabled {
        return None;
    }
    let client = tracy_client::Client::start();
    eprintln!("[e] tracy client started — connect the profiler");
    Some(client)
}

#[cfg(not(feature = "tracy"))]
pub fn start_tracy(_enabled: bool) -> Option<()> {
    None
}

#[cfg(feature = "tracy")]
#[doc(hidden)]
pub use tracy_client as __tracy_client;

#[cfg(feature = "tracy")]
#[macro_export]
macro_rules! tracy_zone {
    ($name:literal) => {{
        match $crate::profile::__tracy_client::Client::running() {
            Some(client) => {
                Some(client.span($crate::profile::__tracy_client::span_location!($name), 0))
            }
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

#[cfg(not(feature = "tracy"))]
#[must_use]
pub struct NoopSpan;

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, buffer::Buffer, layout::Rect};

    #[test]
    fn disabled_metrics_store_nothing() {
        let mut metrics = FrameMetrics::new(false, 2, 1);
        assert!(metrics.record(FrameSample::default()).is_none());
        assert_eq!(metrics.len(), 0);
    }

    #[test]
    fn metrics_are_bounded_and_report_required_fields() {
        let mut metrics = FrameMetrics::new(true, 2, 3);
        for n in 1..=3 {
            let report = metrics.record(FrameSample {
                total: Duration::from_millis(n),
                render: Duration::from_micros(n * 100),
                changed_cells: n,
                emitted_bytes: n * 10,
                cache_patches: 1,
                materialized_rows: 20,
                ..FrameSample::default()
            });
            if n == 3 {
                let report = report.expect("third sample reports");
                for field in ["p50=", "p95=", "p99=", "cells_p95=", "bytes_p95="] {
                    assert!(report.contains(field), "missing {field}: {report}");
                }
            }
        }
        assert_eq!(metrics.len(), 2, "oldest sample evicted");
    }

    #[test]
    fn counting_writer_counts_successful_bytes() {
        let counters = IoCounters::default();
        let mut writer = CountingWriter::new(Vec::new(), counters.clone());
        writer.write_all(b"abc").unwrap();
        writer.write_all(b"de").unwrap();
        assert_eq!(counters.snapshot().emitted_bytes, 5);
        assert_eq!(writer.into_inner(), b"abcde");
    }

    #[test]
    fn counting_backend_counts_diff_cells_without_changing_output() {
        let counters = IoCounters::default();
        let inner = TestBackend::new(4, 2);
        let mut backend = CountingBackend::new(inner, counters.clone());
        let buffer = Buffer::with_lines(["ab"]);
        backend
            .draw(buffer.content.iter().enumerate().map(|(index, cell)| {
                let x = index as u16 % 4;
                let y = index as u16 / 4;
                (x, y, cell)
            }))
            .unwrap();
        assert_eq!(
            counters.snapshot().changed_cells,
            buffer.content.len() as u64
        );
        assert_eq!(backend.inner().buffer()[(0, 0)].symbol(), "a");
    }

    #[test]
    fn distribution_uses_nearest_rank_ceiling() {
        let d = distribution(vec![1.0, 2.0, 3.0, 4.0]);
        assert_eq!(d.count, 4);
        assert_eq!(d.p50, 2.0);
        assert_eq!(d.p95, 4.0);
        assert_eq!(d.max, 4.0);
        let _ = Rect::new(0, 0, 1, 1);
    }
}
