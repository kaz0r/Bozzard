//! Same world-clock rules as factory/clock.rhai, for native save descriptions.
pub fn day_seconds(elapsed: f32) -> f32 {
    (elapsed.max(0.).rem_euclid(21_600.) * 4. + 28_800.).rem_euclid(86_400.)
}
pub fn daytime(elapsed: f32, moon: bool) -> bool {
    !moon && (21_600. ..64_800.).contains(&day_seconds(elapsed))
}
pub fn day_number(elapsed: f32) -> u64 {
    ((f64::from(elapsed.max(0.)) * 4. + 28_800.) / 86_400.).floor() as u64 + 1
}
pub fn label(elapsed: f32) -> String {
    let seconds = day_seconds(elapsed).floor() as u32;
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}
