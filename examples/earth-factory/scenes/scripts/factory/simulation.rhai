// Rhai gameplay: simulation.
import "factory-data" as data;
import "factory-grid" as grid;
import "factory-factory_state" as factory_state;
import "factory-item_fx" as item_fx;
import "factory-power" as power;
import "factory-visuals" as visuals;
import "factory-transports" as transports;

fn factory_step() {
    visuals::animate_items(1.0);
    item_fx::retire_previous();
    power::update_power();
    // Every planet advances exactly once per beat, including during rocket flight.
    let tick=get_scene_variable("ticks").to_int()+1;
    let regions=0;
    for planet in 0..data::planet_count() { regions+=step_planet(planet,tick); }
    data::session_set(114,regions.to_float());
    set_scene_variable("ticks",tick.to_float());
}

fn step_planet(planet,tick) {
    let state=factory_state::read(planet);
    let ids=state.ids; let lookup=state.lookup; let count=ids.len();
    if count==0 {
        transports::record(planet,tick,[],[],[],[]);
        if state.active { item_fx::write([],[],[]); }
        return 0;
    }
    let resident=if state.active { get_object_list("resident") } else { grid::empty_numbers(289) };
    let pool=if state.active { get_scene_list("item_pool") } else { [] }; let motions=[];
    let nodes=state.columns.nodes; let builds=state.columns.builds; let facings=state.columns.facings;
    let items=state.columns.items; let amounts=state.columns.item_amounts;
    let inputs=state.columns.input_items; let input_amounts=state.columns.input_amounts;
    let progress=state.columns.progress; let iron=state.columns.assembler_iron; let copper=state.columns.assembler_copper;
    let split=state.columns.split_state; let rotating=state.columns.rotating;
    let powered=state.columns.powered; let recipes=state.columns.recipes;
    let counts=state.counts;
    let visuals=if state.active { item_fx::read_handles(ids) } else { grid::empty_text(count) };
    let machine_cells=[]; for cell in 0..count { machine_cells.push(cell); }
    {
        for entry in machine_cells {
            let cell = entry.to_int();
            let kind = builds[cell];
            if rotating[cell] > 0.0 || powered[cell] == 0.0 { continue; }
            if kind == 1.0 && nodes[cell] != 0.0 && amounts[cell] < data::buffer_capacity() && tick % 2 == 0 {
                items[cell] = nodes[cell];
                amounts[cell] += 1.0;
            } else if kind == 3.0 || kind == 11.0 {
                let output = if kind == 3.0 { data::smelt_output(inputs[cell]) } else { recipes[cell] };
                let cost = data::recipe_cost(output.to_int());
                if output <= 0.0 || cost.len() == 0 { continue; }
                let produced = data::recipe_yield(output.to_int());
                if input_amounts[cell] < cost[1] || inputs[cell] != cost[0].to_float() ||
                    (amounts[cell] > 0.0 && items[cell] != output) ||
                    amounts[cell] + input_amounts[cell] - cost[1] + produced > data::buffer_capacity() { continue; }
                progress[cell] += 1.0;
                if progress[cell] >= 2.0 {
                    items[cell] = output; amounts[cell] += produced;
                    input_amounts[cell] -= cost[1];
                    if input_amounts[cell] == 0.0 { inputs[cell] = 0.0; }
                    progress[cell] = 0.0;
                }
            } else if kind == 5.0 && recipes[cell] > 0.0 && tick % 2 == 0 {
                let cost = data::recipe_cost(recipes[cell].to_int());
                if cost.len() != 4 || iron[cell] < cost[1] || copper[cell] < cost[3] ||
                    (amounts[cell] > 0.0 && items[cell] != recipes[cell]) { continue; }
                iron[cell] -= cost[1]; copper[cell] -= cost[3];
                items[cell] = recipes[cell]; amounts[cell] += 1.0;
            }
        }
    }

    for entry in machine_cells {
        let cell = entry.to_int();
        if amounts[cell] == 0.0 || resident[ids[cell]/225]==0.0 { continue; }
        if visuals[cell] == "" {
            let position = [grid::global_x(ids[cell]), visuals::item_height(builds[cell]), grid::global_z(ids[cell])];
            if pool.len() > 0 {
                visuals[cell] = pool.pop();
                set_position(visuals[cell], position);
                set_visible(visuals[cell], true);
            } else { visuals[cell] = spawn_prefab("item", position); }
        }
        set_color(visuals[cell], data::item_color(items[cell]));
    }
    // Storage offers its first occupied slot from the start-of-transport snapshot.
    // Keep these offers separate from machine buffers: stored stock never becomes
    // a second inventory or a stationary item floating above the container.
    let offered_items=items; let offered_amounts=amounts;
    let storage_slots=grid::empty_numbers(count);
    for key in state.storage.keys() {
        let cell=parse_int(key); let slots=state.storage[key];
        for slot in 0..16 {
            if slots[slot*2+1]<=0.0 { continue; }
            offered_items[cell]=slots[slot*2]; offered_amounts[cell]=1.0;
            storage_slots[cell]=(slot+1).to_float(); break;
        }
    }
    // Reserve destinations before removing any source items. An occupied belt
    // blocks its upstream tile for this beat, irrespective of placement order.
    // Received items cannot jump through several machines in the same beat.
    let next_items = items;
    let next_amounts = amounts;
    let next_visuals = visuals;
    let routes = grid::empty_numbers(count); // destination + 1; zero means not moving
    for attempt in 0..3 {
        let requests = grid::empty_numbers(count);
        let any_requests = false;
        for entry in machine_cells {
            let cell = entry.to_int();
            let kind = builds[cell];
            if routes[cell] != 0.0 || offered_amounts[cell] == 0.0 || rotating[cell] > 0.0 { continue; }
            if kind != 1.0 && kind != 2.0 && kind != 3.0 &&
                kind != 4.0 && kind != 5.0 && kind != 7.0 && kind != 8.0 && kind != 11.0 { continue; }
            if attempt > 0 && kind != 7.0 { continue; }
            let direction = facings[cell].to_int();
            if kind == 7.0 {
                // Forward, right, left: try each outlet at most once, skipping
                // disconnected/full outputs without losing the waiting item.
                let branch = (split[cell].to_int() + attempt) % 3;
                direction = (direction + [0, 1, 3][branch]) % 4;
            }
            let address=grid::global_neighbor(ids[cell],direction,state.radius).to_string();
            if !lookup.contains(address) { continue; }
            let destination=lookup[address];
            if rotating[destination]>0.0 { continue; }
            let target = builds[destination];
            if target != 2.0 && target != 3.0 && target != 4.0 && target != 5.0 &&
                target != 7.0 && target != 8.0 && target != 11.0 { continue; }
            if !data::accepts_transport_input(target, facings[destination].to_int(), direction) { continue; }
            requests[cell] = (destination + 1).to_float();
            any_requests = true;
        }
        if !any_requests { continue; }
        for entry in machine_cells {
            let destination = entry.to_int();
            let target = builds[destination];
            if target != 2.0 && target != 3.0 && target != 4.0 && target != 5.0 &&
                target != 7.0 && target != 8.0 && target != 11.0 { continue; }
            let priority = split[destination].to_int();
            for port in 0..4 {
                let side = port;
                let inlet = (priority + port) % 3;
                if target == 8.0 {
                    if port == 3 { continue; }
                    side = (facings[destination].to_int() + [2, 3, 1][inlet]) % 4;
                }
                let address=grid::global_neighbor(ids[destination],side,state.radius).to_string();
                if !lookup.contains(address) { continue; }
                let cell=lookup[address];
                if requests[cell]!=(destination+1).to_float() { continue; }
                let item = offered_items[cell];
                let load = next_amounts[destination] + input_amounts[destination] + iron[destination] + copper[destination];
                let accepted = false;
                // A junction stores one material stack. Let a waiting different
                // material drain the current batch instead of starving forever
                // behind a continuously replenished stack of the same material.
                if target == 8.0 && next_amounts[destination] > 0.0 && next_items[destination] != item { break; }
                if target == 4.0 {
                    let key=destination.to_string(); let slots=state.storage[key]; let slot=-1;
                    for i in 0..16 { if slots[i*2]==item && slots[i*2+1]<100.0 { slot=i; break; } }
                    if slot<0 { for i in 0..16 { if slots[i*2+1]==0.0 { slot=i; break; } } }
                    if slot>=0 {
                        slots[slot*2]=item; slots[slot*2+1]+=1.0;
                        state.storage[key]=slots; state.storage_changed[key]=true;
                        counts[item.to_int()]+=1.0; accepted=true;
                    }
                } else if target == 2.0 || target == 7.0 || target == 8.0 {
                    if load < data::transport_capacity(target) &&
                        (next_amounts[destination] == 0.0 || next_items[destination] == item) {
                        next_items[destination] = item;
                        next_amounts[destination] += 1.0;
                        accepted = true;
                    }
                } else if load < data::buffer_capacity() {
                    if target == 5.0 && recipes[destination] > 0.0 {
                        let cost = data::recipe_cost(recipes[destination].to_int());
                        if cost.len() == 4 {
                            if item == cost[0].to_float() && load + 1.0 + max(0.0, cost[3] - copper[destination]) <= data::buffer_capacity() {
                                iron[destination] += 1.0; accepted = true;
                            } else if item == cost[2].to_float() && load + 1.0 + max(0.0, cost[1] - iron[destination]) <= data::buffer_capacity() {
                                copper[destination] += 1.0; accepted = true;
                            }
                        }
                    } else if target == 3.0 || target == 11.0 {
                        let recipe = if target == 3.0 { data::smelt_output(item) } else { recipes[destination] };
                        let cost = data::recipe_cost(recipe.to_int());
                        // Reserve the extra three output spaces for a four-fastener batch.
                        let reserve = max(0.0, data::recipe_yield(recipe.to_int()) - 1.0);
                        if cost.len() == 2 && item == cost[0].to_float() && load + 1.0 + reserve <= data::buffer_capacity() &&
                            (input_amounts[destination] == 0.0 || inputs[destination] == item) {
                            inputs[destination] = item; input_amounts[destination] += 1.0; accepted = true;
                        }
                    }
                }
                if accepted {
                    routes[cell] = (destination + 1).to_float();
                    if builds[cell] == 7.0 { split[cell] = ((split[cell].to_int() + attempt + 1) % 3).to_float(); }
                    // Advance only after accepting an item. If only one space is
                    // free, the next beat starts with the following merger inlet.
                    if target == 8.0 {
                        split[destination] = ((inlet + 1) % 3).to_float();
                        break;
                    }
                }
            }
        }
    }
    // Detach every departing visual before assigning arrivals, so a junction
    // receiving and sending together never overwrites an incoming item's model.
    for entry in machine_cells {
        let cell = entry.to_int();
        if routes[cell] == 0.0 { continue; }
        if builds[cell]==4.0 {
            let key=cell.to_string(); let slot=storage_slots[cell].to_int()-1;
            let slots=state.storage[key]; slots[slot*2+1]-=1.0;
            if slots[slot*2+1]==0.0 { slots[slot*2]=0.0; }
            state.storage[key]=slots; state.storage_changed[key]=true;
            counts[offered_items[cell].to_int()]-=1.0;
        } else {
            next_amounts[cell] -= 1.0;
            if next_amounts[cell] == 0.0 { next_items[cell] = 0.0; }
        }
        next_visuals[cell] = "";
    }
    for entry in machine_cells {
        let cell = entry.to_int();
        if routes[cell] == 0.0 { continue; }
        let destination = routes[cell].to_int() - 1;
        let target = builds[destination];
        if visuals[cell]=="" && resident[ids[destination]/225]>0.0 {
            let position=[grid::global_x(ids[cell]),visuals::item_height(builds[cell]),grid::global_z(ids[cell])];
            if pool.len()>0 { visuals[cell]=pool.pop(); set_position(visuals[cell],position); set_visible(visuals[cell],true); }
            else { visuals[cell]=spawn_prefab("item",position); }
            set_color(visuals[cell],data::item_color(offered_items[cell]));
        }
        if visuals[cell]=="" { continue; }
        let retained=resident[ids[destination]/225]>0.0 &&
            (target==2.0 || target==7.0 || target==8.0) && next_visuals[destination]=="";
        if retained { next_visuals[destination]=visuals[cell]; }
        motions.push([ids[cell],ids[destination],visuals[cell],visuals::item_height(builds[cell]),visuals::item_height(target),!retained]);
    }
    // Sending one item from a stack leaves a single stationary representative.
    for entry in machine_cells {
        let cell = entry.to_int();
        if next_amounts[cell] == 0.0 || next_visuals[cell] != "" || resident[ids[cell]/225]==0.0 { continue; }
        let position = [grid::global_x(ids[cell]), visuals::item_height(builds[cell]), grid::global_z(ids[cell])];
        if pool.len() > 0 {
            next_visuals[cell] = pool.pop();
            set_position(next_visuals[cell], position);
            set_visible(next_visuals[cell], true);
        } else { next_visuals[cell] = spawn_prefab("item", position); }
        set_color(next_visuals[cell], data::item_color(next_items[cell]));
    }

    transports::record(planet,tick,ids,routes,offered_items,builds);
    state.columns.items=next_items; state.columns.item_amounts=next_amounts;
    state.columns.input_items=inputs; state.columns.input_amounts=input_amounts;
    state.columns.progress=progress; state.columns.assembler_iron=iron; state.columns.assembler_copper=copper;
    state.columns.split_state=split;
    state.counts=counts;
    factory_state::write(state);
    if state.active {
        item_fx::write(ids,next_visuals,motions);
        set_scene_list("item_pool",pool);
    }
    state.regions.len()
}
