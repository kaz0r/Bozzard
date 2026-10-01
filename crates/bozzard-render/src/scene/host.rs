/// Full WGSL for one host flavor: shared lighting includes plus the host fragment shader.
pub(crate) fn host_text(pbr: bool) -> String {
    let includes = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        include_str!("environment_sample.wgsl"),
        include_str!("shadow_sample.wgsl"),
        include_str!("local_lights.wgsl"),
        include_str!("gi.wgsl"),
        include_str!("effects.wgsl"),
        include_str!("fog.wgsl"),
    );
    // These helpers also serve particles and volumetrics, whose own uniforms use
    // `object`. Only surface hosts take their lighting/camera data from `frame`.
    let includes = [
        "inverse_view_projection",
        "viewport",
        "sun",
        "sun_color",
        "ambient_color",
        "fog_color",
        "fog_density",
        "fog_height",
    ]
    .into_iter()
    .fold(includes, |source, field| {
        source.replace(&format!("object.{field}"), &format!("frame.{field}"))
    });
    format!(
        "{}\n{}\n{}\n{}\n{}",
        include_str!("object.wgsl"),
        include_str!("frame.wgsl"),
        include_str!("surface_lights.wgsl"),
        includes,
        if pbr {
            include_str!("../pbr.wgsl")
        } else {
            include_str!("../scene.wgsl")
        },
    )
}
