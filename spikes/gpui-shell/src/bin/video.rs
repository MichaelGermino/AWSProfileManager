//! Phase 0 spike (c): background video in GPUI on Windows via Media Foundation.
//! Decodes to BGRA on a worker thread, paces frames by timestamp, loops at end of stream, and draws
//! GPUI content on top. Usage: video.exe <file.mp4>. SPIKE_AUTOTEST=<seconds> prints stats and quits.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use futures::channel::mpsc::{channel, Sender};
use futures::{SinkExt, StreamExt};
use gpui::{
    div, img, prelude::*, px, rgb, size, App, Bounds, Context, ObjectFit, RenderImage, Window, WindowBounds,
    WindowOptions,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Frame {
    pts_100ns: i64,
    image: Arc<RenderImage>,
    loop_index: u64,
}

static FRAMES_PER_LOOP: [AtomicU64; 4] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];

#[cfg(windows)]
fn decode(path: String, mut tx: Sender<Frame>, info: Sender<String>) -> windows::core::Result<()> {
    use windows::core::HSTRING;
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::Media::MediaFoundation::*;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
        let mut attrs: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut attrs, 2)?;
        let attrs = attrs.unwrap();
        // Lets the reader convert H.264/YUV to RGB32 for us.
        attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;
        let reader = MFCreateSourceReaderFromURL(&HSTRING::from(path.as_str()), &attrs)?;
        let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
        let mt = MFCreateMediaType()?;
        mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        mt.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
        reader.SetCurrentMediaType(stream, None, &mt)?;
        let cur = reader.GetCurrentMediaType(stream)?;
        let fs = cur.GetUINT64(&MF_MT_FRAME_SIZE)?;
        let (w, h) = ((fs >> 32) as u32, (fs & 0xffff_ffff) as u32);
        let stride = cur.GetUINT32(&MF_MT_DEFAULT_STRIDE).map(|s| s as i32).unwrap_or((w * 4) as i32);
        let dur = reader
            .GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
            .ok()
            .and_then(|pv| u64::try_from(&pv).ok())
            .unwrap_or(0);
        let mut info = info;
        let _ = futures::executor::block_on(info.send(format!(
            "{w}x{h}, stride {stride}, declared duration {:.2}s",
            dur as f64 / 1e7
        )));

        let mut loop_index = 0u64;
        loop {
            let (mut flags, mut ts) = (0u32, 0i64);
            let mut sample: Option<IMFSample> = None;
            reader.ReadSample(stream, 0, None, Some(&mut flags), Some(&mut ts), Some(&mut sample))?;
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                loop_index += 1;
                reader.SetCurrentPosition(&windows::core::GUID::zeroed(), &PROPVARIANT::from(0i64))?;
                continue;
            }
            let Some(sample) = sample else { continue };
            let buf = sample.ConvertToContiguousBuffer()?;
            let (mut p, mut len) = (std::ptr::null_mut(), 0u32);
            buf.Lock(&mut p, None, Some(&mut len))?;
            let src = std::slice::from_raw_parts(p, len as usize);
            let row = (w * 4) as usize;
            let mut bgra = vec![0u8; row * h as usize];
            for y in 0..h as usize {
                // Negative stride = bottom-up rows.
                let sy = if stride < 0 { h as usize - 1 - y } else { y };
                let off = sy * stride.unsigned_abs() as usize;
                bgra[y * row..(y + 1) * row].copy_from_slice(&src[off..off + row]);
            }
            buf.Unlock()?;
            for px in bgra.chunks_exact_mut(4) {
                px[3] = 255; // RGB32 leaves X undefined; GPUI reads BGRA.
            }
            if let Some(c) = FRAMES_PER_LOOP.get(loop_index as usize) {
                c.fetch_add(1, Ordering::Relaxed);
            }
            let rgba = image::RgbaImage::from_raw(w, h, bgra).expect("frame size");
            let image = Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(rgba)]));
            if futures::executor::block_on(tx.send(Frame { pts_100ns: ts, image, loop_index })).is_err() {
                return Ok(());
            }
        }
    }
}

struct Player {
    current: Option<Arc<RenderImage>>,
    status: String,
    shown: u64,
}

impl Render for Player {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .relative()
            .bg(rgb(0x000000))
            .children(self.current.clone().map(|i| img(i).absolute().size_full().object_fit(ObjectFit::Cover)))
            .child(
                div()
                    .absolute()
                    .top(px(24.))
                    .left(px(24.))
                    .p(px(16.))
                    .rounded(px(12.))
                    .bg(gpui::rgba(0x18181bd0))
                    .text_color(rgb(0xe4e4e7))
                    .child("GPUI content over the video")
                    .child(div().text_sm().child(format!("{} | frames shown {}", self.status, self.shown))),
            )
    }
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: video <file.mp4>");
    let autotest = std::env::var("SPIKE_AUTOTEST").ok().and_then(|s| s.parse::<u64>().ok());
    gpui_platform::application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(960.), px(560.)), cx);
        let win = cx
            .open_window(WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() }, |_, cx| {
                cx.new(|_| Player { current: None, status: "starting".into(), shown: 0 })
            })
            .expect("window");
        cx.activate(true);

        let (tx, mut rx) = channel::<Frame>(2);
        let (info_tx, mut info_rx) = channel::<String>(1);
        std::thread::spawn(move || {
            if let Err(e) = decode(path, tx, info_tx) {
                eprintln!("decode error: {e}");
            }
        });

        cx.spawn(async move |cx| {
            let info = info_rx.next().await.unwrap_or_default();
            println!("video: {info}");
            let mut origin = Instant::now();
            let mut cur_loop = 0;
            while let Some(f) = rx.next().await {
                if f.loop_index != cur_loop {
                    cur_loop = f.loop_index;
                    origin = Instant::now();
                }
                let due = origin + Duration::from_nanos((f.pts_100ns.max(0) as u64) * 100);
                if let Some(wait) = due.checked_duration_since(Instant::now()) {
                    cx.background_executor().timer(wait).await;
                }
                let status = info.clone();
                let ok = win.update(cx, move |p, window, cx| {
                    if let Some(old) = p.current.replace(f.image) {
                        let _ = window.drop_image(old); // release the previous frame's atlas tile
                    }
                    p.shown += 1;
                    p.status = format!("{status} | loop {}", f.loop_index);
                    cx.notify();
                });
                if ok.is_err() {
                    break;
                }
            }
        })
        .detach();

        if let Some(secs) = autotest {
            cx.spawn(async move |cx| {
                cx.background_executor().timer(Duration::from_secs(secs)).await;
                let shown = win.update(cx, |p, _, _| p.shown).unwrap_or(0);
                let loops: Vec<u64> = FRAMES_PER_LOOP.iter().map(|c| c.load(Ordering::Relaxed)).collect();
                println!("frames shown {shown}; decoded per loop {loops:?}");
                let _ = cx.update(|cx| cx.quit());
            })
            .detach();
        }
    });
}
