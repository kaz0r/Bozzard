//! Atlas sprites, frame animation and batched tilemaps. Simulation and layout remain CPU-only.
use super::{
    curve::{Playhead, Repeat},
    registry::Authored,
    signals::{Kind, Signal, Signals},
    timeline::crossed_markers,
};
use crate::{
    AssetKind, Component, Field, FieldValue, Layer, Object, Scene, SceneInstance, Ui, VectorRole,
    World,
};
use anyhow::{Context, Result, ensure};
use glam::Mat4;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Atlas {
    pub columns: u32,
    pub rows: u32,
}
impl Default for Atlas {
    fn default() -> Self {
        Self {
            columns: 1,
            rows: 1,
        }
    }
}
impl Atlas {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.columns > 0
                && self.rows > 0
                && self.columns <= 256
                && self.rows <= 256
                && self.columns * self.rows <= 4096,
            "atlas needs 1–4096 frames"
        );
        Ok(())
    }
    pub fn frames(self) -> u32 {
        self.columns * self.rows
    }
    pub fn uv(self, frame: u32, flip: [bool; 2]) -> [f32; 4] {
        let mut uv = [
            (frame % self.columns) as f32 / self.columns as f32,
            (frame / self.columns) as f32 / self.rows as f32,
            1. / self.columns as f32,
            1. / self.rows as f32,
        ];
        for axis in 0..2 {
            if flip[axis] {
                uv[axis] += uv[axis + 2];
                uv[axis + 2] = -uv[axis + 2];
            }
        }
        uv
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameEvent {
    pub frame: u32,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Clip {
    pub name: String,
    pub fps: f32,
    pub frames: Vec<u32>,
    pub repeat: Repeat,
    pub events: Vec<FrameEvent>,
}
impl Default for Clip {
    fn default() -> Self {
        Self {
            name: "Animation".into(),
            fps: 12.,
            frames: vec![0],
            repeat: Repeat::Loop,
            events: vec![],
        }
    }
}
impl Clip {
    pub fn duration(&self) -> f32 {
        self.frames.len() as f32 / self.fps
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sprite {
    pub enabled: bool,
    pub image: String,
    pub layer: Layer,
    pub atlas: Atlas,
    pub frame: u32,
    pub size: [f32; 2],
    pub pivot: [f32; 2],
    pub flip: [bool; 2],
    pub color: [f32; 4],
    pub autoplay: bool,
    pub initial: String,
    pub speed: f32,
    pub clips: Arc<Vec<Clip>>,
}
impl Default for Sprite {
    fn default() -> Self {
        Self {
            enabled: true,
            image: String::new(),
            layer: Layer::TwoD,
            atlas: Default::default(),
            frame: 0,
            size: [1.; 2],
            pivot: [0.5; 2],
            flip: [false; 2],
            color: [1.; 4],
            autoplay: true,
            initial: String::new(),
            speed: 1.,
            clips: Arc::new(vec![]),
        }
    }
}
impl Component for Sprite {
    const NAME: &'static str = "sprite";
    const LABEL: &'static str = "Sprite";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str = "Atlas frames count left-to-right, then top-to-bottom. Object transform positions/rotates the sprite in the XY plane. Frame clips and named events are controlled by Blueprints.";
    fn fields() -> &'static [Field] {
        const FIELDS: &[Field] = &[
            Field::bool("enabled", "Enabled"),
            Field::asset("image", "Image / atlas", AssetKind::Image),
            Field::options("layer", "View", &["2D", "3D"]),
            Field::integer_range("columns", "Atlas columns", 1., 1., 256.),
            Field::integer_range("rows", "Atlas rows", 1., 1., 256.),
            Field::integer_range("frame", "Frame", 1., 0., 4095.),
            Field::vector2("size", "Size", VectorRole::Scale, 0.05),
            Field::vector2("pivot", "Pivot", VectorRole::Position, 0.05).clamp(0., 1.),
            Field::bool("flip_x", "Flip X"),
            Field::bool("flip_y", "Flip Y"),
            Field::vector("color", "Color", VectorRole::Color, 0.01).clamp(0., 1.),
            Field::range("opacity", "Opacity", 0.01, 0., 1.),
            Field::bool("autoplay", "Play on start"),
            Field::range("speed", "Playback speed", 0.05, 0., 16.),
        ];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        Some(match key {
            "enabled" => FieldValue::Bool(self.enabled),
            "image" => FieldValue::Text(self.image.clone()),
            "layer" => FieldValue::Index(usize::from(self.layer == Layer::ThreeD)),
            "columns" => FieldValue::Number(self.atlas.columns as f32),
            "rows" => FieldValue::Number(self.atlas.rows as f32),
            "frame" => FieldValue::Number(self.frame as f32),
            "size" => FieldValue::Vector([self.size[0], self.size[1], 0.]),
            "pivot" => FieldValue::Vector([self.pivot[0], self.pivot[1], 0.]),
            "flip_x" => FieldValue::Bool(self.flip[0]),
            "flip_y" => FieldValue::Bool(self.flip[1]),
            "color" => FieldValue::Vector([self.color[0], self.color[1], self.color[2]]),
            "opacity" => FieldValue::Number(self.color[3]),
            "autoplay" => FieldValue::Bool(self.autoplay),
            "speed" => FieldValue::Number(self.speed),
            _ => return None,
        })
    }
    fn set_field(&mut self, key: &str, v: FieldValue) -> Result<()> {
        match key {
            "enabled" => self.enabled = v.bool()?,
            "image" => self.image = v.text()?.into(),
            "layer" => {
                self.layer = if v.index()? == 0 {
                    Layer::TwoD
                } else {
                    Layer::ThreeD
                }
            }
            "columns" => self.atlas.columns = v.number()? as u32,
            "rows" => self.atlas.rows = v.number()? as u32,
            "frame" => self.frame = v.number()? as u32,
            "size" => {
                let p = v.vector()?;
                self.size = [p[0], p[1]];
            }
            "pivot" => {
                let p = v.vector()?;
                self.pivot = [p[0], p[1]];
            }
            "flip_x" => self.flip[0] = v.bool()?,
            "flip_y" => self.flip[1] = v.bool()?,
            "color" => self.color[..3].copy_from_slice(&v.vector()?),
            "opacity" => self.color[3] = v.number()?,
            "autoplay" => self.autoplay = v.bool()?,
            "speed" => self.speed = v.number()?,
            _ => anyhow::bail!("unknown sprite field"),
        };
        Ok(())
    }
}
impl Authored for Sprite {
    fn validate(&self) -> Result<()> {
        self.atlas.validate()?;
        ensure!(
            self.frame < self.atlas.frames()
                && self
                    .size
                    .iter()
                    .all(|v| v.is_finite() && (0.001..=10000.).contains(v))
                && self
                    .pivot
                    .iter()
                    .chain(&self.color)
                    .all(|v| v.is_finite() && (0.0..=1.).contains(v))
                && self.speed.is_finite()
                && (0.0..=16.).contains(&self.speed),
            "invalid sprite dimensions, color, pivot or frame"
        );
        ensure!(self.clips.len() <= 64, "sprite exceeds 64 clips");
        let mut names = BTreeSet::new();
        for clip in self.clips.iter() {
            ensure!(
                !clip.name.is_empty()
                    && clip.name.len() <= 128
                    && names.insert(clip.name.as_str())
                    && clip.fps.is_finite()
                    && (0.1..=240.).contains(&clip.fps)
                    && !clip.frames.is_empty()
                    && clip.frames.len() <= 4096
                    && clip.frames.iter().all(|f| *f < self.atlas.frames())
                    && clip.events.len() <= 1024,
                "invalid sprite animation clip"
            );
            for event in &clip.events {
                ensure!(
                    (event.frame as usize) < clip.frames.len()
                        && !event.name.is_empty()
                        && event.name.len() <= 256,
                    "invalid sprite animation event"
                );
            }
        }
        ensure!(
            self.initial.is_empty() || names.contains(self.initial.as_str()),
            "sprite initial clip missing"
        );
        Ok(())
    }
    fn initialize_runtime(&self, world: &mut World, owner: &str) -> Result<()> {
        if world.resource::<Runtime>().is_none() {
            world.insert_resource(Runtime::default());
        }
        world
            .resource_mut::<Runtime>()
            .unwrap()
            .players
            .insert(owner.into(), Player::new(self));
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Tilemap {
    pub enabled: bool,
    pub image: String,
    pub layer: Layer,
    pub atlas: Atlas,
    pub dimensions: [u32; 2],
    pub tile_size: [f32; 2],
    pub color: [f32; 4],
    pub cells: Arc<Vec<u32>>,
    pub solid: Arc<BTreeSet<u32>>,
}
impl Default for Tilemap {
    fn default() -> Self {
        Self {
            enabled: true,
            image: String::new(),
            layer: Layer::TwoD,
            atlas: Default::default(),
            dimensions: [8, 8],
            tile_size: [1.; 2],
            color: [1.; 4],
            cells: Arc::new(vec![0; 64]),
            solid: Arc::new(BTreeSet::new()),
        }
    }
}
impl Tilemap {
    pub fn resize(&mut self, size: [u32; 2]) -> Result<()> {
        ensure!(
            size.iter().all(|v| *v > 0 && *v <= 256) && size[0] * size[1] <= 65536,
            "tilemap dimensions exceed 256×256"
        );
        let mut cells = vec![0; (size[0] * size[1]) as usize];
        for y in 0..self.dimensions[1].min(size[1]) {
            for x in 0..self.dimensions[0].min(size[0]) {
                cells[(y * size[0] + x) as usize] =
                    self.cells[(y * self.dimensions[0] + x) as usize];
            }
        }
        self.dimensions = size;
        self.cells = Arc::new(cells);
        Ok(())
    }
    pub fn quads(&self) -> Arc<[[f32; 8]]> {
        self.cells
            .iter()
            .enumerate()
            .filter(|(_, f)| **f > 0)
            .map(|(i, &tile)| {
                let uv = self.atlas.uv(tile - 1, [false; 2]);
                [
                    (i % self.dimensions[0] as usize) as f32 * self.tile_size[0],
                    -((i / self.dimensions[0] as usize) as f32) * self.tile_size[1],
                    self.tile_size[0],
                    self.tile_size[1],
                    uv[0],
                    uv[1],
                    uv[2],
                    uv[3],
                ]
            })
            .collect::<Vec<_>>()
            .into()
    }
}
impl Component for Tilemap {
    const NAME: &'static str = "tilemap";
    const LABEL: &'static str = "Tilemap";
    const UI: Ui = Ui::Generic;
    const HELP: &'static str = "Tiles start at the object's top-left and extend right/down in local XY. Zero is empty; tile 1 uses atlas frame 0. Solid tile IDs become box query geometry. Rendering batches the entire map into one mesh.";
    fn fields() -> &'static [Field] {
        const FIELDS: &[Field] = &[
            Field::bool("enabled", "Enabled"),
            Field::asset("image", "Atlas image", AssetKind::Image),
            Field::integer_range("columns", "Atlas columns", 1., 1., 256.),
            Field::integer_range("rows", "Atlas rows", 1., 1., 256.),
            Field::integer_range("width", "Map width", 1., 1., 256.),
            Field::integer_range("height", "Map height", 1., 1., 256.),
            Field::vector2("tile_size", "Tile size", VectorRole::Scale, 0.05),
            Field::options("layer", "View", &["2D", "3D"]),
        ];
        FIELDS
    }
    fn field(&self, key: &str) -> Option<FieldValue> {
        Some(match key {
            "enabled" => FieldValue::Bool(self.enabled),
            "image" => FieldValue::Text(self.image.clone()),
            "columns" => FieldValue::Number(self.atlas.columns as f32),
            "rows" => FieldValue::Number(self.atlas.rows as f32),
            "width" => FieldValue::Number(self.dimensions[0] as f32),
            "height" => FieldValue::Number(self.dimensions[1] as f32),
            "tile_size" => FieldValue::Vector([self.tile_size[0], self.tile_size[1], 0.]),
            "layer" => FieldValue::Index(usize::from(self.layer == Layer::ThreeD)),
            _ => return None,
        })
    }
    fn set_field(&mut self, key: &str, v: FieldValue) -> Result<()> {
        match key {
            "enabled" => self.enabled = v.bool()?,
            "image" => self.image = v.text()?.into(),
            "columns" => self.atlas.columns = v.number()? as u32,
            "rows" => self.atlas.rows = v.number()? as u32,
            "width" => self.resize([v.number()? as u32, self.dimensions[1]])?,
            "height" => self.resize([self.dimensions[0], v.number()? as u32])?,
            "tile_size" => {
                let p = v.vector()?;
                self.tile_size = [p[0], p[1]];
            }
            "layer" => {
                self.layer = if v.index()? == 0 {
                    Layer::TwoD
                } else {
                    Layer::ThreeD
                }
            }
            _ => anyhow::bail!("unknown tilemap field"),
        };
        Ok(())
    }
}
impl Authored for Tilemap {
    fn validate_scene(&self, owner: &Object, _scene: &Scene, _ids: &BTreeSet<&str>) -> Result<()> {
        self.validate()?;
        ensure!(
            self.solid.is_empty()
                || owner.collider.is_none()
                    && owner.mesh_collider.is_none()
                    && owner.gravity.is_none()
                    && owner.player_controller.is_none(),
            "solid tilemaps own static collision geometry"
        );
        Ok(())
    }
    fn validate(&self) -> Result<()> {
        self.atlas.validate()?;
        ensure!(
            self.dimensions.iter().all(|v| *v > 0 && *v <= 256)
                && self.cells.len() == (self.dimensions[0] * self.dimensions[1]) as usize
                && self.cells.iter().all(|v| *v <= self.atlas.frames())
                && self
                    .solid
                    .iter()
                    .all(|v| *v > 0 && *v <= self.atlas.frames())
                && self
                    .tile_size
                    .iter()
                    .all(|v| v.is_finite() && (0.001..=1000.).contains(v))
                && self
                    .color
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.).contains(v)),
            "invalid tilemap dimensions, cells or atlas IDs"
        );
        Ok(())
    }
    fn initialize_runtime(&self, world: &mut World, owner: &str) -> Result<()> {
        if world.resource::<Runtime>().is_none() {
            world.insert_resource(Runtime::default());
        }
        world.resource_mut::<Runtime>().unwrap().tiles.insert(
            owner.into(),
            TileCache {
                source: self.clone(),
                quads: self.quads(),
                solids: self.solid_boxes(),
            },
        );
        Ok(())
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Player {
    pub clip: Option<usize>,
    pub clock: Playhead,
    pub frame: u32,
    pub include_start: bool,
    #[serde(skip)]
    source: Option<Sprite>,
    #[serde(skip)]
    quads: Arc<[[f32; 8]]>,
}
impl Player {
    fn new(source: &Sprite) -> Self {
        let clip = source.clips.iter().position(|c| c.name == source.initial);
        let mut player = Self {
            clip,
            frame: source.frame,
            include_start: true,
            ..Default::default()
        };
        player.clock.playing = source.autoplay && clip.is_some();
        player.refresh(source);
        player
    }
    fn refresh(&mut self, source: &Sprite) {
        if self.source.as_ref().is_none_or(|old| {
            old.atlas != source.atlas
                || old.size != source.size
                || old.pivot != source.pivot
                || old.flip != source.flip
        }) || self.quads.first().is_none_or(|q| {
            let uv = source.atlas.uv(self.frame, source.flip);
            q[4..] != uv
        }) {
            let uv = source.atlas.uv(self.frame, source.flip);
            self.quads = vec![[
                -source.size[0] * source.pivot[0],
                source.size[1] * (1. - source.pivot[1]),
                source.size[0],
                source.size[1],
                uv[0],
                uv[1],
                uv[2],
                uv[3],
            ]]
            .into();
            self.source = Some(source.clone());
        }
    }
}
#[derive(Clone, Debug)]
struct TileCache {
    source: Tilemap,
    quads: Arc<[[f32; 8]]>,
    solids: Vec<crate::BoxCollider>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Runtime {
    pub players: BTreeMap<String, Player>,
    #[serde(skip)]
    tiles: BTreeMap<String, TileCache>,
}
#[derive(Clone, Debug)]
pub struct Visual {
    pub motion_id: u64,
    pub model: Mat4,
    pub image: String,
    pub color: [f32; 4],
    pub quads: Arc<[[f32; 8]]>,
}
pub enum Control {
    Play { clip: String, restart: bool },
    Pause,
    Stop,
    Frame(u32),
}
impl SceneInstance {
    pub fn control_sprite(&self, world: &mut World, owner: &str, control: Control) -> Result<()> {
        let entity = self.entity(owner).context("sprite target missing")?;
        let source = world
            .get::<Sprite>(entity)
            .context("target has no Sprite")?
            .clone();
        let clip = if let Control::Play { clip, .. } = &control {
            Some(
                source
                    .clips
                    .iter()
                    .position(|c| &c.name == clip)
                    .context("sprite clip missing")?,
            )
        } else {
            None
        };
        if let Control::Frame(frame) = control {
            ensure!(frame < source.atlas.frames(), "sprite frame outside atlas");
        }
        if world.resource::<Runtime>().is_none() {
            world.insert_resource(Runtime::default());
        }
        let run = world
            .resource_mut::<Runtime>()
            .unwrap()
            .players
            .entry(owner.into())
            .or_insert_with(|| Player::new(&source));
        match control {
            Control::Play { restart, .. } => {
                let restart = restart || run.clip != clip;
                run.clip = clip;
                run.clock.play(restart);
                run.include_start = restart;
            }
            Control::Pause => run.clock.playing = false,
            Control::Stop => {
                run.clock = Playhead::default();
                run.clip = None;
                run.frame = source.frame;
            }
            Control::Frame(frame) => {
                run.clock.playing = false;
                run.clip = None;
                run.frame = frame;
            }
        }
        run.refresh(&source);
        Ok(())
    }
    pub fn set_tile(
        &self,
        world: &mut World,
        owner: &str,
        x: u32,
        y: u32,
        tile: u32,
    ) -> Result<()> {
        let entity = self.entity(owner).context("tilemap target missing")?;
        let source = {
            let mut map = world
                .get_mut::<Tilemap>(entity)
                .context("target has no Tilemap")?;
            ensure!(
                x < map.dimensions[0] && y < map.dimensions[1] && tile <= map.atlas.frames(),
                "tile coordinate or atlas ID out of range"
            );
            let index = (y * map.dimensions[0] + x) as usize;
            Arc::make_mut(&mut map.cells)[index] = tile;
            map.clone()
        };
        if world.resource::<Runtime>().is_none() {
            world.insert_resource(Runtime::default());
        }
        world.resource_mut::<Runtime>().unwrap().tiles.insert(
            owner.into(),
            TileCache {
                quads: source.quads(),
                solids: source.solid_boxes(),
                source,
            },
        );
        Ok(())
    }
    pub fn step_sprites(&self, world: &mut World, dt: f32) -> Result<()> {
        ensure!(dt.is_finite() && dt >= 0., "invalid sprite timestep");
        let mut runtime = world.remove_resource::<Runtime>().unwrap_or_default();
        let mut signals = world.remove_resource::<Signals>().unwrap_or_default();
        signals.begin(Kind::Sprite);
        let result = (|| -> Result<()> {
            for (owner, &entity) in &self.entities {
                if let Some(source) = world.get::<Sprite>(entity) {
                    let run = runtime
                        .players
                        .entry(owner.clone())
                        .or_insert_with(|| Player::new(source));
                    if source.enabled {
                        if let Some(clip) = run.clip.and_then(|i| source.clips.get(i)) {
                            let before = run.clock.elapsed;
                            let playing = run.clock.playing;
                            let completed = run.clock.completed;
                            run.clock
                                .advance(dt, source.speed, clip.duration(), clip.repeat)?;
                            let position = run.clock.position(clip.duration(), clip.repeat);
                            run.frame = clip.frames[((position * clip.fps).floor() as usize)
                                .min(clip.frames.len() - 1)];
                            if playing {
                                for i in crossed_markers(
                                    clip.events.iter().map(|e| e.frame as f32 / clip.fps),
                                    before,
                                    run.clock.elapsed,
                                    clip.duration(),
                                    clip.repeat,
                                    run.include_start,
                                )? {
                                    signals.emit(
                                        owner,
                                        Signal {
                                            kind: Kind::Sprite,
                                            name: clip.events[i].name.clone(),
                                            other: None,
                                            value: clip.events[i].frame as f32,
                                        },
                                    )?;
                                }
                                run.include_start = false;
                            }
                            if !completed && run.clock.completed {
                                signals.emit(
                                    owner,
                                    Signal {
                                        kind: Kind::Sprite,
                                        name: "Finished".into(),
                                        other: None,
                                        value: run.frame as f32,
                                    },
                                )?;
                            }
                        }
                        run.refresh(source);
                    }
                }
                if let Some(source) = world.get::<Tilemap>(entity)
                    && runtime.tiles.get(owner).is_none_or(|c| &c.source != source)
                {
                    runtime.tiles.insert(
                        owner.clone(),
                        TileCache {
                            source: source.clone(),
                            quads: source.quads(),
                            solids: source.solid_boxes(),
                        },
                    );
                }
            }
            runtime.players.retain(|id, _| {
                self.entity(id)
                    .is_some_and(|e| world.get::<Sprite>(e).is_some())
            });
            runtime.tiles.retain(|id, _| {
                self.entity(id)
                    .is_some_and(|e| world.get::<Tilemap>(e).is_some())
            });
            Ok(())
        })();
        world.insert_resource(runtime);
        world.insert_resource(signals);
        result
    }
    pub fn sprite_frame(&self, world: &World, layer: Layer) -> Result<Vec<Visual>> {
        let matrices = self.global_transforms(world)?;
        self.sprite_frame_with_matrices(world, layer, &matrices)
    }
    pub(crate) fn sprite_frame_with_matrices(
        &self,
        world: &World,
        layer: Layer,
        matrices: &BTreeMap<String, Mat4>,
    ) -> Result<Vec<Visual>> {
        let runtime = world.resource::<Runtime>();
        let mut visuals = Vec::new();
        for (owner, &entity) in &self.entities {
            if world
                .get::<crate::BlueprintHidden>(entity)
                .is_some_and(|h| h.0)
                || world
                    .resource::<crate::GameplayState>()
                    .is_some_and(|s| s.collected.contains(owner))
            {
                continue;
            }
            if let Some(source) = world
                .get::<Sprite>(entity)
                .filter(|s| s.enabled && s.layer == layer && !s.image.is_empty())
            {
                let quads = runtime
                    .and_then(|r| r.players.get(owner))
                    .filter(|r| !r.quads.is_empty())
                    .map_or_else(|| Player::new(source).quads, |r| r.quads.clone());
                use std::hash::{Hash, Hasher};
                let mut identity = std::collections::hash_map::DefaultHasher::new();
                (entity, "sprite").hash(&mut identity);
                visuals.push(Visual {
                    motion_id: identity.finish().max(1),
                    model: matrices[owner],
                    image: source.image.clone(),
                    color: source.color,
                    quads,
                });
            }
            if let Some(source) = world
                .get::<Tilemap>(entity)
                .filter(|s| s.enabled && s.layer == layer && !s.image.is_empty())
            {
                let quads = runtime
                    .and_then(|r| r.tiles.get(owner))
                    .map_or_else(|| source.quads(), |t| t.quads.clone());
                use std::hash::{Hash, Hasher};
                let mut identity = std::collections::hash_map::DefaultHasher::new();
                (entity, "tilemap").hash(&mut identity);
                visuals.push(Visual {
                    motion_id: identity.finish().max(1),
                    model: matrices[owner],
                    image: source.image.clone(),
                    color: source.color,
                    quads,
                });
            }
        }
        Ok(visuals)
    }
}

impl Tilemap {
    /// Merge adjacent solid tiles into rectangles so flat maps do not create one collider per tile.
    pub fn solid_boxes(&self) -> Vec<crate::BoxCollider> {
        let width = self.dimensions[0] as usize;
        let height = self.dimensions[1] as usize;
        let mut used = vec![false; self.cells.len()];
        let mut boxes = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let index = y * width + x;
                if used[index] || !self.solid.contains(&self.cells[index]) {
                    continue;
                }
                let mut w = 1;
                while x + w < width
                    && !used[index + w]
                    && self.solid.contains(&self.cells[index + w])
                {
                    w += 1;
                }
                let mut h = 1;
                while y + h < height
                    && (0..w).all(|dx| {
                        !used[(y + h) * width + x + dx]
                            && self.solid.contains(&self.cells[(y + h) * width + x + dx])
                    })
                {
                    h += 1;
                }
                for dy in 0..h {
                    for dx in 0..w {
                        used[(y + dy) * width + x + dx] = true;
                    }
                }
                boxes.push(crate::BoxCollider {
                    enabled: true,
                    center: [
                        (x as f32 + w as f32 * 0.5) * self.tile_size[0],
                        -(y as f32 + h as f32 * 0.5) * self.tile_size[1],
                        0.,
                    ],
                    size: [
                        w as f32 * self.tile_size[0],
                        h as f32 * self.tile_size[1],
                        0.5,
                    ],
                });
            }
        }
        boxes
    }
}
pub(crate) fn collision_boxes<'a>(
    world: &'a World,
    owner: &str,
    source: &Tilemap,
) -> std::borrow::Cow<'a, [crate::BoxCollider]> {
    world
        .resource::<Runtime>()
        .and_then(|r| r.tiles.get(owner))
        .filter(|t| &t.source == source)
        .map_or_else(
            || std::borrow::Cow::Owned(source.solid_boxes()),
            |t| std::borrow::Cow::Borrowed(t.solids.as_slice()),
        )
}
impl Runtime {
    pub fn restore_with_visuals(mut self, world: &mut World) {
        if let Some(old) = world.remove_resource::<Self>() {
            self.tiles = old.tiles;
            for (owner, player) in &mut self.players {
                if let Some(source) = old.players.get(owner).and_then(|p| p.source.as_ref()) {
                    player.refresh(source);
                }
            }
        }
        world.insert_resource(self);
    }
}

impl Sprite {
    pub fn bounds(&self) -> [glam::Vec3; 2] {
        [
            glam::Vec3::new(
                -self.size[0] * self.pivot[0],
                -self.size[1] * self.pivot[1],
                0.,
            ),
            glam::Vec3::new(
                self.size[0] * (1. - self.pivot[0]),
                self.size[1] * (1. - self.pivot[1]),
                0.,
            ),
        ]
    }
    pub fn contains(&self, p: glam::Vec3) -> bool {
        let [min, max] = self.bounds();
        p.x >= min.x && p.y >= min.y && p.x <= max.x && p.y <= max.y
    }
}
impl Tilemap {
    pub fn bounds(&self) -> [glam::Vec3; 2] {
        [
            glam::Vec3::new(0., -(self.dimensions[1] as f32) * self.tile_size[1], 0.),
            glam::Vec3::new(self.dimensions[0] as f32 * self.tile_size[0], 0., 0.),
        ]
    }
    pub fn contains(&self, p: glam::Vec3) -> bool {
        let x = (p.x / self.tile_size[0]).floor();
        let y = (-p.y / self.tile_size[1]).floor();
        x >= 0.
            && y >= 0.
            && x < self.dimensions[0] as f32
            && y < self.dimensions[1] as f32
            && self.cells[y as usize * self.dimensions[0] as usize + x as usize] != 0
    }
}
