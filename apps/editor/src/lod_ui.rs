use super::*;

pub struct LodTools {
    levels: usize,
    distance: f32,
    ratio: f32,
    error: f32,
    lock_borders: bool,
}
impl Default for LodTools {
    fn default() -> Self {
        Self {
            levels: 2,
            distance: 25.,
            ratio: 0.5,
            error: 0.01,
            lock_borders: true,
        }
    }
}
impl LodTools {
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        object: &bozzard_scene::Object,
        assets: &bozzard_assets::AssetStore,
    ) -> bool {
        let Some(bozzard_scene::Drawable {
            mesh: Mesh::Asset(id),
            ..
        }) = &object.drawable
        else {
            return false;
        };
        let Some(bozzard_assets::AssetData::Mesh(mesh)) = assets
            .handle(id)
            .and_then(|h| assets.get(h))
            .and_then(|e| e.data())
        else {
            return false;
        };
        let mut generate = false;
        egui::CollapsingHeader::new("GENERATE LODS").show(ui,|ui| {
            if mesh.skin.is_some() {ui.weak("Automatic LOD generation requires a static mesh.");return;}
            ui.label(format!("Base mesh: {} triangles",mesh.indices.len()/3));
            ui.horizontal(|ui| {ui.label("Levels");ui.add(egui::DragValue::new(&mut self.levels).range(1..=8));});
            ui.horizontal(|ui| {ui.label("First distance");ui.add(egui::DragValue::new(&mut self.distance).range(0.1..=100_000.).speed(1.));});
            ui.add(egui::Slider::new(&mut self.ratio,0.05..=0.95).text("Triangle ratio per level"));
            ui.add(egui::Slider::new(&mut self.error,0. ..=0.2).text("Maximum relative error"));
            ui.checkbox(&mut self.lock_borders,"Keep borders fixed");
            ui.small("Distances double per level. Seams and error limits can prevent the target ratio. Source geometry stays unchanged.");
            generate=ui.button(if object.lod.is_some() {"Generate and replace LOD levels"} else {"Generate LOD levels"}).clicked();
        });
        generate
    }
    pub fn requests(&self) -> Vec<bozzard_editor::LodRequest> {
        (0..self.levels)
            .map(|index| bozzard_editor::LodRequest {
                switch: self.distance * 2_f32.powi(index as i32),
                settings: bozzard_assets::SimplifySettings {
                    ratio: self.ratio.powi(index as i32 + 1),
                    max_error: self.error,
                    lock_borders: self.lock_borders,
                },
            })
            .collect()
    }
}
