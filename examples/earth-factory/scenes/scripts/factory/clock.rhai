// Saved session[120] is elapsed real play time. The displayed 24-hour clock
// advances four seconds per real second, beginning at 08:00 on a new world.
fn day_seconds(elapsed) { ((max(elapsed,0.0)%21600.0)*4.0+28800.0)%86400.0 }
fn daytime(elapsed,planet) {
    let seconds=day_seconds(elapsed);
    planet==0 && seconds>=21600.0 && seconds<64800.0
}
fn daylight(elapsed,planet) {
    if planet==1 {return -1.0;}
    sin((day_seconds(elapsed)-21600.0)/86400.0*6.283185307)
}
fn two_digits(value) { if value<10 {"0"+value.to_string()}else{value.to_string()} }
fn label(elapsed) {
    let seconds=floor(day_seconds(elapsed)).to_int();
    two_digits(seconds/3600)+":"+two_digits(seconds/60%60)+":"+two_digits(seconds%60)
}
fn update_view(elapsed) {
    let second=floor(day_seconds(elapsed));
    if get_object_variable("renewable-view","clock_second")!=second {
        set_ui_text("world-clock",label(elapsed));
        set_object_variable("renewable-view","clock_second",second);
    }
}
fn reset_accumulation() {
    set_object_variable("renewable-view","clock_observed",-1.0);
    set_object_variable("renewable-view","clock_correction",0.0);
}
fn advance(dt) {
    let elapsed=get_object_list_item("controller","session",120);
    let correction=if get_object_variable("renewable-view","clock_observed")==elapsed {
        get_object_variable("renewable-view","clock_correction")
    }else{0.0};
    // Compensated summation preserves the rate after hours of f32 additions.
    // External edits, a fresh world, and load restoration reset the residual.
    let increment=max(dt,0.0)-correction;
    let next=elapsed+increment;
    set_object_variable("renewable-view","clock_correction",(next-elapsed)-increment);
    set_object_variable("renewable-view","clock_observed",next);
    let session=get_object_list("controller","session");session[120]=next;
    set_object_list("controller","session",session);
}
