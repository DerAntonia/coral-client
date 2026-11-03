// wgpu-Rendering, Chunk-Meshing, Kamera.

pub mod block_renderer;
pub mod camera;
pub mod custom_sky;
pub mod entity_outline;
pub mod frustum;
pub mod gradient_sky;
pub mod glint;
pub mod lod_renderer;
pub mod motion_blur;
pub mod occlusion;
pub mod shader_path;
pub mod vanilla_draw;
pub mod vanilla_shader;
pub mod voxel_volume;
mod terrain_arena;

pub use block_renderer::{encode_tiles, upload_tiles, BlockRenderer, HitboxVertex, ParticleDraws, ParticleVertex, WeatherVertex, PARTICLE_VERTEX_CAPACITY, WEATHER_VERTEX_CAPACITY};
pub use camera::Camera;
pub use frustum::Frustum;

// Gegenfarbe zum Untergrund (contrast_color), angehaengt an block.wgsl und an den
// Fadenkreuz-Shader der App.
pub const CONTRAST_COLOR_WGSL: &str = include_str!("contrast_color.wgsl");

// block.wgsl samt contrast_color.wgsl, so wie die Pipelines und der Shader-Test ihn sehen.
pub fn block_shader_source() -> String {
    format!("{}\n{}", include_str!("block.wgsl"), CONTRAST_COLOR_WGSL)
}
