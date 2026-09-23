# runner

A Wayland launcher (layer-shell, GTK4) that uses [elephant](https://github.com/abenz1267/elephant)
as its backend: runner is only the UI, elephant does the searching and launching.

## Requirements

- `elephant` running (`elephant service enable` to run it as a user service)
- `gtk4`, `gtk4-layer-shell`
- a compositor with `wlr-layer-shell` (Hyprland, Sway, niri, …)

## Build

```sh
cargo build --release
install -Dm755 target/release/runner ~/.local/bin/runner
```

## Usage

```sh
runner                     # providers from the config
runner -q /bluetooth       # open straight into bluetooth mode
runner -p bluetooth        # a single provider
runner -q fire             # initial text
```

Launching it while it is already open closes it, so a single keybind is enough. Hyprland:

```
bind = SUPER, SPACE, exec, runner
```

### Modes

Every installed elephant provider is a mode:

| Type | What happens |
|---|---|
| `/` | List of installed providers (`elephant listproviders`); `Enter`/`Tab` enters one |
| `/blu` | Filtered list |
| `/bluetooth` · `/bluetooth headphones` | Enters the mode: the prompt becomes `bluetooth ❯` and only the query stays in the search bar |
| `/menus:<name>` | An elephant menu; `Enter` opens submenus |
| config prefix (e.g. `=2+2`) | Shortcut to a provider |

Inside a mode, the **provider's own commands** appear in a separate group below the
results (for bluetooth: power on/off, scan for devices; for submenus: go to the
parent menu). They come after the results, so pressing `Enter` right after entering
a mode acts on an item, not on the provider.

Providers listed in `keep_open` (default: bluetooth, wireplumber, playerctl, todo)
don't close the launcher after an action: they wait for elephant to finish (a
spinner shows while connecting, or during a 5 s scan) and then reload the list.

### Keys

| Key | Action |
|---|---|
| `↑` `↓` · `Tab` · `Ctrl+J/K` · `Ctrl+N/P` | Move the selection |
| `Enter` / click | The item's main action (e.g. start, connect) |
| `Alt+2`…`Alt+9` | Other actions, listed in the footer |
| `Backspace` on an empty search bar | Leave the mode |
| `Esc` | Close |

## Configuration

- `~/.config/runner/config.toml`: see [`config.example.toml`](config.example.toml)
- `~/.config/runner/style.css`: loaded on top of the default style
  ([`src/style.css`](src/style.css))

### Look

Runner takes its look from the desktop instead of having one of its own:

- **Colors from pywal** (`~/.cache/wal/colors.json`): `background`, `foreground`,
  and `color4` as the accent, the same one the GTK theme uses. Surfaces and lines
  are mixes of background and foreground (`mix()`), so they work with any
  wallpaper. Without pywal, a fallback palette is used.
- **Border from Hyprland:** the same gradient as `general:col.active_border`, with
  square corners. The launcher looks like a focused window.
- To change the colors, redefine `runner_bg`, `runner_fg` and `runner_accent` with
  `@define-color` in your `style.css`.

## Notes

- **Action order:** elephant doesn't say which action is the default, and the
  bluetooth provider sends `remove` first. Runner orders them itself: the config's
  `primary_actions` first, then the rest, with destructive ones (`remove`,
  `erase_history`, `delete`…) last and duplicates dropped.
- **Protocol:** runner talks to elephant's socket
  (`$XDG_RUNTIME_DIR/elephant/elephant.sock`) directly, in JSON, so no protobuf is
  needed. Request frame `[type u8][format u8][len u32 BE][payload]`, response frame
  `[type u8][len u32 BE][payload]`.
- **Renderer:** runner sets `GSK_RENDERER=gl` unless it is already set. With the
  default Vulkan renderer, GTK enumerates every GPU and wakes a suspended discrete
  GPU on hybrid laptops: startup went from ~0.2 s to ~2 s.
