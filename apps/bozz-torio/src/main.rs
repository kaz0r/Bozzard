mod save;
mod sim;
mod sprites;
mod steam;
mod ui;

use eframe::egui;

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
            "--screenshot" => {
                screenshot =
                    Some(std::path::PathBuf::from(args.next().ok_or_else(|| {
                        anyhow::anyhow!("--screenshot needs a path")
                    })?))
            }
            "--help" | "-h" => {
                println!(
                    "Bozz-torio\n  --play                 Go straight to the factory\n  --offline              Skip Steam initialization\n  --steam-app-id NUMBER  Use this Steam App ID (default: SteamAppId, bundled steam_appid.txt, or 480 for development)\n  --screenshot PATH      Capture the scene and exit"
                );
                return Ok(());
            }
            other => anyhow::bail!("Unknown argument: {other}"),
        }
    }
    let steam = steam::SteamBridge::new(app_id, offline);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("BOZZ-TORIO · Pocket Factory")
            .with_inner_size([1320.0, 830.0])
            .with_min_inner_size([960.0, 690.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Bozz-torio",
        options,
        Box::new(move |cc| {
            Ok(Box::new(ui::FactoryApp::new(
                cc,
                steam,
                start_playing,
                screenshot,
            )?))
        }),
    )?;
    Ok(())
}
