//! Platform-independent window-layout engine.

pub mod geometry;
pub mod layout;
pub mod session;
pub mod spring;
pub mod strip;

pub use geometry::Rect;
pub use layout::{place, Blocked, CenterMode, Geometry, LayoutParams};
pub use spring::Spring;
pub use strip::{Dir, OpenAt, Side, Strip, WinId};

use serde::{Deserialize, Serialize};

/// Identifies one strip: a display (by its stable UUID) and a macOS Space on it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StripKey {
    pub display: String,
    pub space: u64,
}
