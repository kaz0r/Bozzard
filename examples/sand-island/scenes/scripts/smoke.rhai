// Voxel smoke from the ship's funnel.
//
// Puff i was released at the last multiple of the release interval; its age alone
// decides where the wind has carried it, how far it has risen, how big and how pale
// it is, so the trail needs no stored state and stays behind the ship.
import "island-lib" as island;

fn on_update(me, dt) {
    let t = get_scene_variable("world_time");
    let light = 0.25 + 0.75 * get_scene_variable("daylight");
    let count = island::smoke();
    let life = island::smoke_life();
    let funnel = island::funnel();
    let interval = life / count.to_float();
    for i in 0..count {
        let offset = i.to_float() * interval;
        let cycle = floor((t - offset) / life);
        let born = cycle * life + offset;
        let age = t - born;
        let start = island::ship_point(born, funnel);
        let rise = 9.0 * (1.0 - exp(-age * 0.45)) + age * 0.35;
        let jitter = sin(cycle * 12.9898 + i.to_float() * 78.233) * 2.0;
        let x = start[0] + age * 1.7 + jitter;
        let y = start[1] + rise;
        let z = start[2] + age * 0.8 - jitter * 0.5;
        let grow = island::smooth(0.0, 0.8, age) * (1.0 - island::smooth(life * 0.45, life, age));
        let size = (0.7 + age * 0.17) * grow + 0.001;
        let shade = (0.05 + 0.22 * island::smooth(0.0, life * 0.7, age)) * light;
        let id = if i < 10 { `smoke-0${i}` } else { `smoke-${i}` };
        set_position(id, [x, y, z]);
        set_scale(id, [size, size, size]);
        set_rotation(id, [0.0, age * 9.0 + i.to_float() * 37.0, 0.0]);
        set_color(id, [shade, shade, shade * 1.03]);
    }
}
