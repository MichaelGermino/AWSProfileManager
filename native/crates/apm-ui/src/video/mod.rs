//! Background video (1.x `BackgroundVideo.tsx` + the `appmedia://` protocol).
//!
//! A platform decoder runs on its own thread and hands finished BGRA frames to the window as
//! `RenderImage`s, which GPUI uploads to its sprite atlas and draws under the UI.
//!
//! Cost is kept down by doing as little per frame as possible:
//! - frames are produced at (about) the size they are drawn, never larger than the video, and at
//!   most `MAX_FRAME` a side (see `mf.rs` for how: CPU scaling of the decoder's NV12 for software
//!   decode, the GPU video processor for hardware decode);
//! - behind the app (scope "app") the video is blurred, so it is decoded much smaller than the
//!   window and blurred on the CPU at that size, which is cheap; GPUI's linear sampling scales it up;
//! - at most 30 frames a second are shown (fewer if the video has fewer), paced by timestamp;
//! - nothing is decoded while the window is hidden or minimized, or while the video is not on
//!   screen, and the decoder is released when the window closes (the entity is dropped with it).
//!
//! Playback loops on end of stream rather than at the duration in the file header, so a fragmented
//! MP4 whose header declares a shorter duration plays to the end (a 1.x bug).
//!
//! GPUI has no way to update an atlas texture in place: each frame is a new `RenderImage`, and the
//! previous one is released from the atlas when the next arrives (`Window::drop_image`). At the
//! sizes used here that upload is small, and the frame shares the atlas texture with the UI's other
//! images instead of needing a texture of its own. The pixel buffer cannot be reused either
//! (`RenderImage` owns it and gives no way back), so each frame is one allocation.
//!
//! The UI over the video ("glass") is drawn by the shell and the views through
//! `AppModel::surface_palette` and `Palette::glass_tint`.

// The frame pipeline helpers are used by the Windows decoder; other platforms have a stub for now.
#![cfg_attr(not(windows), allow(dead_code))]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use futures::channel::mpsc;
use futures::StreamExt as _;
use gpui::{div, img, prelude::*, Context, ObjectFit, RenderImage, Task, Window};

#[cfg(windows)]
mod mf;

/// 1.x `DEFAULT_BACKGROUND_BLUR` / `MIN_` / `MAX_`: low enough that the video still visibly
/// moves, high enough that text stays comfortable.
pub const DEFAULT_BLUR: f32 = 12.0;
pub const MIN_BLUR: f32 = 0.0;
pub const MAX_BLUR: f32 = 40.0;

/// Largest decoded frame side, in pixels (see [`Params::output_size`]).
pub const MAX_FRAME: u32 = 1024;

/// What to play and how. Sizes are in logical pixels of the area the video covers.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoRequest {
    pub path: PathBuf,
    pub width: f32,
    pub height: f32,
    /// Blur radius in logical pixels (CSS `blur()`, i.e. a Gaussian standard deviation). 0 = sharp.
    pub blur: f32,
}

/// Output settings the decoder reads before each frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Params {
    /// Cover-fit target in pixels; the decoder never scales above the video's own size.
    pub width: u32,
    pub height: u32,
    pub blur: f32,
}

impl Params {
    /// How much smaller than the target the video is decoded when it is blurred. A blur of σ px
    /// removes detail finer than ~σ, so decoding at 1/(σ/2) loses nothing visible and leaves a
    /// small blur (σ ≈ 2 px) to do at that size: enough that GPUI's bilinear upscale shows no
    /// blocks.
    pub fn downscale(&self) -> f32 {
        (self.blur / 2.0).floor().clamp(1.0, 20.0)
    }

    /// The decoded frame size for a `vw` × `vh` video: cover the target, divided by the blur
    /// downscale, never larger than the video, even dimensions.
    pub fn output_size(&self, vw: u32, vh: u32) -> (u32, u32) {
        let cover = (self.width as f32 / vw as f32).max(self.height as f32 / vh as f32);
        // At most MAX_FRAME on either side: a frame that size shares GPUI's 1024² sprite atlas
        // texture with the UI's other images, so each new frame reuses atlas space instead of
        // creating (and destroying) a dedicated texture.
        let fit = (MAX_FRAME as f32 / vw as f32).min(MAX_FRAME as f32 / vh as f32);
        let s = (cover / self.downscale()).min(1.0).min(fit);
        let even = |v: f32| ((v.round() as u32) & !1).max(2);
        (even(vw as f32 * s), even(vh as f32 * s))
    }

    /// σ left to apply on the decoded frame, in its own pixels.
    pub fn residual_sigma(&self) -> f32 {
        if self.blur <= 0.0 {
            0.0
        } else {
            self.blur / self.downscale()
        }
    }
}

/// UI ↔ decoder thread.
pub(crate) struct Control {
    state: Mutex<State>,
    cv: Condvar,
}

#[derive(Clone, Copy)]
struct State {
    paused: bool,
    stop: bool,
    params: Params,
    /// Bumped when `params` changes, so the decoder reconfigures once.
    generation: u64,
}

pub(crate) enum Wake {
    Stop,
    /// Resumed after this long paused.
    Run { paused_for: Duration, params: Params, generation: u64 },
}

impl Control {
    fn new(params: Params, paused: bool) -> Self {
        Control { state: Mutex::new(State { paused, stop: false, params, generation: 0 }), cv: Condvar::new() }
    }

    fn update(&self, f: impl FnOnce(&mut State)) {
        let mut s = self.state.lock().unwrap();
        f(&mut s);
        self.cv.notify_all();
    }

    /// Decoder: block while paused; report how long, and the current params.
    pub fn wait_runnable(&self) -> Wake {
        let mut s = self.state.lock().unwrap();
        let started = Instant::now();
        while s.paused && !s.stop {
            s = self.cv.wait(s).unwrap();
        }
        if s.stop {
            return Wake::Stop;
        }
        Wake::Run { paused_for: started.elapsed(), params: s.params, generation: s.generation }
    }

    /// Decoder: sleep until `due`, returning early when anything changes (stop, pause, params).
    pub fn sleep_until(&self, due: Instant, generation: u64) {
        let mut s = self.state.lock().unwrap();
        loop {
            if s.stop || s.paused || s.generation != generation {
                return;
            }
            let now = Instant::now();
            if now >= due {
                return;
            }
            s = self.cv.wait_timeout(s, due - now).unwrap().0;
        }
    }

    pub fn stopped(&self) -> bool {
        self.state.lock().unwrap().stop
    }
}

pub(crate) enum Msg {
    /// A frame and its mean relative luminance (0–1, linear light), for the overlay tint.
    Frame(Arc<RenderImage>, f32),
    /// The file could not be opened or decoded; it is not retried until it changes.
    Failed(String),
}

/// Wrap decoded BGRA pixels (alpha already 255) for GPUI. `RenderImage` stores BGRA, so the
/// bytes go in unchanged.
pub(crate) fn render_image(width: u32, height: u32, bgra: Vec<u8>) -> Option<Arc<RenderImage>> {
    let buf = image::RgbaImage::from_raw(width, height, bgra)?;
    Some(Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(buf)])))
}

/// One decoded NV12 frame (the decoders' native output): a Y plane and an interleaved UV plane at
/// half resolution, both `pitch` bytes per row.
pub(crate) struct Nv12<'a> {
    pub y: &'a [u8],
    pub uv: &'a [u8],
    pub pitch: usize,
    /// Visible size (the decoded surface can be taller, e.g. 1088 for 1080p).
    pub width: usize,
    pub height: usize,
    /// BT.709 (HD) rather than BT.601 (SD) colour.
    pub bt709: bool,
    /// 0–255 levels rather than video's 16–235.
    pub full_range: bool,
}

/// Convert NV12 to BGRA (alpha 255) at `ow` × `oh`, averaging each output pixel's block of source
/// pixels (a box filter, so downscaling does not alias). Doing this on the CPU is cheap because
/// the output is small: the work is one pass over the source, and it replaces Media Foundation's
/// video processor, which measured ~8 MB of extra buffers.
pub(crate) fn nv12_to_bgra(src: &Nv12, ow: usize, oh: usize, out: &mut Vec<u8>) {
    out.clear();
    out.resize(ow * oh * 4, 0);
    // (Y scale, R←V, G←U, G←V, B←U) in 1/256 units.
    let (ys, rv, gu, gv, bu, y0) = match (src.bt709, src.full_range) {
        (true, false) => (298, 459, -55, -136, 541, 16),
        (false, false) => (298, 409, -100, -208, 516, 16),
        (true, true) => (256, 403, -48, -120, 475, 0),
        (false, true) => (256, 359, -88, -183, 454, 0),
    };
    let to_bgra = |y: i32, u: i32, v: i32, px: &mut [u8]| {
        let c = ys * (y - y0);
        let (d, e) = (u - 128, v - 128);
        px[0] = ((c + bu * d + 128) >> 8).clamp(0, 255) as u8;
        px[1] = ((c + gu * d + gv * e + 128) >> 8).clamp(0, 255) as u8;
        px[2] = ((c + rv * e + 128) >> 8).clamp(0, 255) as u8;
        px[3] = 255;
    };
    let (w, h, pitch) = (src.width, src.height, src.pitch);
    if ow == w && oh == h {
        // 1:1 (a sharp background no larger than MAX_FRAME): straight conversion.
        for (row, line) in out.chunks_exact_mut(ow * 4).enumerate() {
            let yrow = &src.y[row * pitch..row * pitch + w];
            let uvrow = &src.uv[(row / 2) * pitch..(row / 2) * pitch + (w / 2) * 2];
            for (x, px) in line.chunks_exact_mut(4).enumerate() {
                let c = (x / 2) * 2;
                let (u, v) = if c + 1 < uvrow.len() { (uvrow[c] as i32, uvrow[c + 1] as i32) } else { (128, 128) };
                to_bgra(yrow[x] as i32, u, v, px);
            }
        }
        return;
    }
    // Source span [a, b) for each output column / row.
    let spans = |n_out: usize, n_in: usize| -> Vec<(usize, usize)> {
        (0..n_out)
            .map(|o| {
                let a = o * n_in / n_out;
                let b = ((o + 1) * n_in / n_out).max(a + 1).min(n_in);
                (a, b)
            })
            .collect()
    };
    let xs = spans(ow, w);
    let ys_ = spans(oh, h);
    let cw = w / 2;
    let ch = h.div_ceil(2);
    // Chroma spans: the half-resolution samples under each output pixel.
    let cxs: Vec<(usize, usize)> = xs.iter().map(|&(a, b)| (a / 2, b.div_ceil(2).min(cw).max(a / 2 + 1).min(cw.max(1)))).collect();
    // Column sums over the current block of source rows: one sequential, vectorisable pass per
    // source row, then a short horizontal sum per output pixel.
    let mut ycol = vec![0u32; w];
    let mut ucol = vec![0u32; cw.max(1)];
    let mut vcol = vec![0u32; cw.max(1)];
    for (oy, &(ya, yb)) in ys_.iter().enumerate() {
        ycol.iter_mut().for_each(|v| *v = 0);
        for row in ya..yb {
            for (acc, &v) in ycol.iter_mut().zip(&src.y[row * pitch..row * pitch + w]) {
                *acc += v as u32;
            }
        }
        let (ca, cb) = (ya / 2, yb.div_ceil(2).min(ch).max(ya / 2 + 1));
        ucol.iter_mut().for_each(|v| *v = 0);
        vcol.iter_mut().for_each(|v| *v = 0);
        for row in ca..cb {
            let line = &src.uv[row * pitch..row * pitch + cw * 2];
            for (c, pair) in line.chunks_exact(2).enumerate() {
                ucol[c] += pair[0] as u32;
                vcol[c] += pair[1] as u32;
            }
        }
        let (rows, crows) = ((yb - ya) as u32, (cb - ca) as u32);
        let line = &mut out[oy * ow * 4..(oy + 1) * ow * 4];
        for (ox, px) in line.chunks_exact_mut(4).enumerate() {
            let (xa, xb) = xs[ox];
            let n = rows * (xb - xa) as u32;
            let ysum: u32 = ycol[xa..xb].iter().sum();
            let (cxa, cxb) = cxs[ox];
            let (u, v) = if cw > 0 && cxb > cxa {
                let cn = crows * (cxb - cxa) as u32;
                let us: u32 = ucol[cxa..cxb].iter().sum();
                let vs: u32 = vcol[cxa..cxb].iter().sum();
                (((us + cn / 2) / cn) as i32, ((vs + cn / 2) / cn) as i32)
            } else {
                (128, 128)
            };
            to_bgra(((ysum + n / 2) / n) as i32, u, v, px);
        }
    }
}

/// Mean relative luminance (linear light, Rec. 709 weights) of a BGRA frame, sampled sparsely.
pub(crate) fn mean_luminance(px: &[u8]) -> f32 {
    static LINEAR: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    let table = LINEAR.get_or_init(|| std::array::from_fn(|i| (i as f32 / 255.0).powf(2.2)));
    let lin = |v: u8| table[v as usize];
    let (mut sum, mut n) = (0.0f32, 0u32);
    for c in px.chunks_exact(4).step_by(7) {
        sum += 0.0722 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.2126 * lin(c[2]);
        n += 1;
    }
    if n == 0 { 0.0 } else { sum / n as f32 }
}

/// Approximate a Gaussian blur of standard deviation `sigma` (pixels) with two box blurs (a tent
/// filter; plenty for a background), separably, in place. `scratch` is reused between frames.
pub(crate) fn blur_bgra(px: &mut [u8], w: usize, h: usize, sigma: f32, scratch: &mut Vec<u8>) {
    if sigma < 0.5 || w < 2 || h < 2 {
        return;
    }
    // Two passes of width `2r+1` have variance 2·((2r+1)²−1)/12.
    let width = (6.0 * sigma * sigma + 1.0).sqrt();
    let r = (((width - 1.0) / 2.0).round() as usize).max(1);
    scratch.resize(px.len(), 0);
    let mut sums = Vec::new();
    for _ in 0..2 {
        box_pass(px, scratch, w, h, r, 4, w * 4); // along rows
        box_columns(scratch, px, w, h, r, &mut sums); // down columns
    }
}

/// The vertical box blur, a whole row at a time (a running sum per byte of the row), so memory is
/// walked in order. Reads `src`, writes `dst` (alpha set to 255).
fn box_columns(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize, sums: &mut Vec<u32>) {
    let stride = w * 4;
    let div = (2 * r + 1) as u32;
    let mul = ((1u32 << 16) + div / 2) / div;
    let row = |y: usize| &src[y * stride..(y + 1) * stride];
    sums.clear();
    sums.resize(stride, 0);
    for i in 0..=2 * r {
        for (s, &v) in sums.iter_mut().zip(row(i.saturating_sub(r).min(h - 1))) {
            *s += v as u32;
        }
    }
    for y in 0..h {
        for (d, &s) in dst[y * stride..(y + 1) * stride].iter_mut().zip(sums.iter()) {
            *d = ((s * mul + (1 << 15)) >> 16) as u8;
        }
        let (add, sub) = (row((y + r + 1).min(h - 1)), row(y.saturating_sub(r)));
        for ((s, &a), &b) in sums.iter_mut().zip(add).zip(sub) {
            *s = *s + a as u32 - b as u32;
        }
    }
    for px in dst.chunks_exact_mut(4) {
        px[3] = 255;
    }
}

/// One box blur along lines: `n` samples per line, `lines` lines; `step` bytes between samples,
/// `line_step` bytes between lines. Edges clamp. Reads `src`, writes `dst` (alpha set to 255).
fn box_pass(src: &[u8], dst: &mut [u8], n: usize, lines: usize, r: usize, step: usize, line_step: usize) {
    // Division by the window width as a multiply and shift.
    let div = (2 * r + 1) as u32;
    let mul = ((1u32 << 16) + div / 2) / div;
    let last = n - 1;
    for line in 0..lines {
        let base = line * line_step;
        let px = |i: usize| -> [u32; 3] {
            let o = base + i * step;
            [src[o] as u32, src[o + 1] as u32, src[o + 2] as u32]
        };
        let mut sum = [0u32; 3];
        for i in 0..=2 * r {
            let p = px(i.saturating_sub(r).min(last));
            for c in 0..3 {
                sum[c] += p[c];
            }
        }
        for i in 0..n {
            let o = base + i * step;
            dst[o] = ((sum[0] * mul + (1 << 15)) >> 16) as u8;
            dst[o + 1] = ((sum[1] * mul + (1 << 15)) >> 16) as u8;
            dst[o + 2] = ((sum[2] * mul + (1 << 15)) >> 16) as u8;
            dst[o + 3] = 255;
            let add = px((i + r + 1).min(last));
            let sub = px(i.saturating_sub(r));
            for c in 0..3 {
                sum[c] = sum[c] + add[c] - sub[c];
            }
        }
    }
}

/// Start decoding on a new thread. Frames and failures arrive on the returned channel.
fn spawn_decoder(path: &Path, ctl: Arc<Control>) -> mpsc::Receiver<Msg> {
    let (tx, rx) = mpsc::channel(1);
    #[cfg(windows)]
    {
        let path = path.to_path_buf();
        let _ = std::thread::Builder::new().name("background-video".into()).spawn(move || mf::run(path, ctl, tx));
    }
    #[cfg(not(windows))]
    {
        // macOS (AVFoundation) comes with the Mac pass; until then the plain background is used.
        let _ = (path, ctl);
        let mut tx = tx;
        let _ = tx.try_send(Msg::Failed("Background video is not supported on this platform yet.".into()));
    }
    rx
}

struct Player {
    path: PathBuf,
    ctl: Arc<Control>,
    _task: Task<()>,
}

impl Drop for Player {
    fn drop(&mut self) {
        // The thread notices, releases the reader (and its D3D device) and exits.
        self.ctl.update(|s| s.stop = true);
    }
}

/// The video layer of the main window. Owned by the shell; dropped (decoder and all) with it.
pub struct BackgroundVideo {
    player: Option<Player>,
    current: Option<Arc<RenderImage>>,
    /// A file that failed to play (1.x `markVideoUnavailable`): treated as no video until the
    /// setting points somewhere else.
    failed: Option<PathBuf>,
    frames: u64,
    luminance: Option<f32>,
    debug_mark: Option<Instant>,
}

impl Default for BackgroundVideo {
    fn default() -> Self {
        Self::new()
    }
}

impl BackgroundVideo {
    pub fn new() -> Self {
        BackgroundVideo { player: None, current: None, failed: None, frames: 0, luminance: None, debug_mark: None }
    }

    /// A frame is on screen (the UI switches to its translucent "glass" surfaces only then, so
    /// it never shows a see-through panel over nothing).
    pub fn showing(&self) -> bool {
        self.current.is_some()
    }

    /// The frame on screen now.
    pub fn frame(&self) -> Option<Arc<RenderImage>> {
        self.current.clone()
    }

    /// Mean relative luminance of the recent frames (0 = black, 1 = white), once one is shown.
    pub fn luminance(&self) -> Option<f32> {
        self.current.as_ref().and(self.luminance)
    }

    /// Frames shown since the window opened (diagnostics).
    pub fn frames_shown(&self) -> u64 {
        self.frames
    }

    /// Bring playback in line with what the window wants: `None` stops and releases the decoder;
    /// `visible: false` pauses it (the decoder stays open, so it resumes at once).
    pub fn sync(&mut self, want: Option<VideoRequest>, visible: bool, window: &mut Window, cx: &mut Context<Self>) {
        let want = want.filter(|w| self.failed.as_deref() != Some(w.path.as_path()));
        let Some(want) = want else {
            if self.player.take().is_some() || self.current.is_some() {
                self.clear(window);
                cx.notify();
            }
            return;
        };
        let params = Params {
            width: (want.width.max(1.0)).round() as u32,
            height: (want.height.max(1.0)).round() as u32,
            blur: want.blur.max(0.0),
        };
        if self.player.as_ref().map(|p| p.path != want.path).unwrap_or(true) {
            self.player = None;
            self.clear(window);
            let ctl = Arc::new(Control::new(params, !visible));
            let mut rx = spawn_decoder(&want.path, ctl.clone());
            let path = want.path.clone();
            let task = cx.spawn_in(window, async move |this, cx| {
                while let Some(msg) = rx.next().await {
                    let alive = this.update_in(cx, |this, window, cx| match msg {
                        Msg::Frame(image, luma) => this.present(image, luma, window, cx),
                        Msg::Failed(e) => {
                            eprintln!("background video: {e}");
                            this.failed = Some(path.clone());
                            this.player = None;
                            this.clear(window);
                            cx.notify();
                        }
                    });
                    if alive.is_err() {
                        break;
                    }
                }
            });
            self.player = Some(Player { path: want.path, ctl, _task: task });
            return;
        }
        if let Some(p) = &self.player {
            p.ctl.update(|s| {
                s.paused = !visible;
                // Resizes arrive per pixel while dragging; reconfigure only on a real change
                // (more than 1/8 either way) so a drag does not rebuild the pipeline each frame.
                let resize = |a: u32, b: u32| (a as f32 - b as f32).abs() > b as f32 / 8.0;
                if resize(params.width, s.params.width) || resize(params.height, s.params.height) || params.blur != s.params.blur {
                    s.params = params;
                    s.generation += 1;
                }
            });
        }
    }

    /// Pause or resume without a redraw (the window may be hidden and not drawing).
    pub fn set_paused(&mut self, paused: bool) {
        if let Some(p) = &self.player {
            p.ctl.update(|s| s.paused = paused);
        }
    }

    fn present(&mut self, image: Arc<RenderImage>, luma: f32, window: &mut Window, cx: &mut Context<Self>) {
        if self.player.is_none() {
            return;
        }
        // Smoothed, so the tint follows scene changes without flickering frame to frame.
        self.luminance = Some(match self.luminance {
            Some(l) => l + (luma - l) * 0.1,
            None => luma,
        });
        if let Some(old) = self.current.replace(image) {
            // Free the previous frame's atlas tile now; nothing draws it after this.
            let _ = window.drop_image(old);
        }
        self.frames += 1;
        if self.frames % 150 == 0 && std::env::var_os("APM_VIDEO_DEBUG").is_some() {
            let now = Instant::now();
            if let Some(t) = self.debug_mark.replace(now) {
                eprintln!("background video: {:.1} frames/s shown", 150.0 / (now - t).as_secs_f64());
            }
        }
        cx.notify();
    }

    fn clear(&mut self, window: &mut Window) {
        if let Some(old) = self.current.take() {
            let _ = window.drop_image(old);
        }
        self.luminance = None;
    }
}

/// The current frame as a full-size layer (cover fit), for the shell to place under the UI.
/// Drawing the same frame twice (under the wizard as well) reuses one atlas tile.
pub fn layer(frame: Arc<RenderImage>) -> gpui::Div {
    div().absolute().top_0().left_0().size_full().child(img(frame).size_full().object_fit(ObjectFit::Cover))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_size_covers_and_downscales() {
        let p = Params { width: 1280, height: 800, blur: 0.0 };
        // Never above the video's own size, nor above MAX_FRAME a side.
        assert_eq!(p.output_size(640, 360), (640, 360));
        assert_eq!(p.output_size(1280, 720), (1024, 576));
        assert_eq!(p.output_size(3840, 2160), (1024, 576));
        let blurred = Params { blur: 12.0, ..p };
        assert_eq!(blurred.downscale(), 6.0);
        assert_eq!(blurred.output_size(3840, 2160), (236, 132));
        assert!((blurred.residual_sigma() - 2.0).abs() < 1e-6);
    }

    /// A flat NV12 frame of one colour.
    fn flat_nv12(w: usize, h: usize, pitch: usize, y: u8, u: u8, v: u8) -> (Vec<u8>, Vec<u8>) {
        let yp = vec![y; pitch * h];
        let mut uv = vec![0u8; pitch * h.div_ceil(2)];
        for row in uv.chunks_exact_mut(pitch) {
            for c in row.chunks_exact_mut(2) {
                c[0] = u;
                c[1] = v;
            }
        }
        (yp, uv)
    }

    #[test]
    fn nv12_converts_and_downscales() {
        // Limited-range BT.709 white, black and a mid grey; pitch wider than the image.
        for (y, expect) in [(235u8, 255u8), (16, 0), (126, 128)] {
            let (yp, uv) = flat_nv12(64, 36, 80, y, 128, 128);
            let src = Nv12 { y: &yp, uv: &uv, pitch: 80, width: 64, height: 36, bt709: true, full_range: false };
            let mut out = Vec::new();
            for (ow, oh) in [(64, 36), (16, 9), (7, 5)] {
                nv12_to_bgra(&src, ow, oh, &mut out);
                assert_eq!(out.len(), ow * oh * 4);
                for px in out.chunks(4) {
                    for c in 0..3 {
                        assert!((px[c] as i32 - expect as i32).abs() <= 1, "y {y} -> {px:?} at {ow}x{oh}");
                    }
                    assert_eq!(px[3], 255);
                }
            }
        }
        // Pure red (BT.709 limited: Y 63, U 102, V 240) comes out red.
        let (yp, uv) = flat_nv12(32, 18, 32, 63, 102, 240);
        let src = Nv12 { y: &yp, uv: &uv, pitch: 32, width: 32, height: 18, bt709: true, full_range: false };
        let mut out = Vec::new();
        nv12_to_bgra(&src, 8, 4, &mut out);
        assert!(out[2] > 240 && out[1] < 15 && out[0] < 15, "{:?}", &out[..4]);
    }

    #[test]
    fn blur_keeps_flat_colour_and_alpha() {
        let (w, h) = (16, 9);
        let mut px: Vec<u8> = (0..w * h).flat_map(|_| [10u8, 20, 30, 0]).collect();
        let mut scratch = Vec::new();
        blur_bgra(&mut px, w, h, 3.0, &mut scratch);
        assert!(px.chunks(4).all(|c| c == [10, 20, 30, 255]));
    }

    #[test]
    fn blur_spreads_a_point() {
        let (w, h) = (21, 21);
        let mut px = vec![0u8; w * h * 4];
        let mid = (10 * w + 10) * 4;
        px[mid] = 255;
        let mut scratch = Vec::new();
        blur_bgra(&mut px, w, h, 2.0, &mut scratch);
        assert!(px[mid] < 255 && px[mid] > 0);
        assert!(px[mid + 4] > 0, "neighbour picks some up");
    }
}

#[cfg(test)]
mod bench {
    use super::*;

    /// `cargo test -p apm-ui --profile fastrel --lib video::bench -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn frame_costs() {
        let (w, h, pitch) = (1280usize, 720usize, 1280usize);
        let y: Vec<u8> = (0..pitch * h).map(|i| (i * 7 % 251) as u8).collect();
        let uv: Vec<u8> = (0..pitch * h / 2).map(|i| (i * 13 % 241) as u8).collect();
        let src = Nv12 { y: &y, uv: &uv, pitch, width: w, height: h, bt709: true, full_range: false };
        let mut out = Vec::new();
        let mut scratch = Vec::new();
        let n = 100;
        for (ow, oh, sigma) in [(236usize, 132usize, 2.0f32), (1280, 720, 0.0), (1024, 576, 0.0)] {
            let t = Instant::now();
            for _ in 0..n {
                nv12_to_bgra(&src, ow, oh, &mut out);
            }
            let conv = t.elapsed() / n;
            let t = Instant::now();
            for _ in 0..n {
                blur_bgra(&mut out, ow, oh, sigma, &mut scratch);
            }
            let blur = t.elapsed() / n;
            let t = Instant::now();
            let mut l = 0.0;
            for _ in 0..n {
                l += mean_luminance(&out);
            }
            let lum = t.elapsed() / n;
            println!("{ow}x{oh}: convert {conv:?}, blur {blur:?}, luminance {lum:?} ({l})");
        }
    }
}
