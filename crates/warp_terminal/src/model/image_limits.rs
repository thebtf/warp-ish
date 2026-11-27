//! Per-image budgets for data received from a terminal application.

pub use warpui_core::image_cache::MAX_TERMINAL_IMAGE_BYTES;
pub const MAX_TERMINAL_IMAGE_PIXELS: usize = MAX_TERMINAL_IMAGE_BYTES / 4;

pub fn valid_image_dimensions(width: u32, height: u32) -> bool {
    width > 0
        && height > 0
        && u64::from(width) * u64::from(height) <= MAX_TERMINAL_IMAGE_PIXELS as u64
}
