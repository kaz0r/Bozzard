fn main() -> anyhow::Result<()> {
    let _steam_shutdown = bozzard_runtime::steam_runtime::ShutdownGuard;
    bozzard_editor_app::run()
}
