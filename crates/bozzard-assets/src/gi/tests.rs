use super::*;
use crate::job::Progress;
use bozzard_scene::{GI_PROBE_STRIDE, Object};
use std::{path::Path, sync::Arc};
fn cube(id: &str, translation: [f32; 3], scale: [f32; 3], color: [f32; 3]) -> Object {
    serde_json::from_value(serde_json::json!({"id":id,"name":id,
        "transform":{"translation":translation,"rotation_degrees":[0,0,0],"scale":scale},
        "drawable":{"layer":"3d","mesh":"cube","texture":"white","color":color,"uv_scale":[1,1]}}))
    .unwrap()
}
fn scene() -> Scene {
    serde_json::from_value(
        serde_json::json!({"version":1,"name":"GI test","views":{},"objects":[],
        "lighting":{"sun_intensity":0,"ambient_intensity":0},"environment":{"intensity":0}}),
    )
    .unwrap()
}
fn store(scene: &Scene) -> AssetStore {
    AssetStore::new(Path::new("."), &scene.assets).unwrap()
}
fn volume() -> GiVolumeSettings {
    GiVolumeSettings {
        min: [-0.1, 1., -0.1],
        max: [0.1, 1.2, 0.1],
        resolution: [2; 3],
        samples: 256,
        bounces: 2,
    }
}
fn irradiance(bake: &BakedGi, index: usize, normal: Vec3) -> Vec3 {
    bake.probes[index * GI_PROBE_STRIDE..][..9]
        .iter()
        .zip(sampling::sh(normal))
        .map(|(c, b)| Vec3::from_slice(c) * b)
        .sum::<Vec3>()
        .max(Vec3::ZERO)
}
#[test]
fn bake_constant_environment_preserves_diffuse_energy_and_is_deterministic() {
    let mut s = scene();
    // A distant small object satisfies the static-volume requirement without covering a ray.
    s.objects
        .push(cube("distant", [1000., 0., 0.], [0.01; 3], [0.; 3]));
    s.environment.zenith = [0.3, 0.5, 0.8];
    s.environment.horizon = s.environment.zenith;
    s.environment.ground = s.environment.zenith;
    s.environment.intensity = 2.;
    let a = bake(&s, &store(&s), volume(), &Progress::default()).unwrap();
    let b = bake(&s, &store(&s), volume(), &Progress::default()).unwrap();
    assert_eq!(a, b);
    for i in 0..8 {
        for n in [Vec3::X, Vec3::Y, Vec3::Z, Vec3::NEG_Y] {
            assert!(irradiance(&a, i, n).distance(Vec3::new(0.6, 1., 1.6)) < 0.015);
        }
    }
    s.gi.volume = volume();
    s.gi.baked = Some(Arc::new(a));
    let json = s.to_json().unwrap();
    assert_eq!(Scene::from_json(&json).unwrap(), s);
}
#[test]
fn sunlight_bounces_wall_color_and_occlusion_removes_it() {
    let mut s = scene();
    s.objects.push(cube(
        "floor",
        [0., -0.1, 0.],
        [100., 0.2, 100.],
        [0.8, 0.1, 0.02],
    ));
    s.lighting.sun_direction = [0., 1., 0.];
    s.lighting.sun_intensity = 2.;
    let tracer = trace::TraceScene::new(&s, &store(&s), &Progress::default()).unwrap();
    let l = tracer.radiance(Vec3::Y, Vec3::NEG_Y, 1, &mut 7);
    assert!(l.distance(Vec3::new(0.8, 0.1, 0.02) * 2. / std::f32::consts::PI) < 1e-4);
    let baked = bake(&s, &store(&s), volume(), &Progress::default()).unwrap();
    let color = irradiance(&baked, 0, Vec3::NEG_Y);
    assert!(
        color.x > 0.45 && color.x > color.y * 6. && color.y > color.z * 4.,
        "{color:?}"
    );
    s.objects
        .push(cube("roof", [0., 2., 0.], [100., 0.2, 100.], [0.; 3]));
    let occluded = bake(&s, &store(&s), volume(), &Progress::default()).unwrap();
    assert!(irradiance(&occluded, 0, Vec3::NEG_Y).length() < 0.001);
    // Probe directional distances see the roof rather than an unoccluded distant sky.
    assert!(
        occluded.probes[9..GI_PROBE_STRIDE]
            .iter()
            .flatten()
            .any(|d| *d < 2.)
    );
}
#[test]
fn diffuse_second_bounce_adds_energy_and_closed_solid_probes_are_rejected() {
    let mut s = scene();
    for (id, p, scale, c) in [
        ("floor", [0., -0.1, 0.], [8., 0.2, 8.], [0.8, 0.1, 0.1]),
        ("wall", [0., 2., -2.], [8., 4., 0.2], [0.8; 3]),
        ("side", [-2., 2., 0.], [0.2, 4., 8.], [0.8; 3]),
    ] {
        s.objects.push(cube(id, p, scale, c));
    }
    s.lighting.sun_direction = [0., 1., 0.];
    s.lighting.sun_intensity = 3.;
    let v = GiVolumeSettings {
        samples: 1024,
        bounces: 1,
        ..volume()
    };
    let one = bake(&s, &store(&s), v, &Progress::default()).unwrap();
    let two = bake(
        &s,
        &store(&s),
        GiVolumeSettings { bounces: 2, ..v },
        &Progress::default(),
    )
    .unwrap();
    let a = irradiance(&one, 0, Vec3::NEG_Z);
    let b = irradiance(&two, 0, Vec3::NEG_Z);
    assert!(b.x > a.x + 0.02, "first={a:?}, second={b:?}");
    s.objects = vec![cube("solid", [0., 1., 0.], [10.; 3], [1.; 3])];
    assert!(
        bake(&s, &store(&s), volume(), &Progress::default())
            .unwrap_err()
            .to_string()
            .contains("inside geometry")
    );
}
#[test]
fn fingerprint_excludes_display_and_camera_but_tracks_static_transport() {
    let mut s = scene();
    s.objects.push(cube("floor", [0.; 3], [1.; 3], [0.6; 3]));
    let a = store(&s);
    let hash = source(&s, &a, volume()).unwrap();
    s.name = "Renamed".into();
    s.objects[0].name = "renamed".into();
    s.display.exposure_ev = 2.;
    s.gi.intensity = 3.;
    s.gi.normal_bias = 0.2;
    s.lighting.ambient_intensity = 1.;
    s.lighting.shadow_resolution = 1024;
    s.environment.background = !s.environment.background;
    assert_eq!(source(&s, &a, volume()).unwrap(), hash);
    for change in 0..4 {
        let mut c = s.clone();
        match change {
            0 => c.objects[0].transform.translation[0] = 1.,
            1 => c.objects[0].drawable.as_mut().unwrap().color[0] = 0.1,
            2 => c.lighting.sun_intensity = 1.,
            _ => c.environment.intensity = 1.,
        }
        assert_ne!(source(&c, &a, volume()).unwrap(), hash);
    }
    let mut dynamic = cube("moving", [5.; 3], [1.; 3], [1.; 3]);
    dynamic.spin = Some(bozzard_scene::Spin([0., 30., 0.]));
    let mut child = cube("child", [1.; 3], [1.; 3], [1.; 3]);
    child.parent = Some("moving".into());
    s.objects.extend([dynamic, child]);
    assert_eq!(source(&s, &a, volume()).unwrap(), hash);
    s.objects[1].transform.translation[0] = 99.;
    s.objects[2].drawable.as_mut().unwrap().color = [0.; 3];
    assert_eq!(source(&s, &a, volume()).unwrap(), hash);
    s.objects[0].drawable.as_mut().unwrap().gi_static = false;
    assert!(static_objects(&s).is_empty());
    assert!(fit_volume(&s, &a).is_err());
}
#[test]
fn realtime_local_shadows_do_not_expire_baked_transport() {
    for kind in [
        bozzard_scene::LightKind::Point,
        bozzard_scene::LightKind::Spot,
    ] {
        realtime_shadow_fingerprint(kind);
    }
}
fn realtime_shadow_fingerprint(kind: bozzard_scene::LightKind) {
    let mut s = scene();
    let mut lamp = cube("lamp", [0., 2., 0.], [1.; 3], [1.; 3]);
    lamp.drawable = None;
    lamp.light = Some(bozzard_scene::Light {
        kind,
        ..Default::default()
    });
    s.objects.push(lamp);
    let a = store(&s);
    let before = source(&s, &a, volume()).unwrap();
    let light = s.objects.last_mut().unwrap().light.as_mut().unwrap();
    // Default shadow fields are absent in JSON, preserving the pre-shadow fingerprint format.
    let json = serde_json::to_value(*light).unwrap();
    assert!(
        json.get("shadows").is_none()
            && json.get("shadow_bias").is_none()
            && json.get("shadow_normal_bias").is_none()
    );
    light.shadows = true;
    light.shadow_bias = 0.1;
    light.shadow_normal_bias = 0.2;
    assert_eq!(before, source(&s, &a, volume()).unwrap());
    s.objects
        .last_mut()
        .unwrap()
        .light
        .as_mut()
        .unwrap()
        .intensity *= 2.;
    assert_ne!(before, source(&s, &a, volume()).unwrap());
    s.objects
        .last_mut()
        .unwrap()
        .light
        .as_mut()
        .unwrap()
        .shadow_bias = f32::NAN;
    assert!(source(&s, &a, volume()).is_err());
}
#[test]
fn malformed_bakes_and_volume_settings_are_rejected() {
    for v in [
        GiVolumeSettings {
            resolution: [1, 2, 2],
            ..volume()
        },
        GiVolumeSettings {
            samples: 65,
            ..volume()
        },
        GiVolumeSettings {
            min: [f32::NAN; 3],
            ..volume()
        },
        GiVolumeSettings {
            bounces: 0,
            ..volume()
        },
    ] {
        assert!(v.validate().is_err());
    }
    let mut data = BakedGi::new(
        "0123456789abcdef".into(),
        volume(),
        Arc::new(vec![[0.; 4]; 8 * GI_PROBE_STRIDE]),
    )
    .unwrap();
    assert!(Arc::get_mut(&mut data.probes).is_none());
    data.validate().unwrap();
    Arc::make_mut(&mut data.probes)[9][0] = f32::NAN;
    assert!(data.validate().is_err());
    Arc::make_mut(&mut data.probes)[9][0] = 0.;
    Arc::make_mut(&mut data.probes).pop();
    assert!(data.validate().is_err());
}

#[test]
fn asset_byte_replacement_invalidates_but_failed_reload_and_rebase_preserve_source() {
    use bozzard_scene::{AssetKind, AssetSource, Texture};
    use std::sync::atomic::{AtomicU64, Ordering};
    struct Dir(std::path::PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = loop {
        let p = std::env::temp_dir().join(format!(
            "bozzard-gi-assets-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match std::fs::create_dir(&p) {
            Ok(()) => break Dir(p),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => panic!("{e}"),
        }
    };
    let png = include_bytes!("../../../../examples/demo/scenes/assets/palette.png");
    let next = include_bytes!("../../../../examples/demo/scenes/assets/palette-reloaded.png");
    let mut s = scene();
    s.objects.push(cube("floor", [0.; 3], [1.; 3], [1.; 3]));
    s.objects[0].drawable.as_mut().unwrap().texture = Texture::Asset("tex".into());
    s.assets.insert(
        "tex".into(),
        AssetSource {
            kind: AssetKind::Image,
            path: "source.png".into(),
        },
    );
    std::fs::write(dir.0.join("source.png"), png).unwrap();
    let mut a = AssetStore::new(&dir.0, &s.assets).unwrap();
    a.load_pending().unwrap();
    let hash = source(&s, &a, volume()).unwrap();
    std::fs::write(dir.0.join("source.png"), b"broken").unwrap();
    a.refresh();
    assert!(a.require_ready().is_err());
    assert_eq!(source(&s, &a, volume()).unwrap(), hash);
    std::fs::write(dir.0.join("source.png"), next).unwrap();
    a.refresh();
    a.require_ready().unwrap();
    assert_ne!(source(&s, &a, volume()).unwrap(), hash);
    std::fs::create_dir(dir.0.join("moved")).unwrap();
    std::fs::write(dir.0.join("moved/renamed.png"), png).unwrap();
    s.assets.get_mut("tex").unwrap().path = "renamed.png".into();
    let mut rebased = AssetStore::new(&dir.0.join("moved"), &s.assets).unwrap();
    rebased.load_pending().unwrap();
    assert_eq!(source(&s, &rebased, volume()).unwrap(), hash);
}

#[test]
fn dynamic_exclusion_handles_reverse_order_deep_hierarchies() {
    let mut s = scene();
    for i in 0..1000 {
        let mut object = cube(&format!("node-{i}"), [0.; 3], [1.; 3], [1.; 3]);
        if i == 0 {
            object.spin = Some(bozzard_scene::Spin([0., 1., 0.]));
        } else {
            object.parent = Some(format!("node-{}", i - 1));
        }
        s.objects.push(object);
    }
    s.objects.reverse();
    assert!(static_objects(&s).is_empty());
}
