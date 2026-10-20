//! User configuration, read from `~/.config/stripline/config.toml`.
//! Every field has a default, so an empty or missing file is valid.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::action::Action;
use crate::engine::{CenterMode, OpenAt};
use crate::keys::KeyCombo;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Start from an empty set of bindings instead of the defaults.
    pub replace_default_bindings: bool,
    pub layout: LayoutConfig,
    pub animation: AnimationConfig,
    pub gestures: GestureConfig,
    pub focus: FocusConfig,
    pub displays: DisplayConfig,
    pub rules: Vec<Rule>,
    /// Key combination -> action. Map a combination to "none" to unbind it.
    pub bindings: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LayoutConfig {
    /// Space between columns and between stacked windows, in points.
    pub gap: f64,
    /// Space between windows and the screen edges.
    pub outer_gap: OuterGap,
    /// Width of new columns, as a fraction of the screen.
    pub default_width: f64,
    /// Widths visited by cycle-width.
    pub width_presets: Vec<f64>,
    /// Step for wider/narrower.
    pub width_step: f64,
    /// Step for taller/shorter.
    pub height_step: f64,
    pub center: CenterMode,
    /// Where new windows open.
    pub open_at: OpenAt,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OuterGap {
    All(f64),
    Sides { top: f64, right: f64, bottom: f64, left: f64 },
}

impl OuterGap {
    /// (top, right, bottom, left)
    pub fn sides(&self) -> (f64, f64, f64, f64) {
        match *self {
            OuterGap::All(g) => (g, g, g, g),
            OuterGap::Sides { top, right, bottom, left } => (top, right, bottom, left),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AnimationConfig {
    pub enabled: bool,
    /// Spring speed; higher settles faster. ~6/speed seconds to settle.
    pub speed: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GestureConfig {
    pub enabled: bool,
    /// Number of fingers for the horizontal swipe (3 or 4).
    pub fingers: u8,
    pub sensitivity: f64,
    /// Content follows the fingers (like natural scrolling).
    pub natural: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FocusConfig {
    pub follows_mouse: bool,
    pub mouse_follows_focus: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DisplayConfig {
    /// Displays left alone, by name ("DELL U2720Q") or UUID. See
    /// `stripline msg displays`.
    pub disabled: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Rule {
    /// Bundle identifier or app name, compared case-insensitively.
    pub app: Option<String>,
    /// Regular expression matched against the window title.
    pub title: Option<String>,
    pub float: Option<bool>,
    pub width: Option<f64>,
    pub open_at: Option<OpenAt>,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        LayoutConfig {
            gap: 8.0,
            outer_gap: OuterGap::All(8.0),
            default_width: 0.5,
            width_presets: vec![1.0 / 3.0, 0.5, 2.0 / 3.0, 0.75],
            width_step: 0.1,
            height_step: 0.1,
            center: CenterMode::Never,
            open_at: OpenAt::Next,
        }
    }
}

impl Default for AnimationConfig {
    fn default() -> Self {
        AnimationConfig { enabled: true, speed: 22.0 }
    }
}

impl Default for GestureConfig {
    fn default() -> Self {
        GestureConfig { enabled: true, fingers: 3, sensitivity: 1.0, natural: true }
    }
}

/// The built-in key bindings. Control+Option is the base so that plain
/// Option keeps typing special characters.
pub fn default_bindings() -> Vec<(String, String)> {
    let mut b: Vec<(String, String)> = [
        ("ctrl+alt+h", "focus-left"),
        ("ctrl+alt+left", "focus-left"),
        ("ctrl+alt+l", "focus-right"),
        ("ctrl+alt+right", "focus-right"),
        ("ctrl+alt+k", "focus-up"),
        ("ctrl+alt+up", "focus-up"),
        ("ctrl+alt+j", "focus-down"),
        ("ctrl+alt+down", "focus-down"),
        ("ctrl+alt+shift+h", "move-left"),
        ("ctrl+alt+shift+left", "move-left"),
        ("ctrl+alt+shift+l", "move-right"),
        ("ctrl+alt+shift+right", "move-right"),
        ("ctrl+alt+shift+k", "move-up"),
        ("ctrl+alt+shift+up", "move-up"),
        ("ctrl+alt+shift+j", "move-down"),
        ("ctrl+alt+shift+down", "move-down"),
        ("ctrl+alt+cmd+left", "focus-first"),
        ("ctrl+alt+cmd+right", "focus-last"),
        ("ctrl+alt+cmd+shift+left", "move-first"),
        ("ctrl+alt+cmd+shift+right", "move-last"),
        ("ctrl+alt+r", "cycle-width"),
        ("ctrl+alt+shift+r", "cycle-width-back"),
        ("ctrl+alt+f", "full-width"),
        ("ctrl+alt+c", "center"),
        ("ctrl+alt+equal", "wider"),
        ("ctrl+alt+minus", "narrower"),
        ("ctrl+alt+shift+equal", "taller"),
        ("ctrl+alt+shift+minus", "shorter"),
        ("ctrl+alt+0", "equalize-heights"),
        ("ctrl+alt+leftbracket", "stack-left"),
        ("ctrl+alt+rightbracket", "stack-right"),
        ("ctrl+alt+shift+leftbracket", "pop-out-left"),
        ("ctrl+alt+shift+rightbracket", "pop-out-right"),
        ("ctrl+alt+t", "toggle-tabbed"),
        ("ctrl+alt+shift+f", "toggle-float"),
        ("ctrl+alt+p", "pin-left"),
        ("ctrl+alt+shift+p", "pin-right"),
        ("ctrl+alt+n", "send-to-next-display"),
        ("ctrl+alt+shift+n", "send-to-prev-display"),
        ("ctrl+alt+o", "focus-next-display"),
        ("ctrl+alt+shift+o", "focus-prev-display"),
        ("ctrl+alt+q", "toggle-tiling"),
    ]
    .iter()
    .map(|(k, a)| (k.to_string(), a.to_string()))
    .collect();
    for n in 1..=9 {
        b.push((format!("ctrl+alt+{n}"), format!("focus-column {n}")));
        b.push((format!("ctrl+alt+shift+{n}"), format!("move-to-column {n}")));
    }
    b
}

pub fn config_dir() -> PathBuf {
    if let Some(x) = std::env::var_os("XDG_CONFIG_HOME").filter(|x| !x.is_empty()) {
        return PathBuf::from(x).join("stripline");
    }
    home().join(".config").join("stripline")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

/// Where the session file lives.
pub fn data_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        home().join("Library/Application Support/stripline")
    } else {
        home().join(".local/share/stripline")
    }
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

impl Config {
    pub fn load(path: &Path) -> Result<Config, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Config, String> {
        let cfg: Config = toml::from_str(text).map_err(|e| e.to_string())?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<(), String> {
        let l = &self.layout;
        let frac = |v: f64| v > 0.0 && v <= 1.0;
        if l.gap < 0.0 {
            return Err("layout.gap must not be negative".into());
        }
        let (t, r, b, lf) = l.outer_gap.sides();
        if [t, r, b, lf].iter().any(|&g| g < 0.0) {
            return Err("layout.outer_gap must not be negative".into());
        }
        if !frac(l.default_width) {
            return Err("layout.default_width must be in (0, 1]".into());
        }
        if l.width_presets.is_empty() || !l.width_presets.iter().all(|&p| frac(p)) {
            return Err("layout.width_presets must be a non-empty list of fractions in (0, 1]".into());
        }
        if !(frac(l.width_step) && frac(l.height_step)) {
            return Err("layout.width_step and layout.height_step must be in (0, 1]".into());
        }
        if self.animation.speed <= 0.0 {
            return Err("animation.speed must be positive".into());
        }
        if !(3..=4).contains(&self.gestures.fingers) {
            return Err("gestures.fingers must be 3 or 4".into());
        }
        if self.gestures.sensitivity <= 0.0 {
            return Err("gestures.sensitivity must be positive".into());
        }
        for (i, r) in self.rules.iter().enumerate() {
            if r.app.is_none() && r.title.is_none() {
                return Err(format!("rules[{i}] needs 'app' and/or 'title'"));
            }
            if let Some(t) = &r.title {
                Regex::new(t).map_err(|e| format!("rules[{i}].title: {e}"))?;
            }
            if r.width.is_some_and(|w| !frac(w)) {
                return Err(format!("rules[{i}].width must be in (0, 1]"));
            }
        }
        Ok(())
    }

    /// Effective key bindings plus warnings for entries that did not parse.
    pub fn bindings(&self) -> (Vec<(KeyCombo, Action)>, Vec<String>) {
        let mut warnings = Vec::new();
        let mut map: Vec<(KeyCombo, Action)> = Vec::new();
        let mut set = |combo: &str, action: &str, warnings: &mut Vec<String>| {
            let k = match KeyCombo::parse(combo) {
                Ok(k) => k,
                Err(e) => return warnings.push(e),
            };
            map.retain(|(c, _)| *c != k);
            if matches!(action.trim(), "" | "none") {
                return;
            }
            match action.parse::<Action>() {
                Ok(a) => map.push((k, a)),
                Err(e) => warnings.push(format!("binding '{combo}': {e}")),
            }
        };
        if !self.replace_default_bindings {
            for (k, a) in default_bindings() {
                set(&k, &a, &mut warnings);
            }
        }
        for (k, a) in &self.bindings {
            set(k, a, &mut warnings);
        }
        (map, warnings)
    }
}

/// Rules with their title patterns compiled.
#[derive(Clone, Debug, Default)]
pub struct Rules(Vec<(Rule, Option<Regex>)>);

/// Combined effect of all rules matching a window.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RuleEffect {
    pub float: Option<bool>,
    pub width: Option<f64>,
    pub open_at: Option<OpenAt>,
}

impl Rules {
    pub fn new(rules: &[Rule]) -> Rules {
        Rules(rules.iter().map(|r| (r.clone(), r.title.as_deref().and_then(|t| Regex::new(t).ok()))).collect())
    }

    /// Later rules override earlier ones field by field.
    pub fn effect(&self, bundle: &str, app_name: &str, title: &str) -> RuleEffect {
        let mut e = RuleEffect::default();
        for (rule, re) in &self.0 {
            let app_ok =
                rule.app.as_deref().is_none_or(|a| a.eq_ignore_ascii_case(bundle) || a.eq_ignore_ascii_case(app_name));
            let title_ok = re.as_ref().is_none_or(|re| re.is_match(title));
            if app_ok && title_ok {
                e.float = rule.float.or(e.float);
                e.width = rule.width.or(e.width);
                e.open_at = rule.open_at.or(e.open_at);
            }
        }
        e
    }
}

/// The commented example written by `stripline init-config`.
pub const EXAMPLE: &str = include_str!("../config.example.toml");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_is_default() {
        assert_eq!(Config::parse("").unwrap(), Config::default());
    }

    #[test]
    fn example_config_parses() {
        let cfg = Config::parse(EXAMPLE).unwrap();
        let (_, warnings) = cfg.bindings();
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn default_bindings_all_parse_and_are_unique() {
        let (b, warnings) = Config::default().bindings();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(b.len(), default_bindings().len());
    }

    #[test]
    fn user_bindings_override_and_unbind() {
        let cfg = Config::parse(
            r#"
            [bindings]
            "ctrl+alt+h" = "none"
            "cmd+alt+h" = "focus-left"
            "ctrl+alt+l" = "focus-first"
            "ctrl+alt+x" = "explode"
            "#,
        )
        .unwrap();
        let (b, warnings) = cfg.bindings();
        let find = |s: &str| {
            let k = KeyCombo::parse(s).unwrap();
            b.iter().find(|(c, _)| *c == k).map(|(_, a)| *a)
        };
        assert_eq!(find("ctrl+alt+h"), None);
        assert_eq!(find("cmd+alt+h"), Some(Action::Focus(crate::engine::Dir::Left)));
        assert_eq!(find("ctrl+alt+l"), Some(Action::FocusFirst));
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn replace_defaults() {
        let cfg = Config::parse("replace_default_bindings = true\n[bindings]\n\"f13\" = \"cycle-width\"").unwrap();
        assert_eq!(cfg.bindings().0.len(), 1);
    }

    #[test]
    fn outer_gap_forms() {
        let c = Config::parse("[layout]\nouter_gap = { top = 4, right = 0, bottom = 8, left = 0 }").unwrap();
        assert_eq!(c.layout.outer_gap.sides(), (4.0, 0.0, 8.0, 0.0));
        let c = Config::parse("[layout]\nouter_gap = 12").unwrap();
        assert_eq!(c.layout.outer_gap.sides(), (12.0, 12.0, 12.0, 12.0));
    }

    #[test]
    fn validation_errors() {
        assert!(Config::parse("[layout]\ndefault_width = 1.5").is_err());
        assert!(Config::parse("[gestures]\nfingers = 2").is_err());
        assert!(Config::parse("[[rules]]\nfloat = true").is_err());
        assert!(Config::parse("[[rules]]\ntitle = \"(\"").is_err());
        assert!(Config::parse("[layout]\ngapp = 3").is_err(), "unknown keys are rejected");
    }

    #[test]
    fn rules_combine() {
        let c = Config::parse(
            r#"
            [[rules]]
            app = "com.apple.finder"
            width = 0.4
            [[rules]]
            app = "Finder"
            title = "^Copy"
            float = true
            "#,
        )
        .unwrap();
        let r = Rules::new(&c.rules);
        let e = r.effect("com.apple.finder", "Finder", "Copy 3 items");
        assert_eq!(e, RuleEffect { float: Some(true), width: Some(0.4), open_at: None });
        let e = r.effect("com.apple.finder", "Finder", "Downloads");
        assert_eq!(e.float, None);
        assert_eq!(r.effect("com.apple.Safari", "Safari", "x"), RuleEffect::default());
    }
}
