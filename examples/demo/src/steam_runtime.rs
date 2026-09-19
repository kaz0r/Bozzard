//! The SDK redistributable used to link this engine, retained for source-independent exports.
#[cfg(feature = "steam")]
pub fn redistributable() -> Option<(&'static str, &'static [u8])> {
    Some((
        env!("BOZZARD_STEAM_LIBRARY"),
        include_bytes!(env!("BOZZARD_STEAM_LIBRARY_PATH")),
    ))
}

#[cfg(not(feature = "steam"))]
pub fn redistributable() -> Option<(&'static str, &'static [u8])> {
    None
}
