//! Bounded, graphics-independent diagnostics. Never part of a scene or save game.
use bozzard_ecs::World;
use serde::Serialize;
use std::{collections::VecDeque, time::Instant};

pub const MAX_EVENTS: usize = 2048;
pub const MAX_MESSAGE_BYTES: usize = 4096;
pub const MAX_SPANS: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Level {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Location {
    pub scene: Option<String>,
    pub object: Option<String>,
    pub attachment: Option<usize>,
    pub node: Option<u32>,
    pub asset: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Event {
    pub id: u64,
    pub level: Level,
    pub source: String,
    pub message: String,
    pub location: Location,
    pub tick: Option<u64>,
    /// Time since this log was created, independent of game pause/restart.
    pub seconds: f64,
    pub repetitions: u64,
    #[serde(skip)]
    search: String,
}
impl Event {
    /// The caller lowercases its filter once, not once per visible row.
    pub fn matches(&self, lowercase_filter: &str) -> bool {
        self.search.contains(lowercase_filter)
    }
}

pub struct Console {
    pub events: VecDeque<Event>,
    pub discarded: u64,
    pub revision: u64,
    next_id: u64,
    started: Instant,
}
impl Default for Console {
    fn default() -> Self {
        Self {
            events: VecDeque::new(),
            discarded: 0,
            revision: 0,
            next_id: 0,
            started: Instant::now(),
        }
    }
}
fn bounded(text: &str, limit: usize) -> &str {
    &text[..text.floor_char_boundary(text.len().min(limit))]
}
impl Console {
    pub fn clear(&mut self) {
        self.events.clear();
        self.discarded = 0;
        self.revision = self.revision.wrapping_add(1);
    }
    pub fn push(
        &mut self,
        level: Level,
        source: &str,
        message: &str,
        mut location: Location,
        tick: Option<u64>,
    ) {
        let message = bounded(message, MAX_MESSAGE_BYTES);
        let source = bounded(source, 128);
        for text in [
            &mut location.scene,
            &mut location.object,
            &mut location.asset,
        ]
        .into_iter()
        .flatten()
        {
            text.truncate(text.floor_char_boundary(text.len().min(1024)));
        }
        let seconds = self.started.elapsed().as_secs_f64();
        if let Some(last) = self.events.back_mut()
            && last.level == level
            && last.source == source
            && last.message == message
            && last.location == location
        {
            last.repetitions = last.repetitions.saturating_add(1);
            last.seconds = seconds;
            last.tick = tick;
            return;
        }
        // Repetition counts do not change search results; keep cached row indices valid.
        self.revision = self.revision.wrapping_add(1);
        if self.events.len() == MAX_EVENTS {
            self.events.pop_front();
            self.discarded = self.discarded.saturating_add(1);
        }
        self.next_id = self.next_id.wrapping_add(1);
        let search = format!(
            "{source} {message} {} {} {}",
            location.object.as_deref().unwrap_or(""),
            location.asset.as_deref().unwrap_or(""),
            location.scene.as_deref().unwrap_or("")
        )
        .to_lowercase();
        self.events.push_back(Event {
            id: self.next_id,
            level,
            source: source.into(),
            message: message.into(),
            location,
            tick,
            seconds,
            repetitions: 1,
            search,
        });
    }
    /// Move a runtime's new messages into the editor log without cloning its history.
    pub fn drain_into(&mut self, target: &mut Console) {
        target.discarded = target.discarded.saturating_add(self.discarded);
        self.discarded = 0;
        for event in self.events.drain(..) {
            target.push(
                event.level,
                &event.source,
                &event.message,
                event.location,
                event.tick,
            );
            if let Some(last) = target.events.back_mut() {
                last.repetitions = last.repetitions.saturating_add(event.repetitions - 1);
            }
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct CpuSpan {
    pub name: &'static str,
    pub parent: Option<usize>,
    pub tick: Option<u64>,
    pub start_ms: f64,
    pub duration_ms: f64,
}
pub struct SpanToken {
    index: usize,
    epoch: u64,
    start: Instant,
}
#[derive(Default)]
pub struct Profiler {
    pub recording: bool,
    pub spans: Vec<CpuSpan>,
    pub dropped: u64,
    start: Option<Instant>,
    stack: Vec<usize>,
    epoch: u64,
}
impl Profiler {
    pub fn begin_frame(&mut self) {
        self.spans.clear();
        self.stack.clear();
        self.dropped = 0;
        self.epoch = self.epoch.wrapping_add(1);
        self.start = self.recording.then(Instant::now);
    }
    pub fn begin(&mut self, name: &'static str, tick: Option<u64>) -> Option<SpanToken> {
        let frame_start = self.start.filter(|_| self.recording)?;
        if self.spans.len() >= MAX_SPANS {
            self.dropped = self.dropped.saturating_add(1);
            return None;
        }
        let start = Instant::now();
        let index = self.spans.len();
        self.spans.push(CpuSpan {
            name,
            parent: self.stack.last().copied(),
            tick,
            start_ms: start.duration_since(frame_start).as_secs_f64() * 1000.,
            duration_ms: 0.,
        });
        self.stack.push(index);
        Some(SpanToken {
            index,
            epoch: self.epoch,
            start,
        })
    }
    pub fn end(&mut self, token: Option<SpanToken>) {
        if let Some(token) = token
            && token.epoch == self.epoch
        {
            self.spans[token.index].duration_ms = token.start.elapsed().as_secs_f64() * 1000.;
            let closed = self.stack.pop();
            debug_assert_eq!(closed, Some(token.index));
        }
    }
}

#[derive(Default)]
pub struct Diagnostics {
    pub console: Console,
    pub profiler: Profiler,
    pub tick: Option<u64>,
}

/// Disabled capture does no clock reads, string construction or allocations.
pub fn measure<T>(
    world: &mut World,
    name: &'static str,
    operation: impl FnOnce(&mut World) -> T,
) -> T {
    let token = world
        .resource_mut::<Diagnostics>()
        .and_then(|d| d.profiler.begin(name, d.tick));
    let result = operation(world);
    if let Some(diagnostics) = world.resource_mut::<Diagnostics>() {
        diagnostics.profiler.end(token);
    }
    result
}
pub fn log(world: &mut World, level: Level, source: &str, message: &str, location: Location) {
    if let Some(diagnostics) = world.resource_mut::<Diagnostics>() {
        diagnostics
            .console
            .push(level, source, message, location, diagnostics.tick);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn console_is_bounded_searchable_collapses_repeats_and_keeps_distinct_sources() {
        let mut log = Console::default();
        let location = Location {
            object: Some("Door".into()),
            node: Some(12),
            ..Default::default()
        };
        for _ in 0..100 {
            log.push(
                Level::Error,
                "Blueprint",
                "Missing target",
                location.clone(),
                Some(3),
            );
        }
        assert_eq!(log.events.len(), 1);
        assert_eq!(
            log.revision, 1,
            "repeats must not invalidate cached filters"
        );
        assert_eq!(log.events[0].repetitions, 100);
        assert!(log.events[0].matches("door"));
        assert!(log.events[0].matches("missing target"));
        log.push(
            Level::Info,
            "Blueprint",
            "Missing target",
            location.clone(),
            None,
        );
        assert_eq!(log.events.len(), 2);
        for i in 0..MAX_EVENTS {
            log.push(
                Level::Info,
                "Test",
                &i.to_string(),
                Location::default(),
                None,
            );
        }
        assert_eq!(log.events.len(), MAX_EVENTS);
        assert_eq!(log.discarded, 2);
        let last = log.events.back().unwrap().id;
        log.clear();
        log.push(
            Level::Info,
            "Test",
            &"🐕".repeat(2000),
            Location::default(),
            None,
        );
        assert!(log.events[0].id > last);
        assert_eq!(log.events[0].message.len(), MAX_MESSAGE_BYTES);
        assert_eq!(log.discarded, 0);
    }
    #[test]
    fn disabled_profiling_is_empty_and_nested_scopes_keep_their_parent() {
        let mut world = World::default();
        world.insert_resource(Diagnostics::default());
        for _ in 0..100 {
            measure(&mut world, "Off", |_| ());
        }
        let diagnostics = world.resource_mut::<Diagnostics>().unwrap();
        assert_eq!(diagnostics.profiler.spans.capacity(), 0);
        diagnostics.tick = Some(42);
        diagnostics.profiler.recording = true;
        diagnostics.profiler.begin_frame();
        let value = measure(&mut world, "Tick", |world| measure(world, "Physics", |_| 7));
        assert_eq!(value, 7);
        let diagnostics = world.resource::<Diagnostics>().unwrap();
        let spans = &diagnostics.profiler.spans;
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[1].parent, Some(0));
        assert_eq!(spans[1].tick, Some(42));
        assert!(spans[0].duration_ms >= spans[1].duration_ms);
        assert!(diagnostics.profiler.stack.is_empty());
    }
    #[test]
    fn profiling_overflow_and_runtime_log_transfer_are_bounded() {
        let mut profiler = Profiler {
            recording: true,
            ..Default::default()
        };
        profiler.begin_frame();
        for _ in 0..MAX_SPANS + 10 {
            let token = profiler.begin("Work", None);
            profiler.end(token);
        }
        assert_eq!(profiler.spans.len(), MAX_SPANS);
        assert_eq!(profiler.dropped, 10);
        profiler.begin_frame();
        assert!(profiler.spans.is_empty());
        let mut runtime = Console::default();
        let mut editor = Console::default();
        for _ in 0..2 {
            runtime.push(Level::Info, "Script", "Hello", Location::default(), None);
        }
        runtime.drain_into(&mut editor);
        runtime.push(Level::Info, "Script", "Hello", Location::default(), None);
        runtime.drain_into(&mut editor);
        assert!(runtime.events.is_empty());
        assert_eq!(editor.events.len(), 1);
        assert_eq!(editor.events[0].repetitions, 3);
    }
}
