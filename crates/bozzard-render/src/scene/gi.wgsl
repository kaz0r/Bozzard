struct GiUniform { low:vec4<f32>, high:vec4<f32>, grid:vec4<f32>, settings:vec4<f32> };
@group(2) @binding(4) var<uniform> gi:GiUniform;
@group(2) @binding(5) var<storage,read> gi_probes:array<vec4<f32>>;
fn gi_sh(n:vec3<f32>) -> array<f32,9> {
    return array<f32,9>(0.2820948,0.48860252*n.y,0.48860252*n.z,0.48860252*n.x,1.0925485*n.x*n.y,1.0925485*n.y*n.z,0.31539157*(3.0*n.z*n.z-1.0),1.0925485*n.x*n.z,0.54627424*(n.x*n.x-n.y*n.y));
}
fn oct_uv(direction:vec3<f32>)->vec2<f32> {
    let n=direction/max(dot(abs(direction),vec3<f32>(1.0)),0.000001);
    var p=n.xy;
    if n.z<0.0 {p=(1.0-abs(p.yx))*select(vec2<f32>(-1.0),vec2<f32>(1.0),p>=vec2<f32>(0.0));}
    return p*0.5+0.5;
}
fn probe_moments(probe:u32,direction:vec3<f32>)->vec2<f32> {
    let p=clamp(oct_uv(direction)*8.0-0.5,vec2<f32>(0.0),vec2<f32>(7.0));
    let base=vec2<u32>(floor(p));let f=fract(p);var result=vec2<f32>(0.0);
    for(var i=0u;i<4u;i++) {
        let offset=vec2<u32>(i&1u,(i>>1u)&1u);let texel=min(base+offset,vec2<u32>(7u));
        let index=texel.y*8u+texel.x;let pair=gi_probes[probe*41u+9u+index/2u];
        let moments=select(pair.xy,pair.zw,index%2u==1u);
        let weights=select(vec2<f32>(1.0)-f,f,offset==vec2<u32>(1u));result+=moments*weights.x*weights.y;
    }
    return result;
}
fn gi_diffuse(world:vec3<f32>,normal:vec3<f32>)->vec3<f32> {
    if gi.grid.w<0.5 || any(world<gi.low.xyz) || any(world>gi.high.xyz) {return diffuse_environment(normal);}
    let position=world+normal*gi.settings.y;
    let grid=vec3<u32>(gi.grid.xyz);
    let coordinate=clamp((position-gi.low.xyz)/(gi.high.xyz-gi.low.xyz)*(gi.grid.xyz-1.0),vec3<f32>(0.0),gi.grid.xyz-1.0);
    let base=min(vec3<u32>(floor(coordinate)),grid-vec3<u32>(2u));let fraction=coordinate-vec3<f32>(base);
    let basis=gi_sh(normal);var color=vec3<f32>(0.0);var total=0.0;
    for(var i=0u;i<8u;i++) {
        let offset=vec3<u32>(i&1u,(i>>1u)&1u,(i>>2u)&1u);let cell=base+offset;
        let index=(cell.z*grid.y+cell.y)*grid.x+cell.x;
        if gi_probes[index*41u].w<0.5 {continue;}
        let probe=gi.low.xyz+vec3<f32>(cell)/(gi.grid.xyz-1.0)*(gi.high.xyz-gi.low.xyz);
        let delta=position-probe;let distance=length(delta);let moments=probe_moments(index,delta);
        let variance=max(moments.y-moments.x*moments.x,0.00001);
        let difference=max(distance-moments.x,0.0);let visibility=variance/(variance+difference*difference);
        let toward=(probe-world)/max(length(probe-world),0.00001);
        let facing=max(0.05,dot(normal,toward)*0.5+0.5);
        let weights=select(vec3<f32>(1.0)-fraction,fraction,offset==vec3<u32>(1u));
        let weight=weights.x*weights.y*weights.z*facing*facing*visibility*visibility*visibility;
        var irradiance=vec3<f32>(0.0);
        for(var band=0u;band<9u;band++) {irradiance+=gi_probes[index*41u+band].rgb*basis[band];}
        color+=max(irradiance,vec3<f32>(0.0))*weight;total+=weight;
    }
    if total<0.0001 {return vec3<f32>(0.0);}
    return color/total*gi.settings.x;
}
