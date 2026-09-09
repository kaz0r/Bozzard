//! One bounded background operation. Cancellation prevents publication; codecs finish their current call.
use anyhow::{Result, bail};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

#[derive(Clone, Default)]
pub struct Progress {
    cancelled: Arc<AtomicBool>,
    label: Arc<Mutex<String>>,
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
            .name("bozzard-assets".into())
            .spawn(move || {
                let result = work(worker_progress.clone()).and_then(|value| {
                    worker_progress.check()?;
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
    pub fn poll(&self) -> Option<Result<T>> {
        match self.receiver.try_recv() {
            Ok(value) => Some(value.and_then(|value| {
                self.progress.check()?;
                Ok(value)
            })),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err(anyhow::anyhow!("Asset worker stopped unexpectedly")))
            }
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
            progress.stage("Decoding model (1/2)")?;
            started.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(42)
        })
        .unwrap();
        start.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(job.label(), "Decoding model (1/2)");
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
