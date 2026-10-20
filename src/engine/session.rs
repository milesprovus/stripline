//! Saving the layout and rebuilding it after a restart.
//!
//! Windows are matched by window id first (stable while the owning app keeps
//! running, which covers restarting Stripline itself), then by app and title
//! (covers apps that were relaunched with the same documents open).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::strip::{Column, Side, Strip, WinId};
use super::StripKey;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub version: u32,
    pub strips: Vec<SavedStrip>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedStrip {
    pub key: StripKey,
    pub focus: usize,
    pub columns: Vec<SavedColumn>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedColumn {
    pub width: f64,
    #[serde(default)]
    pub restore_width: Option<f64>,
    #[serde(default)]
    pub tabbed: bool,
    #[serde(default)]
    pub pin: Option<Side>,
    #[serde(default)]
    pub active: usize,
    pub windows: Vec<SavedWindow>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedWindow {
    pub id: WinId,
    pub app: String,
    pub title: String,
    #[serde(default = "one")]
    pub weight: f64,
}

fn one() -> f64 {
    1.0
}

/// A window that exists right now and needs a place.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveWindow {
    pub id: WinId,
    pub app: String,
    pub title: String,
    pub key: StripKey,
}

impl Session {
    pub const VERSION: u32 = 1;

    /// Snapshot the current strips. `describe` returns (app, title) for a window.
    pub fn capture<F>(strips: &HashMap<StripKey, Strip>, describe: F) -> Session
    where
        F: Fn(WinId) -> (String, String),
    {
        let mut keys: Vec<&StripKey> = strips.keys().collect();
        keys.sort();
        let strips = keys
            .into_iter()
            .filter(|k| !strips[*k].is_empty())
            .map(|k| {
                let s = &strips[k];
                SavedStrip {
                    key: k.clone(),
                    focus: s.focus,
                    columns: s
                        .columns
                        .iter()
                        .map(|c| SavedColumn {
                            width: c.width,
                            restore_width: c.restore_width,
                            tabbed: c.tabbed,
                            pin: c.pin,
                            active: c.active,
                            windows: c
                                .windows
                                .iter()
                                .zip(&c.weights)
                                .map(|(&id, &weight)| {
                                    let (app, title) = describe(id);
                                    SavedWindow { id, app, title, weight }
                                })
                                .collect(),
                        })
                        .collect(),
                }
            })
            .collect();
        Session { version: Self::VERSION, strips }
    }

    /// Rebuild strips for the given live windows. Returns the strips and the
    /// indices (into `live`) of windows that had no saved place.
    pub fn restore(&self, live: &[LiveWindow]) -> (HashMap<StripKey, Strip>, Vec<usize>) {
        let mut used = vec![false; live.len()];
        // (strip, column, window) -> live index
        let mut assigned: HashMap<(usize, usize, usize), usize> = HashMap::new();
        let by_id: HashMap<WinId, usize> = live.iter().enumerate().map(|(i, w)| (w.id, i)).collect();

        let saved_windows = || {
            self.strips.iter().enumerate().flat_map(|(s, strip)| {
                strip
                    .columns
                    .iter()
                    .enumerate()
                    .flat_map(move |(c, col)| col.windows.iter().enumerate().map(move |(w, win)| ((s, c, w), win)))
            })
        };

        for (pos, win) in saved_windows() {
            if let Some(&i) = by_id.get(&win.id) {
                if !used[i] && live[i].app == win.app {
                    used[i] = true;
                    assigned.insert(pos, i);
                }
            }
        }
        for (pos, win) in saved_windows() {
            if assigned.contains_key(&pos) || win.title.is_empty() {
                continue;
            }
            let found = live.iter().enumerate().find(|(i, l)| !used[*i] && l.app == win.app && l.title == win.title);
            if let Some((i, _)) = found {
                used[i] = true;
                assigned.insert(pos, i);
            }
        }

        let mut strips: HashMap<StripKey, Strip> = HashMap::new();
        for (s, saved) in self.strips.iter().enumerate() {
            for (c, col) in saved.columns.iter().enumerate() {
                // A saved column may now be split across strips (a display
                // went away); keep each part together.
                let mut groups: Vec<(StripKey, Column)> = Vec::new();
                for (w, win) in col.windows.iter().enumerate() {
                    let Some(&i) = assigned.get(&(s, c, w)) else { continue };
                    let key = &live[i].key;
                    let group = match groups.iter_mut().find(|(k, _)| k == key) {
                        Some((_, g)) => g,
                        None => {
                            let mut col_new = Column::new(live[i].id, col.width);
                            col_new.windows.clear();
                            col_new.weights.clear();
                            col_new.restore_width = col.restore_width;
                            col_new.tabbed = col.tabbed;
                            col_new.pin = col.pin;
                            groups.push((key.clone(), col_new));
                            &mut groups.last_mut().unwrap().1
                        }
                    };
                    if w == col.active {
                        group.active = group.windows.len();
                    }
                    group.windows.push(live[i].id);
                    group.weights.push(if win.weight > 0.0 { win.weight } else { 1.0 });
                }
                for (key, column) in groups {
                    let strip = strips.entry(key).or_default();
                    if c == saved.focus {
                        strip.focus = strip.columns.len();
                    }
                    strip.columns.push(column);
                }
            }
        }

        for strip in strips.values_mut() {
            let n = strip.columns.len();
            for (i, col) in strip.columns.iter_mut().enumerate() {
                let ok = match col.pin {
                    Some(Side::Left) => i == 0,
                    Some(Side::Right) => i + 1 == n && !(i == 0 && n > 1),
                    None => true,
                };
                if !ok {
                    col.pin = None;
                }
                col.active = col.active.min(col.windows.len() - 1);
            }
            strip.focus = strip.focus.min(n.saturating_sub(1));
        }

        let leftovers = (0..live.len()).filter(|&i| !used[i]).collect();
        (strips, leftovers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::strip::OpenAt;

    fn key(d: &str, s: u64) -> StripKey {
        StripKey { display: d.into(), space: s }
    }

    fn live(id: WinId, app: &str, title: &str, k: &StripKey) -> LiveWindow {
        LiveWindow { id, app: app.into(), title: title.into(), key: k.clone() }
    }

    fn sample() -> (HashMap<StripKey, Strip>, StripKey) {
        let k = key("D1", 7);
        let mut s = Strip::default();
        s.insert_window(10, 0.5, OpenAt::End, true);
        s.insert_window(11, 0.33, OpenAt::End, true);
        s.insert_window(12, 0.5, OpenAt::End, true);
        s.stack_into(Side::Left); // [10] [11,12]
        s.insert_window(13, 0.75, OpenAt::End, true);
        s.focus_index(1);
        (HashMap::from([(k.clone(), s)]), k)
    }

    fn names(id: WinId) -> (String, String) {
        ("com.example.app".into(), format!("Doc {id}"))
    }

    #[test]
    fn round_trip_by_id() {
        let (strips, k) = sample();
        let session = Session::capture(&strips, names);
        let json = serde_json::to_string(&session).unwrap();
        let session: Session = serde_json::from_str(&json).unwrap();
        let ws: Vec<LiveWindow> =
            [13, 12, 11, 10].iter().map(|&id| live(id, "com.example.app", &format!("Doc {id}"), &k)).collect();
        let (restored, left) = session.restore(&ws);
        assert!(left.is_empty());
        let s = &restored[&k];
        assert_eq!(
            s.columns.iter().map(|c| c.windows.clone()).collect::<Vec<_>>(),
            vec![vec![10], vec![11, 12], vec![13]]
        );
        assert_eq!(s.focus, 1);
        assert_eq!(s.columns[2].width, 0.75);
        assert_eq!(s.columns[1].active, 1);
    }

    #[test]
    fn falls_back_to_app_and_title_and_new_space_ids() {
        let (strips, _) = sample();
        let session = Session::capture(&strips, names);
        // After a reboot: new window ids, new space id, same display.
        let k2 = key("D1", 99);
        let ws = vec![
            live(501, "com.example.app", "Doc 13", &k2),
            live(502, "com.example.app", "Doc 10", &k2),
            live(503, "com.other", "Untitled", &k2),
        ];
        let (restored, left) = session.restore(&ws);
        assert_eq!(left, vec![2]);
        let s = &restored[&k2];
        assert_eq!(s.columns.iter().map(|c| c.windows.clone()).collect::<Vec<_>>(), vec![vec![502], vec![501]]);
        assert_eq!(s.focus, 0, "saved focus column vanished, clamp");
    }

    #[test]
    fn id_reuse_by_other_app_is_not_matched() {
        let (strips, k) = sample();
        let session = Session::capture(&strips, names);
        let ws = vec![live(10, "com.other", "x", &k)];
        let (restored, left) = session.restore(&ws);
        assert!(restored.is_empty());
        assert_eq!(left, vec![0]);
    }

    #[test]
    fn split_column_across_displays_and_bad_pins() {
        let k = key("D1", 1);
        let mut s = Strip::default();
        s.insert_window(1, 0.5, OpenAt::End, true);
        s.insert_window(2, 0.5, OpenAt::End, true);
        s.insert_window(3, 0.5, OpenAt::End, true);
        s.stack_into(Side::Left); // [1] [2,3]
        s.focus_index(0);
        s.toggle_pin(Side::Left);
        let session = Session::capture(&HashMap::from([(k.clone(), s)]), names);
        let other = key("D2", 5);
        let ws = vec![live(2, "com.example.app", "Doc 2", &k), live(3, "com.example.app", "Doc 3", &other)];
        let (restored, _) = session.restore(&ws);
        assert_eq!(restored[&k].columns[0].windows, vec![2]);
        assert_eq!(restored[&other].columns[0].windows, vec![3]);
        // Column 1 (pinned left) was lost; nothing else may inherit a stale pin.
        assert!(restored.values().all(|s| s.columns.iter().all(|c| c.pin.is_none())));
    }
}
