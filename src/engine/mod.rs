//! Platform-independent window-layout engine.

pub mod geometry;
pub mod layout;
pub mod session;
pub mod spring;
pub mod strip;
pub mod tree;

pub use geometry::Rect;
pub use layout::{place, Blocked, CenterMode, Geometry, LayoutParams};
pub use spring::Spring;
pub use strip::{Dir, Mode, OpenAt, Side, Strip, WinId};
pub use tree::Axis;

use serde::{Deserialize, Serialize};

/// Identifies one strip: a display (by its stable UUID), a macOS Space on
/// it, and a Stripline workspace (1-9) within that.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StripKey {
    pub display: String,
    pub space: u64,
    #[serde(default = "first_workspace")]
    pub workspace: u8,
}

fn first_workspace() -> u8 {
    1
}
