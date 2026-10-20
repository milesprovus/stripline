# Stripline

A tiling window manager for macOS with workspaces and two layouts, written in
Rust. MIT licensed. Built on the public macOS Accessibility API, so SIP can
stay on.

- **Workspaces 1–9** switched with ⌘1–9, with ⌘⇧1–9 to send the focused
  window to one, as on [Omarchy](https://omarchy.org)/Hyprland.
- **Split layout** (the default): windows share the screen, and a new window
  splits the focused one in half, side by side or stacked depending on its
  shape (Hyprland's "dwindle").
- **Scrolling layout**: each window is a column on a horizontal strip that
  can be wider than the screen, so opening a window never shrinks the others
  (the model of [PaperWM](https://github.com/paperwm/PaperWM) and
  [niri](https://github.com/YaLTeR/niri)).

Each workspace can use either layout; ⌃⌥S switches the current one.

```
 split layout                          scrolling layout
 ┌───────────────┬───────────────┐            ┌──────────── screen ────────────┐
 │               │     Editor    │     ◀ ░░░░ │ ┌──────────┐ ┌───────────────┐ │ ┌──────┐ ▶
 │   Terminal    ├───────┬───────┤            │ │ Terminal │ │    Editor     │ │ │ Mail │
 │               │ Logs  │ Mail  │            │ │          │ ├───────────────┤ │ │      │
 │               │       │       │            │ │          │ │     Logs      │ │ │      │
 └───────────────┴───────┴───────┘            │ └──────────┘ └───────────────┘ │ └──────┘
  new windows split the focused one           └────────────────────────────────┘
                                                columns keep their width; it scrolls
```

> **Status:** early (0.1). The layout engine is covered by unit tests. The
> macOS layer compiles for Apple silicon and Intel, and CI builds it on a
> macOS runner, but it hasn't been used day-to-day yet. Expect rough edges
> and please file issues.

## Features

| | |
|---|---|
| Workspaces | Nine per setup, shown one per display. Switching is instant (windows of hidden workspaces are parked off screen, no macOS Space animation). ⌘-Tab or the Dock to a window on a hidden workspace switches to it. The menu bar shows the current number. |
| Split layout | Hyprland-style dwindle tiling: new windows split the focused one, the split direction follows the tile's shape, and closing a window gives its space to its sibling. Move focus and swap windows by direction, resize splits, flip a split between side by side and stacked. |
| Scrolling strip | One column per window. Columns keep their width, and the strip scrolls just far enough to show the focused one. |
| Stacks and tabs | Move windows into neighbouring columns or pop them back out. Columns can be stacked (split height) or tabbed (one visible at a time). |
| Widths | Cycle through presets (⅓, ½, ⅔, ¾ by default), step wider/narrower, toggle full width, centre a column. |
| Heights | Taller, shorter, or equal heights inside a stack. |
| Pinned columns | Keep one column fixed at the left or right edge while the rest scrolls. |
| Centring modes | `never` (scroll just enough), `on-overflow`, `always`. |
| Trackpad swipe | A 3- or 4-finger swipe moves the strip 1:1 with your fingers, then springs to the nearest column. |
| Spring animation | Critically damped scroll animation that you can retarget mid-flight. |
| Multiple displays | One strip per display and per Space. Windows never hang over onto a neighbouring display. Send windows between displays. Exclude specific displays. |
| macOS Spaces | Stripline's workspaces work inside the current macOS Space; one Space per display is recommended. Native full-screen windows are ignored. |
| Smart floating | Dialogs, panels and fixed-size windows float automatically. You can float or unfloat any window. Floating windows stay visible on every workspace. |
| Rules | Per app (bundle ID or name) and title regex: float, initial width, or where new windows open. |
| No focus stealing | Windows that open in background apps are added to the strip without taking focus. |
| Focus border | A coloured line around the focused window. Colour, thickness, corner radius, offset and an optional glow are configurable. |
| Mouse | Drag a window onto another column to reorder or stack. Drag its edge to change the column width. Optional focus-follows-mouse and mouse-follows-focus. |
| Session restore | Workspaces, layouts, column order, widths, stacks and pins survive a restart. Windows are matched by ID, then by app and title. |
| Settings window | Menu bar → Settings… (⌘,) or `stripline settings`. Every option has a control: layout, gaps with a live preview, the focus border with a live preview, trackpad and mouse, a shortcut recorder with conflict warnings, window rules, displays, and the checks from `stripline doctor`. Changes apply as you make them. |
| Live config | TOML file, reloaded automatically when you save it. Shortcuts follow your keyboard layout (QWERTZ, AZERTY, Dvorak, ...). |
| Automation | `stripline msg <action>` over a Unix socket, plus a JSON `state` dump. |
| Menu bar item | Settings, pause or resume, reload config, re-tile, open config file, quit. The top line shows when the config was last loaded, or why it failed (hover for the full error). |
| Launch at login | `stripline service install` sets up a LaunchAgent that also restarts Stripline if it crashes. |
| `stripline doctor` | Checks permissions, conflicting apps, Stage Manager, Spaces settings and trackpad gesture clashes. |

## Install

Requires macOS 12 or later and Rust 1.85 or later.

```sh
git clone <this repo> && cd stripline
./scripts/bundle.sh                 # builds dist/Stripline.app (universal binary)
cp -R dist/Stripline.app /Applications/
/Applications/Stripline.app/Contents/MacOS/stripline service install
```

On first launch macOS asks for **Accessibility** permission. Grant it under
System Settings → Privacy & Security → Accessibility. Stripline starts tiling
as soon as the permission is granted. No restart is needed.

> The bundle is ad-hoc signed. macOS ties the Accessibility permission to the
> signature, so after rebuilding you may need to remove and re-add the entry.

For a quick try without bundling, run `cargo run --release` from a terminal.
macOS then attributes the permission to the terminal app.

Then run `stripline doctor`. It checks the settings that commonly get in the way:

- another window manager or snapping tool is running
- Stage Manager is on
- macOS uses the same swipe to switch Spaces
- three-finger drag is on

## Default shortcuts

Workspaces use **⌘** like Super on Omarchy; everything else uses **⌃⌥**
(Control+Option), which leaves plain ⌥ free for typing special characters.

| Keys | Action |
|---|---|
| ⌘ 1 … 9 | show workspace *n* |
| ⌘⇧ 1 … 9 | move the focused window to workspace *n* (and follow it) |
| ⌃⌥ Tab / ⌃⌥⇧ Tab | next / previous workspace that has windows |
| ⌃⌥ S | switch this workspace between split and scrolling layout |
| ⌃⌥⇧ S | split layout: flip the focused split (side by side ⇄ stacked) |
| ⌃⌥ H / J / K / L, or arrows | focus left / down / up / right |
| ⌃⌥⇧ + the same | split: swap with the neighbour. Scroll: move column left/right, or window up/down in a stack |
| ⌃⌥⌘ ← / → | focus first / last column (add ⇧ to move) |
| ⌃⌥ 1 … 9 | focus column *n* (add ⇧ to move the column there) |
| ⌃⌥ R | cycle width presets (⇧ goes backwards) |
| ⌃⌥ F | full width (toggle) |
| ⌃⌥ C | centre the focused column |
| ⌃⌥ = / - | wider / narrower (split: moves the nearest side-by-side split) |
| ⌃⌥⇧ = / - | taller / shorter (split: moves the nearest stacked split) |
| ⌃⌥ 0 | equalize heights (split: reset every split to half) |
| ⌃⌥ [ / ] | stack into the left / right column |
| ⌃⌥⇧ [ / ] | pop out to the left / right |
| ⌃⌥ T | toggle tabbed column |
| ⌃⌥⇧ F | float / unfloat |
| ⌃⌥ P / ⌃⌥⇧ P | pin to left / right edge (toggle) |
| ⌃⌥ N / ⌃⌥⇧ N | send window to next / previous display |
| ⌃⌥ O / ⌃⌥⇧ O | focus next / previous display |
| ⌃⌥ Q | pause / resume tiling |

Width presets, full width, centring, stacks, tabs and pins are scrolling-layout
features. In the split layout those keys do nothing.

⌘1–9 take over those keys in every app (browser tabs and so on), and ⌘⇧3/4/5
are also macOS's screenshot shortcuts; `stripline doctor` points this out. To
use other keys, rebind `workspace N` and `move-to-workspace N`.

`stripline bindings` prints the bindings currently in effect. To change them,
add entries to the `[bindings]` table of the config file.

## Configuration

The easiest way is the settings window: **Settings…** in the menu bar menu
(⌘,), or `stripline settings`. It writes only the values you change to the
config file below and applies them immediately. The first time it saves, it
keeps your hand-written file as `config.toml.bak`.

The file itself is `~/.config/stripline/config.toml`. Run `stripline init-config` to write a
commented copy of [`config.example.toml`](config.example.toml). Every key is
optional. The file is re-read within two seconds of being saved, and
`stripline check-config` validates it.

```toml
[layout]
default_mode = "split"       # or "scroll" for the scrolling strip
gap = 8
default_width = 0.5
width_presets = [0.333, 0.5, 0.667, 0.75]
center = "on-overflow"

[gestures]
fingers = 3

[border]
color = "#3D8BFF"            # or "#RRGGBBAA" for transparency
width = 3
radius = 12                  # match your windows' corner radius
glow = 6                     # 0 = no glow

[[rules]]
app = "com.apple.calculator"
float = true

[bindings]
"hyper+h" = "focus-left"     # hyper = ctrl+alt+shift+cmd
"ctrl+alt+h" = "none"        # unbind a default
```

## Command line

```
stripline                     run the window manager
stripline msg focus-right     run any action (see `stripline actions`)
stripline msg set-width 40%
stripline msg workspace 3
stripline msg move-to-workspace-silent 5   # move without following
stripline msg state           JSON dump of strips, columns and windows
stripline msg displays        display names and UUIDs (for [displays].disabled)
stripline msg pause|resume|reload|quit
```

Every keyboard action is also a `msg` command, so it's easy to call from
Raycast, Alfred, skhd, BetterTouchTool or shell scripts.

## How it works

```
 src/engine/      platform-independent, unit tested
   strip.rs       a workspace's windows: columns, stacks, pins (scroll) or a tree (split)
   tree.rs        split layout: binary split tree, directional focus, resizing
   layout.rs      strip -> window frames; scroll targets; swipe snapping
   spring.rs      critically damped spring (closed form)
   session.rs     save / restore layout
 src/macos/
   wm.rs          event-driven core: keeps strips in sync with real windows
   ax.rs          Accessibility (AXUIElement) wrapper
   observer.rs    per-app AXObserver (window created/destroyed/moved/...)
   eventtap.rs    key bindings (active tap), mouse + trackpad touches (passive tap)
   display.rs     NSScreen geometry, Spaces, mouse
   keymap.rs      character -> key code for the active keyboard layout
   border.rs      focus border overlay (one Core Animation layer)
   sink.rs        invisible window that takes focus on an empty workspace
   settings_window.rs  native window hosting the settings page (WebKit)
 src/settings/     settings back end, runs on any OS
   mod.rs         HTTP server on 127.0.0.1 (random port, per-run token) + JSON API
   model.rs       shortcut catalogue, binding overrides, minimal TOML writer
   ui.html        the settings page (no dependencies; light and dark)
   menu.rs        menu bar item
 src/config.rs, keys.rs, action.rs, ipc.rs, service.rs
```

- **Moving windows.** Window frames are set through `AXUIElementSetAttributeValue`,
  the same mechanism Rectangle, Amethyst and AeroSpace use.
- **Off-screen windows.** macOS won't let a window go fully off screen, so
  columns outside the viewport are parked in a bottom corner with one pixel
  showing. Columns that are partly in view are drawn at their real position
  and hang off the screen edge. The exception is an edge that touches
  another display or a pinned column: there they are parked instead, so
  nothing overlaps.
- **Private APIs.** Three read-only private calls are used:
  `_AXUIElementGetWindow` (window IDs), plus `CGSCopyManagedDisplaySpaces`
  and `CGSCopySpacesForWindows` (which Space a window is on). Nothing needs
  SIP disabled.
- **Gestures.** Touches come from a listen-only `CGEventTap` on gesture
  events, so Stripline never blocks or delays trackpad input.
- **Threading.** Everything runs on the main thread. OS callbacks only queue
  work on the main dispatch queue, so the event taps stay fast.

## Not implemented (yet)

Compared with commercial scrolling window managers, these are missing:

- on-screen overlays other than the focus border: strip indicator, dimming
  of inactive windows, clickable tab bars
- treating a native macOS tab group as one column (each tab is managed as its
  own window)
- a URL scheme and automatic updates

Contributions are welcome. The engine has no macOS dependencies, so most
layout behaviour can be developed and tested on any OS:

```sh
cargo test                                      # runs anywhere
cargo clippy --target aarch64-apple-darwin      # type-checks the macOS layer from Linux too
cargo run -- settings --standalone              # the settings page in your browser, on any OS
```

## License

MIT. See [LICENSE](LICENSE).
