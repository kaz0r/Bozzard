struct MeterSettings {
    limits: vec4<f32>, // min EV, max EV, desired gray, strength
    adaptation: vec4<f32>, // brighten rate, darken rate, delta seconds, reset
    metering: vec4<f32>, // center weight
}
struct ExposureState { multiplier: f32, ev: f32, target_ev: f32, log_luminance: f32 }
@group(0) @binding(0) var hdr: texture_2d<f32>;
@group(0) @binding(1) var<uniform> settings: MeterSettings;
@group(0) @binding(2) var<storage,read_write> bins: array<atomic<u32>,256>;
@group(0) @binding(3) var<storage,read_write> exposure: ExposureState;
// Fixed 128 x 128 stratified metering grid bounds work independently of viewport size.
@compute @workgroup_size(8,8) fn histogram(@builtin(global_invocation_id) id: vec3<u32>) {
    let uv = (vec2<f32>(id.xy)+0.5)/128.0;
    let size = textureDimensions(hdr);
    let p = min(vec2<u32>(uv*vec2<f32>(size)),size-vec2<u32>(1));
    let color = max(textureLoad(hdr,vec2<i32>(p),0).rgb,vec3<f32>(0));
    let luminance = max(dot(color,vec3<f32>(0.2126,0.7152,0.0722)),exp2(-12.0));
    let bin = u32(clamp((log2(luminance)+12.0)/28.0*256.0,0.0,255.0));
    let center = exp(-8.0*dot(uv-0.5,uv-0.5));
    let weight = u32(round(1.0+15.0*mix(1.0,center,settings.metering.x)));
    atomicAdd(&bins[bin],weight);
}
@compute @workgroup_size(1) fn adapt() {
    var total = 0.0;
    for(var i=0u;i<256u;i++) { total += f32(atomicLoad(&bins[i])); }
    // Trim the brightest and darkest 2% to avoid small sparks or black borders pumping exposure.
    let low = total*0.02;
    let high = total*0.98;
    var cumulative = 0.0;
    var log_sum = 0.0;
    var count = 0.0;
    for(var i=0u;i<256u;i++) {
        let next = cumulative+f32(atomicLoad(&bins[i]));
        let weight = max(0.0,min(next,high)-max(cumulative,low));
        log_sum += (-12.0+(f32(i)+0.5)*28.0/256.0)*weight;
        count += weight;
        cumulative = next;
    }
    let mean = log_sum/max(count,1.0);
    let desired = clamp(log2(settings.limits.z)-mean,settings.limits.x,settings.limits.y);
    var current = clamp(exposure.ev,settings.limits.x,settings.limits.y);
    let rate = select(settings.adaptation.y,settings.adaptation.x,desired>current);
    current = mix(current,desired,1.0-exp(-rate*settings.adaptation.z));
    if settings.adaptation.w>0.5 { current = desired; }
    exposure = ExposureState(exp2(current*settings.limits.w),current,desired,mean);
}
