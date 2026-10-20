//! Binary split tree for the "split" layout: every window fills the screen
//! together with the others, and a new window splits the focused one in two
//! (the dwindle scheme Hyprland uses).
//!
//! A split's axis is normally chosen from the shape of the area it divides
//! (wider than tall: side by side; otherwise stacked) and recomputed on
//! every layout, so it adapts to the display. `toggle_axis` pins it.

use serde::{Deserialize, Serialize};

use super::geometry::Rect;
use super::strip::{Dir, WinId};

const MIN_RATIO: f64 = 0.1;
const MAX_RATIO: f64 = 0.9;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Axis {
    /// Children side by side.
    Horizontal,
    /// Children stacked top to bottom.
    Vertical,
}

impl Axis {
    fn other(self) -> Axis {
        match self {
            Axis::Horizontal => Axis::Vertical,
            Axis::Vertical => Axis::Horizontal,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Node {
    Leaf(WinId),
    Split {
        /// `None`: decided by the area's shape at layout time.
        axis: Option<Axis>,
        /// Share of the area given to `first`.
        ratio: f64,
        first: Box<Node>,
        second: Box<Node>,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Tree {
    pub root: Option<Node>,
    pub focus: Option<WinId>,
}

fn resolve(axis: Option<Axis>, r: Rect) -> Axis {
    axis.unwrap_or(if r.w >= r.h { Axis::Horizontal } else { Axis::Vertical })
}

/// Divide `r` along `axis`, leaving `gap` between the parts.
fn split_rect(r: Rect, axis: Axis, ratio: f64, gap: f64) -> (Rect, Rect) {
    match axis {
        Axis::Horizontal => {
            let a = ((r.w - gap).max(2.0) * ratio).round();
            (Rect::new(r.x, r.y, a, r.h), Rect::new(r.x + a + gap, r.y, (r.w - a - gap).max(1.0), r.h))
        }
        Axis::Vertical => {
            let a = ((r.h - gap).max(2.0) * ratio).round();
            (Rect::new(r.x, r.y, r.w, a), Rect::new(r.x, r.y + a + gap, r.w, (r.h - a - gap).max(1.0)))
        }
    }
}

/// One step on the way from the root to a leaf.
#[derive(Clone, Copy, Debug)]
struct Step {
    /// The leaf is under the split's second child.
    second: bool,
    /// The split's effective axis.
    axis: Axis,
    /// Length of the split's area along its axis.
    span: f64,
}

impl Node {
    fn leaves(&self, out: &mut Vec<WinId>) {
        match self {
            Node::Leaf(w) => out.push(*w),
            Node::Split { first, second, .. } => {
                first.leaves(out);
                second.leaves(out);
            }
        }
    }

    fn first_leaf(&self) -> WinId {
        match self {
            Node::Leaf(w) => *w,
            Node::Split { first, .. } => first.first_leaf(),
        }
    }

    fn contains(&self, w: WinId) -> bool {
        match self {
            Node::Leaf(x) => *x == w,
            Node::Split { first, second, .. } => first.contains(w) || second.contains(w),
        }
    }

    fn split_leaf(&mut self, target: WinId, new: WinId) -> bool {
        match self {
            Node::Leaf(w) if *w == target => {
                let old = Node::Leaf(*w);
                *self = Node::Split { axis: None, ratio: 0.5, first: Box::new(old), second: Box::new(Node::Leaf(new)) };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { first, second, .. } => first.split_leaf(target, new) || second.split_leaf(target, new),
        }
    }

    /// Remove leaf `w`; its sibling takes the parent's place.
    fn remove(self, w: WinId) -> (Option<Node>, bool) {
        match self {
            Node::Leaf(x) if x == w => (None, true),
            leaf @ Node::Leaf(_) => (Some(leaf), false),
            Node::Split { axis, ratio, first, second } => {
                let (f, found) = first.remove(w);
                if found {
                    let node = match f {
                        None => *second,
                        Some(f) => Node::Split { axis, ratio, first: Box::new(f), second },
                    };
                    return (Some(node), true);
                }
                let f = f.expect("an untouched subtree is kept");
                let (s, found) = second.remove(w);
                let node = match s {
                    None => f,
                    Some(s) => Node::Split { axis, ratio, first: Box::new(f), second: Box::new(s) },
                };
                (Some(node), found)
            }
        }
    }

    /// First leaf of the sibling subtree of leaf `w`, if `w` has a parent.
    fn sibling_leaf(&self, w: WinId) -> Option<WinId> {
        let Node::Split { first, second, .. } = self else { return None };
        if matches!(**first, Node::Leaf(x) if x == w) {
            return Some(second.first_leaf());
        }
        if matches!(**second, Node::Leaf(x) if x == w) {
            return Some(first.first_leaf());
        }
        first.sibling_leaf(w).or_else(|| second.sibling_leaf(w))
    }

    fn layout(&self, r: Rect, gap: f64, out: &mut Vec<(WinId, Rect)>) {
        match self {
            Node::Leaf(w) => out.push((*w, r)),
            Node::Split { axis, ratio, first, second } => {
                let (a, b) = split_rect(r, resolve(*axis, r), *ratio, gap);
                first.layout(a, gap, out);
                second.layout(b, gap, out);
            }
        }
    }

    fn path(&self, w: WinId, r: Rect, gap: f64, out: &mut Vec<Step>) -> bool {
        match self {
            Node::Leaf(x) => *x == w,
            Node::Split { axis, ratio, first, second } => {
                let ax = resolve(*axis, r);
                let (a, b) = split_rect(r, ax, *ratio, gap);
                let span = match ax {
                    Axis::Horizontal => r.w,
                    Axis::Vertical => r.h,
                };
                out.push(Step { second: false, axis: ax, span });
                if first.path(w, a, gap, out) {
                    return true;
                }
                out.last_mut().unwrap().second = true;
                if second.path(w, b, gap, out) {
                    return true;
                }
                out.pop();
                false
            }
        }
    }

    /// The node reached by following `path` (true = second child).
    fn at_mut(&mut self, path: &[bool]) -> &mut Node {
        match (path.split_first(), self) {
            (None, node) => node,
            (Some((&second, rest)), Node::Split { first, second: s, .. }) => {
                if second {
                    s.at_mut(rest)
                } else {
                    first.at_mut(rest)
                }
            }
            (Some(_), leaf) => leaf,
        }
    }

    fn for_each_split(&mut self, f: &mut impl FnMut(&mut Option<Axis>, &mut f64)) {
        if let Node::Split { axis, ratio, first, second } = self {
            f(axis, ratio);
            first.for_each_split(f);
            second.for_each_split(f);
        }
    }

    fn rename(&mut self, from: WinId, to: WinId) {
        match self {
            Node::Leaf(w) if *w == from => *w = to,
            Node::Leaf(_) => {}
            Node::Split { first, second, .. } => {
                first.rename(from, to);
                second.rename(from, to);
            }
        }
    }
}

impl Tree {
    /// Build a dwindle spiral from a window order (each window splits the
    /// previous one), then focus `focus`.
    pub fn from_windows(order: &[WinId], focus: Option<WinId>) -> Tree {
        let mut t = Tree::default();
        for &w in order {
            t.insert(w, true);
        }
        if let Some(f) = focus.filter(|f| t.contains(*f)) {
            t.focus = Some(f);
        }
        t
    }

    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    pub fn windows(&self) -> Vec<WinId> {
        let mut v = Vec::new();
        if let Some(r) = &self.root {
            r.leaves(&mut v);
        }
        v
    }

    pub fn contains(&self, w: WinId) -> bool {
        self.root.as_ref().is_some_and(|r| r.contains(w))
    }

    /// Split the focused window (or the last one) and put `w` in the new half.
    pub fn insert(&mut self, w: WinId, focus: bool) {
        if self.contains(w) {
            return;
        }
        match &mut self.root {
            None => self.root = Some(Node::Leaf(w)),
            Some(root) => {
                let target = self.focus.filter(|f| root.contains(*f)).unwrap_or_else(|| {
                    let mut v = Vec::new();
                    root.leaves(&mut v);
                    *v.last().unwrap()
                });
                root.split_leaf(target, w);
            }
        }
        if focus || self.focus.is_none() {
            self.focus = Some(w);
        }
    }

    /// Remove a window; its sibling grows into the space. If it had focus,
    /// focus moves to that sibling.
    pub fn remove(&mut self, w: WinId) -> bool {
        let Some(root) = self.root.take() else { return false };
        let sibling = root.sibling_leaf(w);
        let (root, found) = root.remove(w);
        self.root = root;
        if found && self.focus == Some(w) {
            self.focus = sibling.or_else(|| self.root.as_ref().map(Node::first_leaf));
        }
        found
    }

    pub fn focus_window(&mut self, w: WinId) -> bool {
        if !self.contains(w) || self.focus == Some(w) {
            return false;
        }
        self.focus = Some(w);
        true
    }

    /// Focus the window at a position in reading order (0-based).
    pub fn focus_index(&mut self, i: usize) -> bool {
        match self.windows().get(i) {
            Some(&w) => self.focus_window(w),
            None => false,
        }
    }

    pub fn layout(&self, area: Rect, gap: f64) -> Vec<(WinId, Rect)> {
        let mut out = Vec::new();
        if let Some(r) = &self.root {
            r.layout(area, gap, &mut out);
        }
        out
    }

    /// The nearest window in a direction from `w`, by on-screen position.
    pub fn neighbor(&self, w: WinId, dir: Dir, area: Rect, gap: f64) -> Option<WinId> {
        let rects = self.layout(area, gap);
        let cur = rects.iter().find(|(x, _)| *x == w)?.1;
        let (cx, cy) = cur.center();
        rects
            .iter()
            .filter(|(x, _)| *x != w)
            .filter_map(|&(x, r)| {
                let (rx, ry) = r.center();
                let (dist, overlaps, off_axis) = match dir {
                    Dir::Left => (cur.x - r.max_x(), r.overlaps_vertically(&cur), (ry - cy).abs()),
                    Dir::Right => (r.x - cur.max_x(), r.overlaps_vertically(&cur), (ry - cy).abs()),
                    Dir::Up => (cur.y - r.max_y(), r.overlaps_horizontally(&cur), (rx - cx).abs()),
                    Dir::Down => (r.y - cur.max_y(), r.overlaps_horizontally(&cur), (rx - cx).abs()),
                };
                (dist >= -1.0 && overlaps).then_some((x, dist, off_axis))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.2.total_cmp(&b.2)))
            .map(|(x, _, _)| x)
    }

    pub fn focus_dir(&mut self, dir: Dir, area: Rect, gap: f64) -> bool {
        let Some(f) = self.focus else { return false };
        match self.neighbor(f, dir, area, gap) {
            Some(n) => self.focus_window(n),
            None => false,
        }
    }

    /// Exchange the places of two windows.
    pub fn swap(&mut self, a: WinId, b: WinId) -> bool {
        if a == b || !self.contains(a) || !self.contains(b) {
            return false;
        }
        let Some(root) = &mut self.root else { return false };
        // WinId 0 is never a real window, so it is safe as a placeholder.
        root.rename(a, 0);
        root.rename(b, a);
        root.rename(0, b);
        true
    }

    /// Swap the focused window with its neighbour in a direction; focus
    /// stays with the moved window.
    pub fn swap_dir(&mut self, dir: Dir, area: Rect, gap: f64) -> bool {
        let Some(f) = self.focus else { return false };
        match self.neighbor(f, dir, area, gap) {
            Some(n) => self.swap(f, n),
            None => false,
        }
    }

    fn focus_path(&self, area: Rect, gap: f64) -> Option<Vec<Step>> {
        let (root, f) = (self.root.as_ref()?, self.focus?);
        let mut path = Vec::new();
        root.path(f, area, gap, &mut path).then_some(path)
    }

    /// Grow (positive) or shrink the focused window along `axis`, by moving
    /// the nearest enclosing split on that axis. With `pixels`, `delta` is in
    /// points; otherwise it is a fraction of that split's area.
    pub fn resize(&mut self, axis: Axis, delta: f64, pixels: bool, area: Rect, gap: f64) -> bool {
        let Some(path) = self.focus_path(area, gap) else { return false };
        let Some(i) = path.iter().rposition(|s| s.axis == axis) else { return false };
        let step = path[i];
        let dirs: Vec<bool> = path[..i].iter().map(|s| s.second).collect();
        let frac = if pixels { delta / (step.span - gap).max(1.0) } else { delta };
        let node = self.root.as_mut().unwrap().at_mut(&dirs);
        if let Node::Split { ratio, .. } = node {
            let d = if step.second { -frac } else { frac };
            let new = (*ratio + d).clamp(MIN_RATIO, MAX_RATIO);
            let changed = (new - *ratio).abs() > 1e-9;
            *ratio = new;
            return changed;
        }
        false
    }

    /// Flip the split that holds the focused window between side by side
    /// and stacked.
    pub fn toggle_axis(&mut self, area: Rect, gap: f64) -> bool {
        let Some(path) = self.focus_path(area, gap) else { return false };
        let Some(last) = path.last().copied() else { return false };
        let dirs: Vec<bool> = path[..path.len() - 1].iter().map(|s| s.second).collect();
        if let Node::Split { axis, .. } = self.root.as_mut().unwrap().at_mut(&dirs) {
            *axis = Some(last.axis.other());
            return true;
        }
        false
    }

    /// Reset every split to an even share.
    pub fn equalize(&mut self) -> bool {
        let Some(root) = &mut self.root else { return false };
        root.for_each_split(&mut |_, ratio| *ratio = 0.5);
        true
    }

    #[cfg(test)]
    pub fn check(&self) {
        let ws = self.windows();
        let mut sorted = ws.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ws.len(), "duplicate window");
        match self.focus {
            Some(f) => assert!(self.contains(f), "focus {f} not in tree"),
            None => assert!(self.is_empty() || ws.is_empty()),
        }
        if let Some(mut r) = self.root.clone() {
            r.for_each_split(&mut |_, ratio| assert!((MIN_RATIO..=MAX_RATIO).contains(ratio)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect::new(0.0, 0.0, 1600.0, 1000.0);
    const GAP: f64 = 10.0;

    fn rect_of(t: &Tree, w: WinId) -> Rect {
        t.layout(AREA, GAP).into_iter().find(|(x, _)| *x == w).unwrap().1
    }

    #[test]
    fn dwindle_spiral() {
        let t = Tree::from_windows(&[1, 2, 3, 4], None);
        // 1 | (2 / (3 | 4))
        assert_eq!(rect_of(&t, 1), Rect::new(0.0, 0.0, 795.0, 1000.0));
        assert_eq!(rect_of(&t, 2), Rect::new(805.0, 0.0, 795.0, 495.0));
        assert_eq!(rect_of(&t, 3), Rect::new(805.0, 505.0, 393.0, 495.0));
        assert_eq!(rect_of(&t, 4), Rect::new(1208.0, 505.0, 392.0, 495.0));
        assert_eq!(t.focus, Some(4));
        t.check();
    }

    #[test]
    fn new_window_splits_the_focused_one() {
        let mut t = Tree::from_windows(&[1, 2], None);
        t.focus_window(1);
        t.insert(3, true);
        // 1 was 795 wide and full height (portrait): split stacked.
        assert_eq!(rect_of(&t, 1), Rect::new(0.0, 0.0, 795.0, 495.0));
        assert_eq!(rect_of(&t, 3), Rect::new(0.0, 505.0, 795.0, 495.0));
        assert_eq!(rect_of(&t, 2), Rect::new(805.0, 0.0, 795.0, 1000.0));
        assert_eq!(t.focus, Some(3));
        // A background window does not take focus.
        t.insert(4, false);
        assert_eq!(t.focus, Some(3));
        t.check();
    }

    #[test]
    fn removing_lets_the_sibling_grow() {
        let mut t = Tree::from_windows(&[1, 2, 3], None);
        assert!(t.remove(3));
        assert_eq!(rect_of(&t, 2), Rect::new(805.0, 0.0, 795.0, 1000.0));
        assert_eq!(t.focus, Some(2), "focus goes to the sibling");
        assert!(t.remove(1));
        assert_eq!(rect_of(&t, 2), AREA);
        assert!(!t.remove(9));
        assert!(t.remove(2));
        assert!(t.is_empty());
        assert_eq!(t.focus, None);
    }

    #[test]
    fn directional_focus_and_swap() {
        let mut t = Tree::from_windows(&[1, 2, 3, 4], None);
        // Focus 4 (bottom right) -> left is 3 -> up is 2 -> left is 1.
        assert!(t.focus_dir(Dir::Left, AREA, GAP));
        assert_eq!(t.focus, Some(3));
        assert!(t.focus_dir(Dir::Up, AREA, GAP));
        assert_eq!(t.focus, Some(2));
        assert!(t.focus_dir(Dir::Left, AREA, GAP));
        assert_eq!(t.focus, Some(1));
        assert!(!t.focus_dir(Dir::Left, AREA, GAP));
        // Move 1 right: swaps with 2 (the window to its right that is closest in y).
        assert!(t.swap_dir(Dir::Right, AREA, GAP));
        assert_eq!(rect_of(&t, 1), Rect::new(805.0, 0.0, 795.0, 495.0));
        assert_eq!(t.focus, Some(1));
        t.check();
    }

    #[test]
    fn resize_moves_the_nearest_split_on_that_axis() {
        let mut t = Tree::from_windows(&[1, 2], None);
        t.focus_window(1);
        assert!(t.resize(Axis::Horizontal, 0.1, false, AREA, GAP));
        assert_eq!(rect_of(&t, 1).w, (1590.0_f64 * 0.6).round());
        // Growing the second child moves the split the other way.
        t.focus_window(2);
        assert!(t.resize(Axis::Horizontal, 0.1, false, AREA, GAP));
        assert_eq!(rect_of(&t, 1).w, 795.0);
        // No vertical split above a lone side-by-side pair.
        assert!(!t.resize(Axis::Vertical, 0.1, false, AREA, GAP));
        // Pixel deltas are converted against the split's own width.
        assert!(t.resize(Axis::Horizontal, 159.0, true, AREA, GAP));
        assert_eq!(rect_of(&t, 2).w, 795.0 + 159.0);
        // Ratios are clamped.
        for _ in 0..20 {
            t.resize(Axis::Horizontal, 0.1, false, AREA, GAP);
        }
        t.check();
    }

    #[test]
    fn toggle_axis_and_equalize() {
        let mut t = Tree::from_windows(&[1, 2], None);
        assert!(t.toggle_axis(AREA, GAP));
        assert_eq!(rect_of(&t, 1), Rect::new(0.0, 0.0, 1600.0, 495.0));
        t.resize(Axis::Vertical, 0.2, false, AREA, GAP);
        t.equalize();
        assert_eq!(rect_of(&t, 1).h, 495.0);
        let mut single = Tree::from_windows(&[1], None);
        assert!(!single.toggle_axis(AREA, GAP), "nothing to flip");
    }

    #[test]
    fn random_ops_keep_invariants_and_never_overlap() {
        let mut seed: u64 = 0x9E3779B97F4A7C15;
        let mut rnd = |n: u64| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) % n
        };
        let dirs = [Dir::Left, Dir::Right, Dir::Up, Dir::Down];
        let mut t = Tree::default();
        let mut next = 1;
        for _ in 0..4000 {
            match rnd(9) {
                0..=2 => {
                    t.insert(next, rnd(2) == 0);
                    next += 1;
                }
                3 => {
                    let ws = t.windows();
                    if !ws.is_empty() {
                        t.remove(ws[rnd(ws.len() as u64) as usize]);
                    }
                }
                4 => {
                    t.focus_dir(dirs[rnd(4) as usize], AREA, GAP);
                }
                5 => {
                    t.swap_dir(dirs[rnd(4) as usize], AREA, GAP);
                }
                6 => {
                    let ax = if rnd(2) == 0 { Axis::Horizontal } else { Axis::Vertical };
                    t.resize(ax, 0.05, false, AREA, GAP);
                }
                7 => {
                    t.toggle_axis(AREA, GAP);
                }
                _ => {
                    t.focus_index(rnd(8) as usize);
                }
            }
            t.check();
            if t.windows().len() > 12 {
                continue; // tiny tiles are allowed to round to 1 pt
            }
            let rects = t.layout(AREA, GAP);
            for (i, (_, a)) in rects.iter().enumerate() {
                assert!(
                    a.x >= AREA.x
                        && a.y >= AREA.y
                        && a.max_x() <= AREA.max_x() + 0.01
                        && a.max_y() <= AREA.max_y() + 0.01
                );
                for (_, b) in &rects[i + 1..] {
                    let ox = a.max_x().min(b.max_x()) - a.x.max(b.x);
                    let oy = a.max_y().min(b.max_y()) - a.y.max(b.y);
                    assert!(ox <= 0.0 || oy <= 0.0, "{a:?} overlaps {b:?}");
                }
            }
        }
    }
}
