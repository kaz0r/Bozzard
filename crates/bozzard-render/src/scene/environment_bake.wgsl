// GGX split-sum integration; design reference: https://google.github.io/filament/main/filament.html
@group(0) @binding(0) var<uniform> bake: vec4<f32>; // face, roughness, size, diffuse
const PI = 3.14159265359;
const COUNT = 128u;
fn sample_point(i: u32) -> vec2<f32> {
    return vec2<f32>(f32(i)/f32(COUNT), f32(reverseBits(i))*2.3283064365386963e-10);
}
fn hemisphere(x: vec2<f32>, roughness: f32) -> vec3<f32> {
    let a2 = pow(max(roughness,0.045),4.0);
    let z = sqrt((1.0-x.y)/(1.0+(a2-1.0)*x.y));
    let r = sqrt(max(0.0,1.0-z*z));
    return vec3<f32>(r*cos(2.0*PI*x.x),r*sin(2.0*PI*x.x),z);
}
fn basis_color(d: vec3<f32>) -> vec3<f32> {
    let t = sqrt(abs(d.y));
    return select(vec3<f32>(0.0,1.0-t,t), vec3<f32>(t,1.0-t,0.0), d.y >= 0.0);
}
fn direction(uv: vec2<f32>, face: u32) -> vec3<f32> {
    switch face {
        case 0u: { return normalize(vec3<f32>(1.0,-uv.y,-uv.x)); }
        case 1u: { return normalize(vec3<f32>(-1.0,-uv.y,uv.x)); }
        case 2u: { return normalize(vec3<f32>(uv.x,1.0,uv.y)); }
        case 3u: { return normalize(vec3<f32>(uv.x,-1.0,-uv.y)); }
        case 4u: { return normalize(vec3<f32>(uv.x,-uv.y,1.0)); }
        default: { return normalize(vec3<f32>(-uv.x,-uv.y,-1.0)); }
    }
}
@vertex fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = array<vec2<f32>,3>(vec2<f32>(-1.0,-1.0),vec2<f32>(3.0,-1.0),vec2<f32>(-1.0,3.0));
    return vec4<f32>(p[i],0.0,1.0);
}
@fragment fn fs_weights(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let n = direction(p.xy/bake.z*2.0-1.0,u32(bake.x));
    if bake.y == 0.0 && bake.w == 0.0 { return vec4<f32>(basis_color(n),1.0); }
    let up = select(vec3<f32>(0.0,0.0,1.0),vec3<f32>(1.0,0.0,0.0),abs(n.z)>0.99);
    let t = normalize(cross(up,n)); let b = cross(n,t);
    let frame = mat3x3<f32>(t,b,n);
    var total = vec3<f32>(0.0); var weight = 0.0;
    for (var i=0u; i<COUNT; i++) {
        let xi = sample_point(i);
        if bake.w > 0.5 {
            // Cosine-distributed samples directly estimate irradiance / pi.
            let r = sqrt(xi.y);
            let local = vec3<f32>(r*cos(2.0*PI*xi.x),r*sin(2.0*PI*xi.x),sqrt(1.0-xi.y));
            total += basis_color(frame*local); weight += 1.0;
        } else {
            let h = frame*hemisphere(xi,bake.y);
            let l = reflect(-n,h);
            let nl = max(dot(n,l),0.0);
            total += basis_color(l)*nl; weight += nl;
        }
    }
    return vec4<f32>(total/max(weight,0.00001),1.0);
}
@fragment fn fs_brdf(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let nv = max(p.x/bake.z,0.0001); let rough = p.y/bake.z;
    let v = vec3<f32>(sqrt(1.0-nv*nv),0.0,nv);
    let a2 = pow(max(rough,0.045),4.0);
    var result = vec2<f32>(0.0);
    for (var i=0u; i<COUNT; i++) {
        let h = hemisphere(sample_point(i),rough);
        let l = reflect(-v,h); let nl = max(l.z,0.0);
        if nl > 0.0 {
            let vh = max(dot(v,h),0.0);
            let visibility = 0.5/max(nl*sqrt(nv*nv*(1.0-a2)+a2)+nv*sqrt(nl*nl*(1.0-a2)+a2),0.0001);
            let response = 4.0*visibility*nl*vh/max(h.z,0.0001);
            let f = pow(1.0-vh,5.0);
            result += vec2<f32>(1.0-f,f)*response;
        }
    }
    return vec4<f32>(result/f32(COUNT),0.0,1.0);
}
