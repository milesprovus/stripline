//! The strip model: an ordered row of columns, each holding one or more
//! windows. Everything here is pure data manipulation, no OS calls.
//!
//! Invariants maintained by every operation:
//! - no column is empty, and `weights.len() == windows.len()`;
//! - `focus < columns.len()` (or 0 when the strip is empty);
//! - a column pinned left sits at index 0, one pinned right at the last index.

use serde::{Deserialize, Serialize};

use super::tree::Tree;

/// Window identifier (the macOS `CGWindowID`).
pub type WinId = u32;

pub const MIN_WIDTH: f64 = 0.05;
pub const MAX_WIDTH: f64 = 1.0;
const MIN_SHARE: f64 = 0.1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Side {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// Where a newly tiled window is inserted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpenAt {
    /// Right of the focused column.
    #[default]
    Next,
    Start,
    End,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Column {
    pub windows: Vec<WinId>,
    /// Relative heights of the windows in a stacked column.
    pub weights: Vec<f64>,
    /// Index of the column's current window.
    pub active: usize,
    /// Width as a fraction of the scrollable area (pinned columns: of the
    /// whole viewport).
    pub width: f64,
    /// Width to return to when leaving full-width mode.
    #[serde(default)]
    pub restore_width: Option<f64>,
    #[serde(default)]
    pub tabbed: bool,
    #[serde(default)]
    pub pin: Option<Side>,
}

impl Column {
    pub fn new(win: WinId, width: f64) -> Self {
        Column {
            windows: vec![win],
            weights: vec![1.0],
            active: 0,
            width: width.clamp(MIN_WIDTH, MAX_WIDTH),
            restore_width: None,
            tabbed: false,
            pin: None,
        }
    }

    pub fn active_window(&self) -> WinId {
        self.windows[self.active.min(self.windows.len() - 1)]
    }

    pub fn len(&self) -> usize {
        self.windows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    fn push(&mut self, win: WinId) {
        self.windows.push(win);
        self.weights.push(1.0);
    }

    fn remove_at(&mut self, i: usize) -> WinId {
        let win = self.windows.remove(i);
        self.weights.remove(i);
        if self.active > i || self.active >= self.windows.len() {
            self.active = self.active.saturating_sub(1);
        }
        win
    }
}

/// How a workspace arranges its windows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Columns on a horizontal strip that scrolls.
    #[default]
    Scroll,
    /// Windows split the screen between them (see `tree.rs`).
    Split,
}

/// The windows of one workspace. In `Scroll` mode they live in `columns`; in
/// `Split` mode in `tree`. The other container is always empty, so the
/// mode-independent methods below can simply look at both.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Strip {
    pub columns: Vec<Column>,
    pub focus: usize,
    /// Current horizontal scroll of the flowing (unpinned) columns, in pixels.
    #[serde(default)]
    pub scroll: f64,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub tree: Tree,
}

impl Strip {
    pub fn with_mode(mode: Mode) -> Strip {
        Strip { mode, ..Strip::default() }
    }

    pub fn is_empty(&self) -> bool {
        self.columns.is_empty() && self.tree.is_empty()
    }

    #[cfg(test)]
    pub fn contains(&self, win: WinId) -> bool {
        self.find(win).is_some() || self.tree.contains(win)
    }

    /// Switch layout, keeping the windows in reading order and the focus.
    /// Columns created from a split layout get `width`.
    pub fn set_mode(&mut self, mode: Mode, width: f64) -> bool {
        if mode == self.mode {
            return false;
        }
        let focus = self.focused_window();
        let order: Vec<WinId> = self.windows().collect();
        self.mode = mode;
        match mode {
            Mode::Split => {
                self.columns.clear();
                self.focus = 0;
                self.scroll = 0.0;
                self.tree = Tree::from_windows(&order, focus);
            }
            Mode::Scroll => {
                self.tree = Tree::default();
                self.columns = order.iter().map(|&w| Column::new(w, width)).collect();
                self.focus = focus.and_then(|f| order.iter().position(|&w| w == f)).unwrap_or(0);
            }
        }
        true
    }

    pub fn len(&self) -> usize {
        self.columns.len()
    }

    pub fn windows(&self) -> impl Iterator<Item = WinId> + '_ {
        self.columns.iter().flat_map(|c| c.windows.iter().copied()).chain(self.tree.windows())
    }

    pub fn find(&self, win: WinId) -> Option<(usize, usize)> {
        self.columns.iter().enumerate().find_map(|(c, col)| col.windows.iter().position(|&w| w == win).map(|r| (c, r)))
    }

    pub fn focused_column(&self) -> Option<&Column> {
        self.columns.get(self.focus)
    }

    fn focused_column_mut(&mut self) -> Option<&mut Column> {
        self.columns.get_mut(self.focus)
    }

    pub fn focused_window(&self) -> Option<WinId> {
        match self.mode {
            Mode::Scroll => self.focused_column().map(Column::active_window),
            Mode::Split => self.tree.focus,
        }
    }

    /// Indices of the columns that scroll (everything except pinned ones).
    pub fn flow_range(&self) -> std::ops::Range<usize> {
        let n = self.columns.len();
        let start = usize::from(self.columns.first().is_some_and(|c| c.pin == Some(Side::Left)));
        let end = if self.columns.last().is_some_and(|c| c.pin == Some(Side::Right)) { n - 1 } else { n };
        start..end.max(start)
    }

    /// Clamp an insertion index so it never lands outside the pinned columns.
    fn clamp_insert(&self, idx: usize) -> usize {
        let r = self.flow_range();
        idx.clamp(r.start, r.end)
    }

    fn insert_column(&mut self, idx: usize, col: Column, focus: bool) -> usize {
        let idx = if self.columns.is_empty() { 0 } else { self.clamp_insert(idx) };
        let was_empty = self.columns.is_empty();
        self.columns.insert(idx, col);
        if focus || was_empty {
            self.focus = idx;
        } else if idx <= self.focus {
            self.focus += 1;
        }
        idx
    }

    // ---- adding and removing windows -----------------------------------

    /// Add a window in a new column. Returns the column index.
    pub fn insert_window(&mut self, win: WinId, width: f64, at: OpenAt, focus: bool) -> usize {
        if self.mode == Mode::Split {
            self.tree.insert(win, focus);
            return 0;
        }
        let idx = match at {
            OpenAt::Next if self.columns.is_empty() => 0,
            OpenAt::Next => self.focus + 1,
            OpenAt::Start => 0,
            OpenAt::End => self.columns.len(),
        };
        self.insert_column(idx, Column::new(win, width), focus)
    }

    /// Add a window in a new column at an exact index (used by drag and drop
    /// and display moves).
    pub fn insert_window_at(&mut self, win: WinId, width: f64, idx: usize, focus: bool) -> usize {
        if self.mode == Mode::Split {
            self.tree.insert(win, focus);
            return 0;
        }
        self.insert_column(idx, Column::new(win, width), focus)
    }

    /// Remove a window. Empty columns disappear; focus moves to the column that
    /// takes the removed one's place, or to its left neighbour at the end.
    pub fn remove_window(&mut self, win: WinId) -> bool {
        let Some((c, r)) = self.find(win) else { return self.tree.remove(win) };
        self.columns[c].remove_at(r);
        if self.columns[c].is_empty() {
            self.columns.remove(c);
            if c < self.focus {
                self.focus -= 1;
            }
            self.focus = self.focus.min(self.columns.len().saturating_sub(1));
        }
        true
    }

    // ---- focus ---------------------------------------------------------

    pub fn focus_window(&mut self, win: WinId) -> bool {
        let Some((c, r)) = self.find(win) else { return self.tree.focus_window(win) };
        let changed = self.focus != c || self.columns[c].active != r;
        self.focus = c;
        self.columns[c].active = r;
        changed
    }

    pub fn focus_index(&mut self, idx: usize) -> bool {
        if idx >= self.columns.len() || idx == self.focus {
            return false;
        }
        self.focus = idx;
        true
    }

    pub fn focus_dir(&mut self, dir: Dir) -> bool {
        match dir {
            Dir::Left if self.focus > 0 => self.focus_index(self.focus - 1),
            Dir::Right => self.focus_index(self.focus + 1),
            Dir::Up | Dir::Down => {
                let Some(col) = self.focused_column_mut() else { return false };
                let next = match dir {
                    Dir::Up => col.active.checked_sub(1),
                    _ => Some(col.active + 1).filter(|&i| i < col.len()),
                };
                match next {
                    Some(i) => {
                        col.active = i;
                        true
                    }
                    None => false,
                }
            }
            _ => false,
        }
    }

    pub fn focus_first(&mut self) -> bool {
        self.focus_index(0)
    }

    pub fn focus_last(&mut self) -> bool {
        self.focus_index(self.columns.len().saturating_sub(1))
    }

    // ---- moving ----------------------------------------------------------

    /// Swap the focused column with its neighbour. Pinned columns stay put.
    pub fn move_column(&mut self, dir: Dir) -> bool {
        let c = self.focus;
        let target = match dir {
            Dir::Left => c.checked_sub(1),
            Dir::Right => Some(c + 1),
            Dir::Up | Dir::Down => return self.move_window_in_column(dir),
        };
        let Some(t) = target.filter(|&t| t < self.columns.len()) else { return false };
        if self.columns[c].pin.is_some() || self.columns[t].pin.is_some() {
            return false;
        }
        self.columns.swap(c, t);
        self.focus = t;
        true
    }

    /// Move the focused column to an index within the scrolling part.
    pub fn move_column_to(&mut self, idx: usize) -> bool {
        let c = self.focus;
        if c >= self.columns.len() || self.columns[c].pin.is_some() {
            return false;
        }
        let r = self.flow_range();
        let idx = idx.clamp(r.start, r.end.saturating_sub(1).max(r.start));
        if idx == c {
            return false;
        }
        let col = self.columns.remove(c);
        self.columns.insert(idx, col);
        self.focus = idx;
        true
    }

    /// Move a column (by index) to another index; used for drag-to-reorder.
    pub fn reorder_column(&mut self, from: usize, to: usize) -> bool {
        if from >= self.columns.len() {
            return false;
        }
        let keep_focus = self.columns[self.focus].active_window();
        self.focus = from;
        let moved = self.move_column_to(to);
        if !moved {
            self.focus_window(keep_focus);
        }
        moved
    }

    fn move_window_in_column(&mut self, dir: Dir) -> bool {
        let Some(col) = self.focused_column_mut() else { return false };
        let i = col.active;
        let j = match dir {
            Dir::Up => i.checked_sub(1),
            _ => Some(i + 1).filter(|&j| j < col.len()),
        };
        let Some(j) = j else { return false };
        col.windows.swap(i, j);
        col.weights.swap(i, j);
        col.active = j;
        true
    }

    /// Move a window into another column (drag and drop onto a column).
    pub fn move_window_to_column(&mut self, win: WinId, target: usize) -> bool {
        let Some((c, r)) = self.find(win) else { return false };
        if target >= self.columns.len() || target == c {
            return false;
        }
        let mut t = target;
        self.columns[c].remove_at(r);
        if self.columns[c].is_empty() {
            self.columns.remove(c);
            if t > c {
                t -= 1;
            }
        }
        let col = &mut self.columns[t];
        col.push(win);
        col.active = col.len() - 1;
        self.focus = t;
        true
    }

    /// Move the focused window into the neighbouring column on `side`.
    pub fn stack_into(&mut self, side: Side) -> bool {
        let c = self.focus;
        let t = match side {
            Side::Left => c.checked_sub(1),
            Side::Right => Some(c + 1),
        };
        let Some(mut t) = t.filter(|&t| t < self.columns.len()) else { return false };
        let col = &mut self.columns[c];
        let win = col.active_window();
        if col.len() == 1 {
            self.columns.remove(c);
            if t > c {
                t -= 1;
            }
        } else {
            let a = col.active;
            col.remove_at(a);
        }
        let target = &mut self.columns[t];
        target.push(win);
        target.active = target.len() - 1;
        self.focus = t;
        true
    }

    /// Take the focused window out of its stack into a new column on `side`.
    pub fn pop_out(&mut self, side: Side) -> bool {
        let c = self.focus;
        let Some(col) = self.columns.get_mut(c) else { return false };
        if col.len() < 2 {
            return false;
        }
        let a = col.active;
        let win = col.remove_at(a);
        let width = col.width;
        let idx = match side {
            Side::Left => c,
            Side::Right => c + 1,
        };
        self.insert_column(idx, Column::new(win, width), true);
        true
    }

    pub fn toggle_tabbed(&mut self) -> bool {
        let Some(col) = self.focused_column_mut() else { return false };
        col.tabbed = !col.tabbed;
        true
    }

    /// Pin the focused column to one edge, or unpin it if already pinned there.
    pub fn toggle_pin(&mut self, side: Side) -> bool {
        let c = self.focus;
        if c >= self.columns.len() {
            return false;
        }
        if self.columns[c].pin == Some(side) {
            self.columns[c].pin = None;
            return true;
        }
        for col in &mut self.columns {
            if col.pin == Some(side) {
                col.pin = None;
            }
        }
        let mut col = self.columns.remove(c);
        col.pin = Some(side);
        let idx = match side {
            Side::Left => 0,
            Side::Right => self.columns.len(),
        };
        self.columns.insert(idx, col);
        self.focus = idx;
        true
    }

    // ---- sizing --------------------------------------------------------

    pub fn set_width(&mut self, width: f64) -> bool {
        let Some(col) = self.focused_column_mut() else { return false };
        col.width = width.clamp(MIN_WIDTH, MAX_WIDTH);
        col.restore_width = None;
        true
    }

    /// Step through `presets` (fractions, any order) from the current width.
    pub fn cycle_width(&mut self, presets: &[f64], forward: bool) -> bool {
        if presets.is_empty() {
            return false;
        }
        let mut sorted: Vec<f64> = presets.to_vec();
        sorted.sort_by(f64::total_cmp);
        let Some(col) = self.focused_column() else { return false };
        let cur = col.width;
        let next = if forward {
            sorted.iter().copied().find(|&p| p > cur + 0.005).unwrap_or(sorted[0])
        } else {
            sorted.iter().rev().copied().find(|&p| p < cur - 0.005).unwrap_or(sorted[sorted.len() - 1])
        };
        self.set_width(next)
    }

    pub fn adjust_width(&mut self, delta: f64) -> bool {
        let Some(col) = self.focused_column() else { return false };
        let w = col.width + delta;
        self.set_width(w)
    }

    pub fn toggle_full_width(&mut self) -> bool {
        let Some(col) = self.focused_column_mut() else { return false };
        match col.restore_width.take() {
            Some(w) => col.width = w,
            None => {
                col.restore_width = Some(col.width);
                col.width = MAX_WIDTH;
            }
        }
        true
    }

    /// Grow or shrink the active window's share of its column's height.
    pub fn adjust_height(&mut self, delta: f64) -> bool {
        let Some(col) = self.focused_column_mut() else { return false };
        let n = col.len();
        if n < 2 {
            return false;
        }
        let total: f64 = col.weights.iter().sum();
        let a = col.active;
        let old = col.weights[a] / total;
        let new = (old + delta).clamp(MIN_SHARE, 1.0 - MIN_SHARE * (n - 1) as f64);
        let rest_old = 1.0 - old;
        let rest_new = 1.0 - new;
        for (i, w) in col.weights.iter_mut().enumerate() {
            *w = if i == a {
                new
            } else if rest_old > 0.0 {
                (*w / total) * rest_new / rest_old
            } else {
                rest_new / (n - 1) as f64
            };
        }
        true
    }

    pub fn equalize_heights(&mut self) -> bool {
        let Some(col) = self.focused_column_mut() else { return false };
        col.weights.iter_mut().for_each(|w| *w = 1.0);
        true
    }

    /// Check the invariants listed at the top of this file.
    #[cfg(test)]
    pub fn check(&self) {
        for (i, c) in self.columns.iter().enumerate() {
            assert!(!c.is_empty(), "empty column {i}");
            assert_eq!(c.windows.len(), c.weights.len());
            assert!(c.active < c.len());
            match c.pin {
                Some(Side::Left) => assert_eq!(i, 0),
                Some(Side::Right) => assert_eq!(i, self.columns.len() - 1),
                None => {}
            }
        }
        if !self.columns.is_empty() {
            assert!(self.focus < self.columns.len());
        }
        match self.mode {
            Mode::Scroll => assert!(self.tree.is_empty(), "scroll mode with a tree"),
            Mode::Split => assert!(self.columns.is_empty(), "split mode with columns"),
        }
        self.tree.check();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(ids: &[WinId]) -> Strip {
        let mut s = Strip::default();
        for &id in ids {
            s.insert_window(id, 0.5, OpenAt::Next, true);
        }
        s
    }

    fn order(s: &Strip) -> Vec<Vec<WinId>> {
        s.columns.iter().map(|c| c.windows.clone()).collect()
    }

    #[test]
    fn new_windows_open_beside_focus() {
        let mut s = strip(&[1, 2, 3]);
        assert_eq!(order(&s), vec![vec![1], vec![2], vec![3]]);
        s.focus_index(0);
        s.insert_window(4, 0.5, OpenAt::Next, true);
        assert_eq!(order(&s), vec![vec![1], vec![4], vec![2], vec![3]]);
        assert_eq!(s.focused_window(), Some(4));
        s.check();
    }

    #[test]
    fn background_window_does_not_steal_focus() {
        let mut s = strip(&[1, 2]);
        s.focus_index(0);
        s.insert_window(3, 0.5, OpenAt::Start, false);
        assert_eq!(order(&s), vec![vec![3], vec![1], vec![2]]);
        assert_eq!(s.focused_window(), Some(1));
        s.check();
    }

    #[test]
    fn remove_moves_focus_to_neighbour() {
        let mut s = strip(&[1, 2, 3]);
        s.focus_index(1);
        assert!(s.remove_window(2));
        assert_eq!(s.focused_window(), Some(3));
        assert!(s.remove_window(3));
        assert_eq!(s.focused_window(), Some(1));
        assert!(s.remove_window(1));
        assert!(s.is_empty());
        assert!(!s.remove_window(1));
        s.check();
    }

    #[test]
    fn remove_left_of_focus_keeps_focused_window() {
        let mut s = strip(&[1, 2, 3]);
        s.remove_window(1);
        assert_eq!(s.focused_window(), Some(3));
        s.check();
    }

    #[test]
    fn focus_navigation() {
        let mut s = strip(&[1, 2, 3]);
        assert!(!s.focus_dir(Dir::Right));
        assert!(s.focus_dir(Dir::Left));
        assert_eq!(s.focused_window(), Some(2));
        assert!(s.focus_first());
        assert!(!s.focus_dir(Dir::Left));
        assert!(s.focus_last());
        assert_eq!(s.focused_window(), Some(3));
    }

    #[test]
    fn move_column_swaps() {
        let mut s = strip(&[1, 2, 3]);
        assert!(s.move_column(Dir::Left));
        assert_eq!(order(&s), vec![vec![1], vec![3], vec![2]]);
        assert_eq!(s.focused_window(), Some(3));
        assert!(s.move_column_to(0));
        assert_eq!(order(&s), vec![vec![3], vec![1], vec![2]]);
        s.check();
    }

    #[test]
    fn stack_and_pop_out() {
        let mut s = strip(&[1, 2, 3]);
        // Focused on 3: stack it into column of 2.
        assert!(s.stack_into(Side::Left));
        assert_eq!(order(&s), vec![vec![1], vec![2, 3]]);
        assert_eq!(s.focused_window(), Some(3));
        // Vertical focus and move.
        assert!(s.focus_dir(Dir::Up));
        assert_eq!(s.focused_window(), Some(2));
        assert!(s.move_column(Dir::Down));
        assert_eq!(order(&s), vec![vec![1], vec![3, 2]]);
        // Pop 2 out to the left.
        assert!(s.pop_out(Side::Left));
        assert_eq!(order(&s), vec![vec![1], vec![2], vec![3]]);
        assert_eq!(s.focused_window(), Some(2));
        assert!(!s.pop_out(Side::Right), "single-window column cannot pop out");
        s.check();
    }

    #[test]
    fn stack_from_multi_window_column() {
        let mut s = strip(&[1, 2, 3]);
        s.stack_into(Side::Left); // [1] [2,3]
        assert!(s.stack_into(Side::Left)); // 3 -> column of 1
        assert_eq!(order(&s), vec![vec![1, 3], vec![2]]);
        assert_eq!(s.focus, 0);
        s.check();
    }

    #[test]
    fn width_cycle_and_full() {
        let mut s = strip(&[1]);
        let presets = [0.75, 0.333, 0.5, 0.667];
        assert!(s.cycle_width(&presets, true));
        assert_eq!(s.columns[0].width, 0.667);
        s.cycle_width(&presets, true);
        assert_eq!(s.columns[0].width, 0.75);
        s.cycle_width(&presets, true);
        assert_eq!(s.columns[0].width, 0.333);
        s.cycle_width(&presets, false);
        assert_eq!(s.columns[0].width, 0.75);
        s.toggle_full_width();
        assert_eq!(s.columns[0].width, 1.0);
        s.toggle_full_width();
        assert_eq!(s.columns[0].width, 0.75);
        s.adjust_width(-0.9);
        assert_eq!(s.columns[0].width, MIN_WIDTH);
    }

    #[test]
    fn heights_keep_shares_bounded() {
        let mut s = strip(&[1, 2, 3]);
        s.stack_into(Side::Left); // [1] [2,3]
        s.stack_into(Side::Left); // 3 moves into column 0: [1,3] [2]
        let col = &s.columns[0];
        assert_eq!(col.len(), 2);
        assert!(s.adjust_height(0.2));
        let w = &s.columns[0].weights;
        let total: f64 = w.iter().sum();
        assert!((total - 1.0).abs() < 1e-9);
        assert!((w[s.columns[0].active] - 0.7).abs() < 1e-9);
        s.adjust_height(5.0);
        assert!(s.columns[0].weights.iter().all(|&x| x >= MIN_SHARE - 1e-9));
        s.equalize_heights();
        assert!(s.columns[0].weights.iter().all(|&x| x == 1.0));
    }

    #[test]
    fn pinning_moves_column_to_edge() {
        let mut s = strip(&[1, 2, 3, 4]);
        s.focus_index(2);
        assert!(s.toggle_pin(Side::Left));
        assert_eq!(order(&s), vec![vec![3], vec![1], vec![2], vec![4]]);
        assert_eq!(s.flow_range(), 1..4);
        // Pinned column cannot be moved and others cannot pass it.
        assert!(!s.move_column(Dir::Right));
        s.focus_index(1);
        assert!(!s.move_column(Dir::Left));
        // New windows never land left of the pinned column.
        s.insert_window(9, 0.5, OpenAt::Start, true);
        assert_eq!(s.columns[1].windows, vec![9]);
        // Pin another column right.
        s.focus_index(2);
        s.toggle_pin(Side::Right);
        assert_eq!(s.columns.last().unwrap().windows, vec![1]);
        s.insert_window(10, 0.5, OpenAt::End, true);
        assert_eq!(s.columns[s.len() - 2].windows, vec![10]);
        // Unpin.
        s.focus_index(0);
        assert!(s.toggle_pin(Side::Left));
        assert_eq!(s.columns[0].pin, None);
        s.check();
    }

    #[test]
    fn pinning_other_column_replaces_pin() {
        let mut s = strip(&[1, 2, 3]);
        s.focus_index(1);
        s.toggle_pin(Side::Left);
        s.focus_index(2);
        s.toggle_pin(Side::Left);
        assert_eq!(s.columns[0].windows, vec![3]);
        assert_eq!(s.columns.iter().filter(|c| c.pin.is_some()).count(), 1);
        s.check();
    }

    #[test]
    fn switching_modes_keeps_order_and_focus() {
        let mut s = strip(&[1, 2, 3]);
        s.focus_index(1);
        assert!(s.set_mode(Mode::Split, 0.5));
        assert!(!s.set_mode(Mode::Split, 0.5));
        assert_eq!(s.windows().collect::<Vec<_>>(), vec![1, 2, 3]);
        assert_eq!(s.focused_window(), Some(2));
        // Generic operations work on the tree.
        s.insert_window(4, 0.5, OpenAt::Next, true);
        assert!(s.contains(4) && s.columns.is_empty());
        assert!(s.remove_window(1));
        assert!(s.focus_window(3));
        s.check();
        assert!(s.set_mode(Mode::Scroll, 0.4));
        // 4 split the focused window 2, so it follows 2 in reading order.
        assert_eq!(order(&s), vec![vec![2], vec![4], vec![3]]);
        assert_eq!(s.focused_window(), Some(3));
        assert!(s.columns.iter().all(|c| c.width == 0.4));
        s.check();
    }

    #[test]
    fn drop_window_onto_column() {
        let mut s = strip(&[1, 2, 3]);
        assert!(s.move_window_to_column(1, 2));
        assert_eq!(order(&s), vec![vec![2], vec![3, 1]]);
        assert_eq!(s.focused_window(), Some(1));
        assert!(!s.move_window_to_column(1, 1), "same column");
        assert!(!s.move_window_to_column(1, 7), "out of range");
        s.check();
    }

    #[test]
    fn reorder_keeps_focus_on_failure() {
        let mut s = strip(&[1, 2, 3]);
        assert!(s.reorder_column(0, 2));
        assert_eq!(order(&s), vec![vec![2], vec![3], vec![1]]);
        assert_eq!(s.focused_window(), Some(1));
        s.focus_index(0);
        assert!(!s.reorder_column(1, 1));
        assert_eq!(s.focused_window(), Some(2));
        s.check();
    }

    #[test]
    fn random_ops_keep_invariants() {
        // Small deterministic LCG so the test needs no extra crates.
        let mut seed: u64 = 0x2545F4914F6CDD1D;
        let mut rnd = |n: u64| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) % n
        };
        let mut s = Strip::default();
        let mut next_id = 1;
        for _ in 0..5000 {
            match rnd(14) {
                0 | 1 => {
                    s.insert_window(next_id, 0.5, OpenAt::Next, rnd(2) == 0);
                    next_id += 1;
                }
                2 => {
                    let n = s.windows().count().max(1) as u64;
                    let w = s.windows().nth(rnd(n) as usize);
                    if let Some(w) = w {
                        s.remove_window(w);
                    }
                }
                3 => {
                    s.focus_dir([Dir::Left, Dir::Right, Dir::Up, Dir::Down][rnd(4) as usize]);
                }
                4 => {
                    s.move_column([Dir::Left, Dir::Right, Dir::Up, Dir::Down][rnd(4) as usize]);
                }
                5 => {
                    s.stack_into(if rnd(2) == 0 { Side::Left } else { Side::Right });
                }
                6 => {
                    s.pop_out(if rnd(2) == 0 { Side::Left } else { Side::Right });
                }
                7 => {
                    s.toggle_pin(if rnd(2) == 0 { Side::Left } else { Side::Right });
                }
                8 => {
                    s.move_column_to(rnd(8) as usize);
                }
                9 => {
                    s.reorder_column(rnd(8) as usize, rnd(8) as usize);
                }
                10 => {
                    s.adjust_height(0.1);
                }
                11 => {
                    s.focus_index(rnd(8) as usize);
                }
                12 => {
                    s.insert_window(next_id, 0.3, OpenAt::End, false);
                    next_id += 1;
                    if rnd(20) == 0 {
                        let m = if s.mode == Mode::Scroll { Mode::Split } else { Mode::Scroll };
                        s.set_mode(m, 0.5);
                    }
                }
                _ => {
                    s.insert_window(next_id, 0.3, OpenAt::Start, true);
                    next_id += 1;
                }
            }
            s.check();
        }
    }
}
