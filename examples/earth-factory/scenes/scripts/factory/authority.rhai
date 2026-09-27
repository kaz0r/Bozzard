// Pure factory transactions. No scene reads, rendering commands or local UI state:
// the host passes one actor and one addressed cell, then commits a valid result.
import "factory-data" as data;
import "factory-backpack" as backpack;
import "factory-progression" as progression;
import "factory-power" as power;
import "factory-deposits" as deposits;

fn denied(ctx,message) { ctx.ok=false; ctx.message=message; ctx }
fn accepted(ctx,message) { ctx.ok=true; ctx.message=message; ctx }
fn stock(ctx) { backpack::totals(ctx.inventory) }
fn pay(ctx,cost) {
    if ctx.creative { return #{ok:true,inventory:ctx.inventory}; }
    let carried=stock(ctx);
    if !data::can_pay(carried,cost) { return #{ok:false,inventory:[]}; }
    backpack::arrange(ctx.inventory,data::spend(carried,cost))
}
fn blank_cell(node) {
    let slots=[]; slots.pad(32,0.0);
    #{node:node,build:0.0,facing:0.0,recipe:0.0,item:0.0,amount:0.0,
      input:0.0,input_amount:0.0,progress:0.0,iron:0.0,copper:0.0,split:0.0,storage:slots}
}

fn apply(ctx,kind,args) {
    let cell=ctx.cell;
    if kind=="place" {
        let machine=args.kind.to_float();
        if !data::tool_unlocked(machine,ctx.phase,ctx.creative) { return denied(ctx,"Machine locked."); }
        if cell.build!=0.0 { return denied(ctx,"A machine already occupies this tile."); }
        let x=ctx.at[1]; let z=ctx.at[2];
        if (x==0 && z==0) || (ctx.phase>=5.0 && z==1 && (x==0 || x==2)) { return denied(ctx,"Keep the landing pod, rocket and dock clear."); }
        if machine==1.0 && cell.node==0.0 { return denied(ctx,"A miner needs a deposit."); }
        if machine==6.0 && cell.node!=4.0 { return denied(ctx,"A generator needs a coal deposit."); }
        if machine!=1.0 && machine!=6.0 && cell.node!=0.0 { return denied(ctx,"Only extraction equipment can cover a deposit."); }
        let cost=data::build_cost(machine);
        if machine==1.0 && stock(ctx)[21]>0.0 { cost=[21,1.0]; }
        let paid=pay(ctx,cost); if !paid.ok { return denied(ctx,"Not enough building materials."); }
        ctx.inventory=paid.inventory;
        cell=blank_cell(cell.node); cell.build=machine; cell.facing=args.direction.to_float();
        if machine==3.0 || machine==5.0 || machine==11.0 { cell.recipe=data::recipe_choices(machine)[0]; }
    } else if kind=="remove" {
        if cell.build==0.0 { return denied(ctx,"No machine here."); }
        cell=blank_cell(cell.node);
    } else if kind=="rotate" {
        if cell.build==0.0 { return denied(ctx,"No machine here."); }
        cell.facing=((cell.facing.to_int()+1)%4).to_float();
    } else if kind=="gather_tick" {
        let node=cell.node.to_int();
        if !((node>=1 && node<=5) || (node>=8 && node<=10) || (node>=24 && node<=26)) {
            return denied(ctx,"Stand on a solid deposit to gather.");
        }
        let result=backpack::give_to(ctx.inventory,cell.node,1.0);
        if result.moved==0.0 { return denied(ctx,"Inventory is full."); }
        ctx.inventory=result.inventory;
    } else if kind=="craft" {
        let item=args.recipe; let cost=data::recipe_cost(item);
        if abs(ctx.here[1])>1 || abs(ctx.here[2])>1 { return denied(ctx,"Craft at the landing workbench."); }
        if !data::recipe_unlocked(item,ctx.phase,ctx.creative) || item==18 || item==22 || item==23 || cost.len()==0 {
            return denied(ctx,"This recipe is unavailable at the workbench.");
        }
        let crafted=0;
        for batch in 0..args.count {
            let carried=stock(ctx);
            if !ctx.creative {
                if !data::can_pay(carried,cost) { break; }
                carried=data::spend(carried,cost);
            }
            carried[item]+=data::recipe_yield(item);
            let result=backpack::arrange(ctx.inventory,carried);
            if !result.ok { break; }
            ctx.inventory=result.inventory; crafted+=1;
        }
        if crafted==0 { return denied(ctx,"Missing ingredients or inventory space."); }
    } else if kind=="deliver" {
        if ctx.phase>=7.0 { return denied(ctx,"All phases are complete."); }
        let cost=progression::delivery_recipe(ctx.phase.to_int()); let carried=stock(ctx);
        if !data::can_pay(carried,cost) { return denied(ctx,"The delivery needs more materials."); }
        if ctx.phase==4.0 && !ctx.landing_clear { return denied(ctx,"Clear the rocket and fuel dock tiles before expanding the landing site."); }
        let result=backpack::arrange(ctx.inventory,data::spend(carried,cost));
        if !result.ok { return denied(ctx,"Cannot complete this delivery."); }
        ctx.inventory=result.inventory; ctx.phase+=1.0;
    } else if kind=="configure" {
        let recipe=args.recipe.to_float();
        if !(cell.build==3.0 || cell.build==5.0 || cell.build==11.0) ||
            !data::recipe_choices(cell.build).contains(recipe) || !data::recipe_unlocked(args.recipe,ctx.phase,ctx.creative) {
            return denied(ctx,"This machine cannot make that recipe.");
        }
        if cell.recipe==recipe { return accepted(ctx,"Recipe already selected."); }
        if cell.amount>0.0 { return denied(ctx,"Collect the finished output first."); }
        let previous=data::recipe_cost(cell.recipe.to_int()); let next=data::recipe_cost(args.recipe);
        if previous!=next || data::recipe_yield(cell.recipe.to_int())!=data::recipe_yield(args.recipe) {
            let carried=stock(ctx);
            if cell.build==5.0 {
                if previous.len()==4 { carried[previous[0]]+=cell.iron; carried[previous[2]]+=cell.copper; }
            } else { carried[cell.input.to_int()]+=cell.input_amount; }
            let result=backpack::arrange(ctx.inventory,carried);
            if !result.ok { return denied(ctx,"Make room for unused ingredients first."); }
            ctx.inventory=result.inventory; cell.input=0.0; cell.input_amount=0.0; cell.iron=0.0; cell.copper=0.0;
        }
        cell.recipe=recipe; cell.progress=0.0;
    } else if kind=="collect" {
        if !(cell.build==1.0 || cell.build==3.0 || cell.build==5.0 || cell.build==7.0 || cell.build==8.0 || cell.build==11.0) || cell.amount<=0.0 {
            return denied(ctx,"No finished output to collect.");
        }
        let result=backpack::give_to(ctx.inventory,cell.item,cell.amount);
        if result.moved==0.0 { return denied(ctx,"Inventory is full."); }
        ctx.inventory=result.inventory; cell.amount-=result.moved;
        if cell.amount==0.0 { cell.item=0.0; }
    } else if kind=="feed" || kind=="insert" {
        let cost=data::recipe_cost(cell.recipe.to_int());
        if !(cell.build==3.0 || cell.build==5.0 || cell.build==11.0) || cost.len()==0 || !data::recipe_unlocked(cell.recipe.to_int(),ctx.phase,ctx.creative) {
            return denied(ctx,"Select an unlocked production recipe first.");
        }
        let carried=stock(ctx); let before=cell.input_amount+cell.iron+cell.copper;
        let given=if kind=="insert" { ctx.inventory[args.slot*2] } else { 0.0 };
        let limit=if kind=="insert" { min(args.amount.to_float(),ctx.inventory[args.slot*2+1]) } else if cell.build==5.0 { 10.0*(cost[1]+cost[3]) } else { 10.0 };
        for i in 0..limit.to_int() {
            let added=false;
            if cell.build==5.0 {
                let order=if cell.iron/cost[1]<=cell.copper/cost[3] { [0,1] } else { [1,0] };
                for side in order {
                    let item=cost[side*2];
                    if kind=="insert" && item.to_float()!=given { continue; }
                    let reserve=if side==0 { max(0.0,cost[3]-cell.copper) } else { max(0.0,cost[1]-cell.iron) };
                    if cell.amount+cell.iron+cell.copper+1.0+reserve>data::buffer_capacity() || (!ctx.creative && carried[item]<1.0) { continue; }
                    if side==0 { cell.iron+=1.0; } else { cell.copper+=1.0; }
                    if !ctx.creative { carried[item]-=1.0; }
                    added=true; break;
                }
            } else {
                let item=cost[0]; let reserve=max(0.0,data::recipe_yield(cell.recipe.to_int())-1.0);
                if kind=="insert" && item.to_float()!=given { break; }
                if (cell.input_amount>0.0 && cell.input!=item.to_float()) || cell.amount+cell.input_amount+1.0+reserve>data::buffer_capacity() || (!ctx.creative && carried[item]<1.0) { break; }
                cell.input=item.to_float(); cell.input_amount+=1.0;
                if !ctx.creative { carried[item]-=1.0; } added=true;
            }
            if !added { break; }
        }
        if cell.input_amount+cell.iron+cell.copper==before { return denied(ctx,"Missing ingredients or machine buffer is full."); }
        if kind=="insert" && !ctx.creative {
            let at=args.slot*2; ctx.inventory[at+1]-=cell.input_amount+cell.iron+cell.copper-before;
            if ctx.inventory[at+1]==0.0 { ctx.inventory[at]=0.0; }
        } else { let result=backpack::arrange(ctx.inventory,carried); ctx.inventory=result.inventory; }
    } else if kind=="take_storage" {
        if cell.build!=4.0 || ctx.phase<2.0 { return denied(ctx,"Storage is unavailable."); }
        let moved=0.0;
        for slot in 0..16 {
            let at=slot*2; if cell.storage[at+1]<=0.0 { continue; }
            let result=backpack::give_to(ctx.inventory,cell.storage[at],cell.storage[at+1]);
            ctx.inventory=result.inventory; cell.storage[at+1]-=result.moved; moved+=result.moved;
            if cell.storage[at+1]==0.0 { cell.storage[at]=0.0; }
        }
        if moved==0.0 { return denied(ctx,"Storage is empty or inventory is full."); }
    } else if kind=="storage_move" || kind=="storage_split" || kind=="storage_delete_kind" {
        if cell.build!=4.0 || ctx.phase<2.0 { return denied(ctx,"Storage is unavailable."); }
        let at=if kind=="storage_move" { args.from*2 } else { args.slot*2 };
        let item=cell.storage[at]; let amount=cell.storage[at+1];
        if amount<=0.0 { return denied(ctx,"That storage stack is empty."); }
        if kind=="storage_delete_kind" {
            for slot in 0..16 { if cell.storage[slot*2]==item { cell.storage[slot*2]=0.0; cell.storage[slot*2+1]=0.0; } }
        } else if kind=="storage_split" {
            let target=-1; for slot in 0..16 { if cell.storage[slot*2+1]==0.0 { target=slot*2; break; } }
            if target<0 || amount<2.0 { return denied(ctx,"Splitting needs an empty slot and two items."); }
            let half=floor(amount/2.0); cell.storage[target]=item; cell.storage[target+1]=half; cell.storage[at+1]-=half;
        } else {
            let to=args.to*2;
            if cell.storage[to]==item {
                let moved=min(amount,100.0-cell.storage[to+1]); cell.storage[to+1]+=moved; cell.storage[at+1]-=moved;
                if cell.storage[at+1]==0.0 { cell.storage[at]=0.0; }
            } else {
                let other=cell.storage[to]; let count=cell.storage[to+1];
                cell.storage[to]=item; cell.storage[to+1]=amount; cell.storage[at]=other; cell.storage[at+1]=count;
            }
        }
    } else if kind=="storage_transfer" {
        if cell.build!=4.0 || ctx.phase<2.0 { return denied(ctx,"Storage is unavailable."); }
        let source=if args.from_storage { cell.storage } else { ctx.inventory };
        let at=args.slot*2; let amount=min(args.amount.to_float(),source[at+1]);
        if amount<=0.0 { return denied(ctx,"The source stack is empty."); }
        let result=backpack::give_to(if args.from_storage { ctx.inventory } else { cell.storage },source[at],amount);
        if result.moved==0.0 { return denied(ctx,"The destination is full."); }
        source[at+1]-=result.moved; if source[at+1]==0.0 { source[at]=0.0; }
        if args.from_storage { cell.storage=source; ctx.inventory=result.inventory; }
        else { cell.storage=result.inventory; ctx.inventory=source; }
    } else if kind=="inventory_move" || kind=="inventory_discard" || kind=="inventory_split" {
        if ctx.phase<2.0 { return denied(ctx,"Player inventory has not unlocked."); }
        let inventory=ctx.inventory;
        let a=if kind=="inventory_move" { args.from*2 } else { args.slot*2 };
        if inventory[a+1]<=0.0 { return denied(ctx,"That stack is empty."); }
        if kind=="inventory_discard" {
            inventory[a+1]-=min(inventory[a+1],args.amount.to_float());
        } else if kind=="inventory_split" {
            let target=-1;
            for slot in 0..25 { if inventory[slot*2+1]==0.0 { target=slot*2; break; } }
            if target<0 || inventory[a+1]<2.0 { return denied(ctx,"Splitting needs an empty slot and two items."); }
            let half=floor(inventory[a+1]/2.0);
            inventory[target]=inventory[a]; inventory[target+1]=half; inventory[a+1]-=half;
        } else {
            let b=args.to*2; let amount=min(inventory[a+1],args.amount.to_float());
            if inventory[b+1]==0.0 || inventory[a]==inventory[b] {
                let moved=min(amount,100.0-inventory[b+1]);
                if moved==0.0 { return denied(ctx,"That stack is full."); }
                inventory[b]=inventory[a]; inventory[b+1]+=moved; inventory[a+1]-=moved;
            } else {
                if amount!=inventory[a+1] { return denied(ctx,"Only whole stacks can swap different items."); }
                let item=inventory[b]; let count=inventory[b+1];
                inventory[b]=inventory[a]; inventory[b+1]=inventory[a+1]; inventory[a]=item; inventory[a+1]=count;
            }
        }
        if inventory[a+1]==0.0 { inventory[a]=0.0; }
        ctx.inventory=inventory;
    } else { return denied(ctx,"Unknown gameplay action."); }
    ctx.cell=cell; accepted(ctx,"Action completed.")
}

fn discover(seed,planet,cx,cz) { deposits::generate_region_nodes(seed,cx,cz,planet) }
fn electrical(kind) { power::electrical(kind) }
fn connection_error(graph,from,to) { power::connection_error(graph,from,to) }
fn resolve_power(graph) { power::resolve_graph(graph).graph }
