use super::*;
use crate::{SceneInstance, World};
use std::hash::{Hash, Hasher};
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rect {
    pub min: [f32; 2],
    pub size: [f32; 2],
}
impl Rect {
    pub fn contains(self, p: [f32; 2]) -> bool {
        self.size.iter().all(|s| *s > 0.)
            && (0..2).all(|i| p[i] >= self.min[i] && p[i] <= self.min[i] + self.size[i])
    }
    pub fn intersect(self, other: Self) -> Self {
        let min = std::array::from_fn(|i| self.min[i].max(other.min[i]));
        Self {
            min,
            size: std::array::from_fn(|i| {
                ((self.min[i] + self.size[i]).min(other.min[i] + other.size[i]) - min[i]).max(0.)
            }),
        }
    }
    pub fn inset(self, padding: [f32; 4]) -> Self {
        Self {
            min: [self.min[0] + padding[0], self.min[1] + padding[1]],
            size: [
                (self.size[0] - padding[0] - padding[2]).max(0.),
                (self.size[1] - padding[1] - padding[3]).max(0.),
            ],
        }
    }
    pub fn array(self) -> [f32; 4] {
        [self.min[0], self.min[1], self.size[0], self.size[1]]
    }
}
#[derive(Clone, Debug)]
pub struct Element {
    pub id: u64,
    pub owner: String,
    pub parent: Option<String>,
    pub rect: Rect,
    pub clip: Rect,
    pub widget: Widget,
    pub text: String,
    pub label: String,
    pub description: String,
    pub value: f32,
    pub enabled: bool,
    pub focused: bool,
    pub hovered: bool,
    pub pressed: bool,
    pub font_size: f32,
    pub scale: f32,
    pub high_contrast: bool,
    pub scroll: f32,
    pub scroll_max: f32,
}
#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub elements: Vec<Element>,
    pub reduced_motion: bool,
    pub size: [f32; 2],
}
impl Frame {
    /// Visible controls and scroll areas need a free pointer. Decorative HUD elements do not.
    pub fn wants_pointer(&self) -> bool {
        self.elements.iter().any(|element| {
            element.enabled
                && element.clip.size.iter().all(|size| *size > 0.)
                && (element.widget.kind.interactive()
                    || element.widget.scrollable && element.scroll_max > 0.)
        })
    }
    pub fn element(&self, owner: &str) -> Option<&Element> {
        self.elements.iter().find(|e| e.owner == owner)
    }
    pub fn scroll_ancestor(&self, element: &Element) -> Option<&Element> {
        let mut parent = element.parent.as_deref();
        while let Some(id) = parent {
            let ancestor = self.element(id)?;
            if ancestor.widget.scrollable {
                return Some(ancestor);
            }
            parent = ancestor.parent.as_deref();
        }
        None
    }
    pub fn focusable(&self) -> Vec<&Element> {
        let mut elements: Vec<_> = self
            .elements
            .iter()
            .filter(|e| {
                e.enabled
                    && e.widget.kind.interactive()
                    && (e.clip.size.iter().all(|v| *v > 0.) || self.scroll_ancestor(e).is_some())
            })
            .collect();
        elements.sort_by_key(|e| e.widget.focus_order);
        elements
    }
    /// Panels occlude lower UI; child labels/images still route activation to a parent button.
    pub fn hit(&self, point: [f32; 2]) -> Option<&Element> {
        for element in self
            .elements
            .iter()
            .rev()
            .filter(|e| e.rect.contains(point) && e.clip.contains(point))
        {
            if element.widget.kind.interactive() {
                return element.enabled.then_some(element);
            }
            if element.widget.kind == WidgetKind::Panel && element.widget.background[3] > 0. {
                let mut parent = element.parent.as_deref();
                while let Some(id) = parent {
                    let ancestor = self.element(id)?;
                    if ancestor.widget.kind.interactive() {
                        return ancestor.enabled.then_some(ancestor);
                    }
                    parent = ancestor.parent.as_deref();
                }
                return None;
            }
        }
        None
    }
    pub fn blocks_pointer(&self, point: [f32; 2]) -> bool {
        self.elements.iter().any(|e| {
            e.rect.contains(point)
                && e.clip.contains(point)
                && (e.widget.kind.interactive()
                    || e.widget.kind == WidgetKind::Panel && e.widget.background[3] > 0.)
        })
    }
}
fn anchored(parent: Rect, a: Anchors, scale: f32) -> Rect {
    let size = std::array::from_fn(|i| {
        (parent.size[i] * (a.max[i] - a.min[i]) + a.size[i] * scale).max(0.)
    });
    Rect {
        min: std::array::from_fn(|i| {
            parent.min[i]
                + parent.size[i] * (a.min[i] + (a.max[i] - a.min[i]) * a.pivot[i])
                + a.offset[i] * scale
                - size[i] * a.pivot[i]
        }),
        size,
    }
}
impl SceneInstance {
    pub fn ui_frame(&self, world: &World, layer: Layer, size: [f32; 2]) -> Result<Frame> {
        self.ui_frame_inner(
            world,
            layer,
            size,
            world.resource::<crate::GameSession>().map(|s| s.phase),
        )
    }
    /// Editor preview uses authored phases without changing the game's session.
    pub fn ui_frame_for_phase(
        &self,
        world: &World,
        layer: Layer,
        size: [f32; 2],
        phase: GamePhase,
    ) -> Result<Frame> {
        self.ui_frame_inner(world, layer, size, Some(phase))
    }
    fn ui_frame_inner(
        &self,
        world: &World,
        layer: Layer,
        size: [f32; 2],
        phase: Option<GamePhase>,
    ) -> Result<Frame> {
        ensure!(
            size.iter()
                .all(|v| v.is_finite() && (1.0..=32768.).contains(v)),
            "invalid UI viewport"
        );
        let runtime = world.resource::<super::runtime::Runtime>();
        let preferences = world.resource::<super::runtime::Preferences>();
        let session = world.resource::<crate::GameSession>();
        let locale = world
            .query::<Localization>()
            .filter_map(|(entity, locale)| {
                self.object_indices
                    .get(&entity)
                    .map(|&index| (&self.document.objects[index].id, locale))
            })
            .min_by_key(|(id, _)| *id)
            .map(|(_, locale)| locale);
        let mut children: BTreeMap<&str, Vec<(&Object, Widget)>> = BTreeMap::new();
        // Project runtime world labels using the active gameplay camera at the actual viewport
        // aspect ratio. The label's size remains in canvas pixels as the world moves beneath it.
        let world_projection =
            if runtime.is_some_and(|r| r.widgets.values().any(|s| s.world_position.is_some())) {
                let camera_id = world
                    .resource::<crate::middleware::timeline::Runtime>()
                    .and_then(|r| r.cameras.get(&layer))
                    .filter(|id| {
                        self.entity(id)
                            .is_some_and(|e| world.get::<crate::Camera>(e).is_some())
                    })
                    .or_else(|| self.document.views.get(&layer));
                camera_id
                    .and_then(|id| {
                        self.entity(id).and_then(|entity| {
                            world
                                .get::<crate::Camera>(entity)
                                .map(|camera| (id, camera))
                        })
                    })
                    .map(|(id, camera)| -> Result<_> {
                        Ok(camera.projection(size[0] / size[1])?
                            * self.global_transform(world, id)?.inverse())
                    })
                    .transpose()?
            } else {
                None
            };
        let mut roots = Vec::new();
        // Query live components (including ones added at runtime), not every terrain
        // and machine object. Preserve document order for widgets with equal order.
        let mut ui_objects: Vec<_> = world
            .query::<Canvas>()
            .map(|(entity, _)| entity)
            .chain(world.query::<Widget>().map(|(entity, _)| entity))
            .filter_map(|entity| self.object_indices.get(&entity).copied())
            .collect();
        ui_objects.sort_unstable();
        ui_objects.dedup();
        for index in ui_objects {
            let object = &self.document.objects[index];
            let entity = self.entities[&object.id];
            if world
                .get::<crate::BlueprintHidden>(entity)
                .is_some_and(|h| h.0)
            {
                continue;
            }
            if let Some(canvas) = world
                .get::<Canvas>(entity)
                .filter(|c| c.enabled && c.layer == layer && c.phase.matches(phase))
            {
                roots.push((object, canvas));
            }
            if let (Some(parent), Some(widget)) =
                (object.parent.as_deref(), world.get::<Widget>(entity))
            {
                let state = runtime.and_then(|r| r.widgets.get(&object.id));
                if state.and_then(|s| s.visible).unwrap_or(widget.visible) {
                    let mut widget = widget.clone();
                    if let Some(size) = state.and_then(|s| s.size) {
                        widget.anchors.size = size;
                    }
                    if let Some(offset) = state.and_then(|s| s.offset) {
                        widget.anchors.offset = offset;
                    }
                    if let Some(color) = state.and_then(|s| s.background) {
                        widget.background = color;
                    }
                    children.entry(parent).or_default().push((object, widget));
                }
            }
        }
        roots.sort_by_key(|(_, c)| c.order);
        for nodes in children.values_mut() {
            nodes.sort_by_key(|(_, w)| w.order);
        }
        let viewport = Rect { min: [0.; 2], size };
        let mut frame = Frame {
            size,
            ..Default::default()
        };
        let mut text_bytes = 0;
        for (root, canvas) in roots {
            let scale = match canvas.scaling {
                ScaleMode::Fit => {
                    (size[0] / canvas.reference[0]).min(size[1] / canvas.reference[1])
                }
                ScaleMode::Width => size[0] / canvas.reference[0],
                ScaleMode::Height => size[1] / canvas.reference[1],
                ScaleMode::Pixels => 1.,
            };
            let text_scale = preferences
                .and_then(|p| p.text_scale)
                .unwrap_or(canvas.text_scale);
            let high_contrast = preferences
                .and_then(|p| p.high_contrast)
                .unwrap_or(canvas.high_contrast);
            frame.reduced_motion |= preferences
                .and_then(|p| p.reduced_motion)
                .unwrap_or(canvas.reduced_motion);
            let mut stack = vec![(root.id.as_str(), viewport, viewport, true, 1.0f32, 0usize)];
            while let Some((
                parent,
                parent_rect,
                inherited_clip,
                parent_enabled,
                parent_opacity,
                depth,
            )) = stack.pop()
            {
                ensure!(depth <= 128, "UI hierarchy exceeds 128 levels");
                let Some(nodes) = children.get(parent) else {
                    continue;
                };
                let parent_widget = self.entity(parent).and_then(|e| world.get::<Widget>(e));
                let inside = parent_widget.map_or(parent_rect, |w| {
                    parent_rect.inset(w.padding.map(|p| p * scale))
                });
                let layout = parent_widget.map_or(Layout::Absolute, |w| w.layout);
                let gap = parent_widget.map_or(0., |w| w.gap * scale);
                let texts: Vec<String> = nodes
                    .iter()
                    .map(|(object, widget)| {
                        let state = runtime.and_then(|r| r.widgets.get(&object.id));
                        let bound = match widget.binding.as_str() {
                            "game.message" => session.map(|s| s.message.as_str()),
                            "game.title" => {
                                self.document.game_flow.as_ref().map(|s| s.title.as_str())
                            }
                            "game.instructions" => self
                                .document
                                .game_flow
                                .as_ref()
                                .map(|s| s.instructions.as_str()),
                            _ => None,
                        };

                        state
                            .and_then(|s| s.text.as_deref())
                            .or(bound)
                            .or_else(|| {
                                locale.and_then(|l| {
                                    l.text(
                                        preferences.and_then(|p| p.language.as_deref()),
                                        &widget.locale_key,
                                    )
                                })
                            })
                            .unwrap_or(&widget.text)
                            .to_owned()
                    })
                    .collect();
                let columns = if layout == Layout::Grid {
                    parent_widget.unwrap().columns as usize
                } else {
                    1
                };
                let width = (inside.size[0] - gap * (columns - 1) as f32) / columns as f32;
                let heights = nodes
                    .iter()
                    .zip(&texts)
                    .map(|((_, w), text)| -> Result<f32> {
                        let base = w.anchors.size[1].max(0.) * scale;
                        if !w.auto_text_height || text.is_empty() {
                            return Ok(base);
                        }
                        let font = (w.font_size * scale * text_scale).clamp(0.001, 1000.);
                        let padding = w.padding.map(|p| p * scale);
                        let extra = if w.kind == WidgetKind::Toggle {
                            font * 0.8 + 8. * scale
                        } else {
                            0.
                        };
                        let width = (width - padding[0] - padding[2] - extra).clamp(0.001, 10000.);
                        let height = bozzard_text::screen_bounds_with_font(
                            text,
                            font,
                            Some(width),
                            false,
                            0,
                            None,
                        )?
                        .map_or(0., |b| b[1][1] - b[0][1]);
                        Ok(base.max(
                            height
                                + padding[1]
                                + padding[3]
                                + if w.kind == WidgetKind::Slider {
                                    18. * scale
                                } else {
                                    0.
                                },
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let fixed = if matches!(layout, Layout::Row | Layout::Column) {
                    let axis = usize::from(layout == Layout::Column);
                    nodes
                        .iter()
                        .enumerate()
                        .filter(|(_, (_, w))| w.grow == 0.)
                        .map(|(i, (_, w))| {
                            if axis == 1 {
                                heights[i]
                            } else {
                                w.anchors.size[0].max(0.) * scale
                            }
                        })
                        .sum::<f32>()
                } else {
                    0.
                };
                let weight = nodes.iter().map(|(_, w)| w.grow).sum::<f32>();
                let mut cursor = 0.;
                let mut descendants = Vec::new();
                let mut rects = Vec::with_capacity(nodes.len());
                let rows = nodes.len().div_ceil(columns);
                let grid_base = ((inside.size[1] - gap * rows.saturating_sub(1) as f32)
                    / rows.max(1) as f32)
                    .max(0.);
                let row_heights: Vec<_> = heights
                    .chunks(columns)
                    .map(|row| row.iter().copied().fold(grid_base, f32::max))
                    .collect();
                for (index, (_, widget)) in nodes.iter().enumerate() {
                    let mut rect = match layout {
                        Layout::Absolute => anchored(inside, widget.anchors, scale),
                        Layout::Row | Layout::Column => {
                            let axis = usize::from(layout == Layout::Column);
                            let remaining = (inside.size[axis]
                                - fixed
                                - gap * nodes.len().saturating_sub(1) as f32)
                                .max(0.);
                            let length = if widget.grow > 0. && weight > 0. {
                                (remaining * widget.grow / weight).max(if axis == 1 {
                                    heights[index]
                                } else {
                                    0.
                                })
                            } else {
                                if axis == 1 {
                                    heights[index]
                                } else {
                                    widget.anchors.size[0].max(0.) * scale
                                }
                            };
                            let mut rect = inside;
                            rect.min[axis] += cursor;
                            rect.size[axis] = length;
                            cursor += length + gap;
                            rect
                        }
                        Layout::Grid => {
                            let columns = parent_widget.unwrap().columns as usize;
                            let cell = [width.max(0.), row_heights[index / columns]];
                            Rect {
                                min: [
                                    inside.min[0] + (index % columns) as f32 * (cell[0] + gap),
                                    inside.min[1]
                                        + row_heights[..index / columns].iter().sum::<f32>()
                                        + (index / columns) as f32 * gap,
                                ],
                                size: cell,
                            }
                        }
                    };
                    if rect.size[1] < heights[index] {
                        if layout == Layout::Absolute {
                            rect.min[1] -=
                                (heights[index] - rect.size[1]) * widget.anchors.pivot[1];
                        }
                        rect.size[1] = heights[index];
                    }
                    rects.push(rect);
                }
                let scroll_max = if parent_widget.is_some_and(|w| w.scrollable) {
                    rects
                        .iter()
                        .map(|r| r.min[1] + r.size[1] - inside.min[1] - inside.size[1])
                        .fold(0., f32::max)
                } else {
                    0.
                };
                let scroll = runtime
                    .and_then(|r| r.widgets.get(parent))
                    .map_or(0., |s| s.scroll)
                    .clamp(0., scroll_max);
                if let Some(element) = frame.elements.iter_mut().find(|e| e.owner == parent) {
                    element.scroll = scroll;
                    element.scroll_max = scroll_max;
                }
                for (((object, widget), mut rect), text) in nodes.iter().zip(rects).zip(texts) {
                    rect.min[1] -= scroll;
                    let state = runtime.and_then(|r| r.widgets.get(&object.id));
                    if let Some(position) = state.and_then(|s| s.screen_position) {
                        rect.min = std::array::from_fn(|axis| {
                            (position[axis] * size[axis] + widget.anchors.offset[axis] * scale
                                - rect.size[axis] * widget.anchors.pivot[axis])
                                .clamp(0., (size[axis] - rect.size[axis]).max(0.))
                        });
                    }
                    if let Some(position) = state.and_then(|s| s.world_position) {
                        let Some(projection) = world_projection else {
                            continue;
                        };
                        let projected = projection * glam::Vec3::from(position).extend(1.);
                        if projected.w <= 0. || projected.z < 0. || projected.z > projected.w {
                            continue;
                        }
                        let ndc = projected.truncate() / projected.w;
                        let pixel = [(ndc.x * 0.5 + 0.5) * size[0], (0.5 - ndc.y * 0.5) * size[1]];
                        rect.min = std::array::from_fn(|axis| {
                            pixel[axis] + widget.anchors.offset[axis] * scale
                                - rect.size[axis] * widget.anchors.pivot[axis]
                        });
                    }
                    if rect.size.iter().any(|v| *v <= 0.) {
                        continue;
                    }
                    let clip = rect.intersect(inherited_clip);
                    let opacity = parent_opacity * state.and_then(|s| s.opacity).unwrap_or(1.);
                    let mut appearance = widget.clone();
                    appearance.background[3] *= opacity;
                    appearance.text_color[3] *= opacity;
                    let enabled =
                        parent_enabled && state.and_then(|s| s.enabled).unwrap_or(widget.enabled);
                    text_bytes += text.len();
                    ensure!(
                        text_bytes <= 65536,
                        "UI text exceeds 65536 visible UTF-8 bytes"
                    );
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    self.entity(&object.id).unwrap().hash(&mut h);
                    frame.elements.push(Element {
                        id: h.finish().max(1),
                        owner: object.id.clone(),
                        parent: object.parent.clone(),
                        rect,
                        clip,
                        widget: appearance,
                        label: if widget.accessible_name.is_empty() {
                            text.clone()
                        } else {
                            widget.accessible_name.clone()
                        },
                        description: widget.description.clone(),
                        text,
                        value: state.and_then(|s| s.value).unwrap_or(widget.value),
                        enabled,
                        focused: enabled
                            && runtime.is_some_and(|r| r.focus.as_deref() == Some(&object.id)),
                        hovered: enabled
                            && runtime
                                .and_then(|r| r.pointer)
                                .is_some_and(|p| rect.contains(p) && clip.contains(p)),
                        pressed: enabled
                            && runtime.is_some_and(|r| r.active.as_deref() == Some(&object.id)),
                        font_size: (widget.font_size * scale * text_scale).clamp(0.001, 1000.),
                        scale,
                        high_contrast,
                        scroll: 0.,
                        scroll_max: 0.,
                    });
                    let next_clip = if widget.clip_children || widget.scrollable {
                        clip
                    } else {
                        inherited_clip
                    };
                    descendants.push((
                        object.id.as_str(),
                        rect,
                        next_clip,
                        enabled,
                        opacity,
                        depth + 1,
                    ));
                }
                // Preserve sibling order while processing each subtree before the next sibling.
                // A final hierarchy sort below makes parent backgrounds precede child content.
                stack.extend(descendants.into_iter().rev());
            }
        }
        let order: BTreeMap<_, _> = self
            .document
            .objects
            .iter()
            .enumerate()
            .map(|(i, o)| (o.id.as_str(), i))
            .collect();
        let index: BTreeMap<_, _> = frame
            .elements
            .iter()
            .enumerate()
            .map(|(i, e)| (e.owner.clone(), i))
            .collect();
        let paths: Vec<Vec<(i32, usize)>> = frame
            .elements
            .iter()
            .map(|e| {
                let mut path = vec![(e.widget.order, order[e.owner.as_str()])];
                let mut parent = e.parent.as_deref();
                while let Some(id) = parent {
                    if let Some(&i) = index.get(id) {
                        let node = &frame.elements[i];
                        path.push((node.widget.order, order[id]));
                        parent = node.parent.as_deref();
                    } else {
                        let canvas = self
                            .entity(id)
                            .and_then(|entity| world.get::<Canvas>(entity));
                        path.push((
                            canvas.map_or(0, |c| c.order),
                            order.get(id).copied().unwrap_or(0),
                        ));
                        break;
                    }
                }
                path.reverse();
                path
            })
            .collect();
        let mut ordered: Vec<_> = frame.elements.into_iter().zip(paths).collect();
        ordered.sort_by(|a, b| a.1.cmp(&b.1));
        frame.elements = ordered.into_iter().map(|(e, _)| e).collect();
        Ok(frame)
    }
}
