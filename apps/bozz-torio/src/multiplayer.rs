//! Host-authoritative factory commands and bounded Steam snapshot protocol.
use crate::sim::{Direction, Game, Item, Kind};
#[cfg(any(feature = "steam", test))]
use crate::sim::{HEIGHT, WIDTH};
use anyhow::Result;
#[cfg(any(feature = "steam", test))]
use anyhow::ensure;
#[cfg(any(feature = "steam", test))]
use base64::{Engine as _, engine::general_purpose::STANDARD};
#[cfg(any(feature = "steam", test))]
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use serde::{Deserialize, Serialize};
#[cfg(any(feature = "steam", test))]
use std::io::{Read, Write};
#[cfg(any(feature = "steam", test))]
use std::time::{Duration, Instant};

#[cfg(any(feature = "steam", test))]
const PROTOCOL: u32 = 1;
#[cfg(any(feature = "steam", test))]
const MAX_PACKET: usize = 16 * 1024;
#[cfg(any(feature = "steam", test))]
const CHUNK_BYTES: usize = 8 * 1024;
#[cfg(any(feature = "steam", test))]
const MAX_COMPRESSED: usize = 2 * 1024 * 1024;
#[cfg(any(feature = "steam", test))]
const MAX_JSON: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Action {
    Place {
        x: u16,
        y: u16,
        kind: Kind,
        direction: Direction,
    },
    Remove {
        x: u16,
        y: u16,
    },
    Rotate {
        x: u16,
        y: u16,
    },
    Upgrade {
        x: u16,
        y: u16,
    },
    Recipe {
        x: u16,
        y: u16,
    },
    TakeOutput {
        x: u16,
        y: u16,
    },
    Inject {
        x: u16,
        y: u16,
        item: Item,
    },
    Buy {
        kind: Kind,
    },
    TogglePause,
}
impl Action {
    pub fn apply(&self, game: &mut Game) -> std::result::Result<(), &'static str> {
        match *self {
            Self::Place {
                x,
                y,
                kind,
                direction,
            } => game.place(x.into(), y.into(), kind, direction),
            Self::Remove { x, y } => game.remove(x.into(), y.into()),
            Self::Rotate { x, y } => game.rotate(x.into(), y.into()),
            Self::Upgrade { x, y } => game.upgrade(x.into(), y.into()),
            Self::Recipe { x, y } => game.set_recipe(x.into(), y.into()),
            Self::TakeOutput { x, y } => game.take_output(x.into(), y.into()),
            Self::Inject { x, y, item } => game.inject(x.into(), y.into(), item),
            Self::Buy { kind } => game.buy(kind),
            Self::TogglePause => {
                game.paused = !game.paused;
                Ok(())
            }
        }
    }
    pub fn structural(&self) -> bool {
        matches!(
            self,
            Self::Place { .. } | Self::Remove { .. } | Self::Rotate { .. }
        )
    }
}

#[cfg(any(feature = "steam", test))]
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    protocol: u32,
    lobby: u64,
    message: Message,
}
#[cfg(any(feature = "steam", test))]
#[derive(Debug, Serialize, Deserialize)]
enum Message {
    Command {
        sequence: u64,
        action: Action,
    },
    StatePart {
        revision: u64,
        index: u16,
        count: u16,
        data: String,
    },
    Rejected {
        sequence: u64,
        reason: String,
    },
    Goodbye,
}
#[cfg(any(feature = "steam", test))]
fn encode(lobby: u64, message: Message) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(&Envelope {
        protocol: PROTOCOL,
        lobby,
        message,
    })?;
    ensure!(bytes.len() <= MAX_PACKET, "factory packet exceeds 16 KiB");
    Ok(bytes)
}
#[cfg(any(feature = "steam", test))]
fn decode(lobby: u64, bytes: &[u8]) -> Result<Message> {
    ensure!(bytes.len() <= MAX_PACKET, "factory packet exceeds 16 KiB");
    let envelope: Envelope = serde_json::from_slice(bytes)?;
    ensure!(
        envelope.protocol == PROTOCOL && envelope.lobby == lobby,
        "wrong factory lobby or protocol"
    );
    Ok(envelope.message)
}
#[cfg(any(feature = "steam", test))]
fn state_parts(lobby: u64, revision: u64, game: &Game) -> Result<Vec<Vec<u8>>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(&serde_json::to_vec(game)?)?;
    let compressed = encoder.finish()?;
    ensure!(
        compressed.len() <= MAX_COMPRESSED,
        "factory too large to share"
    );
    let count = compressed.len().div_ceil(CHUNK_BYTES);
    ensure!(
        count > 0 && count <= u16::MAX as usize,
        "invalid snapshot size"
    );
    compressed
        .chunks(CHUNK_BYTES)
        .enumerate()
        .map(|(index, chunk)| {
            encode(
                lobby,
                Message::StatePart {
                    revision,
                    index: index as u16,
                    count: count as u16,
                    data: STANDARD.encode(chunk),
                },
            )
        })
        .collect()
}
#[cfg(any(feature = "steam", test))]
fn read_state(chunks: &[Option<Vec<u8>>]) -> Result<Game> {
    let mut compressed = Vec::new();
    for chunk in chunks {
        compressed.extend_from_slice(
            chunk
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("incomplete factory snapshot"))?,
        );
        ensure!(
            compressed.len() <= MAX_COMPRESSED,
            "factory snapshot is too large"
        );
    }
    let mut json = Vec::new();
    ZlibDecoder::new(compressed.as_slice())
        .take(MAX_JSON + 1)
        .read_to_end(&mut json)?;
    ensure!(
        json.len() as u64 <= MAX_JSON,
        "factory snapshot expands too far"
    );
    let game: Game = serde_json::from_slice(&json)?;
    ensure!(
        game.tiles.len() == WIDTH * HEIGHT && game.terrain.len() == WIDTH * HEIGHT,
        "invalid factory world"
    );
    ensure!(
        game.hub[0] < WIDTH && game.hub[1] < HEIGHT,
        "factory hub is outside the world"
    );
    let hub = game.hub[1] * WIDTH + game.hub[0];
    ensure!(
        game.tiles[hub]
            .building
            .as_ref()
            .is_some_and(|b| b.kind == Kind::Hub),
        "factory has no valid hub"
    );
    ensure!(
        game.buildings.len() >= 8 && game.first_order_amount > 0,
        "invalid factory inventory or contract"
    );
    ensure!(
        game.order_index <= 1_000_000,
        "factory progression is out of range"
    );
    Ok(game)
}

#[cfg(any(feature = "steam", test))]
struct Assembly {
    revision: u64,
    chunks: Vec<Option<Vec<u8>>>,
    started: Instant,
}

/// Pure assembly boundary shared by Steam and deterministic fault tests. A complete, validated
/// factory is returned only after every part arrives; the caller owns publication.
#[cfg(any(feature = "steam", test))]
fn accept_state_part(
    assembly: &mut Option<Assembly>,
    applied_revision: u64,
    revision: u64,
    index: u16,
    count: u16,
    data: &str,
    now: Instant,
) -> Result<Option<Game>> {
    ensure!(
        count > 0 && count as usize <= MAX_COMPRESSED.div_ceil(CHUNK_BYTES) && index < count,
        "invalid snapshot part"
    );
    if revision <= applied_revision {
        return Ok(None);
    }
    ensure!(
        data.len() <= CHUNK_BYTES.div_ceil(3) * 4,
        "oversized snapshot part"
    );
    let chunk = STANDARD.decode(data)?;
    ensure!(chunk.len() <= CHUNK_BYTES, "oversized snapshot part");
    if assembly
        .as_ref()
        .is_none_or(|part| revision > part.revision)
    {
        *assembly = Some(Assembly {
            revision,
            chunks: vec![None; count as usize],
            started: now,
        });
    }
    let part = assembly.as_mut().expect("snapshot assembly");
    if revision < part.revision {
        return Ok(None);
    }
    ensure!(
        part.chunks.len() == count as usize,
        "snapshot part count changed"
    );
    if let Some(previous) = &part.chunks[index as usize] {
        ensure!(previous == &chunk, "conflicting duplicate snapshot part");
        return Ok(None);
    }
    part.chunks[index as usize] = Some(chunk);
    if part.chunks.iter().all(Option::is_some) {
        let game = read_state(&part.chunks)?;
        *assembly = None;
        return Ok(Some(game));
    }
    Ok(None)
}

#[cfg(any(feature = "steam", test))]
fn expire_assembly(assembly: &mut Option<Assembly>, now: Instant, timeout: Duration) -> bool {
    if assembly
        .as_ref()
        .is_some_and(|part| now.duration_since(part.started) > timeout)
    {
        *assembly = None;
        true
    } else {
        false
    }
}

/// What changed after Steam traffic was applied to the local presentation.
#[derive(Default)]
pub struct Change {
    pub state: bool,
    pub structural: bool,
    pub guest_lost: bool,
    pub join_failed: bool,
}

#[cfg(any(feature = "steam", test))]
pub fn structural_difference(old: &Game, new: &Game) -> bool {
    old.seed != new.seed
        || old.hub != new.hub
        || old.terrain != new.terrain
        || old.tiles.len() != new.tiles.len()
        || old.tiles.iter().zip(&new.tiles).any(|(a, b)| {
            a.deposit != b.deposit
                || match (&a.building, &b.building) {
                    (None, None) => false,
                    (Some(a), Some(b)) => a.kind != b.kind || a.direction != b.direction,
                    _ => true,
                }
        })
}

#[cfg(feature = "steam")]
mod steam_net;
#[cfg(feature = "steam")]
pub use steam_net::Network;

#[cfg(not(feature = "steam"))]
pub struct Network;
#[cfg(not(feature = "steam"))]
impl Network {
    pub fn new(
        _: &crate::steam::SteamBridge,
        _: &crate::scene::SceneSource,
        join_lobby: Option<u64>,
    ) -> Result<Self> {
        ensure_no_steam_join(join_lobby)?;
        Ok(Self)
    }
    pub fn status(&self) -> &str {
        "Steam multiplayer support is not compiled"
    }
    pub fn busy(&self) -> bool {
        false
    }
    pub fn lobby_id(&self) -> Option<u64> {
        None
    }
    pub fn members_len(&self) -> usize {
        0
    }
    pub fn has_state(&self) -> bool {
        true
    }
    pub fn can_create_or_join(&self) -> bool {
        false
    }
    pub fn is_host(&self) -> bool {
        false
    }
    pub fn is_guest_or_joining(&self) -> bool {
        false
    }
    pub fn create(&mut self) -> Result<()> {
        anyhow::bail!("Steam multiplayer support is not compiled")
    }
    pub fn join(&mut self, _: u64) -> Result<()> {
        anyhow::bail!("Steam multiplayer support is not compiled")
    }
    pub fn invite(&mut self) -> Result<()> {
        anyhow::bail!("Steam multiplayer support is not compiled")
    }
    pub fn leave(&mut self) {}
    pub fn command(&mut self, _: Action) -> Result<()> {
        anyhow::bail!("Steam multiplayer support is not compiled")
    }
    pub fn force_publish(&mut self) {}
    pub fn update(&mut self, _: &mut Game) -> Result<Change> {
        Ok(Change::default())
    }
    pub fn publish(&mut self, _: &Game) -> Result<()> {
        Ok(())
    }
}

#[cfg(not(feature = "steam"))]
fn ensure_no_steam_join(join_lobby: Option<u64>) -> Result<()> {
    if join_lobby.is_some() {
        anyhow::bail!("Steam multiplayer support is not compiled");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chunked_factory_state_roundtrips_and_rejects_other_lobbies() {
        let game = Game::new();
        let packets = state_parts(123, 7, &game).unwrap();
        assert!(packets.len() > 1);
        assert!(packets.iter().all(|bytes| bytes.len() <= MAX_PACKET));
        let mut chunks = vec![None; packets.len()];
        for bytes in packets {
            assert!(decode(124, &bytes).is_err());
            let Message::StatePart { index, data, .. } = decode(123, &bytes).unwrap() else {
                panic!("expected snapshot part")
            };
            chunks[index as usize] = Some(STANDARD.decode(data).unwrap());
        }
        let restored = read_state(&chunks).unwrap();
        assert_eq!(restored.tiles.len(), WIDTH * HEIGHT);
        assert_eq!(restored.seed, game.seed);
    }

    #[test]
    fn building_changes_rebuild_the_replica_but_item_progress_does_not() {
        let original = Game::new();
        let mut next = original.clone();
        next.ticks += 1;
        assert!(!structural_difference(&original, &next));
        next.place(
            crate::sim::PATCH_X + 4,
            crate::sim::PATCH_Y + 7,
            Kind::Miner,
            Direction::East,
        )
        .unwrap();
        assert!(structural_difference(&original, &next));
    }

    #[test]
    fn fragmented_snapshots_never_publish_partial_state_and_recover_after_timeout() {
        let mut original = Game::from_seed(7);
        original.credits = 11;
        let mut newer = original.clone();
        newer.credits = 29;
        let old = state_parts(123, 4, &original).unwrap();
        let packets = state_parts(123, 5, &newer).unwrap();
        assert!(packets.len() > 1);
        let mut assembly = None;
        let mut displayed = Game::from_seed(8);
        let initial_credits = displayed.credits;
        let start = Instant::now();
        let ingest = |bytes: &[u8],
                      at: Instant,
                      assembly: &mut Option<Assembly>,
                      displayed: &mut Game|
         -> Result<bool> {
            let Message::StatePart {
                revision,
                index,
                count,
                data,
            } = decode(123, bytes)?
            else {
                anyhow::bail!("expected state part")
            };
            if let Some(game) = accept_state_part(assembly, 0, revision, index, count, &data, at)? {
                *displayed = game;
                Ok(true)
            } else {
                Ok(false)
            }
        };
        // Missing part, reverse order, and identical duplicate keep the old presentation.
        for bytes in packets.iter().skip(1).rev() {
            assert!(!ingest(bytes, start, &mut assembly, &mut displayed).unwrap());
        }
        assert!(!ingest(&packets[1], start, &mut assembly, &mut displayed).unwrap());
        assert_eq!(displayed.credits, initial_credits);
        // An older part cannot replace the newer partial assembly.
        assert!(!ingest(&old[0], start, &mut assembly, &mut displayed).unwrap());
        assert_eq!(assembly.as_ref().unwrap().revision, 5);
        // Timeout clears bounded partial memory; a complete resend recovers.
        assert!(expire_assembly(
            &mut assembly,
            start + Duration::from_secs(16),
            Duration::from_secs(15)
        ));
        assert!(assembly.is_none());
        for (index, bytes) in packets.iter().enumerate().rev() {
            let published = ingest(
                bytes,
                start + Duration::from_secs(17),
                &mut assembly,
                &mut displayed,
            )
            .unwrap();
            assert_eq!(published, index == 0);
        }
        assert_eq!(displayed.credits, 29);
        assert!(assembly.is_none());
    }

    #[test]
    fn malformed_and_conflicting_chunks_are_rejected_without_a_factory_swap() {
        let game = Game::from_seed(9);
        let packets = state_parts(2, 3, &game).unwrap();
        let mut assembly = None;
        let now = Instant::now();
        let Message::StatePart {
            revision,
            index,
            count,
            data,
        } = decode(2, &packets[0]).unwrap()
        else {
            panic!()
        };
        assert!(
            accept_state_part(&mut assembly, 0, revision, index, count, &data, now)
                .unwrap()
                .is_none()
        );
        assert!(
            accept_state_part(
                &mut assembly,
                0,
                revision,
                index,
                count,
                &STANDARD.encode(b"conflict"),
                now
            )
            .is_err()
        );
        assembly = None; // Steam discards a rejected assembly.
        assert!(
            accept_state_part(
                &mut assembly,
                0,
                4,
                0,
                1,
                &STANDARD.encode(b"invalid zlib"),
                now
            )
            .is_err()
        );
        assembly = None;
        let mut published = None;
        for bytes in &packets {
            let Message::StatePart {
                revision,
                index,
                count,
                data,
            } = decode(2, bytes).unwrap()
            else {
                panic!()
            };
            if let Some(next) =
                accept_state_part(&mut assembly, 0, revision, index, count, &data, now).unwrap()
            {
                published = Some(next);
            }
        }
        assert_eq!(published.unwrap().seed, game.seed);
    }
}
