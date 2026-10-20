# Stripline

A scrolling tiling window manager for macOS, written in Rust. MIT licensed.

Each window is a column on a horizontal strip that can be wider than the
screen. Opening a window adds a column next to the focused one instead of
shrinking everything else. You move along the strip with the keyboard or a
trackpad swipe. It's the layout model of [PaperWM](https://github.com/paperwm/PaperWM)
and [niri](https://github.com/YaLTeR/niri), built on the public macOS
Accessibility API, so SIP can stay on.

```
        ┌──────────── screen ────────────┐
 ◀ ░░░░ │ ┌──────────┐ ┌───────────────┐ │ ┌──────┐ ┌──────────┐ ▶
        │ │ Terminal │ │    Editor     │ │ │ Mail │ │ Browser  │
        │ │          │ ├───────────────┤ │ │      │ │          │
        │ │          │ │     Logs      │ │ │      │ │          │
        │ └──────────┘ └───────────────┘ │ └──────┘ └──────────┘
        └────────────────────────────────┘
          columns keep their width; the strip scrolls
```

> **Status:** early (0.1). The layout engine is covered by unit tests. The
> macOS layer compiles for Apple silicon and Intel, and CI builds it on a
> macOS runner, but it hasn't been used day-to-day yet. Expect rough edges
> and please file issues.

## Features

| | |
|---|---|
| Scrolling strip | One column per window. Columns keep their width, and the strip scrolls just far enough to show the focused one. |
| Stacks and tabs | Move windows into neighbouring columns or pop them back out. Columns can be stacked (split height) or tabbed (one visible at a time). |
| Widths | Cycle through presets (⅓, ½, ⅔, ¾ by default), step wider/narrower, toggle full width, centre a column. |
| Heights | Taller, shorter, or equal heights inside a stack. |
| Pinned columns | Keep one column fixed at the left or right edge while the rest scrolls. |
| Centring modes | `never` (scroll just enough), `on-overflow`, `always`. |
| Trackpad swipe | A 3- or 4-finger swipe moves the strip 1:1 with your fingers, then springs to the nearest column. |
| Spring animation | Critically damped scroll animation that you can retarget mid-flight. |
| Multiple displays | One strip per display and per Space. Windows never hang over onto a neighbouring display. Send windows between displays. Exclude specific displays. |
| Spaces | Every Space keeps its own strip. Native full-screen windows are ignored. |
| Smart floating | Dialogs, panels and fixed-size windows float automatically. You can float or unfloat any window. |
| Rules | Per app (bundle ID or name) and title regex: float, initial width, or where new windows open. |
| No focus stealing | Windows that open in background apps are added to the strip without taking focus. |
| Mouse | Drag a window onto another column to reorder or stack. Drag its edge to change the column width. Optional focus-follows-mouse and mouse-follows-focus. |
| Session restore | Column order, widths, stacks and pins survive a restart. Windows are matched by ID, then by app and title. |
| Live config | TOML file, reloaded automatically when you save it. Shortcuts follow your keyboard layout (QWERTZ, AZERTY, Dvorak, ...). |
| Automation | `stripline msg <action>` over a Unix socket, plus a JSON `state` dump. |
| Menu bar item | Pause or resume, re-tile, open config, quit. |
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

All shortcuts use **⌃⌥** (Control+Option). Plain ⌥ is left alone so it can
still type special characters.

| Keys | Action |
|---|---|
| ⌃⌥ H / J / K / L, or arrows | focus left / down / up / right |
| ⌃⌥⇧ + the same | move column left/right, move window up/down in a stack |
| ⌃⌥⌘ ← / → | focus first / last column (add ⇧ to move) |
| ⌃⌥ 1 … 9 | focus column *n* (add ⇧ to move the column there) |
| ⌃⌥ R | cycle width presets (⇧ goes backwards) |
| ⌃⌥ F | full width (toggle) |
| ⌃⌥ C | centre the focused column |
| ⌃⌥ = / - | wider / narrower |
| ⌃⌥⇧ = / - | taller / shorter |
| ⌃⌥ 0 | equalize heights in the stack |
| ⌃⌥ [ / ] | stack into the left / right column |
| ⌃⌥⇧ [ / ] | pop out to the left / right |
| ⌃⌥ T | toggle tabbed column |
| ⌃⌥⇧ F | float / unfloat |
| ⌃⌥ P / ⌃⌥⇧ P | pin to left / right edge (toggle) |
| ⌃⌥ N / ⌃⌥⇧ N | send window to next / previous display |
| ⌃⌥ O / ⌃⌥⇧ O | focus next / previous display |
| ⌃⌥ Q | pause / resume tiling |

`stripline bindings` prints the bindings currently in effect. To change them,
add entries to the `[bindings]` table of the config file.

## Configuration

`~/.config/stripline/config.toml`. Run `stripline init-config` to write a
commented copy of [`config.example.toml`](config.example.toml). Every key is
optional. The file is re-read within two seconds of being saved, and
`stripline check-config` validates it.

```toml
[layout]
gap = 8
default_width = 0.5
width_presets = [0.333, 0.5, 0.667, 0.75]
center = "on-overflow"

[gestures]
fingers = 3

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
stripline msg state           JSON dump of strips, columns and windows
stripline msg displays        display names and UUIDs (for [displays].disabled)
stripline msg pause|resume|reload|quit
```

Every keyboard action is also a `msg` command, so it's easy to call from
Raycast, Alfred, skhd, BetterTouchTool or shell scripts.

## How it works

```
 src/engine/      platform-independent, unit tested
   strip.rs       columns, stacks, focus, pins, every strip operation
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

- a settings window (configuration is the TOML file)
- on-screen overlays: strip indicator, focus border, dimming of inactive
  windows, clickable tab bars
- treating a native macOS tab group as one column (each tab is managed as its
  own window)
- a URL scheme and automatic updates

Contributions are welcome. The engine has no macOS dependencies, so most
layout behaviour can be developed and tested on any OS:

```sh
cargo test                                      # runs anywhere
cargo clippy --target aarch64-apple-darwin      # type-checks the macOS layer from Linux too
```

## License

MIT. See [LICENSE](LICENSE).
