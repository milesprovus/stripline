//! Data the settings UI works with: the action catalogue, converting between
//! "the shortcuts I want" and the overrides stored in the config file, and
//! writing a config file that only contains changed values.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::action::{Action, MAX_WORKSPACE};
use crate::config::{default_bindings, Config};
use crate::keys::KeyCombo;

#[derive(Clone, Debug, Serialize)]
pub struct CatalogEntry {
    pub group: &'static str,
    pub action: String,
    pub label: String,
    /// Which layout the action applies to: "any", "scroll" or "split".
    pub layout: &'static str,
}

/// Every bindable action, grouped and labelled for the shortcuts editor.
pub fn catalog() -> Vec<CatalogEntry> {
    let mut v = Vec::new();
    let mut add = |group, action: String, label: String, layout| v.push(CatalogEntry { group, action, label, layout });
    for n in 1..=MAX_WORKSPACE {
        add("Workspaces", format!("workspace {n}"), format!("Show workspace {n}"), "any");
    }
    for n in 1..=MAX_WORKSPACE {
        add("Workspaces", format!("move-to-workspace {n}"), format!("Move window to workspace {n}"), "any");
    }
    for n in 1..=MAX_WORKSPACE {
        add(
            "Workspaces",
            format!("move-to-workspace-silent {n}"),
            format!("Move window to workspace {n} (stay here)"),
            "any",
        );
    }
    add("Workspaces", "next-workspace".into(), "Next workspace".into(), "any");
    add("Workspaces", "prev-workspace".into(), "Previous workspace".into(), "any");

    let simple: &[(&str, &str, &str, &str)] = &[
        ("Focus", "focus-left", "Focus left", "any"),
        ("Focus", "focus-right", "Focus right", "any"),
        ("Focus", "focus-up", "Focus up", "any"),
        ("Focus", "focus-down", "Focus down", "any"),
        ("Focus", "focus-first", "Focus first window", "any"),
        ("Focus", "focus-last", "Focus last window", "any"),
        ("Focus", "focus-next-display", "Focus next display", "any"),
        ("Focus", "focus-prev-display", "Focus previous display", "any"),
        ("Move", "move-left", "Move left", "any"),
        ("Move", "move-right", "Move right", "any"),
        ("Move", "move-up", "Move up", "any"),
        ("Move", "move-down", "Move down", "any"),
        ("Move", "move-first", "Move column to the start", "scroll"),
        ("Move", "move-last", "Move column to the end", "scroll"),
        ("Move", "send-to-next-display", "Send window to next display", "any"),
        ("Move", "send-to-prev-display", "Send window to previous display", "any"),
        ("Layout", "toggle-layout", "Switch layout (split / scroll)", "any"),
        ("Layout", "toggle-split", "Flip split direction", "split"),
        ("Layout", "toggle-float", "Float / tile window", "any"),
        ("Layout", "toggle-tabbed", "Tabbed column", "scroll"),
        ("Layout", "stack-left", "Stack into left column", "scroll"),
        ("Layout", "stack-right", "Stack into right column", "scroll"),
        ("Layout", "pop-out-left", "Pop out to the left", "scroll"),
        ("Layout", "pop-out-right", "Pop out to the right", "scroll"),
        ("Layout", "pin-left", "Pin to left edge", "scroll"),
        ("Layout", "pin-right", "Pin to right edge", "scroll"),
        ("Size", "wider", "Wider", "any"),
        ("Size", "narrower", "Narrower", "any"),
        ("Size", "taller", "Taller", "any"),
        ("Size", "shorter", "Shorter", "any"),
        ("Size", "equalize-heights", "Equal sizes", "any"),
        ("Size", "cycle-width", "Cycle width presets", "scroll"),
        ("Size", "cycle-width-back", "Cycle width presets backwards", "scroll"),
        ("Size", "full-width", "Full width", "scroll"),
        ("Size", "center", "Centre column", "scroll"),
        ("Stripline", "toggle-tiling", "Pause / resume tiling", "any"),
        ("Stripline", "retile", "Re-tile all windows", "any"),
        ("Stripline", "reload-config", "Reload config", "any"),
        ("Stripline", "quit", "Quit Stripline", "any"),
    ];
    for &(group, action, label, layout) in simple {
        add(group, action.into(), label.into(), layout);
    }
    for n in 1..=9 {
        add("Focus", format!("focus-column {n}"), format!("Focus column / window {n}"), "any");
    }
    for n in 1..=9 {
        add("Move", format!("move-to-column {n}"), format!("Move column to position {n}"), "scroll");
    }
    v
}

pub fn normalize_combo(combo: &str) -> Result<String, String> {
    KeyCombo::parse(combo).map(|k| k.to_string())
}

/// Bindings in effect for a config, as (combo, action) strings, sorted.
pub fn effective(cfg: &Config) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = cfg.bindings().0.iter().map(|(k, a)| (k.to_string(), a.to_string())).collect();
    v.sort();
    v
}

/// Turn the full set of wanted bindings into the `[bindings]` table: only
/// differences from the defaults ("none" unbinds a default).
pub fn overrides(wanted: &[(String, String)], replace_defaults: bool) -> Result<BTreeMap<String, String>, String> {
    let mut want: BTreeMap<String, String> = BTreeMap::new();
    for (combo, action) in wanted {
        let c = normalize_combo(combo)?;
        let a = action.parse::<Action>().map_err(|e| format!("{combo}: {e}"))?.to_string();
        if let Some(prev) = want.insert(c.clone(), a.clone()) {
            if prev != a {
                return Err(format!("{c} is bound to both '{prev}' and '{a}'"));
            }
        }
    }
    if replace_defaults {
        return Ok(want);
    }
    let defaults: BTreeMap<String, String> = default_bindings()
        .into_iter()
        .filter_map(|(c, a)| Some((normalize_combo(&c).ok()?, a.parse::<Action>().ok()?.to_string())))
        .collect();
    let mut out = BTreeMap::new();
    for (c, a) in &defaults {
        match want.get(c) {
            Some(w) if w == a => {}
            Some(w) => {
                out.insert(c.clone(), w.clone());
            }
            None => {
                out.insert(c.clone(), "none".into());
            }
        }
    }
    for (c, a) in &want {
        if !defaults.contains_key(c) {
            out.insert(c.clone(), a.clone());
        }
    }
    Ok(out)
}

pub const HEADER: &str = "\
# Stripline configuration, written by Stripline Settings.
# Only values that differ from the defaults are listed. You can still edit
# this file by hand; the settings window keeps your values but not comments.
# All options: https://github.com/milesprovus/stripline/blob/main/config.example.toml

";

fn strip_defaults(value: &mut toml::Value, default: &toml::Value) {
    let (toml::Value::Table(t), toml::Value::Table(d)) = (value, default) else { return };
    t.retain(|k, v| match d.get(k) {
        Some(dv) if v == dv => false,
        Some(dv) => {
            strip_defaults(v, dv);
            !matches!(v, toml::Value::Table(t) if t.is_empty())
        }
        None => !matches!(v, toml::Value::Table(t) if t.is_empty()),
    });
}

/// TOML text for a config, leaving out everything that equals the default.
pub fn to_toml(cfg: &Config) -> Result<String, String> {
    let mut v = toml::Value::try_from(cfg).map_err(|e| e.to_string())?;
    let d = toml::Value::try_from(Config::default()).map_err(|e| e.to_string())?;
    strip_defaults(&mut v, &d);
    let body = toml::to_string_pretty(&v).map_err(|e| e.to_string())?;
    let text = format!("{HEADER}{body}");
    // Never write something that would not load back identically.
    let back = Config::parse(&text).map_err(|e| format!("internal error, generated config does not load: {e}"))?;
    if back != *cfg {
        return Err("internal error: generated config does not round-trip".into());
    }
    Ok(text)
}

/// `KeyboardEvent.code` of every character key, with the character it types
/// on a US layout. The macOS daemon translates these through the active
/// keyboard layout so recorded shortcuts match what the user sees.
pub const CHAR_KEY_CODES: &[(&str, char)] = &[
    ("KeyA", 'a'),
    ("KeyB", 'b'),
    ("KeyC", 'c'),
    ("KeyD", 'd'),
    ("KeyE", 'e'),
    ("KeyF", 'f'),
    ("KeyG", 'g'),
    ("KeyH", 'h'),
    ("KeyI", 'i'),
    ("KeyJ", 'j'),
    ("KeyK", 'k'),
    ("KeyL", 'l'),
    ("KeyM", 'm'),
    ("KeyN", 'n'),
    ("KeyO", 'o'),
    ("KeyP", 'p'),
    ("KeyQ", 'q'),
    ("KeyR", 'r'),
    ("KeyS", 's'),
    ("KeyT", 't'),
    ("KeyU", 'u'),
    ("KeyV", 'v'),
    ("KeyW", 'w'),
    ("KeyX", 'x'),
    ("KeyY", 'y'),
    ("KeyZ", 'z'),
    ("Digit0", '0'),
    ("Digit1", '1'),
    ("Digit2", '2'),
    ("Digit3", '3'),
    ("Digit4", '4'),
    ("Digit5", '5'),
    ("Digit6", '6'),
    ("Digit7", '7'),
    ("Digit8", '8'),
    ("Digit9", '9'),
    ("Minus", '-'),
    ("Equal", '='),
    ("BracketLeft", '['),
    ("BracketRight", ']'),
    ("Backslash", '\\'),
    ("Semicolon", ';'),
    ("Quote", '\''),
    ("Comma", ','),
    ("Period", '.'),
    ("Slash", '/'),
    ("Backquote", '`'),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_actions_all_parse_and_cover_defaults() {
        let cat = catalog();
        for e in &cat {
            assert!(e.action.parse::<Action>().is_ok(), "{}", e.action);
        }
        for (_, a) in default_bindings() {
            let a = a.parse::<Action>().unwrap().to_string();
            assert!(cat.iter().any(|e| e.action == a), "default action {a} missing from catalogue");
        }
    }

    #[test]
    fn overrides_round_trip_through_config() {
        let mut want = effective(&Config::default());
        // Unbind focus-left on ctrl+alt+h, rebind it to hyper+h, add a new one.
        want.retain(|(c, _)| c != "ctrl+alt+h" && c != "ctrl+alt+=");
        want.push(("hyper+h".into(), "focus-left".into()));
        want.push(("ctrl+alt+=".into(), "cycle-width".into())); // was "wider"
        let o = overrides(&want, false).unwrap();
        assert_eq!(o.get("ctrl+alt+h").map(String::as_str), Some("none"));
        assert_eq!(o.get("ctrl+alt+shift+cmd+h").map(String::as_str), Some("focus-left"));
        assert_eq!(o.get("ctrl+alt+=").map(String::as_str), Some("cycle-width"));
        assert_eq!(o.len(), 3, "{o:?}");
        let cfg = Config { bindings: o, ..Config::default() };
        let mut got = effective(&cfg);
        let mut want_n: Vec<(String, String)> =
            want.iter().map(|(c, a)| (normalize_combo(c).unwrap(), a.clone())).collect();
        got.sort();
        want_n.sort();
        assert_eq!(got, want_n);
    }

    #[test]
    fn overrides_reject_conflicts_and_garbage() {
        let both = vec![("ctrl+a".into(), "quit".into()), ("ctrl+a".into(), "retile".into())];
        assert!(overrides(&both, false).is_err());
        assert!(overrides(&[("ctrl+a".into(), "explode".into())], false).is_err());
        assert!(overrides(&[("ctrl++".into(), "quit".into())], false).is_err());
        let only = overrides(&[("f13".into(), "retile".into())], true).unwrap();
        assert_eq!(only.len(), 1);
    }

    #[test]
    fn defaults_write_an_empty_file() {
        let text = to_toml(&Config::default()).unwrap();
        assert_eq!(text, HEADER);
    }

    #[test]
    fn only_changes_are_written_and_they_load_back() {
        let mut cfg = Config::default();
        cfg.layout.gap = 12.0;
        cfg.layout.outer_gap = crate::config::OuterGap::Sides { top: 4.0, right: 8.0, bottom: 8.0, left: 8.0 };
        cfg.border.color = "#FF000080".into();
        cfg.border.glow = 6.0;
        cfg.displays.disabled = vec!["DELL U2720Q".into()];
        cfg.rules.push(crate::config::Rule {
            app: Some("com.apple.calculator".into()),
            float: Some(true),
            ..Default::default()
        });
        cfg.bindings.insert("ctrl+alt+h".into(), "none".into());
        let text = to_toml(&cfg).unwrap();
        assert!(text.contains("gap = 12"));
        assert!(!text.contains("default_width"), "unchanged values are left out:\n{text}");
        assert!(!text.contains("[animation]"));
        assert!(text.contains("[[rules]]"));
        assert_eq!(Config::parse(&text).unwrap(), cfg);
    }

    #[test]
    fn char_key_codes_have_ansi_positions() {
        for (code, c) in CHAR_KEY_CODES {
            assert!(crate::keys::ansi_code(*c).is_some(), "{code}");
        }
    }
}
