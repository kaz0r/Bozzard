// Twenty-five freely arranged stacks. Each pair in session[64..114] is kind/amount.
import "factory-net_input" as net;
// stock remains a derived per-material total for recipes, deliveries and the HUD.
fn slots() { get_object_list("session").extract(64, 50) }
fn totals(inventory) {
    let stock = []; stock.pad(64, 0.0);
    for slot in 0..25 { if inventory[slot*2+1] > 0.0 { stock[inventory[slot*2].to_int()] += inventory[slot*2+1]; } }
    stock
}
fn commit(inventory) {
    let state = get_object_list("session");
    for i in 0..50 { state[64+i] = inventory[i]; }
    state[49] += 1.0;
    set_object_list("session", state);
    set_object_list("stock", totals(inventory));
}
fn clear() { if net::guest() {net::send("inventory_clear",#{});return;} let inventory = []; inventory.pad(50, 0.0); commit(inventory); }
// Atomic net transaction: remove ingredients first, then fit all resulting outputs.
// Failing capacity leaves both materials and their slot positions untouched.
fn set_stock(desired) {
    let result=arrange(slots(),desired);
    if !result.ok { return false; }
    commit(result.inventory); true
}
// Shared pure transaction for host-authoritative actors and the local backpack.
fn arrange(inventory,desired) {
    if desired.len()<64 {desired.pad(64,0.0);}
    let current = totals(inventory);
    for slot in 0..25 {
        let at = (24-slot)*2; let kind = inventory[at].to_int();
        if kind == 0 { continue; }
        let removed = min(inventory[at+1], max(0.0,current[kind]-desired[kind]));
        inventory[at+1] -= removed; current[kind] -= removed;
        if inventory[at+1] == 0.0 { inventory[at] = 0.0; }
    }
    for kind in 1..64 {
        let remaining = desired[kind]-current[kind];
        if desired[kind] < 0.0 || floor(desired[kind]) != desired[kind] { return #{ok:false,inventory:[]}; }
        for empty in [false,true] {
            for slot in 0..25 {
                if remaining <= 0.0 { break; }
                let at = slot*2;
                if (empty && inventory[at+1] == 0.0) || (!empty && inventory[at] == kind.to_float()) {
                    let amount = min(remaining,100.0-inventory[at+1]);
                    inventory[at] = kind.to_float(); inventory[at+1] += amount; remaining -= amount;
                }
            }
        }
        if remaining > 0.0 { return #{ok:false,inventory:[]}; }
    }
    #{ok:true,inventory:inventory}
}
fn give(kind, amount) {
    let result=give_to(slots(),kind,amount);
    if result.moved>0.0 { commit(result.inventory); }
    result.moved
}
fn give_to(inventory,kind,amount) {
    let remaining = amount;
    for empty in [false,true] {
        for slot in 0..inventory.len()/2 {
            if remaining <= 0.0 { break; }
            let at = slot*2;
            if (empty && inventory[at+1] == 0.0) || (!empty && inventory[at] == kind) {
                let moved = min(remaining,100.0-inventory[at+1]);
                inventory[at] = kind; inventory[at+1] += moved; remaining -= moved;
            }
        }
    }
    #{inventory:inventory,moved:amount-remaining}
}
fn move_stack(from, to) {
    if from < 0 || to < 0 || from == to || from >= 25 || to >= 25 { return; }
    if net::guest() {net::send("inventory_move",#{from:from,to:to,amount:100});return;}
    let inventory = slots(); let a=from*2; let b=to*2;
    if inventory[a+1] <= 0.0 { return; }
    if inventory[a] == inventory[b] {
        let moved=min(inventory[a+1],100.0-inventory[b+1]);
        inventory[a+1]-=moved; inventory[b+1]+=moved;
        if inventory[a+1]==0.0 { inventory[a]=0.0; }
    } else {
        let kind=inventory[b]; let amount=inventory[b+1];
        inventory[b]=inventory[a]; inventory[b+1]=inventory[a+1]; inventory[a]=kind; inventory[a+1]=amount;
    }
    commit(inventory);
}
fn split(slot) {
    if slot < 0 || slot >= 25 { return false; }
    if net::guest() {return net::send("inventory_split",#{slot:slot});}
    let inventory=slots(); let at=slot*2;
    if inventory[at+1] < 2.0 { return false; }
    for other in 0..25 {
        if inventory[other*2+1]==0.0 {
            let half=floor(inventory[at+1]/2.0);
            inventory[other*2]=inventory[at]; inventory[other*2+1]=half; inventory[at+1]-=half;
            commit(inventory); return true;
        }
    }
    false
}
fn destroy(slot) {
    if slot < 0 || slot >= 25 { return; }
    if net::guest() {net::send("inventory_discard",#{slot:slot,amount:100});return;}
    let inventory=slots(); inventory[slot*2]=0.0; inventory[slot*2+1]=0.0; commit(inventory);
}
