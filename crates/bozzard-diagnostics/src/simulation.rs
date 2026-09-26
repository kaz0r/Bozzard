//! Last completed simulation batch, independent of renderer and presentation timing.
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct SimulationMetrics {
    pub available: bool,
    pub threaded: bool,
    /// Time advancing simulation, including any fixed-step catch-up ticks.
    pub cpu_ms: f64,
    /// Main-thread wait after submitting the prepared frame. Not total worker time.
    pub wait_ms: f64,
    pub steps: u32,
}
