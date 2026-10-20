//! Turns a [`Strip`] into window frames for one display.
//!
//! Coordinates: the strip's flowing columns live on a horizontal line that
//! starts at 0 ("strip space"). `strip.scroll` is the strip-space position of
//! the left edge of the scrollable area, so a column at strip offset `o` is
//! drawn at screen x `area.x + o - scroll`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::geometry::Rect;
use super::strip::{Mode, Side, Strip, WinId};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CenterMode {
    /// Scroll only as far as needed to show the focused column.
    #[default]
    Never,
    /// Left-align while everything fits; once the strip overflows, centre a
    /// column whenever focusing it requires scrolling.
    OnOverflow,
    /// Always keep the focused column centred.
    Always,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutParams {
    /// Space between columns and between stacked windows.
    pub gap: f64,
    pub center: CenterMode,
}

/// Edges a window may not hang over, because another display (or a pinned
/// column) is on that side.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Blocked {
    pub left: bool,
    pub right: bool,
}

/// Resolved horizontal geometry of a strip on one viewport.
#[derive(Clone, Debug, PartialEq)]
pub struct Geometry {
    /// Area the flowing columns scroll in (viewport minus pinned columns).
    pub area: Rect,
    /// Pixel width of every column, indexed like `strip.columns`.
    pub widths: Vec<f64>,
    /// Strip-space offset of every flowing column (pinned columns: 0).
    pub offsets: Vec<f64>,
    /// Total width of the flowing columns, gaps included.
    pub total: f64,
}

impl Geometry {
    pub fn new(strip: &Strip, viewport: Rect, gap: f64, min_widths: &HashMap<WinId, f64>) -> Geometry {
        let resolve = |frac: f64, avail: f64| (frac * (avail + gap) - gap).max(1.0);
        let col_min = |i: usize| {
            strip.columns[i].windows.iter().filter_map(|w| min_widths.get(w)).fold(0.0_f64, |a, &b| a.max(b))
        };

        let mut area = viewport;
        let mut widths = vec![0.0; strip.columns.len()];
        for (i, col) in strip.columns.iter().enumerate() {
            if let Some(side) = col.pin {
                let w = resolve(col.width, viewport.w).max(col_min(i)).min(viewport.w);
                widths[i] = w;
                let used = (w + gap).min(area.w - 1.0);
                area.w -= used;
                if side == Side::Left {
                    area.x += used;
                }
            }
        }

        let range = strip.flow_range();
        let mut offsets = vec![0.0; strip.columns.len()];
        let mut x = 0.0;
        for i in range.clone() {
            let w = resolve(strip.columns[i].width, area.w).max(col_min(i));
            widths[i] = w;
            offsets[i] = x;
            x += w + gap;
        }
        let total = if range.is_empty() { 0.0 } else { x - gap };
        Geometry { area, widths, offsets, total }
    }

    fn is_flow(&self, strip: &Strip, i: usize) -> bool {
        strip.columns[i].pin.is_none()
    }

    /// Scroll position that keeps every column on screen when possible.
    pub fn clamp_scroll(&self, scroll: f64, mode: CenterMode) -> f64 {
        if mode == CenterMode::Always {
            return scroll;
        }
        let max = (self.total - self.area.w).max(0.0);
        scroll.clamp(0.0, max)
    }

    /// Screen x of column `i` at `scroll`.
    pub fn column_x(&self, strip: &Strip, viewport: Rect, i: usize, scroll: f64) -> f64 {
        match strip.columns[i].pin {
            Some(Side::Left) => viewport.x,
            Some(Side::Right) => viewport.max_x() - self.widths[i],
            None => self.area.x + self.offsets[i] - scroll,
        }
    }

    /// Column under screen x (the gap right of a column counts as part of it).
    pub fn column_at(&self, strip: &Strip, viewport: Rect, gap: f64, x: f64) -> Option<usize> {
        (0..strip.columns.len()).find(|&i| {
            let cx = self.column_x(strip, viewport, i, strip.scroll);
            x >= cx && x < cx + self.widths[i] + gap
        })
    }

    /// Index at which a column dropped at screen x should be inserted.
    pub fn insert_index_at(&self, strip: &Strip, viewport: Rect, gap: f64, x: f64) -> usize {
        match self.column_at(strip, viewport, gap, x) {
            Some(i) => {
                let cx = self.column_x(strip, viewport, i, strip.scroll);
                if x > cx + self.widths[i] / 2.0 {
                    i + 1
                } else {
                    i
                }
            }
            None if strip.columns.is_empty() => 0,
            None => {
                let first = self.column_x(strip, viewport, 0, strip.scroll);
                if x < first {
                    0
                } else {
                    strip.columns.len()
                }
            }
        }
    }

    /// Whether column `i` is fully inside the scroll area at `scroll`.
    pub fn fully_visible(&self, strip: &Strip, i: usize, scroll: f64) -> bool {
        if !self.is_flow(strip, i) {
            return true;
        }
        let x = self.offsets[i];
        x >= scroll - 0.5 && x + self.widths[i] <= scroll + self.area.w + 0.5
    }

    /// The scroll position that the focused column wants, starting from the
    /// strip's current scroll.
    pub fn desired_scroll(&self, strip: &Strip, mode: CenterMode) -> f64 {
        let cur = strip.scroll;
        let f = strip.focus;
        if strip.columns.is_empty() || !self.is_flow(strip, f) {
            return self.clamp_scroll(cur, mode);
        }
        let (x, w, aw) = (self.offsets[f], self.widths[f], self.area.w);
        let centred = x + w / 2.0 - aw / 2.0;
        let target = match mode {
            CenterMode::Always => centred,
            CenterMode::OnOverflow => {
                if self.total <= aw {
                    0.0
                } else if self.fully_visible(strip, f, cur) {
                    cur
                } else {
                    centred
                }
            }
            CenterMode::Never => {
                if w >= aw || x < cur {
                    x
                } else if x + w > cur + aw {
                    x + w - aw
                } else {
                    cur
                }
            }
        };
        self.clamp_scroll(target, mode)
    }

    /// Where a released swipe should come to rest. `projected` is the scroll
    /// position extrapolated from the swipe velocity. Returns the column to
    /// focus and the scroll target.
    pub fn snap(&self, strip: &Strip, projected: f64, mode: CenterMode) -> (usize, f64) {
        let flow: Vec<usize> = strip.flow_range().collect();
        if flow.is_empty() {
            return (strip.focus, self.clamp_scroll(projected, mode));
        }
        let aw = self.area.w;
        let target = if mode == CenterMode::Always {
            let mid = projected + aw / 2.0;
            let best = *flow
                .iter()
                .min_by(|&&a, &&b| {
                    let da = (self.offsets[a] + self.widths[a] / 2.0 - mid).abs();
                    let db = (self.offsets[b] + self.widths[b] / 2.0 - mid).abs();
                    da.total_cmp(&db)
                })
                .unwrap();
            self.offsets[best] + self.widths[best] / 2.0 - aw / 2.0
        } else {
            let p = self.clamp_scroll(projected, mode);
            let max = (self.total - aw).max(0.0);
            // Rest with some column's left or right edge on the area's edge.
            flow.iter()
                .flat_map(|&i| [self.offsets[i], self.offsets[i] + self.widths[i] - aw])
                .chain([0.0, max])
                .map(|s| s.clamp(0.0, max))
                .min_by(|a, b| (a - p).abs().total_cmp(&(b - p).abs()))
                .unwrap()
        };

        let focus = if self.fully_visible(strip, strip.focus, target) {
            strip.focus
        } else {
            let mid = target + aw / 2.0;
            let dist = |i: usize| (self.offsets[i] + self.widths[i] / 2.0 - mid).abs();
            let visible: Vec<usize> = flow.iter().copied().filter(|&i| self.fully_visible(strip, i, target)).collect();
            let pool = if visible.is_empty() { &flow } else { &visible };
            *pool.iter().min_by(|&&a, &&b| dist(a).total_cmp(&dist(b))).unwrap()
        };
        (focus, target)
    }
}

/// Final placement of one window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub id: WinId,
    /// Frame the window would have at its place on the strip.
    pub frame: Rect,
    /// `Some` when the window is out of view and should be parked off screen
    /// on that side instead of being drawn at `frame`.
    pub parked: Option<Side>,
}

/// Compute frames for every window of the strip at its current scroll.
pub fn place(
    strip: &Strip,
    viewport: Rect,
    params: &LayoutParams,
    blocked: Blocked,
    min_widths: &HashMap<WinId, f64>,
) -> (Geometry, Vec<Placement>) {
    let geo = Geometry::new(strip, viewport, params.gap, min_widths);
    if strip.mode == Mode::Split {
        let out = strip.tree.layout(viewport, params.gap);
        return (geo, out.into_iter().map(|(id, frame)| Placement { id, frame, parked: None }).collect());
    }
    let mut out = Vec::with_capacity(strip.windows().count());
    let area = geo.area;
    let pinned_left = strip.columns.first().is_some_and(|c| c.pin == Some(Side::Left));
    let pinned_right = strip.columns.last().is_some_and(|c| c.pin == Some(Side::Right));

    for (i, col) in strip.columns.iter().enumerate() {
        let w = geo.widths[i];
        let x = match col.pin {
            Some(Side::Left) => viewport.x,
            Some(Side::Right) => viewport.max_x() - w,
            None => area.x + geo.offsets[i] - strip.scroll,
        };

        let parked = if col.pin.is_some() {
            None
        } else if x + w <= area.x + 0.5 {
            Some(Side::Left)
        } else if x >= area.max_x() - 0.5 {
            Some(Side::Right)
        } else {
            let over_left = x < area.x - 0.5;
            let over_right = x + w > area.max_x() + 0.5;
            let bad_left = over_left && (blocked.left || pinned_left);
            let bad_right = over_right && (blocked.right || pinned_right);
            if bad_left || bad_right {
                let centre = x + w / 2.0;
                Some(if centre < area.x + area.w / 2.0 { Side::Left } else { Side::Right })
            } else {
                None
            }
        };

        if col.tabbed {
            let frame = Rect::new(x, viewport.y, w, viewport.h);
            for (r, &id) in col.windows.iter().enumerate() {
                let hide = (r != col.active).then_some(Side::Right);
                out.push(Placement { id, frame, parked: parked.or(hide) });
            }
        } else {
            let n = col.len() as f64;
            let avail = (viewport.h - params.gap * (n - 1.0)).max(n);
            let total: f64 = col.weights.iter().sum();
            let mut y = viewport.y;
            for (r, &id) in col.windows.iter().enumerate() {
                let h = (avail * col.weights[r] / total).round();
                let h = if r + 1 == col.len() { viewport.max_y() - y } else { h };
                out.push(Placement { id, frame: Rect::new(x, y, w, h.max(1.0)), parked });
                y += h + params.gap;
            }
        }
    }
    (geo, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::strip::{Dir, OpenAt};

    const VP: Rect = Rect::new(0.0, 0.0, 1000.0, 800.0);

    fn params(center: CenterMode) -> LayoutParams {
        LayoutParams { gap: 10.0, center }
    }

    fn strip(n: u32, width: f64) -> Strip {
        let mut s = Strip::default();
        for id in 1..=n {
            s.insert_window(id, width, OpenAt::Next, true);
        }
        s.focus = 0;
        s
    }

    fn by_id(p: &[Placement], id: WinId) -> Placement {
        *p.iter().find(|x| x.id == id).unwrap()
    }

    #[test]
    fn two_halves_fill_the_viewport_exactly() {
        let s = strip(2, 0.5);
        let (geo, p) = place(&s, VP, &params(CenterMode::Never), Blocked::default(), &HashMap::new());
        assert_eq!(geo.widths, vec![495.0, 495.0]);
        assert_eq!(by_id(&p, 1).frame, Rect::new(0.0, 0.0, 495.0, 800.0));
        assert_eq!(by_id(&p, 2).frame, Rect::new(505.0, 0.0, 495.0, 800.0));
        assert!(p.iter().all(|x| x.parked.is_none()));
        assert_eq!(geo.total, 1000.0);
    }

    #[test]
    fn widths_survive_new_windows() {
        let s = strip(5, 0.5);
        let (geo, p) = place(&s, VP, &params(CenterMode::Never), Blocked::default(), &HashMap::new());
        assert!(geo.widths.iter().all(|&w| w == 495.0));
        // Columns 3.. are off to the right and parked.
        assert_eq!(by_id(&p, 3).parked, Some(Side::Right));
        assert_eq!(by_id(&p, 5).parked, Some(Side::Right));
    }

    #[test]
    fn scroll_just_enough() {
        let mut s = strip(4, 0.5);
        let geo = Geometry::new(&s, VP, 10.0, &HashMap::new());
        s.focus_index(2);
        let t = geo.desired_scroll(&s, CenterMode::Never);
        // Column 2 starts at 1010 and is 495 wide: right edge must be at 1000.
        assert_eq!(t, 1010.0 + 495.0 - 1000.0);
        s.scroll = t;
        s.focus_dir(Dir::Left);
        // Column 1 (505..1000) is fully visible at scroll 505: no movement.
        assert_eq!(geo.desired_scroll(&s, CenterMode::Never), 505.0);
        s.focus_first();
        assert_eq!(geo.desired_scroll(&s, CenterMode::Never), 0.0);
    }

    #[test]
    fn center_modes() {
        let mut s = strip(4, 0.5);
        let geo = Geometry::new(&s, VP, 10.0, &HashMap::new());
        s.focus_index(1);
        // Always: centre column 1 (505..1000 -> centre 752.5).
        assert_eq!(geo.desired_scroll(&s, CenterMode::Always), 752.5 - 500.0);
        // OnOverflow: column 1 is visible at scroll 0, so stay.
        assert_eq!(geo.desired_scroll(&s, CenterMode::OnOverflow), 0.0);
        s.focus_index(2);
        assert_eq!(geo.desired_scroll(&s, CenterMode::OnOverflow), 1010.0 + 247.5 - 500.0);
        // Everything fits: OnOverflow left-aligns.
        let mut small = strip(2, 0.3);
        small.focus_index(1);
        let g2 = Geometry::new(&small, VP, 10.0, &HashMap::new());
        assert_eq!(g2.desired_scroll(&small, CenterMode::OnOverflow), 0.0);
    }

    #[test]
    fn scroll_is_clamped_after_close() {
        let mut s = strip(3, 0.5);
        s.focus_index(2);
        s.scroll = 515.0;
        s.remove_window(3);
        let geo = Geometry::new(&s, VP, 10.0, &HashMap::new());
        assert_eq!(geo.desired_scroll(&s, CenterMode::Never), 0.0);
    }

    #[test]
    fn partially_visible_columns_peek_unless_blocked() {
        let mut s = strip(3, 0.6);
        s.scroll = 100.0;
        let p0 = params(CenterMode::Never);
        let (_, p) = place(&s, VP, &p0, Blocked::default(), &HashMap::new());
        // Column 0 is 594 wide at x=-100: hangs off the left edge.
        assert_eq!(by_id(&p, 1).parked, None);
        assert_eq!(by_id(&p, 1).frame.x, -100.0);
        let (_, p) = place(&s, VP, &p0, Blocked { left: true, right: false }, &HashMap::new());
        assert_eq!(by_id(&p, 1).parked, Some(Side::Left));
        // Column 1 starts at 504 and overflows to the right.
        let (_, p) = place(&s, VP, &p0, Blocked { left: false, right: true }, &HashMap::new());
        assert_eq!(by_id(&p, 2).parked, Some(Side::Right));
    }

    #[test]
    fn stacked_heights_and_tabs() {
        let mut s = strip(2, 0.5);
        s.focus_index(1);
        s.stack_into(Side::Left);
        let (_, p) = place(&s, VP, &params(CenterMode::Never), Blocked::default(), &HashMap::new());
        assert_eq!(by_id(&p, 1).frame, Rect::new(0.0, 0.0, 495.0, 395.0));
        assert_eq!(by_id(&p, 2).frame, Rect::new(0.0, 405.0, 495.0, 395.0));
        s.toggle_tabbed();
        let (_, p) = place(&s, VP, &params(CenterMode::Never), Blocked::default(), &HashMap::new());
        assert_eq!(by_id(&p, 2).parked, None, "active tab is shown");
        assert_eq!(by_id(&p, 2).frame.h, 800.0);
        assert_eq!(by_id(&p, 1).parked, Some(Side::Right), "inactive tab is parked");
    }

    #[test]
    fn pinned_column_shrinks_the_scroll_area() {
        let mut s = strip(3, 0.5);
        s.focus_index(2);
        s.toggle_pin(Side::Left);
        s.columns[0].width = 0.3;
        let (geo, p) = place(&s, VP, &params(CenterMode::Never), Blocked::default(), &HashMap::new());
        let pinned_w = 0.3 * 1010.0 - 10.0;
        assert_eq!(by_id(&p, 3).frame, Rect::new(0.0, 0.0, pinned_w, 800.0));
        assert_eq!(geo.area.x, pinned_w + 10.0);
        assert_eq!(geo.area.w, 1000.0 - pinned_w - 10.0);
        // A flowing column scrolled under the pinned one is parked, not overlapped.
        s.scroll = 50.0;
        let (_, p) = place(&s, VP, &params(CenterMode::Never), Blocked::default(), &HashMap::new());
        assert_eq!(by_id(&p, 1).parked, Some(Side::Left));
    }

    #[test]
    fn stubborn_min_width_pushes_neighbours() {
        let s = strip(2, 0.5);
        let mins = HashMap::from([(1, 600.0)]);
        let (geo, p) = place(&s, VP, &params(CenterMode::Never), Blocked::default(), &mins);
        assert_eq!(geo.widths[0], 600.0);
        assert_eq!(by_id(&p, 2).frame.x, 610.0);
    }

    #[test]
    fn split_mode_fills_the_viewport() {
        let mut s = strip(3, 0.5);
        s.set_mode(Mode::Split, 0.5);
        let (_, p) = place(&s, VP, &params(CenterMode::Never), Blocked::default(), &HashMap::new());
        assert_eq!(p.len(), 3);
        assert!(p.iter().all(|x| x.parked.is_none()));
        assert_eq!(by_id(&p, 1).frame, Rect::new(0.0, 0.0, 495.0, 800.0));
        let area: f64 = p.iter().map(|x| x.frame.w * x.frame.h).sum();
        assert!(area > 0.95 * 1000.0 * 800.0, "split tiles cover the screen minus gaps");
    }

    #[test]
    fn hit_testing_columns() {
        let mut s = strip(3, 0.5);
        s.scroll = 505.0;
        let geo = Geometry::new(&s, VP, 10.0, &HashMap::new());
        // On screen: column 1 at 0..495, column 2 at 505..1000, column 0 off left.
        assert_eq!(geo.column_at(&s, VP, 10.0, 100.0), Some(1));
        assert_eq!(geo.column_at(&s, VP, 10.0, 500.0), Some(1), "gap belongs to the left column");
        assert_eq!(geo.column_at(&s, VP, 10.0, 600.0), Some(2));
        assert_eq!(geo.insert_index_at(&s, VP, 10.0, 100.0), 1);
        assert_eq!(geo.insert_index_at(&s, VP, 10.0, 900.0), 3);
        assert_eq!(geo.insert_index_at(&s, VP, 10.0, -900.0), 0);
        assert_eq!(geo.insert_index_at(&Strip::default(), VP, 10.0, 5.0), 0);
    }

    #[test]
    fn swipe_snaps_to_column_edges() {
        let s = strip(4, 0.5);
        let geo = Geometry::new(&s, VP, 10.0, &HashMap::new());
        let (focus, target) = geo.snap(&s, 480.0, CenterMode::Never);
        assert_eq!(target, 505.0);
        assert_eq!(focus, 1, "old focus (0) went off screen; nearest visible column wins");
        let (_, target) = geo.snap(&s, 5000.0, CenterMode::Never);
        assert_eq!(target, geo.total - 1000.0);
        // Viewport centre at 300 + 500 = 800 is nearest column 1 (centre 752.5).
        let (focus, target) = geo.snap(&s, 300.0, CenterMode::Always);
        assert_eq!(focus, 1);
        assert_eq!(target, 505.0 + 247.5 - 500.0);
    }
}
