pub mod spheres;
pub mod ball_stick;
pub mod billboards;
pub mod ribbon;
pub mod surface;

pub use spheres::SpheresRenderer;
pub use ball_stick::BallStickRenderer;
pub use billboards::BillboardRenderer;
pub use ribbon::RibbonRenderer;
pub use surface::{SurfaceRenderer, SurfaceConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepresentationType {
    VanDerWaals,
    BallAndStick,
    Ribbon,
    Surface,
}
