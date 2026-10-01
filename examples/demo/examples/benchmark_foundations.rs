//! Compare steady simulation and indoor routines on a sealed factory across a chunk seam.
//! Run without a GPU: cargo run -p bozzard-demo --example benchmark_foundations -- 6
use bozzard_scene::{
    BlueprintRuntime, Scene,
    blueprint::{BlackboardValue, Value},
};
use std::{collections::BTreeMap, path::PathBuf, time::Instant};

fn main() -> anyhow::Result<()> {
    let size: usize = std::env::args()
        .nth(1)
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(6);
    anyhow::ensure!((3..=18).contains(&size), "factory side must be 3..18 tiles");
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../earth-factory/scenes/earth.json");
    let source = std::fs::read_to_string(path.parent().unwrap().join("scripts/earth_factory.rs"))?;
    for (stage, body) in [
        ("no-foundations", "factory_update(me,dt);"),
        ("full-game", "factory_update(me,dt);"),
        ("without-indoor-update", "factory_update(me,dt);"),
        ("indoor-update", "interiors::update(dt);"),
        (
            "doors",
            "interiors::update_doors(dt,get_object_list(\"resident\"));",
        ),
        (
            "archive-comparison",
            "let equal=object_lists_equal(\"controller\",\"cache_structures\",\"architecture-view\",\"observed\");",
        ),
        ("room-lookup", "interiors::room_at(7,4);"),
        (
            "refresh-view",
            "interiors::set_field(\"view\",\"\");interiors::update(0.0);",
        ),
    ] {
        let mut scene = Scene::from_json(&std::fs::read_to_string(&path)?)?;
        let controller = scene
            .objects
            .iter_mut()
            .find(|o| o.id == "controller")
            .unwrap();
        controller.blackboard.insert(
            "creative".into(),
            BlackboardValue::Scalar(Value::Bool(true)),
        );
        controller.blackboard.insert(
            "title_open".into(),
            BlackboardValue::Scalar(Value::Bool(false)),
        );
        scene
            .blackboard
            .insert("seed".into(), BlackboardValue::Scalar(Value::Number(4.)));
        let mut demo = bozzard_demo::SceneDemo::new_with_prefabs(&scene, Some(&path))?;
        let source = if stage == "without-indoor-update" {
            source.replace("interiors::update(dt);", "")
        } else {
            source.clone()
        };
        let source = source
            .replace("fn on_start(me)", "fn factory_start(me)")
            .replace("fn on_update(me, dt)", "fn factory_update(me, dt)");
        let fixture = if stage == "no-foundations" {
            String::new()
        } else {
            format!(
                r#"
            let pages=#{{}};
            for z in 2..2+{size} {{for x in 6..6+{size} {{
                for kind in [30.0,36.0] {{
                    let a=architecture::address(x,z,kind,0);let key=a.region.to_string();
                    if !pages.contains(key) {{pages[key]=grid::empty_numbers(900);}}
                    pages[key][a.slot]=kind;
                }}
                for dir in 0..4 {{
                    let nx=x+grid::step_x(dir);let nz=z+grid::step_z(dir);
                    if nx>=6 && nx<6+{size} && nz>=2 && nz<2+{size} {{continue;}}
                    let a=architecture::address(x,z,32.0,dir);let key=a.region.to_string();
                    if !pages.contains(key) {{pages[key]=grid::empty_numbers(900);}}
                    pages[key][a.slot]=32.0;
                }}
            }}}}
            let door=architecture::address(7,1+{size},38.0,1);pages[door.region.to_string()][door.slot]=38.0;
            let window=architecture::address(5+{size},4,39.0,0);pages[window.region.to_string()][window.slot]=39.0;
            for key in pages.keys() {{
                let region=parse_int(key);grid::cache_put("cache_structures",region,grid::pack_numbers(pages[key]));
                chunks::discover_chunk(region%17-8,region/17-8);interiors::load(region);
            }}
            set_scene_variable("cursor_x",7.0);set_scene_variable("cursor_z",4.0);
            set_position("camera-rig",[{center_x}.0,0.0,{center_z}.0]);set_object_variable("camera_pan_progress",1.0);
            interiors::update(0.0);
        "#,
                center_x = 6 + size / 2,
                center_z = 2 + size / 2
            )
        };
        demo.with_instance(|instance,_|instance.register_script("earth-factory".into(),format!("{source}\nfn on_start(me) {{factory_start(me);{fixture}}}\nfn on_update(me,dt) {{{body}}}")))?;
        let mut samples = Vec::new();
        let mut spans = BTreeMap::<_, Vec<f64>>::new();
        for tick in 0..150 {
            let profiler = &mut demo
                .app
                .world
                .resource_mut::<bozzard_diagnostics::Diagnostics>()
                .unwrap()
                .profiler;
            profiler.recording = true;
            profiler.begin_frame();
            let started = Instant::now();
            demo.app.step();
            demo.check_simulation()?;
            if tick >= 30 {
                samples.push(started.elapsed().as_secs_f64() * 1000.);
                for span in &demo
                    .app
                    .world
                    .resource::<bozzard_diagnostics::Diagnostics>()
                    .unwrap()
                    .profiler
                    .spans
                {
                    spans.entry(span.name).or_default().push(span.duration_ms);
                }
            }
        }
        samples.sort_by(f64::total_cmp);
        if stage != "no-foundations" {
            let board = demo
                .app
                .world
                .resource::<BlueprintRuntime>()
                .unwrap()
                .object_blackboard("architecture-view")
                .unwrap();
            anyhow::ensure!(
                matches!(&board["inside"],BlackboardValue::Scalar(Value::Number(room)) if *room>0.),
                "benchmark must remain inside its sealed factory"
            );
        }
        println!(
            "{stage}: size={size} median={:.3}ms p95={:.3}ms",
            samples[samples.len() / 2],
            samples[samples.len() * 95 / 100]
        );
        for name in ["Script hooks", "Script read view", "Script commands"] {
            let values = spans.get_mut(name).unwrap();
            values.sort_by(f64::total_cmp);
            println!("  {name}: median={:.3}ms", values[values.len() / 2]);
        }
    }
    Ok(())
}
