use bozzard_audio::AudioOutput;
use bozzard_scene::{
    Scene,
    middleware::audio::{Bus, Frame, Playback, Transport},
};
use kira::backend::{Backend, Renderer};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Option<Renderer>>>);
impl Backend for Capture {
    type Settings = Self;
    type Error = ();
    fn setup(settings: Self, _: usize) -> Result<(Self, u32), ()> {
        Ok((settings, 24000))
    }
    fn start(&mut self, renderer: Renderer) -> Result<(), ()> {
        *self.0.lock().unwrap() = Some(renderer);
        Ok(())
    }
}
impl Capture {
    fn samples(&self, frames: usize) -> Vec<f32> {
        let mut out = vec![0.; frames * 2];
        let mut renderer = self.0.lock().unwrap();
        let renderer = renderer.as_mut().unwrap();
        renderer.on_start_processing();
        renderer.process(&mut out, 2);
        out
    }
    fn energy(&self) -> [f32; 2] {
        self.samples(2400); // Let gain ramps settle.
        let samples = self.samples(2400);
        std::array::from_fn(|channel| {
            samples
                .chunks_exact(2)
                .map(|f| f[channel] * f[channel])
                .sum::<f32>()
                / 2400.
        })
    }
}
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes")
}
fn fixture() -> (Scene, Frame) {
    let scene = Scene::from_json(r#"{"version":1,"name":"audio","views":{},"objects":[],"assets":{"chime":{"kind":"audio","path":"assets/middleware-chime.wav"}}}"#).unwrap();
    let frame = Frame {
        master: 1.,
        buses: [1.; 4],
        sources: vec![Playback {
            id: 1,
            owner: "speaker".into(),
            asset: "chime".into(),
            bus: Bus::Sfx,
            transport: Transport::Playing,
            position: 0.,
            epoch: 0,
            looping: true,
            streaming: false,
            volume: 0.5,
            pitch: 1.,
            pan: -1.,
        }],
    };
    (scene, frame)
}
#[test]
fn mixer_routes_pans_ramps_pauses_and_reuses_decoded_frames() {
    let capture = Capture::default();
    let mut output = AudioOutput::<Capture>::new(capture.clone()).unwrap();
    let (scene, mut frame) = fixture();
    output.sync(&frame, &scene, &root()).unwrap();
    let initial = capture.energy();
    assert!(initial[0] > 0.001 && initial[1] < 1e-12, "{initial:?}");
    assert_eq!(output.cached_bytes(), 24000 * 8);
    frame.buses[Bus::Sfx as usize] = 0.5;
    output.sync(&frame, &scene, &root()).unwrap();
    let quiet = capture.energy();
    assert!((quiet[0] / initial[0] - 0.25).abs() < 0.01);
    frame.sources[0].pan = 1.;
    output.sync(&frame, &scene, &root()).unwrap();
    let right = capture.energy();
    assert!(right[0] < 1e-12 && right[1] > 0.0001);
    frame.sources[0].transport = Transport::Paused;
    output.sync(&frame, &scene, &root()).unwrap();
    assert!(capture.energy().iter().all(|e| *e < 1e-12));
    frame.sources[0].transport = Transport::Playing;
    frame.sources[0].position = 0.25;
    frame.sources[0].epoch += 1;
    output.sync(&frame, &scene, &root()).unwrap();
    assert!(capture.energy()[1] > 0.0001);
    let mut second = frame.sources[0].clone();
    second.id = 2;
    second.bus = Bus::Music;
    frame.sources.push(second);
    output.sync(&frame, &scene, &root()).unwrap();
    assert_eq!(
        output.cached_bytes(),
        24000 * 8,
        "instances share sample data"
    );
    assert_eq!(output.active_voices(), 2);
    output.stop_all();
    assert!(capture.energy().iter().all(|e| *e < 1e-12));
}
#[test]
fn streaming_produces_samples_without_retaining_the_file_in_the_cache() {
    let capture = Capture::default();
    let mut output = AudioOutput::<Capture>::new(capture.clone()).unwrap();
    let (scene, mut frame) = fixture();
    frame.sources[0].streaming = true;
    output.sync(&frame, &scene, &root()).unwrap();
    let mut energy = 0.;
    // The file decoder runs on a worker; give it bounded time to supply the ring buffer.
    for _ in 0..100 {
        std::thread::sleep(Duration::from_millis(2));
        energy += capture.samples(128).iter().map(|v| v * v).sum::<f32>();
        if energy > 0.1 {
            break;
        }
    }
    assert!(energy > 0.1);
    assert_eq!(output.cached_bytes(), 0);
    frame.sources.clear();
    output.sync(&frame, &scene, &root()).unwrap();
    assert_eq!(output.active_voices(), 0);
}

#[test]
fn compressed_formats_decode_through_the_same_mixer() {
    for extension in ["ogg", "mp3", "flac"] {
        let capture = Capture::default();
        let mut output = AudioOutput::<Capture>::new(capture.clone()).unwrap();
        let (mut scene, frame) = fixture();
        scene.assets.get_mut("chime").unwrap().path =
            format!("assets/middleware-chime.{extension}");
        output.sync(&frame, &scene, &root()).unwrap();
        assert!(capture.energy()[0] > 0.001, "{extension}");
        assert!(output.cached_bytes() < 1024 * 1024);
    }
}

#[test]
#[ignore = "requires a native audio output device"]
fn native_device_opens_and_accepts_static_and_streamed_playback() {
    let mut output = AudioOutput::<kira::backend::DefaultBackend>::new(Default::default()).unwrap();
    let (scene, mut frame) = fixture();
    frame.sources[0].volume = 0.05;
    output.sync(&frame, &scene, &root()).unwrap();
    std::thread::sleep(Duration::from_millis(120));
    frame.sources[0].streaming = true;
    output.sync(&frame, &scene, &root()).unwrap();
    std::thread::sleep(Duration::from_millis(120));
    assert_eq!(output.active_voices(), 1);
    output.stop_all();
}
