//! Authored content systems shared by the editor, native player and headless simulation.
pub mod animation;
pub mod audio;
pub mod curve;
pub mod navigation;
pub mod particle;
pub mod registry;
pub mod signals;
pub mod sprite;
pub mod timeline;
pub mod tween;
pub mod ui;
pub const ENTRIES: &[registry::Entry] = &[
    registry::entry::<tween::Tween>(),
    registry::entry::<timeline::Timeline>(),
    registry::entry::<animation::Animator>(),
    registry::entry::<audio::AudioSource>(),
    registry::entry::<audio::AudioMixer>(),
    registry::entry::<audio::AudioListener>(),
    registry::entry::<navigation::NavSurface>(),
    registry::entry::<navigation::NavAgent>(),
    registry::entry::<sprite::Sprite>(),
    registry::entry::<sprite::Tilemap>(),
    registry::entry::<ui::Canvas>(),
    registry::entry::<ui::Widget>(),
    registry::entry::<ui::Localization>(),
    registry::entry::<particle::Modules>(),
];

pub mod checkpoint;

impl crate::SceneInstance {
    pub fn step_middleware(&self, world: &mut crate::World, dt: f32) -> anyhow::Result<()> {
        self.step_tweens(world, dt)?;
        self.step_timelines(world, dt)?;
        self.step_animations(world, dt)?;
        self.step_navigation(world, dt)?;
        self.step_sprites(world, dt)
    }
}
