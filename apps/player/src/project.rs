use super::*;

pub fn resolve(options: &mut Options) -> Result<()> {
    ensure!(
        options.content_catalog.is_some() == options.content_address.is_some(),
        "--content-catalog and --content must be supplied together"
    );
    ensure!(
        options.content_cache.is_none() || options.content_catalog.is_some(),
        "--content-cache needs --content-catalog and --content"
    );
    if options.content_catalog.is_some() {
        ensure!(
            options.project.is_none()
                && options.scene.is_none()
                && options.export_project.is_none(),
            "addressable content cannot be combined with --project, --scene or export"
        );
    }
    ensure!(
        options.project.is_none() || options.scene.is_none(),
        "--project and --scene are mutually exclusive"
    );
    ensure!(
        options.export_project.is_some() == options.export_dir.is_some(),
        "--export-project and --export-dir must be supplied together"
    );
    if options.export_project.is_some() {
        ensure!(
            options.project.is_none()
                && options.scene.is_none()
                && !options.smoke
                && options.frames.is_none()
                && options.write_scene.is_none()
                && !options.verify_first_trail
                && !options.verify_flap_woods,
            "export is a standalone command"
        );
        return Ok(());
    }
    ensure!(
        !options.verify_flap_woods
            || (!options.verify_first_trail
                && !options.smoke
                && options.write_scene.is_none()
                && options.frames.is_none()),
        "--verify-flap-woods is a standalone CPU command"
    );
    ensure!(
        !options.verify_first_trail
            || (!options.smoke
                && options.write_scene.is_none()
                && options.frames.is_none_or(|n| n == 340)),
        "--verify-first-trail is standalone; optional --frames must be 340"
    );
    if let Some(location) = &options.content_catalog {
        let progress = bozzard_assets::job::Progress::default();
        let catalog = bozzard_project::content::load_catalog(location, &progress)?;
        let cache = match &options.content_cache {
            Some(path) => path.clone(),
            None => bozzard_project::content::default_cache_directory()?,
        };
        let mut store = bozzard_project::content::ContentStore::new(cache);
        let resolved = store.resolve(
            &catalog,
            options.content_address.as_ref().unwrap(),
            &progress,
        )?;
        options.layer = resolved.scene_view()?;
        options.scene = Some(resolved.path());
        options.game_name = Some(resolved.pack().name().into());
        options.content_handle = Some(resolved);
    }
    if options.project.is_none() && options.scene.is_none() {
        options.project = bozzard_project::bundled_project(&std::env::current_exe()?);
    }
    if let Some(path) = &options.project {
        let (project, source) = bozzard_project::Project::load(path)?;
        project.validate_scene(&load_document(Some(&source))?)?;
        options.game_name = Some(project.name);
        options.layer = project.view;
        options.scene = Some(source);
    }
    Ok(())
}

/// The reference acceptance route feeds the same physical-key adapter used by native events.
/// It checks packaged gameplay; it does not emulate OS focus or mouse gestures.
pub fn route_tick(player: &mut Player, tick: u64) -> Result<()> {
    ensure!(
        player.demo.gameplay().is_some(),
        "First Trail verification requires a Player Controller"
    );
    if tick >= 340 {
        return Ok(());
    }
    player.gameplay_controls.event(&WindowEvent::Focused(true));
    player.dispatch_keyboard(
        PhysicalKey::Code(KeyCode::KeyW),
        &Key::Character("w".into()),
        ElementState::Pressed,
        false,
        false,
    )?;
    if tick == 80 {
        player.dispatch_keyboard(
            PhysicalKey::Code(KeyCode::Space),
            &Key::Named(NamedKey::Space),
            ElementState::Pressed,
            false,
            false,
        )?;
    }
    player.demo.app.step();
    player.demo.check_simulation()
}

pub fn verify_route(player: &mut Player) -> Result<()> {
    let state = player.demo.gameplay().context("missing gameplay state")?;
    ensure!(
        player.demo.app.ticks() == 340
            && state.won
            && state.collected.len() == 3
            && state.total == 3
            && state.checkpoint.as_deref() == Some("checkpoint")
            && state.respawns == 0,
        "First Trail route failed: ticks={} state={state:?}",
        player.demo.app.ticks()
    );
    player.dispatch_keyboard(
        PhysicalKey::Code(KeyCode::KeyR),
        &Key::Character("r".into()),
        ElementState::Pressed,
        false,
        false,
    )?;
    let state = player.demo.gameplay().context("restart lost gameplay")?;
    ensure!(
        !state.won && state.collected.is_empty() && state.checkpoint.is_none(),
        "restart did not reset gameplay"
    );
    println!(
        "first_trail_ok ticks=340 collected=3 checkpoint=checkpoint won=true respawns=0 restart=true"
    );
    Ok(())
}
