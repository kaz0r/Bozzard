//! Presentation measurements, independent of the fixed simulation clock and GPU APIs.
use serde::Serialize;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct RenderCounters {
    /// World objects with at least one surface inside the camera frustum. Excludes HUD.
    /// GPU occlusion may reject more objects after submission.
    pub visible_entities: usize,
    /// Color-pass mesh commands/triangles; excludes HUD, shadows, sky and particles.
    pub draw_calls: usize,
    pub triangles: u64,
    /// CPU renderer preparation, encoding and submission; not GPU execution time.
    pub cpu_draw_ms: f64,
    /// Last successfully rendered viewport; zero before a native frame is available.
    pub viewport_aspect: f32,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct RenderMetrics {
    pub available: bool,
    pub rate_ready: bool,
    pub fps: f64,
    pub frame_ms: f64,
    #[serde(flatten)]
    pub counters: RenderCounters,
}

/// Updated only by successful renders. Runtime-only: never serialized into a save.
#[derive(Default)]
pub struct RenderDiagnostics {
    pub metrics: RenderMetrics,
    last_frame: Option<Instant>,
    elapsed: Duration,
    frames: u32,
}

impl RenderDiagnostics {
    /// Average completed-frame intervals over at least a quarter second. Pauses and
    /// slow frames remain in the measurement instead of being reported as 60 Hz ticks.
    pub fn record(&mut self, now: Instant, counters: RenderCounters) {
        self.metrics.available = true;
        self.metrics.counters = counters;
        if let Some(previous) = self.last_frame.replace(now) {
            self.elapsed += now.saturating_duration_since(previous);
            self.frames += 1;
            if self.elapsed >= Duration::from_millis(250) {
                self.metrics.frame_ms = self.elapsed.as_secs_f64() * 1000. / f64::from(self.frames);
                self.metrics.fps = 1000. / self.metrics.frame_ms;
                self.metrics.rate_ready = true;
                self.elapsed = Duration::ZERO;
                self.frames = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_uses_completed_frame_intervals_and_includes_stalls() {
        let mut diagnostics = RenderDiagnostics::default();
        assert!(!diagnostics.metrics.available);
        let start = Instant::now();
        let counters = RenderCounters {
            visible_entities: 23,
            cpu_draw_ms: 2.5,
            ..Default::default()
        };
        diagnostics.record(start, counters);
        assert!(diagnostics.metrics.available);
        assert!(!diagnostics.metrics.rate_ready);
        for i in 1..=10 {
            diagnostics.record(start + Duration::from_millis(i * 25), counters);
        }
        assert_eq!(diagnostics.metrics.fps, 40.);
        assert_eq!(diagnostics.metrics.frame_ms, 25.);
        assert_eq!(diagnostics.metrics.counters.visible_entities, 23);
        diagnostics.record(start + Duration::from_millis(750), counters);
        assert_eq!(diagnostics.metrics.fps, 2.);
        assert_eq!(diagnostics.metrics.frame_ms, 500.);
    }
}
