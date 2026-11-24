//! The selected app icon, pre-scaled to the pixels it is drawn at.
//!
//! The icon files are large (a design's `ui.png` is 128 px, Classic's `icon.png` 652×514) and are
//! drawn at 18 px in the title bar. GPUI samples an image bilinearly with no mipmaps, so a 7×
//! shrink skips most source pixels and the edges come out jagged and soft. 1.x drew the same
//! files through Chromium, which filters properly. So the icon is resized here, once per size,
//! with a Lanczos filter to exactly `logical size × scale factor` device pixels.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{ImageSource, RenderImage};

thread_local! {
    /// (path, device px) → the scaled image. A miss (unreadable file) is cached as `None` so a
    /// broken file is not decoded again every frame.
    static CACHE: RefCell<HashMap<(PathBuf, u32), Option<Arc<RenderImage>>>> = RefCell::new(HashMap::new());
}

/// The icon at `path`, fitted (aspect kept) into a `logical_px` square at `scale`. Falls back to
/// the file itself, unscaled, if it cannot be decoded.
pub fn source(path: &Path, logical_px: f32, scale: f32) -> ImageSource {
    let device = (logical_px * scale.max(1.0)).round().max(1.0) as u32;
    let key = (path.to_path_buf(), device);
    let hit = CACHE.with(|c| c.borrow().get(&key).cloned());
    let image = match hit {
        Some(image) => image,
        None => {
            let image = scaled(path, device);
            CACHE.with(|c| c.borrow_mut().insert(key, image.clone()));
            image
        }
    };
    match image {
        Some(image) => ImageSource::Render(image),
        None => ImageSource::from(path.to_path_buf()),
    }
}

fn scaled(path: &Path, device: u32) -> Option<Arc<RenderImage>> {
    let rgba = image::load_from_memory(&std::fs::read(path).ok()?).ok()?.into_rgba8();
    let (w, h) = rgba.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    // Fit the longer side; Classic's icon is not square.
    let (tw, th) = if w >= h {
        (device, ((h as f32 * device as f32 / w as f32).round() as u32).max(1))
    } else {
        (((w as f32 * device as f32 / h as f32).round() as u32).max(1), device)
    };
    let mut out = image::imageops::resize(&rgba, tw, th, image::imageops::FilterType::Lanczos3);
    // RenderImage holds BGRA (as GPUI's own PNG loader produces).
    for px in out.pixels_mut() {
        px.0.swap(0, 2);
    }
    Some(Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(out)])))
}
