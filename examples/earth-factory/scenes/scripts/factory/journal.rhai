// Rhai gameplay: journal.
import "factory-data" as data;
import "factory-grid" as grid;
import "factory-panels" as panels;
import "factory-progression" as progression;
import "factory-visuals" as visuals;

fn update_journal(dt) {
    if !input_pressed("J") && !get_object_variable("journal_open") && get_object_variable("journal_alpha") <= 0.0 { return; }
    let open = get_object_variable("journal_open");
    let page = get_object_variable("journal_page").to_int();
    if input_pressed("J") {
        open = !open;
        if open { panels::close_storage(); panels::set_assembler(-1); }
    }
    if open {
        if input_pressed("ArrowLeft") { page = max(1, page - 1); }
        if input_pressed("ArrowRight") { page = min(3, page + 1); }
        for event in ui_events() {
            if event.kind != "activate" { continue; }
            if event.target == "journal-close" { open = false; }
            for i in 1..4 { if event.target == "journal-tab-" + i.to_string() { page = i; } }
            if page == 1 && event.target == "journal-deliver" { progression::deliver_phase(); }
            if page == 2 {
                for item in [11,12,14,15,16,17,13,21,18,20,23,22] {
                    if event.target == "journal-recipe-" + item.to_string() { data::session_set(3,item.to_float()); }
                }
                if event.target == "journal-craft-one" { progression::craft_batch(data::session_value(3).to_int(),1); }
                if event.target == "journal-craft-ten" { progression::craft_batch(data::session_value(3).to_int(),10); }
            }
        }
    }
    set_object_variable("journal_open",open); set_object_variable("journal_page",page.to_float());
    let alpha = clamp(get_object_variable("journal_alpha") + if open { dt / 0.20 } else { -dt / 0.16 },0.0,1.0);
    set_object_variable("journal_alpha",alpha);
    set_ui_visible("journal-overlay",alpha > 0.0); set_ui_enabled("journal-book",open);
    set_ui_opacity("journal-overlay",grid::smooth(alpha)); set_ui_offset("journal-book",0.0,(1.0-grid::smooth(alpha))*24.0);
    if alpha <= 0.0 { return; }
    let stock = get_object_list("stock"); let phase = get_object_variable("phase").to_int();
    let state = [open, page, phase, data::free_build(), data::session_value(3),
        progression::at_workbench(), if page == 1 && phase == 4 { visuals::landing_clear() } else { true }].to_string() + stock.to_string();
    if !data::ui_view_changed(1, state) { return; }
    for i in 1..4 { set_ui_background("journal-tab-" + i.to_string(), if i == page { [0.64,0.47,0.24,1.0] } else { [0.77,0.67,0.48,1.0] }); }
    set_ui_visible("journal-deliver",page == 1 && phase < 7);
    set_ui_visible("journal-craft-one",page == 2); set_ui_visible("journal-craft-ten",page == 2);
    set_ui_visible("journal-left-body",page != 2);
    for item in [11,12,14,15,16,17,13,21,18,20,23,22] {
        let id = "journal-recipe-" + item.to_string();
        set_ui_visible(id,page == 2);
        if page == 2 {
            set_ui_text(id,(if data::recipe_unlocked(item) { "" } else { "Locked: " }) + data::item_name(item.to_float()));
            set_ui_background(id,if data::session_value(3) == item.to_float() { [0.66,0.53,0.32,1.0] } else { [0.84,0.77,0.60,1.0] });
        }
    }
    set_ui_text("journal-subtitle",data::planet_name() + "  /  " + data::phase_label(phase));
    set_ui_text("journal-backpack","CARRIED: " + stock[1].to_int().to_string() + " iron ore / " + stock[2].to_int().to_string() + " copper ore\n" + stock[11].to_int().to_string() + " iron ingots / " + stock[12].to_int().to_string() + " copper ingots\n" + if phase >= 2 { "I opens your full inventory." } else { "Inventory screen unlocks in Tier 1 Phase 3." });
    if page == 1 {
        set_ui_text("journal-left-title","I  /  Unlocked equipment");
        let body = "Hand tools and pod workbench\n";
        for kind in [3.0,1.0,2.0,9.0,10.0,4.0,6.0,11.0,5.0] {
            body += (if data::tool_unlocked(kind) { "Ready    " } else { "Locked  " }) + data::build_name(kind) + "\n";
        }
        body += (if phase >= 2 { "Ready    " } else { "Locked  " }) + "Inventory [I]";
        set_ui_text("journal-left-body",body);
        set_ui_text("journal-right-title",if phase < 7 { data::phase_label(phase+1) } else { "Rocket complete" });
        if phase < 7 {
            let cost = progression::delivery_recipe(phase);
            set_ui_text("journal-right-body",data::phase_name(phase+1) + "\n\nDeliver from carried materials:\n" + data::carried_cost_label(cost,stock) + "\n\nCurrent: " + data::phase_name(phase));
            set_ui_enabled("journal-deliver",open && data::can_pay(stock,cost) && (phase != 4 || visuals::landing_clear()));
            if phase == 4 && !visuals::landing_clear() { set_ui_text("journal-backpack","Clear the rocket and dock tiles first:\nRegion 0,0 / Tiles 0,1 and 2,1\nCollect items before removing machines."); }
        } else { set_ui_text("journal-right-body",if data::on_moon() { "STELLA-Z2 / Lunar outpost\n\nExplore for Amorium and Moondust.\nTechtorium is exceptionally rare.\n\nEarth equipment remains unlocked.\nLunar recipes and phases come later.\n\nReturn to Stellar-BX by rocket [E]." } else { "The rocket is 100% assembled.\n\nApproach it at the landing site\nand press E to enter.\n\nNext destination: Stella-Z2\nReturn trips need no fuel yet." }); }
    } else if page == 2 {
        let item = data::session_value(3).to_int(); let cost = data::recipe_cost(item);
        let process = if item == 22 || item == 23 { "Build from the Power action bar" } else if item == 18 || item == 20 { "Assembler" } else if item == 15 || item == 16 || item == 17 { "Constructor / journal" } else if item == 21 { "Journal: equipment" } else if item == 13 { "Assembler / journal" } else { "Smelter / journal" };
        set_ui_text("journal-left-title","II  /  Choose a recipe");
        set_ui_text("journal-right-title",data::item_name(item.to_float()));
        let body = (if data::recipe_unlocked(item) { process } else { "LOCKED / " + if item == 18 || item == 20 { "Tier 2 Phase 1" } else if item == 22 { "Tier 1 Phase 4" } else { "Tier 1 Phase 2" } }) + "\n\n";
        for i in 0..cost.len()/2 { body += cost[i*2+1].to_int().to_string() + " " + data::item_name(cost[i*2].to_float()) + "   (have " + stock[cost[i*2]].to_int().to_string() + ")\n"; }
        body += "-> " + data::recipe_yield(item).to_int().to_string() + " " + data::item_name(item.to_float()) + "\n\n" + recipe_steps(item);
        set_ui_text("journal-right-body",body);
        set_ui_text("journal-backpack",if item == 22 || item == 23 { "Ctrl+3 opens the Power action bar.\nChoose the machine, then Space to build." } else if item == 18 { "Concrete is produced in an assembler.\nFeed 2 stone + 1 limestone per craft." } else if progression::at_workbench() { "WORKBENCH READY\nCraft once or up to ten batches." } else { "Return to the landing pod to hand-craft." });
        let can = open && progression::at_workbench() && data::recipe_unlocked(item) && item != 18 && item != 22 && item != 23 && (data::free_build() || data::can_pay(stock,cost));
        set_ui_enabled("journal-craft-one",can); set_ui_enabled("journal-craft-ten",can);
    } else {
        let ready = phase >= 7; let half = phase >= 6; let pad = phase >= 5;
        set_ui_text("journal-left-title","III  /  Space program");
        set_ui_text("journal-left-body","LANDING SITE\n" + if pad { "4 x 4 asphalt platform ready\nFuel INPUT dock installed" } else { "Tier 2 Phase 2: asphalt + fuel dock" }
            + "\n\nROCKET ASSEMBLY\n" + if ready { "100% / Completed" } else if half { "50% / Lower hull assembled" } else { "0% / Not assembled" }
            + "\n\nDESTINATION\nStellar-BX -> Stella-Z2\nEight more planets: Coming soon");
        set_ui_text("journal-right-title",if ready { "Ready to board" } else { "Construction order" });
        set_ui_text("journal-right-body","1. T2 P1: automate components\n2. T2 P2: upgrade the landing site\n3. T2 P3: assemble the lower hull\n4. T2 P4: complete the rocket\n\nPress E near the finished rocket\nto travel between Stellar-BX and Stella-Z2.\n\nNo fuel is required yet.");
    }
}

fn recipe_steps(item) {
    if item == 22 || item == 23 { "Gather -> smelt -> craft components\nUse the Power bar to build." }
    else if item == 11 || item == 12 || item == 14 { "1. Gather ore / sand [F]\n2. Smelt or use the pod workbench" }
    else if item == 18 { "1. Gather stone and limestone\n2. Feed both to an assembler" }
    else { "1. Gather the matching ores\n2. Smelt into ingots\n3. Process the ingots into this item" }
}
