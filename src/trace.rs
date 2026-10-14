//! Lightweight engine call tracing (elapsed timings for optimization).

use std::sync::Mutex;
use std::time::Instant;

const MAX_EVENTS: usize = 256;

#[derive(Debug, Clone)]
pub struct TraceEvent {
    pub name: &'static str,
    pub elapsed_ms: u128,
    pub detail: String,
    pub ok: bool,
}

static LOG: Mutex<Vec<TraceEvent>> = Mutex::new(Vec::new());

pub fn clear() {
    if let Ok(mut g) = LOG.lock() {
        g.clear();
    }
}

pub fn snapshot() -> Vec<TraceEvent> {
    LOG.lock().map(|g| g.clone()).unwrap_or_default()
}

pub fn format_report() -> String {
    let events = snapshot();
    if events.is_empty() {
        return "No engine trace events yet.".into();
    }
    let mut lines = Vec::with_capacity(events.len() + 2);
    lines.push(format!(
        "Engine trace ({} event(s), newest last):",
        events.len()
    ));
    for e in &events {
        let status = if e.ok { "ok" } else { "ERR" };
        if e.detail.is_empty() {
            lines.push(format!(
                "  [{:>6} ms] {:<28} {status}",
                e.elapsed_ms, e.name
            ));
        } else {
            lines.push(format!(
                "  [{:>6} ms] {:<28} {status}  {}",
                e.elapsed_ms, e.name, e.detail
            ));
        }
    }
    let total: u128 = events.iter().map(|e| e.elapsed_ms).sum();
    lines.push(format!("  sum(spans) = {total} ms (overlaps possible)"));
    lines.join("\n")
}

fn push(ev: TraceEvent) {
    eprintln!(
        "trace: {:>6} ms  {:<28} {} {}",
        ev.elapsed_ms,
        ev.name,
        if ev.ok { "ok" } else { "ERR" },
        ev.detail
    );
    if let Ok(mut g) = LOG.lock() {
        g.push(ev);
        let excess = g.len().saturating_sub(MAX_EVENTS);
        if excess > 0 {
            g.drain(0..excess);
        }
    }
}

/// RAII span — records on drop. Use `fail()` before early return on error paths
/// when you don't use [`timed`]/[`timed_result`].
pub struct Span {
    name: &'static str,
    detail: String,
    start: Instant,
    ok: bool,
    finished: bool,
}

impl Span {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            detail: String::new(),
            start: Instant::now(),
            ok: true,
            finished: false,
        }
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }

    pub fn set_detail(&mut self, detail: impl Into<String>) {
        self.detail = detail.into();
    }

    pub fn fail(&mut self) {
        self.ok = false;
    }

    pub fn finish(mut self) {
        self.record();
        self.finished = true;
    }

    fn record(&mut self) {
        if self.finished {
            return;
        }
        push(TraceEvent {
            name: self.name,
            elapsed_ms: self.start.elapsed().as_millis(),
            detail: std::mem::take(&mut self.detail),
            ok: self.ok,
        });
        self.finished = true;
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        self.record();
    }
}

/// Time a fallible async engine call and record ok/ERR.
pub async fn timed_result<T, E, F>(
    name: &'static str,
    detail: impl Into<String>,
    f: F,
) -> Result<T, E>
where
    F: std::future::Future<Output = Result<T, E>>,
{
    let mut span = Span::new(name).detail(detail);
    let res = f.await;
    if res.is_err() {
        span.fail();
    }
    span.finish();
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_snapshots() {
        clear();
        let s = Span::new("test.span").detail("x");
        s.finish();
        let snap = snapshot();
        assert!(snap
            .iter()
            .any(|e| e.name == "test.span" && e.detail == "x" && e.ok));
        let report = format_report();
        assert!(report.contains("test.span"));
        clear();
    }

    #[test]
    fn fail_marks_err() {
        clear();
        let mut s = Span::new("test.fail");
        s.fail();
        drop(s);
        let snap = snapshot();
        assert!(snap.iter().any(|e| e.name == "test.fail" && !e.ok));
        clear();
    }
}
