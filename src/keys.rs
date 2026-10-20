//! Key-combination parsing. Letters and punctuation are kept as characters
//! and resolved against the active keyboard layout at runtime, so a binding
//! for "ctrl+alt+z" follows the Z key on QWERTZ and AZERTY too. Named keys
//! (arrows, F-keys, ...) map straight to macOS virtual key codes.

use std::fmt;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Mods(pub u8);

impl Mods {
    pub const CTRL: Mods = Mods(1);
    pub const ALT: Mods = Mods(2);
    pub const SHIFT: Mods = Mods(4);
    pub const CMD: Mods = Mods(8);

    pub fn contains(self, other: Mods) -> bool {
        self.0 & other.0 == other.0
    }

    /// Convert `CGEventFlags`, ignoring caps lock, fn and keypad bits.
    pub fn from_cg_flags(flags: u64) -> Mods {
        let mut m = 0;
        if flags & 0x0004_0000 != 0 {
            m |= Mods::CTRL.0;
        }
        if flags & 0x0008_0000 != 0 {
            m |= Mods::ALT.0;
        }
        if flags & 0x0002_0000 != 0 {
            m |= Mods::SHIFT.0;
        }
        if flags & 0x0010_0000 != 0 {
            m |= Mods::CMD.0;
        }
        Mods(m)
    }
}

impl std::ops::BitOr for Mods {
    type Output = Mods;
    fn bitor(self, rhs: Mods) -> Mods {
        Mods(self.0 | rhs.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// A macOS virtual key code (`kVK_*`).
    Code(u16),
    /// A character, looked up on the current keyboard layout.
    Char(char),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyCombo {
    pub mods: Mods,
    pub key: Key,
}

const NAMED: &[(&str, u16)] = &[
    ("return", 0x24),
    ("enter", 0x24),
    ("tab", 0x30),
    ("space", 0x31),
    ("delete", 0x33),
    ("backspace", 0x33),
    ("escape", 0x35),
    ("esc", 0x35),
    ("forwarddelete", 0x75),
    ("home", 0x73),
    ("end", 0x77),
    ("pageup", 0x74),
    ("pagedown", 0x79),
    ("left", 0x7B),
    ("right", 0x7C),
    ("down", 0x7D),
    ("up", 0x7E),
    ("f1", 0x7A),
    ("f2", 0x78),
    ("f3", 0x63),
    ("f4", 0x76),
    ("f5", 0x60),
    ("f6", 0x61),
    ("f7", 0x62),
    ("f8", 0x64),
    ("f9", 0x65),
    ("f10", 0x6D),
    ("f11", 0x67),
    ("f12", 0x6F),
    ("f13", 0x69),
    ("f14", 0x6B),
    ("f15", 0x71),
    ("f16", 0x6A),
    ("f17", 0x40),
    ("f18", 0x4F),
    ("f19", 0x50),
    ("f20", 0x5A),
];

const PUNCT_NAMES: &[(&str, char)] = &[
    ("minus", '-'),
    ("equal", '='),
    ("plus", '='),
    ("leftbracket", '['),
    ("rightbracket", ']'),
    ("semicolon", ';'),
    ("quote", '\''),
    ("comma", ','),
    ("period", '.'),
    ("slash", '/'),
    ("backslash", '\\'),
    ("grave", '`'),
    ("backtick", '`'),
];

/// Virtual key code of a character's key on a US ANSI keyboard. Used when the
/// active layout cannot produce the character without modifiers.
pub fn ansi_code(c: char) -> Option<u16> {
    let code = match c.to_ascii_lowercase() {
        'a' => 0x00,
        's' => 0x01,
        'd' => 0x02,
        'f' => 0x03,
        'h' => 0x04,
        'g' => 0x05,
        'z' => 0x06,
        'x' => 0x07,
        'c' => 0x08,
        'v' => 0x09,
        'b' => 0x0B,
        'q' => 0x0C,
        'w' => 0x0D,
        'e' => 0x0E,
        'r' => 0x0F,
        'y' => 0x10,
        't' => 0x11,
        '1' => 0x12,
        '2' => 0x13,
        '3' => 0x14,
        '4' => 0x15,
        '6' => 0x16,
        '5' => 0x17,
        '=' => 0x18,
        '9' => 0x19,
        '7' => 0x1A,
        '-' => 0x1B,
        '8' => 0x1C,
        '0' => 0x1D,
        ']' => 0x1E,
        'o' => 0x1F,
        'u' => 0x20,
        '[' => 0x21,
        'i' => 0x22,
        'p' => 0x23,
        'l' => 0x25,
        'j' => 0x26,
        '\'' => 0x27,
        'k' => 0x28,
        ';' => 0x29,
        '\\' => 0x2A,
        ',' => 0x2B,
        '/' => 0x2C,
        'n' => 0x2D,
        'm' => 0x2E,
        '.' => 0x2F,
        '`' => 0x32,
        _ => return None,
    };
    Some(code)
}

impl KeyCombo {
    pub fn parse(s: &str) -> Result<KeyCombo, String> {
        let s = s.trim();
        if s.is_empty() {
            return Err("empty key combination".into());
        }
        let parts: Vec<&str> = s.split('+').map(str::trim).collect();
        let (key_part, mod_parts) = parts.split_last().unwrap();
        let mut mods = Mods::default();
        for m in mod_parts {
            mods = mods
                | match m.to_ascii_lowercase().as_str() {
                    "ctrl" | "control" | "⌃" => Mods::CTRL,
                    "alt" | "opt" | "option" | "⌥" => Mods::ALT,
                    "shift" | "⇧" => Mods::SHIFT,
                    "cmd" | "command" | "super" | "⌘" => Mods::CMD,
                    "meh" => Mods::CTRL | Mods::ALT | Mods::SHIFT,
                    "hyper" => Mods::CTRL | Mods::ALT | Mods::SHIFT | Mods::CMD,
                    "" => return Err(format!("'{s}': stray '+' (write 'equal' or 'plus' for that key)")),
                    other => return Err(format!("'{s}': unknown modifier '{other}'")),
                };
        }
        let lower = key_part.to_ascii_lowercase();
        let key = if let Some((_, code)) = NAMED.iter().find(|(n, _)| *n == lower) {
            Key::Code(*code)
        } else if let Some((_, c)) = PUNCT_NAMES.iter().find(|(n, _)| *n == lower) {
            Key::Char(*c)
        } else {
            let mut chars = key_part.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Key::Char(c.to_lowercase().next().unwrap_or(c)),
                _ => return Err(format!("'{s}': unknown key '{key_part}'")),
            }
        };
        Ok(KeyCombo { mods, key })
    }

    /// Resolve to a virtual key code, using `layout` to find characters.
    pub fn resolve(&self, layout: &dyn Fn(char) -> Option<u16>) -> Option<(u16, Mods)> {
        let code = match self.key {
            Key::Code(c) => Some(c),
            Key::Char(c) => layout(c).or_else(|| ansi_code(c)),
        }?;
        Some((code, self.mods))
    }
}

impl fmt::Display for KeyCombo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (m, name) in [(Mods::CTRL, "ctrl"), (Mods::ALT, "alt"), (Mods::SHIFT, "shift"), (Mods::CMD, "cmd")] {
            if self.mods.contains(m) {
                write!(f, "{name}+")?;
            }
        }
        match self.key {
            Key::Char(c) => write!(f, "{c}"),
            Key::Code(code) => match NAMED.iter().find(|(_, c)| *c == code) {
                Some((n, _)) => f.write_str(n),
                None => write!(f, "keycode-{code}"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_combos() {
        let k = KeyCombo::parse("ctrl+alt+shift+H").unwrap();
        assert_eq!(k.mods, Mods::CTRL | Mods::ALT | Mods::SHIFT);
        assert_eq!(k.key, Key::Char('h'));
        let k = KeyCombo::parse("⌃+⌥+left").unwrap();
        assert_eq!((k.mods, k.key), (Mods::CTRL | Mods::ALT, Key::Code(0x7B)));
        assert_eq!(KeyCombo::parse("hyper+-").unwrap().key, Key::Char('-'));
        assert_eq!(KeyCombo::parse("ctrl+alt+equal").unwrap().key, Key::Char('='));
        assert_eq!(KeyCombo::parse("cmd+F12").unwrap().key, Key::Code(0x6F));
        assert!(KeyCombo::parse("ctrl++").is_err());
        assert!(KeyCombo::parse("ctrl+alt+banana").is_err());
        assert!(KeyCombo::parse("win+a").is_err());
    }

    #[test]
    fn display_round_trip() {
        for s in ["ctrl+alt+h", "ctrl+alt+shift+left", "cmd+f5", "alt+["] {
            assert_eq!(KeyCombo::parse(s).unwrap().to_string(), s);
        }
    }

    #[test]
    fn resolves_through_layout_then_ansi() {
        // Fake AZERTY: 'a' is where US 'q' is.
        let azerty = |c: char| match c {
            'a' => Some(0x0C),
            'q' => Some(0x00),
            _ => None,
        };
        let a = KeyCombo::parse("ctrl+a").unwrap();
        assert_eq!(a.resolve(&azerty), Some((0x0C, Mods::CTRL)));
        // '[' is not on the fake layout: fall back to its US position.
        let b = KeyCombo::parse("ctrl+[").unwrap();
        assert_eq!(b.resolve(&azerty), Some((0x21, Mods::CTRL)));
    }

    #[test]
    fn cg_flags() {
        // control | option | caps lock | numeric pad | fn
        let flags = 0x0004_0000 | 0x0008_0000 | 0x0001_0000 | 0x0020_0000 | 0x0080_0000;
        assert_eq!(Mods::from_cg_flags(flags), Mods::CTRL | Mods::ALT);
    }
}
