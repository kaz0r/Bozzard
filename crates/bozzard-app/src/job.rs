//! One bounded background operation. Cancellation prevents publication; codecs finish their current call.
use anyhow::{Result, bail};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, Ordering},
    mpsc,
};

#[derive(Clone, Default)]
pub struct Progress {
    cancelled: Arc<AtomicBool>,
    label: Arc<Mutex<String>>,
    fraction: Arc<AtomicU32>,
    range: Option<(f32, f32)>,
}
impl Progress {
    pub fn check(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Relaxed) {
            bail!("Loading cancelled");
        }
        Ok(())
    }
    pub fn stage(&self, label: impl Into<String>) -> Result<()> {
        self.check()?;
        *self.label.lock().unwrap_or_else(|e| e.into_inner()) = label.into();
        Ok(())
    }
    pub fn report(&self, completed: usize, total: usize, label: impl Into<String>) -> Result<()> {
        anyhow::ensure!(
            total > 0 && completed <= total,
            "invalid background progress"
        );
        self.stage(label)?;
        self.set_fraction(completed as f32 / total as f32)
    }
    /// Update measured progress without reallocating or replacing the stage label.
    pub fn set_fraction(&self, fraction: f32) -> Result<()> {
        anyhow::ensure!(
            (0.0..=1.0).contains(&fraction),
            "invalid background progress"
        );
        self.check()?;
        let (start, end) = self.range.unwrap_or((0., 1.));
        self.fraction.store(
            (start + (end - start) * fraction).to_bits(),
            Ordering::Relaxed,
        );
        Ok(())
    }
    pub fn fraction(&self) -> f32 {
        f32::from_bits(self.fraction.load(Ordering::Relaxed))
    }
    /// Share cancellation/labels while reporting a bounded part of a parent operation.
    pub fn subtask(&self, start: f32, end: f32) -> Result<Self> {
        anyhow::ensure!(
            (0.0..=1.0).contains(&start) && (start..=1.0).contains(&end),
            "invalid progress range"
        );
        let (base, limit) = self.range.unwrap_or((0., 1.));
        let mut child = self.clone();
        child.range = Some((base + (limit - base) * start, base + (limit - base) * end));
        Ok(child)
    }
}

pub struct Job<T> {
    receiver: mpsc::Receiver<Result<T>>,
    progress: Progress,
}
impl<T: Send + 'static> Job<T> {
    pub fn start(
        label: &str,
        work: impl FnOnce(Progress) -> Result<T> + Send + 'static,
    ) -> Result<Self> {
        let progress = Progress::default();
        progress.stage(label)?;
        let worker_progress = progress.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("bozzard-worker".into())
            .spawn(move || {
                let result = work(worker_progress.clone()).and_then(|value| {
                    worker_progress.check()?;
                    worker_progress
                        .fraction
                        .store(1_f32.to_bits(), Ordering::Relaxed);
                    Ok(value)
                });
                let _ = sender.send(result);
            })?;
        Ok(Self { receiver, progress })
    }
    pub fn cancel(&self) {
        self.progress.cancelled.store(true, Ordering::Relaxed);
    }
    pub fn cancelled(&self) -> bool {
        self.progress.cancelled.load(Ordering::Relaxed)
    }
    pub fn label(&self) -> String {
        self.progress
            .label
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    pub fn fraction(&self) -> f32 {
        self.progress.fraction()
    }
    pub fn poll(&self) -> Option<Result<T>> {
        match self.receiver.try_recv() {
            Ok(value) => Some(value.and_then(|value| {
                self.progress.check()?;
                Ok(value)
            })),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(anyhow::anyhow!(
                "Background worker stopped unexpectedly"
            ))),
        }
    }
}
impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        self.progress.cancelled.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    #[test]
    fn nested_progress_ranges_share_cancellation_and_report_parent_fraction() {
        let parent = Progress::default();
        let child = parent.subtask(0.2, 0.6).unwrap();
        child.report(1, 2, "halfway").unwrap();
        assert!((parent.fraction() - 0.4).abs() < 1e-6);
        let nested = child.subtask(0.5, 1.).unwrap();
        nested.report(1, 2, "nested").unwrap();
        assert!((parent.fraction() - 0.5).abs() < 1e-6);
        assert!(parent.subtask(f32::NAN, 1.).is_err());
        assert!(parent.subtask(0.9, 0.1).is_err());
        nested.set_fraction(0.25).unwrap();
        assert!((parent.fraction() - 0.45).abs() < 1e-6);
        assert!(parent.set_fraction(f32::NAN).is_err());
        parent.cancelled.store(true, Ordering::Relaxed);
        assert!(nested.report(1, 1, "cancelled").is_err());
    }
    fn wait<T: Send + 'static>(job: &Job<T>) -> Result<T> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(result) = job.poll() {
                return result;
            }
            assert!(Instant::now() < deadline, "worker timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn polling_does_not_wait_and_cancellation_discards_results() {
        let (release, gate) = mpsc::channel();
        let (started, start) = mpsc::channel();
        let job = Job::start("Queued", move |progress| {
            assert!(progress.report(2, 1, "invalid").is_err());
            assert!(progress.report(0, 0, "invalid").is_err());
            progress.report(1, 2, "Decoding model (1/2)")?;
            started.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(42)
        })
        .unwrap();
        start.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(job.label(), "Decoding model (1/2)");
        assert_eq!(job.fraction(), 0.5);
        assert!(job.poll().is_none());
        job.cancel();
        release.send(()).unwrap();
        assert!(wait(&job).unwrap_err().to_string().contains("cancelled"));
    }
    #[test]
    fn worker_errors_and_panics_become_visible_failures() {
        let error = Job::<()>::start("Error", |_| bail!("broken image")).unwrap();
        assert!(
            wait(&error)
                .unwrap_err()
                .to_string()
                .contains("broken image")
        );
        let panic = Job::<()>::start("Panic", |_| panic!("test worker panic")).unwrap();
        assert!(
            wait(&panic)
                .unwrap_err()
                .to_string()
                .contains("stopped unexpectedly")
        );
    }
    #[test]
    fn cancelling_an_already_completed_result_still_discards_it() {
        let job = Job::start("Result", |_| Ok(42)).unwrap();
        // Wait for delivery without publishing the result through poll.
        let result = job.receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        let (sender, receiver) = mpsc::sync_channel(1);
        sender.send(result).unwrap();
        let job = Job {
            receiver,
            progress: job.progress.clone(),
        };
        job.cancel();
        assert!(job.poll().unwrap().is_err());
    }
}
