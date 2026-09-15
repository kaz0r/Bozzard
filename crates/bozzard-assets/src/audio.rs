//! File-backed audio catalog entries retain metadata and a digest, never a music file's bytes.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{fs::File, io::Read, path::Path, time::SystemTime};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioData {
    pub duration: f64,
    pub sample_rate: u32,
    pub frames: u64,
    pub source_bytes: u64,
    pub digest: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Stamp {
    size: u64,
    modified: SystemTime,
}
pub(super) fn stamp(path: &Path) -> Result<Stamp> {
    let m = path.metadata()?;
    ensure!(m.is_file(), "audio asset is not a file");
    Ok(Stamp {
        size: m.len(),
        modified: m.modified()?,
    })
}
pub(super) fn probe(path: &Path, progress: &super::job::Progress) -> Result<AudioData> {
    let before = stamp(path)?;
    ensure!(
        before.size > 0 && before.size <= 1024 * 1024 * 1024,
        "audio file must be between 1 byte and 1 GiB"
    );
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    ensure!(
        matches!(extension.as_str(), "wav" | "ogg" | "mp3" | "flac"),
        "audio import supports WAV, OGG/Vorbis, MP3 and FLAC"
    );
    let stream = kira::sound::streaming::StreamingSoundData::from_file(path)
        .context("probing audio stream")?;
    let duration = stream.duration().as_secs_f64();
    let frames = stream.num_frames() as u64;
    ensure!(
        duration > 0. && duration <= 86400. && frames > 0,
        "audio duration must be within one day"
    );
    let sample_rate = (frames as f64 / duration).round() as u32;
    ensure!(
        (8000..=384000).contains(&sample_rate),
        "unsupported audio sample rate"
    );
    drop(stream);
    let mut file = File::open(path)?;
    let mut chunk = [0u8; 65536];
    let mut digest = 0xcbf29ce484222325u64;
    let mut read = 0u64;
    loop {
        progress.check()?;
        let count = file.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        read += count as u64;
        ensure!(read <= before.size, "audio file changed during import");
        for &byte in &chunk[..count] {
            digest = (digest ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    }
    ensure!(
        read == before.size && stamp(path)? == before,
        "audio file changed during import; reload it"
    );
    Ok(AudioData {
        duration,
        sample_rate,
        frames,
        source_bytes: before.size,
        digest,
    })
}

fn updates(
    store: &super::AssetStore,
    scene: &bozzard_scene::Scene,
) -> Result<Vec<(usize, bozzard_scene::middleware::audio::AudioSource)>> {
    use bozzard_scene::middleware::{audio::AudioSource, registry};
    let mut result = Vec::new();
    for (index, object) in scene.objects.iter().enumerate() {
        let Some(mut source) = registry::get::<AudioSource>(object)? else {
            continue;
        };
        let Some(super::AssetData::Audio(data)) = store
            .handle(&source.asset)
            .and_then(|h| store.get(h))
            .and_then(|e| e.data())
        else {
            continue;
        };
        if source.duration != data.duration {
            source.duration = data.duration;
            result.push((index, source));
        }
    }
    Ok(result)
}

impl super::AssetStore {
    /// Read-only preflight lets authoring hosts avoid cloning an unchanged scene for an undo command.
    pub fn audio_metadata_current(&self, scene: &bozzard_scene::Scene) -> Result<bool> {
        for document in
            std::iter::once(scene).chain(scene.runtime_scenes.values().map(AsRef::as_ref))
        {
            if !updates(self, document)?.is_empty() {
                return Ok(false);
            }
        }
        Ok(true)
    }
    /// Bake clip lengths into documents so device playback and headless completion events agree.
    /// The catalog must be loaded first. Only changed sources detach shared level documents.
    pub fn bake_audio_metadata(&self, scene: &mut bozzard_scene::Scene) -> Result<usize> {
        use bozzard_scene::middleware::registry;
        let mut count = 0;
        for (index, source) in updates(self, scene)? {
            registry::set(&mut scene.objects[index], &source)?;
            count += 1;
        }
        for level in scene.runtime_scenes.values_mut() {
            let pending = updates(self, level)?;
            if !pending.is_empty() {
                let level = std::sync::Arc::make_mut(level);
                for (index, source) in pending {
                    registry::set(&mut level.objects[index], &source)?;
                    count += 1;
                }
            }
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compressed_catalogs_keep_only_checked_metadata_and_detect_content_changes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes/assets");
        let mut digests = std::collections::BTreeSet::new();
        for extension in ["wav", "ogg", "mp3", "flac"] {
            let data = probe(
                &root.join(format!("middleware-chime.{extension}")),
                &Default::default(),
            )
            .unwrap();
            assert!((0.99..1.2).contains(&data.duration));
            assert_eq!(data.sample_rate, 24000);
            assert!(
                serde_json::to_vec(&data).unwrap().len() < 256,
                "catalog must not retain audio bytes"
            );
            assert!(digests.insert(data.digest));
        }
        assert!(probe(&root.join("animated-banner.gltf"), &Default::default()).is_err());
    }
}
