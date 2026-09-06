//! Graphics-independent application, ordered systems, and fixed-step simulation.
pub use bozzard_ecs::{Commands, Entity, World};
use std::{collections::HashSet, fmt, num::NonZeroU32, time::Duration};

#[derive(Clone, Copy, Debug)]
pub struct Tick {
    pub number: u64,
    pub delta: Duration,
}

type System = Box<dyn FnMut(&mut World, &mut Commands, Tick) + Send>;

/// Compiled-in module interface. Dynamic binary loading/unloading is not supported yet.
pub trait Plugin {
    fn name(&self) -> &'static str;
    fn build(&self, app: &mut App);
}

#[derive(Debug, PartialEq, Eq)]
pub struct DuplicatePlugin(pub &'static str);
impl fmt::Display for DuplicatePlugin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "plugin '{}' is already registered", self.0)
    }
}
impl std::error::Error for DuplicatePlugin {}

#[derive(Debug, Clone, Copy)]
pub struct Advance {
    pub steps: u32,
    /// Wall time discarded to prevent unbounded catch-up; the fractional tick is retained.
    pub dropped: Duration,
    /// Fraction of the next simulation tick, for future render interpolation.
    pub interpolation: f64,
}

pub struct App {
    pub world: World,
    systems: Vec<System>,
    plugins: HashSet<&'static str>,
    commands: Commands,
    timestep: Duration,
    max_catch_up: NonZeroU32,
    accumulator: Duration,
    ticks: u64,
}

impl Default for App {
    fn default() -> Self {
        Self::new(
            Duration::from_secs_f64(1.0 / 60.0),
            NonZeroU32::new(8).unwrap(),
        )
    }
}

impl App {
    /// Panics if timestep is zero.
    pub fn new(timestep: Duration, max_catch_up: NonZeroU32) -> Self {
        assert!(!timestep.is_zero(), "timestep must be positive");
        Self {
            world: World::new(),
            systems: Vec::new(),
            plugins: HashSet::new(),
            commands: Commands::default(),
            timestep,
            max_catch_up,
            accumulator: Duration::ZERO,
            ticks: 0,
        }
    }
    pub fn add_plugin(&mut self, plugin: impl Plugin) -> Result<(), DuplicatePlugin> {
        if !self.plugins.insert(plugin.name()) {
            return Err(DuplicatePlugin(plugin.name()));
        }
        plugin.build(self);
        Ok(())
    }
    /// Systems execute serially in registration order. Commands flush after all systems.
    pub fn add_system(
        &mut self,
        system: impl FnMut(&mut World, &mut Commands, Tick) + Send + 'static,
    ) {
        self.systems.push(Box::new(system));
    }
    pub fn timestep(&self) -> Duration {
        self.timestep
    }
    pub fn ticks(&self) -> u64 {
        self.ticks
    }
    /// Advances exactly one tick, independent of wall-clock time (replays, tests, servers).
    pub fn step(&mut self) {
        let tick = Tick {
            number: self.ticks,
            delta: self.timestep,
        };
        for system in &mut self.systems {
            system(&mut self.world, &mut self.commands, tick);
        }
        self.commands.apply(&mut self.world);
        self.ticks = self.ticks.checked_add(1).expect("tick counter exhausted");
    }
    pub fn advance(&mut self, elapsed: Duration) -> Advance {
        self.accumulator = self.accumulator.saturating_add(elapsed);
        let mut steps = 0;
        while self.accumulator >= self.timestep && steps < self.max_catch_up.get() {
            self.step();
            self.accumulator -= self.timestep;
            steps += 1;
        }
        let remainder = self.accumulator.as_nanos() % self.timestep.as_nanos();
        let remainder = Duration::new(
            (remainder / 1_000_000_000) as u64,
            (remainder % 1_000_000_000) as u32,
        );
        let dropped = self.accumulator - remainder;
        self.accumulator = remainder;
        Advance {
            steps,
            dropped,
            interpolation: remainder.as_secs_f64() / self.timestep.as_secs_f64(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordered_systems_and_end_of_tick_barrier() {
        let mut app = App::default();
        app.world.insert_resource(Vec::<u64>::new());
        app.add_system(|world, commands, tick| {
            world.resource_mut::<Vec<u64>>().unwrap().push(tick.number);
            commands.queue(|world| {
                let e = world.spawn();
                world.insert(e, true).unwrap();
            });
        });
        app.add_system(|world, _, tick| {
            assert_eq!(
                world.resource::<Vec<u64>>().unwrap().last(),
                Some(&tick.number)
            );
            assert_eq!(world.query::<bool>().count(), tick.number as usize);
        });
        app.step();
        app.step();
        assert_eq!(app.world.len(), 2);
    }

    #[test]
    fn fixed_steps_are_independent_of_frame_partition() {
        let mut a = App::new(Duration::from_millis(10), NonZeroU32::new(8).unwrap());
        let mut b = App::new(Duration::from_millis(10), NonZeroU32::new(8).unwrap());
        for ms in [3, 8, 7, 12, 5] {
            a.advance(Duration::from_millis(ms));
        }
        let result = b.advance(Duration::from_millis(35));
        assert_eq!(a.ticks(), b.ticks());
        assert_eq!(result.steps, 3);
        assert!((result.interpolation - 0.5).abs() < 1e-9);
        assert_eq!(result.dropped, Duration::ZERO);
    }

    #[test]
    fn catch_up_is_bounded_without_losing_fractional_time() {
        let mut app = App::new(Duration::from_millis(10), NonZeroU32::new(2).unwrap());
        let result = app.advance(Duration::from_millis(105));
        assert_eq!(result.steps, 2);
        assert_eq!(result.dropped, Duration::from_millis(80));
        assert_eq!(app.advance(Duration::from_millis(5)).steps, 1);
    }

    #[test]
    fn duplicate_module_is_rejected_before_build() {
        struct Module;
        impl Plugin for Module {
            fn name(&self) -> &'static str {
                "test"
            }
            fn build(&self, app: &mut App) {
                app.world.spawn();
            }
        }
        let mut app = App::default();
        app.add_plugin(Module).unwrap();
        assert_eq!(app.add_plugin(Module), Err(DuplicatePlugin("test")));
        assert_eq!(app.world.len(), 1);
    }
}
