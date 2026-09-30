// One-hop, conservative fluid transport. Empty pipes adopt their first fluid;
// occupied pipes never mix materials. Unpowered producers retain their output.
import "factory-data" as data;
import "factory-grid" as grid;
import "factory-buffers" as buffers;

fn step(state) {
    let builds=state.columns.builds; let facings=state.columns.facings;
    let original=state.columns.item_amounts; let old_items=state.columns.items;
    let available=original; let amounts=original; let items=old_items;
    let original_slots=state.storage;
    for source in 0..state.ids.len() {
        if state.columns.rotating[source]>0.0 {continue;}
        let kind=builds[source];
        let pipe=data::pipe(kind);
        if !pipe && !data::expansion(kind) && kind!=4.0 {continue;}
        let slots=if pipe {[]}else{original_slots[source.to_string()]};
        for outlet in 0..(if pipe {4}else if kind==4.0 {16}else{8}) {
            let side=outlet; let slot=(if kind==4.0 {0}else{8})+outlet; let fluid=items[source]; let offered=available[source];
            if pipe {
                if !buffers::pipe_port(kind,facings[source].to_int(),side) {continue;}
            } else {
                fluid=slots[slot*2]; offered=slots[slot*2+1];
                if offered==0.0 || !data::fluid(fluid) {continue;}
                side=if kind==4.0 {facings[source].to_int()}else{buffers::fluid_output_side(kind,facings[source].to_int(),slot)};
            }
            if offered<=0.0 {continue;}
            let address=grid::global_neighbor(state.ids[source],side,state.radius).to_string();
            if !state.lookup.contains(address) {continue;}
            let target=state.lookup[address]; let target_kind=builds[target];
            if state.columns.rotating[target]>0.0 {continue;}
            let opposite=(side+2)%4; let moved=0.0;
            if data::pipe(target_kind) && buffers::pipe_port(target_kind,facings[target].to_int(),opposite) {
                if (original[target]>0.0 && old_items[target]!=fluid) || (amounts[target]>0.0 && items[target]!=fluid) {continue;}
                let pressure=if pipe {original[source]-original[target]}else{10.0};
                if pressure<=0.0 {continue;}
                moved=min(min(offered,5.0),min(ceil(pressure/2.0),100.0-amounts[target]));
                if moved>0.0 {items[target]=fluid; amounts[target]+=moved;}
            } else if buffers::fluid_input(target_kind,facings[target].to_int(),opposite) {
                let key=target.to_string(); let slots=state.storage[key];
                for unit in 0..min(offered,5.0).to_int() {
                    if target_kind==4.0 {
                        let at=-1;
                        for i in 0..16 {if slots[i*2]==fluid && slots[i*2+1]<100.0 {at=i;break;}}
                        if at<0 {for i in 0..16 {if slots[i*2+1]==0.0 {at=i;break;}}}
                        if at<0 {break;}
                        slots[at*2]=fluid;slots[at*2+1]+=1.0;moved+=1.0;state.counts[fluid.to_int()]+=1.0;continue;
                    }
                    let accepted=buffers::accept(slots,state.columns.recipes[target].to_int(),fluid);
                    if !accepted.ok {break;} slots=accepted.slots; moved+=1.0;
                }
                if moved>0.0 {state.storage[key]=slots;state.storage_changed[key]=true;}
            }
            if moved<=0.0 {continue;}
            if pipe {
                available[source]-=moved; amounts[source]-=moved;
                if amounts[source]==0.0 {items[source]=0.0;}
            } else {
                let key=source.to_string(); state.storage[key]=buffers::remove(state.storage[key],slot,moved); state.storage_changed[key]=true;
                if kind==4.0 {state.counts[fluid.to_int()]-=moved;}
            }
        }
    }
    state.columns.items=items; state.columns.item_amounts=amounts; state
}
