// Each half is an exactly representable 16-bit integer in the existing record.
fn object_light_mask() -> u32 {
    // Uniform early exit keeps lightless views out of the per-instance mask path.
    if local_lights.count.x == 0.0 { return 0u; }
    return u32(object.raster.z) | (u32(object.raster.w) << 16u);
}
