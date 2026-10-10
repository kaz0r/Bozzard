use anyhow::{Result, ensure};
use bozzard_assets::{
    CookSource, cooked_model,
    job::{Job, Progress},
    terrain::{BrushMode, Terrain, TerrainBrush, TerrainPaintBrush},
};
use bozzard_editor::{Editor, TerrainRequest};
use bozzard_scene::{AssetKind, Material, Mesh, Scene, Texture};
use glam::Vec3;
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Result<Self> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "bozzard-terrain-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn wait<T: Send + 'static>(job: &Job<T>) -> Result<T> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(result) = job.poll() {
            return result;
        }
        ensure!(Instant::now() < deadline, "terrain preparation timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn editor(temp: &Temp) -> Result<Editor> {
    Editor::new(
        Scene::from_json(r#"{"version":1,"name":"Terrain","views":{},"objects":[]}"#)?,
        &temp.0.join("scene.json"),
    )
}
fn hit(editor: &Editor) -> Result<f32> {
    Ok(editor
        .collisions()?
        .raycast(Vec3::new(0., 10., 0.), Vec3::NEG_Y, 20., None)?
        .unwrap()
        .position
        .y)
}

fn paint_dirt(terrain: &mut Terrain) -> Result<()> {
    assert!(terrain.paint(TerrainPaintBrush {
        layer: 1,
        center: [0., 0.],
        radius: 3.,
        strength: 0.8,
    })?);
    Ok(())
}

#[test]
fn painting_is_one_undo_step_with_immutable_sources_and_portable_cooked_pixels() -> Result<()> {
    let temp = Temp::new()?;
    let mut editor = editor(&temp)?;
    let job = editor.terrain_job(TerrainRequest::Create {
        terrain: Terrain::flat([9, 9], [8., 8.])?,
        position: [0.; 3],
    })?;
    let id = editor.accept_terrain(wait(&job)?)?;
    let initial = editor.scene().clone();
    let initial_path = temp.0.join(&initial.assets.values().next().unwrap().path);
    let initial_bytes = fs::read(&initial_path)?;
    let vertices = editor.selected_mesh().unwrap().vertices.clone();
    let indices = editor.selected_mesh().unwrap().indices.clone();
    let original_collision = editor
        .selected_object()
        .unwrap()
        .mesh_collider
        .clone()
        .unwrap();
    let source = editor.terrain_source(&id)?;
    let mut terrain = source.terrain.clone();
    paint_dirt(&mut terrain)?;
    let paint = terrain.paint.clone();
    let before_revision = editor.revision();
    let job = editor.terrain_job(TerrainRequest::Sculpt { source, terrain })?;
    editor.accept_terrain(wait(&job)?)?;
    assert_eq!(editor.revision(), before_revision + 1);
    assert_eq!(editor.undo_label(), Some("Paint terrain"));
    assert_eq!(editor.selected_mesh().unwrap().vertices, vertices);
    assert_eq!(editor.selected_mesh().unwrap().indices, indices);
    assert_eq!(
        editor
            .selected_object()
            .unwrap()
            .mesh_collider
            .as_ref()
            .unwrap(),
        &original_collision
    );
    // The immutable triangles/BVH are retained, rather than replaced by equal geometry.
    assert_eq!(
        editor
            .selected_object()
            .unwrap()
            .mesh_collider
            .as_ref()
            .unwrap()
            .mesh
            .triangles()
            .as_ptr(),
        original_collision.mesh.triangles().as_ptr()
    );
    assert!(hit(&editor)?.abs() < 1e-5);
    assert_eq!(fs::read(&initial_path)?, initial_bytes);
    let drawable = editor.selected_object().unwrap().drawable.as_ref().unwrap();
    assert_eq!(drawable.texture, Texture::White);
    assert_eq!(drawable.color, [1.; 3]);
    assert_eq!(drawable.uv_scale, [1.; 2]);
    let edited = editor.scene().clone();
    let painted_path = temp.0.join(&edited.assets.values().next().unwrap().path);
    let painted_bytes = fs::read(&painted_path)?;
    let painted_mesh = editor.selected_mesh().unwrap().clone();
    let image = painted_mesh.parts[0].image.as_ref().unwrap().clone();
    assert_ne!(initial_bytes, painted_bytes);
    editor.undo()?;
    assert_eq!(editor.scene(), &initial);
    assert!(editor.selected_mesh().unwrap().parts.is_empty());
    editor.redo()?;
    assert_eq!(editor.scene(), &edited);
    assert_eq!(editor.undo_label(), Some("Paint terrain"));
    let scene_path = editor.path.clone();
    editor.save(&scene_path)?;
    let mut reopened = Editor::open(&scene_path)?;
    reopened.select_object(Some(id.clone()));
    assert_eq!(reopened.terrain_source(&id)?.terrain.paint, paint);
    assert_eq!(
        reopened.selected_mesh().unwrap().parts[0]
            .image
            .as_ref()
            .unwrap()
            .rgba,
        image.rgba
    );
    assert!(hit(&reopened)?.abs() < 1e-5);

    // Exercise the same source snapshot and cooked-model codec used by export.
    let progress = Progress::default();
    let snapshot = CookSource::read(AssetKind::Mesh, &painted_path, &progress)?;
    let cooked_bytes = snapshot.cook(&[], &progress)?;
    fs::remove_file(&painted_path)?;
    assert_eq!(snapshot.cook(&[], &progress)?, cooked_bytes);
    let cooked = cooked_model::decode(&cooked_bytes)?;
    assert_eq!(cooked.parts.len(), 1);
    assert_eq!(cooked.indices.len(), indices.len());
    assert_eq!(cooked.parts[0].source_key, painted_mesh.parts[0].source_key);
    assert_eq!(cooked.parts[0].start, painted_mesh.parts[0].start);
    assert_eq!(cooked.parts[0].count, painted_mesh.parts[0].count);
    let shading = cooked.parts[0].shading.as_ref().unwrap();
    let source_shading = painted_mesh.parts[0].shading.as_ref().unwrap();
    // Cooking may weld/reorder vertex storage. Compare the exact attributes in
    // triangle order so geometry, winding, normals, UVs and PBR tangents survive.
    for (&index, &source_index) in cooked.indices.iter().zip(&indices) {
        assert_eq!(
            cooked.vertices[index as usize].map(f32::to_bits),
            vertices[source_index as usize].map(f32::to_bits)
        );
        assert_eq!(
            shading.vertices[(index - shading.vertex_start) as usize].map(f32::to_bits),
            source_shading.vertices[(source_index - source_shading.vertex_start) as usize]
                .map(f32::to_bits)
        );
    }
    assert_eq!(cooked.parts[0].color, [1.; 4]);
    assert_eq!(shading.material.metallic, source_shading.material.metallic);
    assert_eq!(
        shading.material.roughness,
        source_shading.material.roughness
    );
    assert_eq!(
        shading.material.base_color_sampler,
        source_shading.material.base_color_sampler
    );
    let cooked_image = cooked.parts[0].image.as_ref().unwrap();
    assert_eq!(
        [cooked_image.width, cooked_image.height],
        [image.width, image.height]
    );
    assert_eq!(cooked_image.rgba, image.rgba);
    assert_eq!(fs::read(&initial_path)?, initial_bytes);
    Ok(())
}

#[test]
fn shared_terrain_paint_updates_defaults_once_and_preserves_authored_appearance() -> Result<()> {
    let temp = Temp::new()?;
    let mut editor = editor(&temp)?;
    let job = editor.terrain_job(TerrainRequest::Create {
        terrain: Terrain::flat([9; 2], [8.; 2])?,
        position: [0.; 3],
    })?;
    let id = editor.accept_terrain(wait(&job)?)?;
    let mut scene = editor.scene().clone();
    let original = editor.selected_object().unwrap().clone();
    let mut shared = original.clone();
    shared.id = "shared-terrain".into();
    shared.transform.translation[0] = 12.;
    scene.objects.push(shared);
    let mut authored = original.clone();
    authored.id = "authored-terrain".into();
    authored.transform.translation[0] = 24.;
    let drawable = authored.drawable.as_mut().unwrap();
    drawable.texture = Texture::Checker;
    drawable.color = [0.9, 0.8, 0.7];
    drawable.uv_scale = [3., 5.];
    authored.material = Some(Material::from_drawable(drawable));
    let authored_before = authored.clone();
    scene.objects.push(authored);
    editor.apply("Add terrain instances", scene)?;
    let source = editor.terrain_source(&id)?;
    let mut terrain = source.terrain.clone();
    paint_dirt(&mut terrain)?;
    let job = editor.terrain_job(TerrainRequest::Sculpt { source, terrain })?;
    editor.accept_terrain(wait(&job)?)?;
    for object in editor
        .scene()
        .objects
        .iter()
        .filter(|o| [id.as_str(), "shared-terrain"].contains(&o.id.as_str()))
    {
        let drawable = object.drawable.as_ref().unwrap();
        assert_eq!(drawable.texture, Texture::White);
        assert_eq!(drawable.color, [1.; 3]);
        assert_eq!(drawable.uv_scale, [1.; 2]);
    }
    assert_eq!(
        editor
            .scene()
            .objects
            .iter()
            .find(|o| o.id == "authored-terrain")
            .unwrap(),
        &authored_before
    );

    let mut scene = editor.scene().clone();
    let mut surface = original;
    surface.id = "painted-surface".into();
    let part = &editor.selected_mesh().unwrap().parts[0];
    let asset = scene.assets.keys().next().unwrap().clone();
    let drawable = surface.drawable.as_mut().unwrap();
    drawable.mesh = Mesh::Surface {
        asset,
        index: 0,
        source: part.source_key.clone(),
    };
    drawable.texture = Texture::White;
    drawable.color = [0.8; 3];
    drawable.uv_scale = [1.; 2];
    let mut material =
        bozzard_scene::SurfaceMaterialOverride::inherited(0, part.source_key.clone());
    material.tint = [0.7, 0.8, 0.9];
    drawable.material_overrides.push(material);
    let surface_before = surface.clone();
    scene.objects.push(surface);
    // An authored checker remains authored even if its value equals the original default.
    scene
        .objects
        .iter_mut()
        .find(|o| o.id == "shared-terrain")
        .unwrap()
        .drawable
        .as_mut()
        .unwrap()
        .texture = Texture::ProceduralChecker;
    editor.apply("Author painted surface", scene)?;
    let source = editor.terrain_source(&id)?;
    let mut terrain = source.terrain.clone();
    terrain.paint.as_mut().unwrap().layers[1].color = [0.4, 0.15, 0.06];
    let job = editor.terrain_job(TerrainRequest::Sculpt { source, terrain })?;
    editor.accept_terrain(wait(&job)?)?;
    let surface = editor
        .scene()
        .objects
        .iter()
        .find(|o| o.id == "painted-surface")
        .unwrap();
    assert_eq!(surface, &surface_before);
    assert!(
        editor
            .assets
            .mesh_surface_binding(&surface.drawable.as_ref().unwrap().mesh)
            .is_some()
    );
    assert_eq!(
        editor
            .scene()
            .objects
            .iter()
            .find(|o| o.id == "shared-terrain")
            .unwrap()
            .drawable
            .as_ref()
            .unwrap()
            .texture,
        Texture::ProceduralChecker
    );
    Ok(())
}

#[test]
fn painted_creation_and_stale_cancelled_or_external_strokes_are_atomic() -> Result<()> {
    let temp = Temp::new()?;
    let mut editor = editor(&temp)?;
    let mut terrain = Terrain::flat([9; 2], [8.; 2])?;
    paint_dirt(&mut terrain)?;
    let job = editor.terrain_job(TerrainRequest::Create {
        terrain,
        position: [0.; 3],
    })?;
    let id = editor.accept_terrain(wait(&job)?)?;
    assert_eq!(
        editor
            .selected_object()
            .unwrap()
            .drawable
            .as_ref()
            .unwrap()
            .texture,
        Texture::White
    );
    assert_eq!(editor.selected_mesh().unwrap().parts.len(), 1);
    let painted = editor.scene().clone();
    let prepare = |editor: &Editor| -> Result<_> {
        let source = editor.terrain_source(&id)?;
        let mut terrain = source.terrain.clone();
        terrain.paint.as_mut().unwrap().layers[2].tiling *= 2.;
        editor.terrain_job(TerrainRequest::Sculpt { source, terrain })
    };
    let job = prepare(&editor)?;
    let prepared = wait(&job)?;
    job.cancel();
    assert!(editor.accept_terrain(prepared).is_err());
    assert_eq!(editor.scene(), &painted);
    assert_eq!(fs::read_dir(temp.0.join("assets"))?.count(), 1);
    let job = prepare(&editor)?;
    let prepared = wait(&job)?;
    let mut changed = painted.clone();
    changed.name = "Changed during painting".into();
    editor.apply("Change scene", changed.clone())?;
    assert!(editor.accept_terrain(prepared).is_err());
    assert_eq!(editor.scene(), &changed);
    assert_eq!(fs::read_dir(temp.0.join("assets"))?.count(), 1);
    let job = prepare(&editor)?;
    let prepared = wait(&job)?;
    let asset_path = temp
        .0
        .join(&editor.scene().assets.values().next().unwrap().path);
    let bytes = fs::read(&asset_path)?;
    fs::write(&asset_path, [bytes.as_slice(), b"\n"].concat())?;
    assert!(editor.accept_terrain(prepared).is_err());
    assert_eq!(editor.scene(), &changed);
    assert_eq!(fs::read_dir(temp.0.join("assets"))?.count(), 1);
    Ok(())
}

#[test]
fn signed_zero_geometry_changes_rebuild_collision_and_retain_paint() -> Result<()> {
    let temp = Temp::new()?;
    let mut editor = editor(&temp)?;
    let mut terrain = Terrain::flat([3; 2], [2.; 2])?;
    paint_dirt(&mut terrain)?;
    let job = editor.terrain_job(TerrainRequest::Create {
        terrain,
        position: [0.; 3],
    })?;
    let id = editor.accept_terrain(wait(&job)?)?;
    let collision = editor
        .selected_object()
        .unwrap()
        .mesh_collider
        .clone()
        .unwrap();
    let source = editor.terrain_source(&id)?;
    let mut terrain = source.terrain.clone();
    let paint = terrain.paint.clone();
    terrain.heights[0] = -0.;
    let job = editor.terrain_job(TerrainRequest::Sculpt { source, terrain })?;
    editor.accept_terrain(wait(&job)?)?;
    assert_eq!(editor.undo_label(), Some("Edit terrain"));
    assert_eq!(editor.terrain_source(&id)?.terrain.paint, paint);
    assert_eq!(
        editor.selected_mesh().unwrap().vertices[0][1].to_bits(),
        (-0_f32).to_bits()
    );
    let rebuilt = &editor
        .selected_object()
        .unwrap()
        .mesh_collider
        .as_ref()
        .unwrap()
        .mesh;
    assert_ne!(
        rebuilt.triangles().as_ptr(),
        collision.mesh.triangles().as_ptr()
    );
    assert_eq!(rebuilt.triangles()[0][0][1].to_bits(), (-0_f32).to_bits());
    Ok(())
}

#[test]
fn sculpt_revisions_keep_render_collision_history_and_saved_sources_consistent() -> Result<()> {
    let temp = Temp::new()?;
    let mut editor = editor(&temp)?;
    let job = editor.terrain_job(TerrainRequest::Create {
        terrain: Terrain::flat([33, 33], [16., 16.])?,
        position: [0.; 3],
    })?;
    let id = editor.accept_terrain(wait(&job)?)?;
    let initial = editor.scene().clone();
    let initial_path = temp.0.join(&initial.assets.values().next().unwrap().path);
    let bytes = fs::read(&initial_path)?;
    assert!(hit(&editor)?.abs() < 1e-5);
    let source = editor.terrain_source(&id)?;
    let mut terrain = source.terrain.clone();
    terrain.brush(TerrainBrush {
        mode: BrushMode::Raise,
        center: [0., 0.],
        radius: 3.,
        strength: 2.,
        target_height: 0.,
    })?;
    let job = editor.terrain_job(TerrainRequest::Sculpt { source, terrain })?;
    editor.accept_terrain(wait(&job)?)?;
    assert_eq!(editor.scene().assets.len(), 1);
    assert_ne!(editor.scene().assets, initial.assets);
    assert_eq!(fs::read(&initial_path)?, bytes);
    assert!((hit(&editor)? - 2.).abs() < 1e-5);
    assert_eq!(
        editor
            .selected_mesh()
            .unwrap()
            .vertices
            .iter()
            .map(|v| v[1])
            .fold(f32::NEG_INFINITY, f32::max),
        2.
    );
    let edited = editor.scene().clone();
    editor.undo()?;
    assert_eq!(editor.scene(), &initial);
    assert!(hit(&editor)?.abs() < 1e-5);
    editor.redo()?;
    assert_eq!(editor.scene(), &edited);
    assert!((hit(&editor)? - 2.).abs() < 1e-5);
    let path = editor.path.clone();
    editor.save(&path)?;
    let opened = Editor::open(&path)?;
    assert_eq!(
        opened
            .terrain_source(&id)?
            .terrain
            .heights
            .iter()
            .copied()
            .fold(0., f32::max),
        2.
    );
    assert!((hit(&opened)? - 2.).abs() < 1e-5);
    Ok(())
}

#[test]
fn stale_cancelled_and_externally_changed_terrain_never_publish() -> Result<()> {
    let temp = Temp::new()?;
    let mut editor = editor(&temp)?;
    let create = || TerrainRequest::Create {
        terrain: Terrain::flat([3, 3], [2., 2.]).unwrap(),
        position: [0.; 3],
    };
    let job = editor.terrain_job(create())?;
    let prepared = wait(&job)?;
    let mut changed = editor.scene().clone();
    changed.name = "Changed".into();
    editor.apply("Change scene", changed.clone())?;
    assert!(editor.accept_terrain(prepared).is_err());
    assert_eq!(editor.scene(), &changed);
    assert_eq!(fs::read_dir(temp.0.join("assets"))?.count(), 0);
    let job = editor.terrain_job(create())?;
    let prepared = wait(&job)?;
    job.cancel();
    assert!(editor.accept_terrain(prepared).is_err());
    assert_eq!(fs::read_dir(temp.0.join("assets"))?.count(), 0);
    let job = editor.terrain_job(create())?;
    let id = editor.accept_terrain(wait(&job)?)?;
    let source = editor.terrain_source(&id)?;
    let terrain = source.terrain.clone();
    let job = editor.terrain_job(TerrainRequest::Sculpt { source, terrain })?;
    let prepared = wait(&job)?;
    let asset = temp
        .0
        .join(&editor.scene().assets.values().next().unwrap().path);
    let current = fs::read(&asset)?;
    fs::write(&asset, [current.as_slice(), b"\n"].concat())?;
    assert!(editor.accept_terrain(prepared).is_err());
    assert_eq!(fs::read_dir(temp.0.join("assets"))?.count(), 1);
    Ok(())
}

#[test]
fn terrain_and_brush_prefabs_keep_saved_geometry_after_later_sculpting() -> Result<()> {
    use bozzard_assets::blockout::{Blockout, BrushPrimitive};
    use bozzard_editor::PrefabCommand;
    use bozzard_scene::Transform;
    let temp = Temp::new()?;
    let mut editor = editor(&temp)?;
    let job = editor.terrain_job(TerrainRequest::Create {
        terrain: Terrain::flat([9; 2], [8.; 2])?,
        position: [0.; 3],
    })?;
    let ground = editor.accept_terrain(wait(&job)?)?;
    let job = editor.blockout_job(
        Blockout {
            version: 1,
            primitive: BrushPrimitive::Ramp,
        },
        vec![Transform {
            translation: [3., 0., 3.],
            ..Default::default()
        }],
    )?;
    let brush = editor.accept_geometry(wait(&job)?)?;
    editor.reparent(&brush, Some(&ground))?;
    editor.select_object(Some(ground.clone()));
    let job = editor.prefab_job(PrefabCommand::Create)?;
    let prefab = editor.accept_prefab(wait(&job)?)?;
    let source = editor.terrain_source(&ground)?;
    let mut terrain = source.terrain.clone();
    terrain.brush(TerrainBrush {
        mode: BrushMode::Raise,
        center: [0.; 2],
        radius: 2.,
        strength: 2.,
        target_height: 0.,
    })?;
    let job = editor.terrain_job(TerrainRequest::Sculpt { source, terrain })?;
    editor.accept_terrain(wait(&job)?)?;
    let job = editor.prefab_job(PrefabCommand::Instantiate {
        asset: prefab,
        position: Some([10., 0., 0.]),
    })?;
    editor.accept_prefab(wait(&job)?)?;
    let instance = editor.selected.clone().unwrap();
    assert!(
        editor
            .terrain_source(&instance)?
            .terrain
            .heights
            .iter()
            .all(|h| *h == 0.)
    );
    assert_eq!(
        editor
            .terrain_source(&ground)?
            .terrain
            .sample([0.; 2])
            .unwrap()
            .0,
        2.
    );
    let collision = editor
        .collisions()?
        .raycast(Vec3::new(10., 10., 0.), Vec3::NEG_Y, 20., None)?
        .unwrap();
    assert!(collision.position.y.abs() < 1e-5);
    assert!(
        editor
            .scene()
            .objects
            .iter()
            .any(|o| o.parent.as_deref() == Some(&instance) && o.mesh_collider.is_some())
    );
    let path = editor.path.clone();
    editor.save(&path)?;
    let reopened = Editor::open(&path)?;
    assert_eq!(reopened.scene(), editor.scene());
    assert!(
        reopened
            .terrain_source(&instance)?
            .terrain
            .heights
            .iter()
            .all(|h| *h == 0.)
    );
    Ok(())
}
