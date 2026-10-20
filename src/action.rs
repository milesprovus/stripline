//! Commands shared by key bindings, the command-line client and the menu.

use std::fmt;
use std::str::FromStr;

use crate::engine::Dir;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Focus(Dir),
    FocusFirst,
    FocusLast,
    /// 1-based column number.
    FocusColumn(usize),
    Move(Dir),
    MoveFirst,
    MoveLast,
    /// 1-based column number.
    MoveToColumn(usize),
    StackLeft,
    StackRight,
    PopOutLeft,
    PopOutRight,
    ToggleTabbed,
    CycleWidth,
    CycleWidthBack,
    SetWidth(f64),
    Wider,
    Narrower,
    FullWidth,
    Center,
    Taller,
    Shorter,
    EqualizeHeights,
    PinLeft,
    PinRight,
    ToggleFloat,
    SendToNextDisplay,
    SendToPrevDisplay,
    FocusNextDisplay,
    FocusPrevDisplay,
    ToggleTiling,
    Retile,
    ReloadConfig,
    Quit,
    /// Show workspace n (1-9).
    Workspace(u8),
    /// Move the focused window to workspace n and follow it.
    MoveToWorkspace(u8),
    /// Move the focused window to workspace n and stay here.
    MoveToWorkspaceSilent(u8),
    NextWorkspace,
    PrevWorkspace,
    /// Switch the current workspace between scrolling and split layout.
    ToggleLayout,
    /// Split layout: flip the focused split between side by side and stacked.
    ToggleSplit,
}

/// Highest workspace number.
pub const MAX_WORKSPACE: u8 = 9;

const SIMPLE: &[(&str, Action)] = &[
    ("focus-left", Action::Focus(Dir::Left)),
    ("focus-right", Action::Focus(Dir::Right)),
    ("focus-up", Action::Focus(Dir::Up)),
    ("focus-down", Action::Focus(Dir::Down)),
    ("focus-first", Action::FocusFirst),
    ("focus-last", Action::FocusLast),
    ("move-left", Action::Move(Dir::Left)),
    ("move-right", Action::Move(Dir::Right)),
    ("move-up", Action::Move(Dir::Up)),
    ("move-down", Action::Move(Dir::Down)),
    ("move-first", Action::MoveFirst),
    ("move-last", Action::MoveLast),
    ("stack-left", Action::StackLeft),
    ("stack-right", Action::StackRight),
    ("pop-out-left", Action::PopOutLeft),
    ("pop-out-right", Action::PopOutRight),
    ("toggle-tabbed", Action::ToggleTabbed),
    ("cycle-width", Action::CycleWidth),
    ("cycle-width-back", Action::CycleWidthBack),
    ("wider", Action::Wider),
    ("narrower", Action::Narrower),
    ("full-width", Action::FullWidth),
    ("center", Action::Center),
    ("taller", Action::Taller),
    ("shorter", Action::Shorter),
    ("equalize-heights", Action::EqualizeHeights),
    ("pin-left", Action::PinLeft),
    ("pin-right", Action::PinRight),
    ("toggle-float", Action::ToggleFloat),
    ("send-to-next-display", Action::SendToNextDisplay),
    ("send-to-prev-display", Action::SendToPrevDisplay),
    ("focus-next-display", Action::FocusNextDisplay),
    ("focus-prev-display", Action::FocusPrevDisplay),
    ("toggle-tiling", Action::ToggleTiling),
    ("retile", Action::Retile),
    ("reload-config", Action::ReloadConfig),
    ("quit", Action::Quit),
    ("next-workspace", Action::NextWorkspace),
    ("prev-workspace", Action::PrevWorkspace),
    ("toggle-layout", Action::ToggleLayout),
    ("toggle-split", Action::ToggleSplit),
];

impl Action {
    /// Every action name, for help output.
    pub fn names() -> Vec<String> {
        let mut v: Vec<String> = SIMPLE.iter().map(|(n, _)| n.to_string()).collect();
        v.extend(
            [
                "focus-column <n>",
                "move-to-column <n>",
                "set-width <fraction>",
                "workspace <1-9>",
                "move-to-workspace <1-9>",
                "move-to-workspace-silent <1-9>",
            ]
            .map(String::from),
        );
        v
    }

    /// Actions that should run even while tiling is switched off.
    pub fn works_when_paused(&self) -> bool {
        matches!(self, Action::ToggleTiling | Action::ReloadConfig | Action::Quit)
    }
}

impl FromStr for Action {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split_whitespace();
        let name = parts.next().ok_or("empty command")?.to_ascii_lowercase();
        let arg = parts.next();
        if parts.next().is_some() {
            return Err(format!("too many arguments in '{s}'"));
        }
        let name = if name == "centre" { "center".to_string() } else { name };
        if let Some((_, a)) = SIMPLE.iter().find(|(n, _)| *n == name) {
            return match arg {
                None => Ok(*a),
                Some(_) => Err(format!("'{name}' takes no argument")),
            };
        }
        let num = |what: &str| -> Result<usize, String> {
            let a = arg.ok_or(format!("'{name}' needs {what}"))?;
            match a.parse::<usize>() {
                Ok(n) if n >= 1 => Ok(n),
                _ => Err(format!("'{a}' is not a column number (1, 2, 3, ...)")),
            }
        };
        match name.as_str() {
            "focus-column" => Ok(Action::FocusColumn(num("a column number")?)),
            "workspace" | "move-to-workspace" | "move-to-workspace-silent" => {
                let a = arg.ok_or(format!("'{name}' needs a workspace number (1-{MAX_WORKSPACE})"))?;
                let n = match a.parse::<u8>() {
                    Ok(n) if (1..=MAX_WORKSPACE).contains(&n) => n,
                    _ => return Err(format!("'{a}' is not a workspace number (1-{MAX_WORKSPACE})")),
                };
                Ok(match name.as_str() {
                    "workspace" => Action::Workspace(n),
                    "move-to-workspace" => Action::MoveToWorkspace(n),
                    _ => Action::MoveToWorkspaceSilent(n),
                })
            }
            "move-to-column" => Ok(Action::MoveToColumn(num("a column number")?)),
            "set-width" => {
                let a = arg.ok_or("'set-width' needs a fraction such as 0.5")?;
                let f: f64 = a.trim_end_matches('%').parse().map_err(|_| format!("'{a}' is not a number"))?;
                let f = if a.ends_with('%') || f > 1.0 { f / 100.0 } else { f };
                if f > 0.0 && f <= 1.0 {
                    Ok(Action::SetWidth(f))
                } else {
                    Err(format!("width '{a}' must be between 0 and 1 (or 1% and 100%)"))
                }
            }
            _ => Err(format!("unknown action '{name}'")),
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some((n, _)) = SIMPLE.iter().find(|(_, a)| a == self) {
            return f.write_str(n);
        }
        match self {
            Action::FocusColumn(n) => write!(f, "focus-column {n}"),
            Action::MoveToColumn(n) => write!(f, "move-to-column {n}"),
            Action::SetWidth(w) => write!(f, "set-width {w}"),
            Action::Workspace(n) => write!(f, "workspace {n}"),
            Action::MoveToWorkspace(n) => write!(f, "move-to-workspace {n}"),
            Action::MoveToWorkspaceSilent(n) => write!(f, "move-to-workspace-silent {n}"),
            _ => unreachable!("every simple action is in the table"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_print_round_trip() {
        for (name, a) in SIMPLE {
            assert_eq!(name.parse::<Action>().unwrap(), *a);
            assert_eq!(a.to_string(), *name);
        }
        for s in [
            "focus-column 3",
            "move-to-column 1",
            "set-width 0.5",
            "workspace 4",
            "move-to-workspace 9",
            "move-to-workspace-silent 2",
        ] {
            assert_eq!(s.parse::<Action>().unwrap().to_string(), s);
        }
    }

    #[test]
    fn arguments() {
        assert!("workspace 0".parse::<Action>().is_err());
        assert!("workspace 10".parse::<Action>().is_err());
        assert!("move-to-workspace".parse::<Action>().is_err());
        assert_eq!("Workspace 3".parse::<Action>().unwrap(), Action::Workspace(3));
        assert_eq!("set-width 50%".parse::<Action>().unwrap(), Action::SetWidth(0.5));
        assert_eq!("set-width 25".parse::<Action>().unwrap(), Action::SetWidth(0.25));
        assert_eq!("Centre".parse::<Action>().unwrap(), Action::Center);
        assert!("focus-column 0".parse::<Action>().is_err());
        assert!("focus-column".parse::<Action>().is_err());
        assert!("focus-left 2".parse::<Action>().is_err());
        assert!("set-width 150%".parse::<Action>().is_err());
        assert!("fly".parse::<Action>().is_err());
        assert!("".parse::<Action>().is_err());
    }
}
