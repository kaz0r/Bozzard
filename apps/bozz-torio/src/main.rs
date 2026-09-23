mod multiplayer;
mod runtime;
mod save;
mod scene;
mod sim;
mod stage;
mod steam;

fn bundled_app_id() -> Option<u32> {
    let executable = std::env::current_exe().ok()?;
    let text = std::fs::read_to_string(executable.parent()?.join("steam_appid.txt")).ok()?;
    text.trim().parse().ok()
}

fn main() -> anyhow::Result<()> {
    let mut app_id = std::env::var("SteamAppId")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .or_else(bundled_app_id)
        .unwrap_or(480);
    let mut offline = false;
    let mut start_playing = false;
    let mut screenshot = None;
    let mut screenshot_after_ms = 0;
    let mut join_lobby = None;
    let mut scene_path = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--steam-app-id" => {
                app_id = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--steam-app-id needs a number"))?
                    .parse()?
            }
            "--offline" => offline = true,
            "--play" => start_playing = true,
            "--scene" => {
                scene_path = Some(std::path::PathBuf::from(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("--scene needs a path"))?,
                ))
            }
            "--screenshot" => {
                screenshot =
                    Some(std::path::PathBuf::from(args.next().ok_or_else(|| {
                        anyhow::anyhow!("--screenshot needs a path")
                    })?))
            }
            "--screenshot-after-ms" => {
                screenshot_after_ms = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--screenshot-after-ms needs a number"))?
                    .parse()?;
            }
            "--join-lobby" | "+connect_lobby" => {
                let id: u64 = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--join-lobby needs a Steam lobby ID"))?
                    .parse()?;
                anyhow::ensure!(id != 0, "Steam lobby ID must be nonzero");
                join_lobby = Some(id);
            }
            "--help" | "-h" => {
                println!(
                    "Bozz-torio\n  --scene PATH           Use an edited Bozzard factory scene\n  --play                 Go straight to the factory\n  --offline              Skip Steam initialization\n  --steam-app-id NUMBER  Use this Steam App ID (default: SteamAppId, bundled steam_appid.txt, or 480 for development)\n  --join-lobby ID        Join a Steam friend's lobby (+connect_lobby ID also works)\n  --screenshot PATH      Capture the scene and exit\n  --screenshot-after-ms N  Wait N milliseconds before capture"
                );
                return Ok(());
            }
            other => anyhow::bail!("Unknown argument: {other}"),
        }
    }
    let scene =
        scene::SceneSource::open(scene_path.unwrap_or_else(scene::SceneSource::default_path))?;
    let steam = steam::SteamBridge::new(app_id, offline);
    runtime::run(runtime::Factory::new(
        scene,
        steam,
        start_playing,
        screenshot,
        std::time::Duration::from_millis(screenshot_after_ms),
        join_lobby,
    )?)
}
