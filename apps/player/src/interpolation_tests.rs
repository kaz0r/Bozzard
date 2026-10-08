use super::*;
use bozzard_scene::Transform;

#[test]
#[ignore = "requires a native graphics adapter"]
fn interpolated_player_pixels_match_an_independently_authored_midpoint() -> Result<()> {
    let scene = bozzard_scene::Scene::from_json(
        r#"{
        "version":1,"name":"Midpoint pixels","views":{"3d":"camera"},"objects":[
            {"id":"camera","name":"Camera","transform":{"translation":[0,0,10],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
             "camera":{"projection":"orthographic","vertical_size":10,"near":0.1,"far":100}},
            {"id":"cube","name":"Cube","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
             "drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[1,0.4,0.1],"uv_scale":[1,1]}}
        ]
    }"#,
    )?;
    let assets = bozzard_assets::AssetStore::new(std::path::Path::new("."), &scene.assets)?;
    let gpu = pollster::block_on(bozzard_render::Gpu::request(
        &bozzard_render::instance(bozzard_render::Backend::native()),
        None,
        false,
    ))?;
    let capture = |render: &RenderScene| -> Result<Vec<u8>> {
        let mut renderer = bozzard_render::SceneRenderer::new(
            &gpu,
            bozzard_render::wgpu::TextureFormat::Rgba8Unorm,
        );
        Ok(bozzard_render::capture_offscreen(&gpu, 320, 320, |target| {
            renderer.draw(&gpu, target, [320, 320], render)
        })?
        .rgba)
    };
    let mut reference = scene.clone();
    reference.objects[0].transform.translation[0] = 0.5;
    reference.objects[1].transform.translation[0] = 1.;
    let reference = extract(&SceneRuntime::new(&reference)?, &assets, Layer::ThreeD, 1.)?;
    let expected = capture(&reference)?;
    for threaded in [false, true] {
        let mut demo = SceneRuntime::new(&scene)?;
        demo.set_threaded_simulation(threaded)?;
        demo.set_render_interpolation(true)?;
        let cube = demo.instance().entity("cube").unwrap();
        let camera = demo.instance().entity("camera").unwrap();
        demo.app.add_system(move |world, _, _| {
            world.get_mut::<Transform>(cube).unwrap().translation[0] += 2.;
            world.get_mut::<Transform>(camera).unwrap().translation[0] += 1.;
        });
        let step = demo.app.timestep();
        demo.advance_with_frame(step + step / 2, || ())?;
        let render = extract(&demo, &assets, Layer::ThreeD, 1.)?;
        assert_eq!(capture(&render)?, expected);
        demo.set_render_interpolation(false)?;
        assert_ne!(
            capture(&extract(&demo, &assets, Layer::ThreeD, 1.)?)?,
            expected
        );
    }
    Ok(())
}
