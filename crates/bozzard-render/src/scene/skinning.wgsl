struct Influence { joints:vec4<u32>, weights:vec4<f32> }
// Four storage bindings fit the renderer's downlevel device limits. The same word layout
// serves 32-byte position/normal/UV vertices and 48-byte PBR tangent/UV attributes.
@group(0) @binding(0) var<storage,read> source:array<u32>;
@group(0) @binding(1) var<storage,read> influences:array<Influence>;
@group(0) @binding(2) var<storage,read> palette:array<mat4x4<f32>>;
@group(0) @binding(3) var<storage,read_write> output:array<u32>;
@group(0) @binding(4) var<uniform> params:vec4<u32>;
fn skin(index:u32)->mat4x4<f32> {
    let inf=influences[index];
    return palette[inf.joints.x]*inf.weights.x + palette[inf.joints.y]*inf.weights.y + palette[inf.joints.z]*inf.weights.z + palette[inf.joints.w]*inf.weights.w;
}
fn read3(offset:u32)->vec3<f32> {return vec3<f32>(bitcast<f32>(source[offset]),bitcast<f32>(source[offset+1u]),bitcast<f32>(source[offset+2u]));}
fn write3(offset:u32,v:vec3<f32>) {output[offset]=bitcast<u32>(v.x); output[offset+1u]=bitcast<u32>(v.y); output[offset+2u]=bitcast<u32>(v.z);}
fn safe_normal(v:vec3<f32>, fallback:vec3<f32>)->vec3<f32> { if dot(v,v)>0.000000000001 {return normalize(v);} return fallback; }
@compute @workgroup_size(64)
fn positions(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x>=params.x {return;}
    let offset=id.x*8u; let m=skin(id.x); let p=(m*vec4<f32>(read3(offset),1.0)).xyz;
    let a=m[0].xyz; let b=m[1].xyz; let c=m[2].xyz;
    let det=dot(a,cross(b,c)); var n=read3(offset+3u);
    if abs(det)>0.000000000001 {n=safe_normal((mat3x3<f32>(cross(b,c),cross(c,a),cross(a,b))*n)/det,n);}
    write3(offset,p); write3(offset+3u,n); output[offset+6u]=source[offset+6u];output[offset+7u]=source[offset+7u];
}
@compute @workgroup_size(64)
fn tangents(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x>=params.x {return;}
    let offset=id.x*12u; let m=skin(id.x+params.y); let v=read3(offset);
    let determinant=dot(m[0].xyz,cross(m[1].xyz,m[2].xyz));
    let t=safe_normal((m*vec4<f32>(v,0.0)).xyz,v);
    write3(offset,t); output[offset+3u]=bitcast<u32>(bitcast<f32>(source[offset+3u])*select(1.0,-1.0,determinant<0.0));
    for(var lane=4u;lane<12u;lane++){output[offset+lane]=source[offset+lane];}
}
