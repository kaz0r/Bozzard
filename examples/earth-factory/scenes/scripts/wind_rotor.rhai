// Registration follows prefab lifetime, including chunk streaming and co-op edits.
import "factory-wind" as wind;
fn on_start(me) {wind::register_rotor(me);}
fn on_destroy(me) {wind::unregister_rotor(me);}
