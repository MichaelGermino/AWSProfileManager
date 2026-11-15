//! Windows decoder: Media Foundation source reader.
//!
//! Two pipelines, chosen by the video's size (see `HARDWARE_ABOVE_PIXELS` for the measurements):
//! - **Software decode, NV12 out** (up to 720p): the decoder's own NV12 frames are scaled and
//!   converted to BGRA on the CPU at the small output size ([`super::nv12_to_bgra`], ~1–2 ms).
//!   No video processor, no D3D device: the smallest pipeline.
//! - **Hardware decode (DXVA) + video processor on the GPU** (above 720p): a D3D11 device
//!   manager lets the decoder use the GPU, and the source reader's video processor scales and
//!   converts there, so only the finished, window-sized RGB32 frame is read back. Costs more
//!   memory (driver buffers live in the process) but its CPU cost does not grow with the picture.
//!   `APM_VIDEO_HW=1` / `0` forces one or the other.
//!
//! Both run in low-latency mode (the decoder hands frames out as soon as they are decoded instead
//! of keeping a reorder queue: measured −20 MB hardware, −70 MB software), select only the video
//! stream (audio is never decoded), and loop by seeking to 0 at end of stream, so playback follows
//! the stream's real end rather than the header's duration (fragmented MP4s whose header is short
//! play fully). Formats the decoder cannot hand out as NV12 fall back to the video processor.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::channel::mpsc::Sender;
use futures::SinkExt as _;
use windows::core::{Interface, HSTRING};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11Multithread, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

use super::{blur_bgra, mean_luminance, nv12_to_bgra, render_image, Control, Msg, Nv12, Wake};

/// Videos larger than this (pixels per frame; 720p, with room for padded rows) decode on the GPU
/// when one is available. Measured with NVIDIA DXVA, 30 fps H.264, app scope, against 72 MB with
/// the window open and no video: 720p software +27 MB / 11–25% of a core, hardware +45–49 MB /
/// 12–15%; 1080p software +47 MB / 25%, hardware +55 MB / 13%. Software is clearly smaller at 720p;
/// above that it costs nearly as much memory as the GPU path and twice the CPU.
const HARDWARE_ABOVE_PIXELS: u64 = 1280 * 736;

/// Never more than this many frames a second (a 60 fps video shows every other frame).
const MAX_FPS: f64 = 30.0;

pub(super) fn run(path: PathBuf, ctl: Arc<Control>, mut tx: Sender<Msg>) {
    let result = unsafe { decode(&path, &ctl, &mut tx) };
    if let Err(e) = result {
        if !ctl.stopped() {
            let _ = futures::executor::block_on(tx.send(Msg::Failed(format!("{}: {}", path.display(), e.message()))));
        }
    }
}

fn fail(what: &str) -> windows::core::Error {
    windows::core::Error::new(windows::core::HRESULT(0x8000_4005u32 as i32), what)
}

struct ComGuard;
impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

struct MfGuard;
impl Drop for MfGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
        }
    }
}

fn unpack(v: u64) -> (u32, u32) {
    ((v >> 32) as u32, v as u32)
}

fn pack(a: u32, b: u32) -> u64 {
    ((a as u64) << 32) | b as u64
}

/// A D3D11 device (video support) wrapped in a DXGI device manager, for DXVA decode.
unsafe fn device_manager() -> windows::core::Result<(IMFDXGIDeviceManager, ID3D11Device)> {
    let mut device = None;
    D3D11CreateDevice(
        None,
        D3D_DRIVER_TYPE_HARDWARE,
        HMODULE::default(),
        D3D11_CREATE_DEVICE_VIDEO_SUPPORT | D3D11_CREATE_DEVICE_BGRA_SUPPORT,
        None,
        D3D11_SDK_VERSION,
        Some(&mut device),
        None,
        None,
    )?;
    let device = device.ok_or_else(|| fail("no D3D11 device"))?;
    // The decoder uses the device from Media Foundation's own threads.
    let mt: ID3D11Multithread = device.cast()?;
    let _ = mt.SetMultithreadProtected(true);
    let mut token = 0u32;
    let mut manager = None;
    MFCreateDXGIDeviceManager(&mut token, &mut manager)?;
    let manager = manager.ok_or_else(|| fail("no DXGI device manager"))?;
    manager.ResetDevice(&device, token)?;
    Ok((manager, device))
}

/// What the reader hands out.
enum Format {
    /// The decoder's NV12; we scale and convert. `coded_height` is the surface height (the UV plane
    /// starts after that many rows), `width`/`height` the visible picture.
    Nv12 { width: u32, height: u32, coded_height: u32, bt709: bool, full_range: bool },
    /// BGRX from the video processor, already at the requested size.
    Rgb32 { width: u32, height: u32 },
}

struct Pipeline {
    reader: IMFSourceReader,
    /// Kept alive for the reader's lifetime (DXVA).
    _device: Option<(IMFDXGIDeviceManager, ID3D11Device)>,
    hardware: bool,
}

const STREAM: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

/// Open the file. `hardware` adds a D3D11 device for DXVA decode, and with it the video processor
/// (on the GPU), so only the small finished RGB32 frame is read back; software decode hands out
/// NV12 and we scale on the CPU (`video_processing` is the fallback for formats without NV12).
unsafe fn open(path: &Path, hardware: bool, video_processing: bool) -> windows::core::Result<Pipeline> {
    let mut attrs = None;
    MFCreateAttributes(&mut attrs, 4)?;
    let attrs = attrs.ok_or_else(|| fail("no attributes"))?;
    attrs.SetUINT32(&MF_LOW_LATENCY, 1)?;
    let device = if hardware { device_manager().ok() } else { None };
    if video_processing || device.is_some() {
        attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1)?;
    }
    if let Some((manager, _)) = &device {
        attrs.SetUnknown(&MF_SOURCE_READER_D3D_MANAGER, manager)?;
        attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
    }
    let reader = MFCreateSourceReaderFromURL(&HSTRING::from(path.as_os_str()), &attrs)?;
    reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
    reader.SetStreamSelection(STREAM, true)?;
    Ok(Pipeline { reader, hardware: device.is_some(), _device: device })
}

unsafe fn video_type(subtype: &windows::core::GUID, size: Option<(u32, u32)>) -> windows::core::Result<IMFMediaType> {
    let mt = MFCreateMediaType()?;
    mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
    mt.SetGUID(&MF_MT_SUBTYPE, subtype)?;
    if let Some((w, h)) = size {
        mt.SetUINT64(&MF_MT_FRAME_SIZE, pack(w, h))?;
        mt.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
    }
    Ok(mt)
}

/// The current output type as a [`Format`].
unsafe fn current_format(reader: &IMFSourceReader) -> windows::core::Result<Format> {
    let cur = reader.GetCurrentMediaType(STREAM)?;
    let (w, h) = unpack(cur.GetUINT64(&MF_MT_FRAME_SIZE)?);
    if cur.GetGUID(&MF_MT_SUBTYPE)? != MFVideoFormat_NV12 {
        return Ok(Format::Rgb32 { width: w, height: h });
    }
    // The visible picture when the surface is padded (1080p decodes into 1088 rows).
    let (vw, vh) = {
        let mut area = MFVideoArea::default();
        let ok = cur
            .GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, std::slice::from_raw_parts_mut(&mut area as *mut _ as *mut u8, std::mem::size_of::<MFVideoArea>()), None)
            .is_ok();
        if ok && area.Area.cx > 0 && area.Area.cy > 0 && area.OffsetX.value == 0 && area.OffsetY.value == 0 {
            ((area.Area.cx as u32).min(w), (area.Area.cy as u32).min(h))
        } else {
            (w, h)
        }
    };
    let matrix = cur.GetUINT32(&MF_MT_YUV_MATRIX).ok();
    let bt709 = match matrix {
        Some(m) if m == MFVideoTransferMatrix_BT601.0 as u32 => false,
        Some(m) if m == MFVideoTransferMatrix_BT709.0 as u32 => true,
        _ => vh >= 720,
    };
    let full_range = cur.GetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE).ok() == Some(MFNominalRange_0_255.0 as u32);
    Ok(Format::Nv12 { width: vw, height: vh, coded_height: h, bt709, full_range })
}

/// Pick the output: NV12 straight from the decoder, or (if it refuses, or `rgb32` is forced)
/// RGB32 at `size` through the video processor. Returns `None` when this reader cannot do NV12
/// and was opened without video processing (the caller reopens with it).
unsafe fn configure(p: &Pipeline, size: (u32, u32), allow_rgb32: bool) -> windows::core::Result<Option<Format>> {
    let force_rgb32 = p.hardware || std::env::var("APM_VIDEO_PATH").map(|v| v == "rgb32").unwrap_or(false);
    if !force_rgb32 && p.reader.SetCurrentMediaType(STREAM, None, &video_type(&MFVideoFormat_NV12, None)?).is_ok() {
        return current_format(&p.reader).map(Some);
    }
    if !allow_rgb32 {
        return Ok(None);
    }
    if p.reader.SetCurrentMediaType(STREAM, None, &video_type(&MFVideoFormat_RGB32, Some(size))?).is_err() {
        p.reader.SetCurrentMediaType(STREAM, None, &video_type(&MFVideoFormat_RGB32, None)?)?;
    }
    current_format(&p.reader).map(Some)
}

/// Lock a sample's buffer as (scan line 0, pitch, bytes available) and run `f` on it.
unsafe fn with_pixels<R>(sample: &IMFSample, f: impl FnOnce(*const u8, isize, usize) -> R) -> windows::core::Result<R> {
    let buffer = sample.ConvertToContiguousBuffer()?;
    // A 2D buffer reports its real pitch (GPU surfaces are padded; negative = bottom-up).
    if let Ok(b2) = buffer.cast::<IMF2DBuffer>() {
        let (mut scan0, mut pitch) = (std::ptr::null_mut(), 0i32);
        b2.Lock2D(&mut scan0, &mut pitch)?;
        let len = b2.GetContiguousLength().unwrap_or(0) as usize;
        let r = f(scan0, pitch as isize, len);
        b2.Unlock2D()?;
        Ok(r)
    } else {
        let (mut p, mut len) = (std::ptr::null_mut(), 0u32);
        buffer.Lock(&mut p, None, Some(&mut len))?;
        let r = f(p, 0, len as usize);
        buffer.Unlock()?;
        Ok(r)
    }
}

/// The frame as BGRA at `out` size (NV12) or at the reader's size (RGB32); alpha 255.
unsafe fn frame_bgra(sample: &IMFSample, format: &Format, out: (u32, u32), bgra: &mut Vec<u8>) -> windows::core::Result<(u32, u32)> {
    match *format {
        Format::Nv12 { width, height, coded_height, bt709, full_range } => {
            let ok = with_pixels(sample, |scan0, pitch, len| {
                let pitch = if pitch > 0 { pitch as usize } else { width as usize };
                let y_len = pitch * coded_height as usize;
                let uv_len = pitch * (height as usize).div_ceil(2);
                if scan0.is_null() || (len != 0 && len < y_len + uv_len) {
                    return false;
                }
                let y = std::slice::from_raw_parts(scan0, y_len);
                let uv = std::slice::from_raw_parts(scan0.add(y_len), uv_len);
                let src = Nv12 { y, uv, pitch, width: width as usize, height: height as usize, bt709, full_range };
                nv12_to_bgra(&src, out.0 as usize, out.1 as usize, bgra);
                true
            })?;
            if !ok {
                return Err(fail("unexpected NV12 buffer"));
            }
            Ok(out)
        }
        Format::Rgb32 { width, height } => {
            let (w, h) = (width as usize, height as usize);
            let row = w * 4;
            bgra.clear();
            bgra.resize(row * h, 0);
            let ok = with_pixels(sample, |scan0, pitch, len| {
                let pitch = if pitch != 0 { pitch } else { row as isize };
                if scan0.is_null() || (pitch > 0 && len != 0 && len < row * h) {
                    return false;
                }
                for y in 0..h {
                    let src = std::slice::from_raw_parts(scan0.offset(y as isize * pitch), row);
                    let dst = &mut bgra[y * row..(y + 1) * row];
                    dst.copy_from_slice(src);
                    for px in dst.as_chunks_mut::<4>().0 {
                        px[3] = 255; // RGB32 leaves the 4th byte undefined
                    }
                }
                true
            })?;
            if !ok {
                return Err(fail("unexpected RGB32 buffer"));
            }
            Ok((width, height))
        }
    }
}

unsafe fn decode(path: &Path, ctl: &Control, tx: &mut Sender<Msg>) -> windows::core::Result<()> {
    CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
    let _com = ComGuard;
    MFStartup(MF_VERSION, MFSTARTUP_LITE)?;
    let _mf = MfGuard;
    let debug = std::env::var_os("APM_VIDEO_DEBUG").is_some();

    let mut pipeline = open(path, false, false)?;
    let native = pipeline.reader.GetNativeMediaType(STREAM, 0)?;
    let (mut vw, mut vh) = unpack(native.GetUINT64(&MF_MT_FRAME_SIZE)?);
    let fps = native.GetUINT64(&MF_MT_FRAME_RATE).ok().map(unpack).filter(|(n, d)| *n > 0 && *d > 0).map(|(n, d)| n as f64 / d as f64).unwrap_or(MAX_FPS);
    // Software decode is the smaller pipeline (DXVA's driver buffers live in the process); above
    // 720p its CPU and frame buffers grow with the picture, so larger videos use the GPU.
    let hardware = match std::env::var("APM_VIDEO_HW").ok().as_deref() {
        Some("0") => false,
        Some("1") => true,
        _ => u64::from(vw) * u64::from(vh) > HARDWARE_ABOVE_PIXELS,
    };
    if hardware {
        pipeline = open(path, true, false)?;
    }
    // Skip frames closer together than this (90% of the interval, so jittery timestamps in a
    // 30 fps file are not dropped).
    let min_gap = (1e7 / fps.min(MAX_FPS) * 0.9) as i64;

    let mut generation = u64::MAX;
    let mut format: Option<Format> = None;
    let mut out = (vw, vh);
    let mut sigma = 0.0f32;
    let mut bgra = Vec::new();
    let mut scratch = Vec::new();
    // Wall-clock time of timestamp 0 in the current loop.
    let mut origin = Instant::now();
    let mut last_sent: Option<i64> = None;
    let mut frames_this_loop = 0u64;
    // (decode, convert + blur) time over the last `n` shown frames, for APM_VIDEO_DEBUG.
    let mut stats = (Duration::ZERO, Duration::ZERO, 0u32);

    loop {
        let params = match ctl.wait_runnable() {
            Wake::Stop => return Ok(()),
            Wake::Run { paused_for, params, generation: g } => {
                origin += paused_for;
                if g == generation {
                    None
                } else {
                    generation = g;
                    Some(params)
                }
            }
        };
        if let Some(params) = params {
            let size = params.output_size(vw, vh);
            if format.is_none() {
                format = match configure(&pipeline, size, false)? {
                    Some(f) => Some(f),
                    None => {
                        // Not NV12: reopen with the video processor and take RGB32.
                        pipeline = open(path, pipeline.hardware, true)?;
                        configure(&pipeline, size, true)?
                    }
                };
            } else if matches!(format, Some(Format::Rgb32 { .. })) {
                format = configure(&pipeline, size, true)?;
            }
            if let Some(Format::Nv12 { width, height, .. }) = format {
                (vw, vh) = (width, height);
            }
            out = match format {
                Some(Format::Nv12 { width, height, .. }) => params.output_size(width, height),
                Some(Format::Rgb32 { width, height }) => (width, height),
                None => size,
            };
            // The blur applies at the size the frame is drawn from.
            sigma = params.residual_sigma() * out.0 as f32 / size.0.max(1) as f32;
            if debug {
                let kind = match format {
                    Some(Format::Nv12 { .. }) => "NV12 + CPU scale",
                    _ => "RGB32 via video processor",
                };
                eprintln!(
                    "background video: {vw}x{vh} @ {fps:.2} fps, {} decode, {kind}, output {}x{}, blur σ {sigma:.1}",
                    if pipeline.hardware { "hardware" } else { "software" },
                    out.0,
                    out.1
                );
            }
        }
        let Some(fmt) = format.as_ref() else { return Err(fail("no output format")) };

        let t_read = Instant::now();
        let (mut flags, mut ts) = (0u32, 0i64);
        let mut sample: Option<IMFSample> = None;
        pipeline.reader.ReadSample(STREAM, 0, None, Some(&mut flags), Some(&mut ts), Some(&mut sample))?;
        if flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0 {
            return Err(fail("decode error"));
        }
        if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0 {
            format = Some(current_format(&pipeline.reader)?);
            generation = u64::MAX; // recompute the output size on the next frame
            continue;
        }
        if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            if frames_this_loop == 0 && last_sent.is_none() {
                return Err(fail("no video frames"));
            }
            pipeline.reader.SetCurrentPosition(&windows::core::GUID::zeroed(), &PROPVARIANT::from(0i64))?;
            origin = Instant::now();
            last_sent = None;
            frames_this_loop = 0;
            continue;
        }
        let Some(sample) = sample else { continue };
        frames_this_loop += 1;
        if let Some(prev) = last_sent {
            if ts - prev < min_gap && ts >= prev {
                continue;
            }
        }

        let due = origin + Duration::from_nanos(ts.max(0) as u64 * 100);
        let now = Instant::now();
        if now > due + Duration::from_millis(250) {
            // Far behind (a stall, or the machine was busy): resync instead of racing to catch up.
            origin = now - Duration::from_nanos(ts.max(0) as u64 * 100);
        }

        let read_time = t_read.elapsed();
        let t_convert = Instant::now();
        let (w, h) = frame_bgra(&sample, fmt, out, &mut bgra)?;
        drop(sample);
        blur_bgra(&mut bgra, w as usize, h as usize, sigma, &mut scratch);
        let luma = mean_luminance(&bgra);
        // The frame moves into the image (GPUI owns it from here); keep the buffer's capacity
        // for the next one only when nothing else needs it.
        let frame = std::mem::replace(&mut bgra, Vec::with_capacity((w * h * 4) as usize));
        let Some(image) = render_image(w, h, frame) else { continue };
        if debug {
            stats = (stats.0 + read_time, stats.1 + t_convert.elapsed(), stats.2 + 1);
            if stats.2 == 150 {
                let ms = |d: Duration| d.as_secs_f64() * 1000.0 / 150.0;
                eprintln!("background video: per shown frame: decode {:.2} ms, convert + blur {:.2} ms", ms(stats.0), ms(stats.1));
                stats = (Duration::ZERO, Duration::ZERO, 0);
            }
        }

        let mut due = due;
        loop {
            ctl.sleep_until(due, generation);
            match ctl.wait_runnable() {
                Wake::Stop => return Ok(()),
                // Paused while waiting: shift the clock by the pause and wait out the rest.
                Wake::Run { paused_for, .. } if paused_for > Duration::from_millis(1) => {
                    origin += paused_for;
                    due += paused_for;
                }
                _ => break,
            }
        }
        if futures::executor::block_on(tx.send(Msg::Frame(image, luma))).is_err() {
            return Ok(()); // the window is gone
        }
        last_sent = Some(ts);
    }
}
