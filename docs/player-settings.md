# Player settings

Each player of a game has a small settings file: window mode and size, VSync, a quality
preset and four volume sliders. It is separate from scenes and from
[save slots](blueprint-depth.md#scenes-and-checkpoints), so loading a checkpoint or restarting
a level never changes it. Games read and change settings through Blueprint nodes or Rhai
functions; the native player and exported games apply them. No game code is needed to get
the defaults.

## Settings

| Setting | Values | Default | Applied as |
| --- | --- | --- | --- |
| Window mode | `windowed`, `borderless`, `fullscreen` | `windowed` | Borderless covers the current monitor. Fullscreen is exclusive, at the monitor's largest and then fastest video mode; a platform that lists no modes gets borderless. |
| Window size | width, height: integers `320`–`16384` | `1024` × `640` | Windowed client size in logical points. Returning from fullscreen restores it. |
| VSync | on/off | on | On is FIFO presentation. Off uses Immediate, else Mailbox; a surface offering neither keeps FIFO and logs a warning. |
| Quality | `low`, `medium`, `high` | `high` | Caps on the scene's authored rendering cost (below). |
| Master, Music, SFX, UI volume | `0`–`1` each | `1` | Multiply the authored [Audio Mixer](middleware.md#audio) master and the Music, Sfx and Ui buses. The Ambience bus follows SFX. |

Quality presets only lower existing renderer switches; they never enable an effect the scene
did not author:

| Preset | Shadow map | Volumetric fog, screen-space reflections | Ambient occlusion, bloom, depth of field, motion blur |
| --- | --- | --- | --- |
| High | as authored | as authored | as authored |
| Medium | at most 1024 | off | as authored |
| Low | at most 512 | off | off |

Temporal anti-aliasing, fog, lighting and materials stay as authored at every preset. The
renderer has no internal resolution scale yet, so there is no resolution-scale setting.

## Edit, apply and save

The runtime keeps two copies. **Edited** values are what the getters report and what the
setters change. **Applied** values are what the host uses. This lets a settings menu show
the player's choices before they take effect.

- **Set …** changes one edited value. Invalid values (an unknown mode, a size outside the range,
  a volume outside `0`–`1`) fail the graph or script with the reason and change nothing.
- **Apply Settings** publishes the edited values. The native player reconfigures the window and
  surface before its next frame; quality and volumes take effect on that frame.
- **Save Settings** writes the edited values. It does not apply them; apply first if the change
  should be visible now.
- **Reset Settings to Defaults** returns the edited values to the defaults above; Apply and Save
  remain separate steps.

Settings survive restarts, Load Scene and Load Game State, and the player's F6/R scene reload.

## Where settings live

The file is `settings.json` in `<user data>/bozzard/settings/<game key>/`. The user data
directory and game key follow [save games](blueprint-depth.md#scenes-and-checkpoints):
`LOCALAPPDATA`, else `XDG_DATA_HOME`, else `~/.local/share`, and a hash of the starting scene's
name. `BOZZARD_SETTINGS_DIR` replaces the directory, and the player's `--settings FILE` replaces
the whole path.

| Host | Reads the file | Writes the file |
| --- | --- | --- |
| Native player and exported games | At startup, before the window opens | Only when the game saves |
| Player `--frames` and `--verify-*` runs | Only with `--settings FILE` | Only when the game saves |
| Editor Play | Never: Play starts from defaults | Only when the game saves |
| Headless server and `SceneRuntime::new` | Never | `new_with_prefabs` hosts save to the file; `SceneRuntime::new` keeps saves in memory |

Editor Play applies volume changes to its audio. Window mode, size, VSync and quality are
player features: the editor owns its window and viewport, so Play reports and saves those
values without applying them.

## File format and versions

```json
{
  "version": 1,
  "window_mode": "borderless",
  "window_size": [1280, 720],
  "vsync": false,
  "quality": "medium",
  "master_volume": 0.8,
  "music_volume": 0.5,
  "sfx_volume": 1.0,
  "ui_volume": 1.0
}
```

`version` is required; any other missing field takes its default, so a hand-written file can
contain just the values it changes. Unknown fields, invalid values, files over 64 KiB and
versions newer than the game supports are rejected. A rejected file never stops the game: the
player prints the reason, logs it as a **Settings** warning in the console (and the next
[crash report](debugging.md#crash-reports)), and starts with defaults. The file stays
untouched. If the game later saves, the rejected file is first renamed to
`settings.json.rejected`, so an older game cannot silently overwrite a newer game's settings.
Saves use a synced temporary file and a rename.

## Blueprint nodes

| Node | Pins |
| --- | --- |
| Window Mode Setting | → Mode (Text) |
| Window Size Setting | → Width, Height (Number) |
| VSync Setting | → Enabled (Bool) |
| Quality Setting | → Preset (Text) |
| Volume Setting | Channel (Text: `master`, `music`, `sfx`, `ui`) → Volume (Number) |
| Set Window Mode Setting | Mode (Text) |
| Set Window Size Setting | Width, Height (Number, integers) |
| Set VSync Setting | Enabled (Bool) |
| Set Quality Setting | Preset (Text) |
| Set Volume Setting | Channel (Text), Volume (Number) |
| Apply Settings / Save Settings / Reset Settings to Defaults | execution only |

Names are case-insensitive; getters return lowercase. Queries read the edited values and are
re-evaluated for each action, so a chain that sets and then reads sees its own change.

## Rhai functions

| Function | Result |
| --- | --- |
| `player_settings()` | Map of every edited value, with the field names of the file |
| `get_window_mode_setting()`, `get_quality_setting()` | `string` |
| `get_window_size_setting()` | `[width, height]` integers |
| `get_vsync_setting()` | `bool` |
| `get_volume_setting(channel)` | `0.0`–`1.0` |
| `set_window_mode_setting(mode)`, `set_quality_setting(preset)` | Edit a value |
| `set_window_size_setting(width, height)` | Integers |
| `set_vsync_setting(enabled)`, `set_volume_setting(channel, volume)` | Edit a value |
| `apply_settings()`, `save_settings()`, `reset_settings()` | As the nodes above |

Setters are validated at the call and, like blackboard writes, are visible to later reads in
the same tick; the requests themselves apply in call order after scripts run.

```rhai
fn on_start(me) {
    set_volume_setting("music", 0.5);
    set_quality_setting("medium");
    apply_settings();
    save_settings();
}
```

## Verification

```sh
cargo test -p bozzard-scene player_settings
cargo test -p bozzard-scene --test player_settings
cargo test -p bozzard-player settings
cargo run -p bozzard-player -- --frames 30 --settings work/settings.json
```

The tests cover round-trips, partial files, validation, defaults, newer and unknown versions,
keeping rejected files aside, edit/apply/save separation, every node and script function,
checkpoint independence, the audio mix, and a headless check that an applied revision
reconfigures the window mode, present mode and quality while volumes reach the mix. With
`vsync: false` in that file, the player's `player_presentation_interval` drops below the
display's refresh interval.

## Limits and follow-ups

- Key rebinding waits for the input-actions feature; scenes still choose their own keys.
- No monitor, refresh-rate or fullscreen-resolution choice: exclusive fullscreen picks the best
  mode of the current monitor.
- No resolution scale or frame-rate cap until the renderer supports them; VSync off is uncapped.
- Accessibility options (UI text scale, contrast, reduced motion) remain the separate
  [UI preference nodes](middleware.md#widgets-2d-and-accessibility) and are not yet stored in this file.
