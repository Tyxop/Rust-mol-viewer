//! VR support with OpenXR
//!
//! This crate provides OpenXR integration for virtual reality support,
//! including session management, input handling, and UI rendering in VR space.

pub mod session;
pub mod input;
pub mod picking;
pub mod ui_panel;
pub mod ui_interaction;
pub mod performance;

// Re-export main types
pub use session::{
    FovConfig, SessionState, ViewConfig, VrSession, VrSwapchain,
};
pub use input::{
    ControllerState, Pose, VrInput,
};
pub use picking::controller_ray;
pub use ui_panel::VrUiPanel;
pub use ui_interaction::{ray_quad_intersection, uv_to_screen_pos, QuadIntersection};
pub use performance::{VrPerformanceMonitor, VrPerformanceStats, TARGET_FRAME_TIME_MS};

// Re-export openxr for external use
pub use openxr;
