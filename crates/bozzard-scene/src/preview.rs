//! Projection of validated source objects into transient authoring documents.
use crate::{Object, Result, middleware::registry};

impl Object {
    /// Keep authoring visuals and transform ancestry while removing gameplay
    /// dependencies that can refer to hidden components. Call only on a copy:
    /// saving, history and Play must continue to use the original document.
    pub fn prepare_authoring_preview(&mut self, visible: bool) -> Result<()> {
        // Exhaustive by design: new built-in fields require a preview decision,
        // just as new middleware declares its policy through Authored::PREVIEW.
        let Self {
            id: _,
            name: _,
            parent: _,
            transform: _,
            camera: _, // Retain inspection cameras even in a hidden document.
            blackboard,
            blueprints,
            script_manager,
            player_controller,
            gravity,
            joint,
            drawable,
            material,
            shader_graph,
            text_rendering,
            particle_emitter,
            light,
            lod,
            collider,
            mesh_collider,
            trigger,
            spin,
            extras: _,
        } = self;
        blackboard.clear();
        blueprints.clear();
        *script_manager = None;
        *player_controller = None;
        *gravity = None;
        *joint = None;
        if !visible {
            *drawable = None;
            *material = None;
            *shader_graph = None;
            *text_rendering = None;
            *particle_emitter = None;
            *light = None;
            *lod = None;
            *collider = None;
            *mesh_collider = None;
            *trigger = None;
            *spin = None;
        }
        registry::prepare_preview(self, visible)
    }
}
