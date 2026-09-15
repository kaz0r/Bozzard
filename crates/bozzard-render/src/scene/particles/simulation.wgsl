struct Particle { position_size:vec4<f32>, velocity_rotation:vec4<f32>, color_opacity:vec4<f32>, kind_soft_trail_seed:vec4<f32> }
struct Input { particle:Particle, clock_gravity:vec4<f32>, drag_curl_speed:vec4<f32>, wind_reset:vec4<f32>, identity:vec4<u32> }
struct State { position_age:vec4<f32>, velocity:vec4<f32> }
struct Record { depth:f32, index:u32, low:u32, high:u32 }
struct Settings { view_projection:mat4x4<f32>, counts:vec4<u32> }
@group(0) @binding(0) var<storage,read> inputs:array<Input>;
@group(0) @binding(1) var<storage,read_write> states:array<State>;
@group(0) @binding(2) var<storage,read_write> output:array<Particle>;
@group(0) @binding(3) var<storage,read_write> records:array<Record>;
@group(0) @binding(4) var<uniform> settings:Settings;
fn curl(p:vec3<f32>,t:f32)->vec3<f32> {
    return vec3<f32>(cos(p.y*1.3+t*0.7)-cos(p.z*0.9+t),cos(p.z*1.1+t*0.8)-cos(p.x*1.2+t*0.6),cos(p.x*0.8+t)-cos(p.y*1.4+t*0.9));
}
@compute @workgroup_size(64) fn simulate(@builtin(global_invocation_id) id:vec3<u32>) {
    let i=id.x;if i>=settings.counts.y{return;}
    if i>=settings.counts.x {records[i]=Record(-3.402823e38,0xffffffffu,0xffffffffu,0xffffffffu);return;}
    let input=inputs[i];var particle=input.particle;
    if input.identity.w!=0u {
        let slot=input.identity.x;let age=input.clock_gravity.x;
        var state=states[slot];
        if input.wind_reset.w>0.5 || age<state.position_age.w {
            state=State(vec4<f32>(particle.position_size.xyz,input.clock_gravity.y),vec4<f32>(particle.velocity_rotation.xyz-input.wind_reset.xyz,0.));
        }
        let delta=max(age-state.position_age.w,0.);
        let steps=max(u32(ceil(delta*60.)),1u);let step=delta/f32(steps);
        var p=state.position_age.xyz;var v=state.velocity.xyz;
        if delta>0. {for(var sub=0u;sub<steps;sub++) {
            v.y+=input.clock_gravity.w*step;
            v+=curl(p*0.8,input.clock_gravity.z-delta+step*f32(sub)+particle.kind_soft_trail_seed.w*13.)*input.drag_curl_speed.y*step;
            v*=exp(-input.drag_curl_speed.x*step);
            p+=(v+input.wind_reset.xyz)*step*input.drag_curl_speed.z;
        }
        }
        states[slot]=State(vec4<f32>(p,age),vec4<f32>(v,0.));
        particle.position_size=vec4<f32>(p,particle.position_size.w);
        particle.velocity_rotation=vec4<f32>(v+input.wind_reset.xyz,particle.velocity_rotation.w);
    }
    output[i]=particle;
    let clip=settings.view_projection*vec4<f32>(particle.position_size.xyz,1.);
    var depth=-3.402823e38;if abs(clip.w)>0.000001 {depth=clip.z/clip.w;}
    records[i]=Record(depth,i,input.identity.y,input.identity.z);
}
