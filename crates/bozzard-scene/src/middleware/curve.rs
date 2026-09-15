//! Validated scalar curves shared by motion, animation, UI and particle modules.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub const MAX_KEYS: usize = 4096;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpolation {
    Step,
    #[default]
    Linear,
    /// Hermite tangents are derivatives per second, as in glTF CUBICSPLINE.
    Cubic,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Key {
    pub time: f32,
    pub value: f32,
    #[serde(default)]
    pub incoming: f32,
    #[serde(default)]
    pub outgoing: f32,
}
impl Key {
    pub fn new(time: f32, value: f32) -> Self {
        Self {
            time,
            value,
            incoming: 0.,
            outgoing: 0.,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Curve {
    pub interpolation: Interpolation,
    pub keys: Vec<Key>,
}
impl Default for Curve {
    fn default() -> Self {
        Self::constant(0.)
    }
}
impl Curve {
    pub fn constant(value: f32) -> Self {
        Self {
            interpolation: Interpolation::Linear,
            keys: vec![Key::new(0., value)],
        }
    }
    pub fn linear(start: f32, end: f32, duration: f32) -> Self {
        Self {
            interpolation: Interpolation::Linear,
            keys: vec![Key::new(0., start), Key::new(duration, end)],
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.keys.is_empty() && self.keys.len() <= MAX_KEYS,
            "curve needs 1–{MAX_KEYS} keys"
        );
        ensure!(
            self.keys
                .iter()
                .all(|k| [k.time, k.value, k.incoming, k.outgoing]
                    .iter()
                    .all(|v| v.is_finite())
                    && k.time >= 0.),
            "curve keys need finite values and nonnegative times"
        );
        ensure!(
            self.keys.windows(2).all(|ks| ks[0].time < ks[1].time),
            "curve key times must increase strictly"
        );
        Ok(())
    }
    pub fn duration(&self) -> f32 {
        self.keys.last().map_or(0., |k| k.time)
    }
    /// O(log keys). Validate at the authoring/import boundary, not on every sampled frame.
    pub fn sample(&self, time: f32) -> f32 {
        let end = self.keys.partition_point(|key| key.time <= time);
        if end == 0 {
            return self.keys.first().map_or(0., |k| k.value);
        }
        if end == self.keys.len() {
            return self.keys[end - 1].value;
        }
        let a = self.keys[end - 1];
        let b = self.keys[end];
        let span = b.time - a.time;
        let t = (time - a.time) / span;
        match self.interpolation {
            Interpolation::Step => a.value,
            Interpolation::Linear => a.value + (b.value - a.value) * t,
            Interpolation::Cubic => {
                let t2 = t * t;
                let t3 = t2 * t;
                (2. * t3 - 3. * t2 + 1.) * a.value
                    + (t3 - 2. * t2 + t) * span * a.outgoing
                    + (-2. * t3 + 3. * t2) * b.value
                    + (t3 - t2) * span * b.incoming
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ease {
    #[default]
    Linear,
    Smooth,
    Smoother,
    InQuad,
    OutQuad,
    InOutQuad,
    InCubic,
    OutCubic,
    InOutCubic,
    InSine,
    OutSine,
    InOutSine,
}
impl Ease {
    pub fn sample(self, value: f32) -> f32 {
        let t = value.clamp(0., 1.);
        match self {
            Self::Linear => t,
            Self::Smooth => t * t * (3. - 2. * t),
            Self::Smoother => t * t * t * (t * (t * 6. - 15.) + 10.),
            Self::InQuad => t * t,
            Self::OutQuad => 1. - (1. - t).powi(2),
            Self::InOutQuad if t < 0.5 => 2. * t * t,
            Self::InOutQuad => 1. - (-2. * t + 2.).powi(2) * 0.5,
            Self::InCubic => t * t * t,
            Self::OutCubic => 1. - (1. - t).powi(3),
            Self::InOutCubic if t < 0.5 => 4. * t * t * t,
            Self::InOutCubic => 1. - (-2. * t + 2.).powi(3) * 0.5,
            Self::InSine => 1. - (t * std::f32::consts::FRAC_PI_2).cos(),
            Self::OutSine => (t * std::f32::consts::FRAC_PI_2).sin(),
            Self::InOutSine => (1. - (t * std::f32::consts::PI).cos()) * 0.5,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Repeat {
    #[default]
    Once,
    Loop,
    PingPong,
}
/// A clock never applies an unbounded catch-up loop. Events consume its finite crossed interval.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Playhead {
    pub elapsed: f64,
    pub playing: bool,
    pub completed: bool,
}
impl Playhead {
    pub fn play(&mut self, restart: bool) {
        if restart || self.completed {
            *self = Self::default();
        }
        self.playing = true;
    }
    pub fn seek(&mut self, seconds: f64) -> Result<()> {
        ensure!(
            seconds.is_finite() && seconds >= 0.,
            "playback time must be finite and nonnegative"
        );
        self.elapsed = seconds;
        self.completed = false;
        Ok(())
    }
    pub fn advance(&mut self, dt: f32, speed: f32, duration: f32, repeat: Repeat) -> Result<()> {
        ensure!(
            dt.is_finite()
                && dt >= 0.
                && speed.is_finite()
                && speed >= 0.
                && duration.is_finite()
                && duration > 0.,
            "invalid playback step"
        );
        if self.playing {
            let next = self.elapsed + f64::from(dt) * f64::from(speed);
            ensure!(next.is_finite(), "playback clock overflow");
            self.elapsed = next;
            if repeat == Repeat::Once && next >= f64::from(duration) {
                self.elapsed = f64::from(duration);
                self.playing = false;
                self.completed = true;
            }
        }
        Ok(())
    }
    pub fn position(&self, duration: f32, repeat: Repeat) -> f32 {
        let d = f64::from(duration);
        match repeat {
            Repeat::Once => self.elapsed.min(d) as f32,
            Repeat::Loop => self.elapsed.rem_euclid(d) as f32,
            Repeat::PingPong => {
                let t = self.elapsed.rem_euclid(2. * d);
                (if t <= d { t } else { 2. * d - t }) as f32
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keys_interpolate_at_boundaries_and_cubic_tangents_use_seconds() {
        let mut curve = Curve::linear(0., 4., 2.);
        curve.validate().unwrap();
        assert_eq!(
            [
                curve.sample(-1.),
                curve.sample(1.),
                curve.sample(2.),
                curve.sample(9.)
            ],
            [0., 2., 4., 4.]
        );
        curve.interpolation = Interpolation::Step;
        assert_eq!(curve.sample(1.999), 0.);
        assert_eq!(curve.sample(2.), 4.);
        curve.interpolation = Interpolation::Cubic;
        curve.keys[0].outgoing = 4.;
        curve.keys[1].incoming = 0.;
        assert_eq!(curve.sample(1.), 3.);
        curve.keys[1].time = 0.;
        assert!(curve.validate().is_err());
    }
    #[test]
    fn playhead_stops_once_and_keeps_long_repeat_steps_bounded() {
        let mut clock = Playhead::default();
        clock.play(true);
        clock.advance(3., 1., 2., Repeat::Once).unwrap();
        assert!(clock.completed && !clock.playing);
        assert_eq!(clock.position(2., Repeat::Once), 2.);
        clock.play(false);
        assert_eq!(clock.elapsed, 0.);
        clock.advance(1_000_003., 1., 2., Repeat::PingPong).unwrap();
        assert_eq!(clock.position(2., Repeat::PingPong), 1.);
        let before = clock;
        assert!(clock.advance(f32::NAN, 1., 2., Repeat::Loop).is_err());
        assert_eq!(clock, before);
    }
}
