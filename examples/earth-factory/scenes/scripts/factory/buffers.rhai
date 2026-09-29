// Expansion machines share sixteen saved slots with containers. Slots 0..7
// hold recipe inputs; slots 8..15 hold every output, including byproducts.
// All transactions are pure so solo play and host authority use identical rules.
import "factory-data" as data;
import "factory-backpack" as backpack;

fn empty() { let slots=[]; slots.pad(32,0.0); slots }
fn read(cell) {
    let slots=[];
    for page in 0..4 {
        let kinds=get_scene_list("storage_kinds_"+page.to_string());let amounts=get_scene_list("storage_amounts_"+page.to_string());
        for slot in 0..4 {slots.push(kinds[cell*4+slot]);slots.push(amounts[cell*4+slot]);}
    }
    slots
}
fn load(slots) { let total=0.0; for i in 0..16 { total+=slots[i*2+1]; } total }
fn amount(slots,item,output) {
    let total=0.0; for i in (if output {8}else{0})..(if output {16}else{8}) { if slots[i*2]==item {total+=slots[i*2+1];} } total
}
fn output_load(slots) { let total=0.0; for i in 8..16 { total+=slots[i*2+1]; } total }
fn input_load(slots) { load(slots)-output_load(slots) }
fn remove(slots,slot,count) { slots[slot*2+1]-=count; if slots[slot*2+1]==0.0 {slots[slot*2]=0.0;} slots }
fn accept(slots,recipe,item) {
    let cost=data::recipe_cost(recipe); let at=-1; let reserve=0.0; let consumed=0.0;
    for i in 0..cost.len()/2 {
        consumed+=cost[i*2+1];
        if cost[i*2].to_float()==item {at=i;} else {reserve+=max(0.0,cost[i*2+1]-slots[i*2+1]);}
    }
    let produced=0.0; let outputs=data::recipe_outputs(recipe);
    for i in 0..outputs.len()/2 {produced+=outputs[i*2+1];}
    reserve+=max(0.0,produced-consumed);
    if at<0 || load(slots)+1.0+reserve>data::buffer_capacity() {return #{ok:false,slots:slots};}
    if slots[at*2+1]>0.0 && slots[at*2]!=item {return #{ok:false,slots:slots};}
    slots[at*2]=item; slots[at*2+1]+=1.0;
    #{ok:true,slots:slots}
}
fn ready(slots,recipe) {
    let cost=data::recipe_cost(recipe); let consumed=0.0;
    for i in 0..cost.len()/2 {
        if slots[i*2]!=cost[i*2].to_float() || slots[i*2+1]<cost[i*2+1] {return false;}
        consumed+=cost[i*2+1];
    }
    let outputs=data::recipe_outputs(recipe); let produced=0.0;
    for i in 0..outputs.len()/2 {
        if slots[17+i*2]>0.0 && slots[16+i*2]!=outputs[i*2].to_float() {return false;}
        produced+=outputs[i*2+1];
    }
    load(slots)-consumed+produced<=data::buffer_capacity()
}
fn produce(slots,recipe) {
    let cost=data::recipe_cost(recipe); let outputs=data::recipe_outputs(recipe);
    for i in 0..cost.len()/2 {slots=remove(slots,i,cost[i*2+1]);}
    for i in 0..outputs.len()/2 {slots[16+i*2]=outputs[i*2].to_float(); slots[17+i*2]+=outputs[i*2+1];}
    slots
}
fn feed(slots,recipe,inventory,creative,given,limit) {
    let cost=data::recipe_cost(recipe); let carried=backpack::totals(inventory); let moved=0.0;
    for step in 0..limit {
        let chosen=-1; let ratio=100000.0;
        for i in 0..cost.len()/2 {
            let item=cost[i*2]; if given>0.0 && item.to_float()!=given {continue;}
            if !creative && carried[item]<1.0 {continue;}
            let test=accept(slots,recipe,item.to_float()); if !test.ok {continue;}
            let value=slots[i*2+1]/cost[i*2+1]; if value<ratio {ratio=value;chosen=i;}
        }
        if chosen<0 {break;}
        let item=cost[chosen*2]; slots=accept(slots,recipe,item.to_float()).slots;
        if !creative {carried[item]-=1.0;} moved+=1.0;
    }
    let result=backpack::arrange(inventory,carried);
    #{slots:slots,inventory:result.inventory,moved:moved}
}
fn collect(slots,inventory) {
    let moved=0.0;
    for i in 8..16 {
        if slots[i*2+1]<=0.0 {continue;}
        let result=backpack::give_to(inventory,slots[i*2],slots[i*2+1]);
        inventory=result.inventory; slots=remove(slots,i,result.moved); moved+=result.moved;
    }
    #{slots:slots,inventory:inventory,moved:moved}
}
fn refund(slots,inventory) {
    let carried=backpack::totals(inventory);
    for i in 0..8 {if slots[i*2+1]>0.0 {carried[slots[i*2].to_int()]+=slots[i*2+1];}}
    backpack::arrange(inventory,carried)
}
fn describe(slots,output) {
    let text="";
    for i in (if output {8}else{0})..(if output {16}else{8}) {
        if slots[i*2+1]<=0.0 {continue;}
        if text!="" {text+=" + ";}
        text+=slots[i*2+1].to_int().to_string()+" "+data::item_name(slots[i*2]);
    }
    if text=="" {"empty"}else{text}
}
// Port sides use the model's local +X/+Z/-X/-Z before the tile rotation.
fn pipe_port(kind,facing,side) {
    let local=(side-facing+4)%4;
    if kind==26.0 {local==0 || local==2}else if kind==27.0 {local==0 || local==1}else{false}
}
fn fluid_input(kind,facing,side) {
    if kind==15.0 || kind==22.0 {side==(facing+1)%4}
    else {[4.0,17.0,18.0,19.0].contains(kind) && side==(facing+2)%4}
}
fn fluid_output_side(kind,facing,slot) {
    // Refinery and electrolyzer have independent product and byproduct outlets.
    (facing+if (kind==17.0 || kind==19.0) && slot==9 {1}else{0})%4
}
