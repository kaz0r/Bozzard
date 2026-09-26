//! A single owned simulation update overlapped with work on an immutable frame.
//!
//! The caller regains its complete App before this API returns, including on errors
//! and unwinding. There is no shared mutable ECS world and no unbounded tick queue.
use crate::{Advance, App};
use anyhow::{Context, Result, bail};
use bozzard_diagnostics::SimulationMetrics;
use std::{
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::mpsc::{self, Receiver, SyncSender},
    thread::JoinHandle,
    time::{Duration, Instant},
};

struct Request {
    app: App,
    elapsed: Duration,
}
struct Completed {
    app: App,
    outcome: Result<Advance, String>,
    cpu_ms: f64,
}

pub struct SimulationWorker {
    requests: Option<SyncSender<Request>>,
    completed: Receiver<Completed>,
    thread: Option<JoinHandle<()>>,
    spare: Option<App>,
    failed: bool,
}

impl SimulationWorker {
    pub fn failed(&self) -> bool {
        self.failed
    }

    pub fn new() -> Result<Self> {
        let (requests, receive) = mpsc::sync_channel::<Request>(1);
        let (publish, completed) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("bozzard-simulation".into())
            .spawn(move || {
                while let Ok(mut request) = receive.recv() {
                    let started = Instant::now();
                    let outcome =
                        catch_unwind(AssertUnwindSafe(|| request.app.advance(request.elapsed)))
                            .map_err(|panic| {
                                panic
                                    .downcast_ref::<String>()
                                    .cloned()
                                    .or_else(|| {
                                        panic.downcast_ref::<&str>().map(|s| (*s).to_owned())
                                    })
                                    .unwrap_or_else(|| "unknown panic".into())
                            });
                    if publish
                        .send(Completed {
                            app: request.app,
                            outcome,
                            cpu_ms: started.elapsed().as_secs_f64() * 1000.,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .context("starting simulation worker")?;
        Ok(Self {
            requests: Some(requests),
            completed,
            thread: Some(thread),
            spare: Some(App::default()),
            failed: false,
        })
    }

    /// `frame` runs on the calling thread while the worker exclusively owns `app`.
    /// Prepare all render/UI/asset inputs before calling; no live world is accessible
    /// to the closure. One completed update is joined before the next can be sent.
    pub fn advance_with<R>(
        &mut self,
        app: &mut App,
        elapsed: Duration,
        frame: impl FnOnce() -> R,
    ) -> Result<R> {
        if self.failed {
            bail!("simulation worker failed; restart the scene");
        }
        let request = Request {
            app: std::mem::replace(app, self.spare.take().expect("idle simulation worker")),
            elapsed,
        };
        if let Err(error) = self.requests.as_ref().expect("live worker").send(request) {
            self.spare = Some(std::mem::replace(app, error.0.app));
            self.failed = true;
            bail!("simulation worker disconnected");
        }
        let rendered = catch_unwind(AssertUnwindSafe(frame));
        let wait = Instant::now();
        let completed = match self.completed.recv() {
            Ok(completed) => completed,
            Err(error) => {
                self.failed = true;
                return Err(error).context("receiving simulation update");
            }
        };
        let wait_ms = wait.elapsed().as_secs_f64() * 1000.;
        self.spare = Some(std::mem::replace(app, completed.app));
        self.failed = completed.outcome.is_err();
        app.world.insert_resource(SimulationMetrics {
            available: true,
            threaded: true,
            cpu_ms: completed.cpu_ms,
            wait_ms,
            steps: completed
                .outcome
                .as_ref()
                .map_or(0, |advance| advance.steps),
        });
        let result = match rendered {
            Ok(result) => result,
            Err(panic) => resume_unwind(panic),
        };
        completed
            .outcome
            .map_err(|error| anyhow::anyhow!("simulation worker panicked: {error}"))?;
        Ok(result)
    }
}

impl Drop for SimulationWorker {
    fn drop(&mut self) {
        // No work is in flight outside advance_with. Closing the sender wakes the
        // idle worker, and joining prevents leaked threads on Stop/reload/Exit.
        self.requests.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Same tick and frame ordering as the worker path, for reproducible A/B comparisons.
pub fn advance_serial<R>(app: &mut App, elapsed: Duration, frame: impl FnOnce() -> R) -> R {
    let started = Instant::now();
    let advance = app.advance(elapsed);
    app.world.insert_resource(SimulationMetrics {
        available: true,
        threaded: false,
        cpu_ms: started.elapsed().as_secs_f64() * 1000.,
        wait_ms: 0.,
        steps: advance.steps,
    });
    frame()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn simulation_overlaps_the_frame_and_reuses_one_worker_without_changing_world_identity() {
        let mut app = App::default();
        let entity = app.world.spawn();
        app.world.insert(entity, 0_u32).unwrap();
        let (started, starts) = mpsc::sync_channel(1);
        let (release, releases) = mpsc::sync_channel(1);
        app.add_system(move |world, _, _| {
            started.send(std::thread::current().id()).unwrap();
            releases.recv_timeout(Duration::from_secs(5)).unwrap();
            *world.get_mut::<u32>(entity).unwrap() += 1;
        });
        let mut worker = SimulationWorker::new().unwrap();
        let mut id = None;
        let step = app.timestep();
        for expected in 1..=3 {
            worker
                .advance_with(&mut app, step, || {
                    // The system cannot finish until this main-thread frame runs.
                    let running = starts.recv_timeout(Duration::from_secs(5)).unwrap();
                    assert_ne!(running, std::thread::current().id());
                    if let Some(id) = id {
                        assert_eq!(id, running);
                    }
                    id = Some(running);
                    release.send(()).unwrap();
                })
                .unwrap();
            assert_eq!(*app.world.get::<u32>(entity).unwrap(), expected);
            assert_eq!(app.ticks(), u64::from(expected));
            assert!(app.world.resource::<SimulationMetrics>().unwrap().threaded);
        }
    }

    #[test]
    fn render_errors_and_panics_restore_the_world_and_worker_panics_are_reported() {
        let mut app = App::default();
        let entity = app.world.spawn();
        app.world.insert(entity, 42_u32).unwrap();
        let step = app.timestep();
        let mut worker = SimulationWorker::new().unwrap();
        let result = worker.advance_with(&mut app, step, || Err::<(), _>("draw failed"));
        assert_eq!(result.unwrap(), Err("draw failed"));
        assert_eq!(app.ticks(), 1);
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                let _ = worker.advance_with(&mut app, step, || panic!("render panic"));
            }))
            .is_err()
        );
        assert_eq!(app.ticks(), 2);
        assert_eq!(app.world.get::<u32>(entity), Some(&42));
        worker.advance_with(&mut app, step, || ()).unwrap();
        app.add_system(|_, _, _| panic!("simulation panic"));
        let error = worker.advance_with(&mut app, step, || ()).unwrap_err();
        assert!(error.to_string().contains("simulation panic"));
        assert_eq!(app.world.get::<u32>(entity), Some(&42));
        assert!(worker.advance_with(&mut app, step, || ()).is_err());
    }

    #[test]
    fn system_order_catch_up_and_pause_match_serial_execution() {
        fn fixture() -> App {
            let mut app = App::default();
            app.world.insert_resource(Vec::<u64>::new());
            for stage in 0..3 {
                app.add_system(move |world, commands, tick| {
                    world
                        .resource_mut::<Vec<u64>>()
                        .unwrap()
                        .push(tick.number * 10 + stage);
                    commands.queue(move |world| {
                        world
                            .resource_mut::<Vec<u64>>()
                            .unwrap()
                            .push(1000 + tick.number * 10 + stage);
                    });
                });
            }
            app
        }
        let mut serial = fixture();
        let mut threaded = fixture();
        let mut worker = SimulationWorker::new().unwrap();
        for (ms, paused) in [
            (1, false),
            (16, false),
            (80, false),
            (300, true),
            (500, false),
        ] {
            for app in [&mut serial, &mut threaded] {
                app.world
                    .resource_mut::<bozzard_diagnostics::ExecutionControl>()
                    .unwrap()
                    .paused = paused;
            }
            advance_serial(&mut serial, Duration::from_millis(ms), || ());
            worker
                .advance_with(&mut threaded, Duration::from_millis(ms), || ())
                .unwrap();
            assert_eq!(serial.ticks(), threaded.ticks());
            assert_eq!(
                serial.world.resource::<Vec<u64>>(),
                threaded.world.resource::<Vec<u64>>()
            );
        }
        // Dropping an idle worker must join immediately rather than leave a process thread.
        let stopped = Arc::new(Mutex::new(false));
        struct Marker(Arc<Mutex<bool>>);
        impl Drop for Marker {
            fn drop(&mut self) {
                *self.0.lock().unwrap() = true;
            }
        }
        threaded.world.insert_resource(Marker(stopped.clone()));
        drop(worker);
        assert!(
            !*stopped.lock().unwrap(),
            "the worker must return the real world"
        );
        drop(threaded);
        assert!(*stopped.lock().unwrap());
    }
}
