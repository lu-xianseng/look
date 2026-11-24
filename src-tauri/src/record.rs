//! 录制会话：抓屏 + 合成 + GIF 编码在同一线程内串行，避免多线程时序错位。

use crate::composite::{compose, ComposeOpts};
use crate::encode::{bgra_to_rgb, resize_rgb, GifWriter};
use crate::input::start as start_input;
use crate::model::{cache_dir, Config, Rect};
use crate::win32::{self, Grabber};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Deserialize)]
pub struct RecordOptions {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub fps: Option<u32>,
    pub max_width: Option<u32>,
    pub quality: Option<String>,
    pub cursor: Option<bool>,
    pub ripple: Option<bool>,
    pub key_hud: Option<bool>,
    pub max_seconds: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecordResult {
    pub path: String,
    pub bytes: u64,
    pub duration_ms: u32,
    pub frames: u32,
    pub dropped: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TickPayload {
    pub ms: u32,
    pub frames: u32,
}

struct Session {
    stop: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

pub fn is_recording() -> bool {
    SESSION.lock().is_some()
}

pub fn is_paused() -> bool {
    SESSION
        .lock()
        .as_ref()
        .map(|s| s.pause.load(Ordering::Relaxed))
        .unwrap_or(false)
}

pub fn start(app: AppHandle, cfg: Config, opts: RecordOptions) -> Result<(), String> {
    if is_recording() {
        return Err("已在录制中".into());
    }
    let rect = Rect::new(opts.x, opts.y, opts.w, opts.h);
    if rect.is_empty() {
        return Err("录制区域为空".into());
    }

    let fps = opts.fps.unwrap_or(cfg.record.fps).clamp(4, 30);
    let max_width = opts.max_width.unwrap_or(cfg.record.max_width).clamp(160, 4096);
    let colors = match opts.quality.as_deref() {
        Some(q) => match q {
            "low" => 128,
            "high" => 256,
            _ => 192,
        },
        None => cfg.record.colors(),
    };
    let max_seconds = opts.max_seconds.unwrap_or(cfg.record.max_seconds).clamp(5, 3600);
    let copts = ComposeOpts {
        cursor: opts.cursor.unwrap_or(cfg.record.cursor),
        ripple: opts.ripple.unwrap_or(cfg.record.click_ripple),
        key_hud: opts.key_hud.unwrap_or(cfg.record.key_hud),
    };

    let stop = Arc::new(AtomicBool::new(false));
    let pause = Arc::new(AtomicBool::new(false));
    let cancel = Arc::new(AtomicBool::new(false));

    let (s1, p1, c1) = (stop.clone(), pause.clone(), cancel.clone());
    let output_dir = cfg.ensure_output_dir();
    let clipboard_mode = cfg.output.gif_clipboard.clone();
    let copy_after = cfg.output.copy_after;

    // 录制线程不 join：线程结束时自行清理 SESSION。
    // 若在热键回调（主线程）里 join，会很轻易与线程内的 app.emit 互相等待死锁。
    std::thread::spawn(move || {
        let _ = run(
            app,
            rect,
            fps,
            max_width,
            colors,
            max_seconds,
            copts,
            output_dir,
            clipboard_mode,
            copy_after,
            s1,
            p1,
            c1,
        );
    });

    *SESSION.lock() = Some(Session { stop, pause, cancel });
    Ok(())
}

pub fn set_pause(paused: bool) {
    if let Some(s) = SESSION.lock().as_ref() {
        s.pause.store(paused, Ordering::Relaxed);
    }
}

/// 请求停止：只置标志位，录制线程自己收尾（保存 / 通知 / 关控制条）。
pub fn request_stop() {
    if let Some(s) = SESSION.lock().as_ref() {
        s.stop.store(true, Ordering::Relaxed);
    }
}

/// 请求丢弃：置 cancel + stop。
pub fn request_cancel() {
    if let Some(s) = SESSION.lock().as_ref() {
        s.cancel.store(true, Ordering::Relaxed);
        s.stop.store(true, Ordering::Relaxed);
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    app: AppHandle,
    rect: Rect,
    fps: u32,
    max_width: u32,
    colors: usize,
    max_seconds: u32,
    copts: ComposeOpts,
    output_dir: PathBuf,
    clipboard_mode: String,
    copy_after: bool,
    stop: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
) -> Result<RecordResult, String> {
    let res = run_inner(
        &app,
        rect,
        fps,
        max_width,
        colors,
        max_seconds,
        copts,
        output_dir,
        clipboard_mode,
        copy_after,
        stop,
        pause,
        cancel,
    );

    // 线程收尾：先清 SESSION（否则热键会认为仍在录制），再发事件与通知
    SESSION.lock().take();
    match &res {
        Ok(r) => {
            let _ = app.emit("rec://saved", r.clone());
            let _ = app.emit("rec://state", serde_json::json!({"state": "done"}));
            let fname = std::path::Path::new(&r.path)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| r.path.clone());
            crate::app::notify(
                &app,
                &format!(
                    "录制完成：{}（{} 帧 / {:.1} MB）",
                    fname,
                    r.frames,
                    r.bytes as f64 / 1048576.0
                ),
            );
        }
        Err(e) => {
            let cancelled = e.contains("已取消");
            let _ = app.emit(
                "rec://state",
                serde_json::json!({"state": if cancelled {"cancelled"} else {"error"}, "error": e}),
            );
            if !cancelled {
                crate::dlog!("[look] 录制失败: {}", e);
                crate::app::notify(&app, &format!("录制失败：{}", e));
            }
        }
    }
    let a2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        crate::app::close_control_bar(&a2);
        crate::app::close_record_frame(&a2);
    });
    res
}

#[allow(clippy::too_many_arguments)]
fn run_inner(
    app: &AppHandle,
    rect: Rect,
    fps: u32,
    max_width: u32,
    colors: usize,
    max_seconds: u32,
    copts: ComposeOpts,
    output_dir: PathBuf,
    clipboard_mode: String,
    copy_after: bool,
    stop: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
) -> Result<RecordResult, String> {
    let mut grabber = Grabber::new(rect)?;

    let src_w = rect.w as u32;
    let src_h = rect.h as u32;
    let (dw, dh, scale) = if src_w > max_width {
        let s = max_width as f32 / src_w as f32;
        let w = ((src_w as f32 * s).round() as u32) & !1;
        let h = ((src_h as f32 * s).round() as u32) & !1;
        (w.max(2), h.max(2), s)
    } else {
        (src_w & !1, src_h & !1, 1.0)
    };

    let tmp: PathBuf = cache_dir().join(format!(
        "rec_{}.gif",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    let mut writer = GifWriter::new(&tmp, dw, dh, colors)?;
    crate::dlog!(
        "[look] 录制开始: rect=({},{} {}x{}) out={}x{} fps={} max={}s",
        rect.x, rect.y, rect.w, rect.h, dw, dh, fps, max_seconds
    );

    let combo = crate::input::parse_combo(&crate::model::Config::load().hotkeys.record);
    // 开发期自检：SCREENKIT_REC_SECS=5 时到点自动停止，不依赖任何热键/键鼠注入
    let auto_stop_ms: Option<u32> = std::env::var("SCREENKIT_REC_SECS")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .map(|s| s * 1000);

    let input = start_input(combo);
    let mut rgb: Vec<u8> = Vec::with_capacity((src_w * src_h * 3) as usize);
    #[allow(unused_assignments)]
    let mut scaled: Vec<u8> = Vec::new();
    let mut frame_no: u32 = 0;
    let mut last_tick = 0u32;
    let downscale = dw != src_w || dh != src_h;

    let _ = app.emit("rec://state", serde_json::json!({"state": "recording"}));

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if input.stop_hit() {
            crate::dlog!("[look] 钩子内命中停止热键");
            break;
        }
        if pause.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(15));
            continue;
        }

        let now = input.elapsed_ms();
        if let Some(ms) = auto_stop_ms {
            if now > ms {
                crate::dlog!("[look] 自检到点停止（{} ms）", ms);
                break;
            }
        }
        let target_ms = (frame_no * 1000) / fps;
        if now < target_ms {
            std::thread::sleep(Duration::from_millis((target_ms - now).min(50) as u64));
            continue;
        }
        if now > max_seconds * 1000 {
            break;
        }

        let bgra = match grabber.grab() {
            Ok(b) => b,
            Err(e) => {
                let _ = app.emit("rec://state", serde_json::json!({"state": "error", "error": e}));
                break;
            }
        };
        bgra_to_rgb(bgra, &mut rgb);
        let events = input.recent(now);

        if downscale {
            scaled = resize_rgb(&rgb, src_w, src_h, dw, dh);
            compose(&mut scaled, dw, dh, rect, scale, now, &events, copts);
            crate::composite::draw_clock(&mut scaled, dw, dh, now);
            if let Err(e) = writer.push(&scaled, now) {
                crate::dlog!("[look] push 失败: {}", e);
                return Err(e);
            }
        } else {
            compose(&mut rgb, dw, dh, rect, 1.0, now, &events, copts);
            crate::composite::draw_clock(&mut rgb, dw, dh, now);
            if let Err(e) = writer.push(&rgb, now) {
                crate::dlog!("[look] push 失败: {}", e);
                return Err(e);
            }
        }

        frame_no += 1;
        if frame_no % 30 == 0 {
            crate::dlog!(
                "[look] frame={} now={}ms writer_frames={} input_events={}",
                frame_no,
                now,
                writer.frames(),
                input.total()
            );
        }
        if now.saturating_sub(last_tick) >= 500 {
            last_tick = now;
            let _ = app.emit(
                "rec://tick",
                TickPayload {
                    ms: now,
                    frames: writer.frames(),
                },
            );
        }
    }

    let now = input.elapsed_ms();
    crate::dlog!("[look] 录制循环结束: frames={} now={}ms", frame_no, now);
    let finish = writer.finish(now);
    if let Err(e) = &finish {
        crate::dlog!("[look] finish 失败: {}", e);
    }
    finish?;
    let frames = writer.frames();
    input.stop();

    if cancel.load(Ordering::Relaxed) {
        let _ = std::fs::remove_file(&tmp);
        return Err("已取消".into());
    }

    let out_path = output_dir.join(crate::model::timestamp_name("Rec", "gif"));
    std::fs::rename(&tmp, &out_path)
        .or_else(|_| std::fs::copy(&tmp, &out_path).map(|_| std::fs::remove_file(&tmp).ok().unwrap_or(())))
        .map_err(|e| format!("保存 GIF 失败: {}", e))?;

    let bytes = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
    let path_str = out_path.to_string_lossy().to_string();

    if copy_after {
        // 同时写入 GIF（动画）与首帧 CF_DIB：微信 / QQ / 画图等按图片粘贴时取 DIB，缺了会贴不上
        let first = gif_first_frame(&out_path);
        let gif_bytes = if clipboard_mode == "file" {
            None
        } else {
            std::fs::read(&out_path).ok()
        };
        let opts = win32::ClipOptions {
            png: None,
            rgba: first.as_ref().map(|(b, w, h)| (b.as_slice(), *w, *h)),
            gif: gif_bytes.as_deref(),
            text: None,
            files: vec![path_str.as_str()],
        };
        let _ = win32::write_clipboard(opts);
    }

    let result = RecordResult {
        path: path_str,
        bytes,
        duration_ms: now,
        frames,
        dropped: false,
    };
    let _ = app.emit("rec://saved", result.clone());
    let _ = app.emit("rec://state", serde_json::json!({"state": "done"}));
    Ok(result)
}

/// 解码 GIF 首帧为 RGBA，用于同时写入剪贴板 CF_DIB。
fn gif_first_frame(path: &Path) -> Option<(Vec<u8>, u32, u32)> {
    let file = std::fs::File::open(path).ok()?;
    let mut opts = gif::DecodeOptions::new();
    opts.set_color_output(gif::ColorOutput::RGBA);
    let mut dec = opts.read_info(file).ok()?;
    let frame = dec.read_next_frame().ok()??;
    Some((frame.buffer.to_vec(), frame.width as u32, frame.height as u32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{Ev, InputEvent};

    /// 无 GUI 的整链测试：抓屏 → 合成光标/涟漪/键帽 → GIF 编码 → 落盘
    #[test]
    fn record_pipeline_headless() {
        let rect = Rect::new(0, 0, 320, 200);
        let mut g = Grabber::new(rect).expect("Grabber");
        let path = cache_dir().join("rec_pipeline_test.gif");
        let mut w = GifWriter::new(&path, 320, 200, 64).expect("writer");
        let mut rgb = Vec::new();
        let events = vec![
            InputEvent { t: 100, ev: Ev::Down(0, 160, 120) },
            InputEvent { t: 130, ev: Ev::KeyDown("Ctrl+A".to_string()) },
            InputEvent { t: 150, ev: Ev::Move(200, 150) },
        ];
        let opts = ComposeOpts { cursor: true, ripple: true, key_hud: true };
        for i in 0..6u32 {
            let f = g.grab().expect("grab");
            bgra_to_rgb(f, &mut rgb);
            compose(&mut rgb, 320, 200, rect, 1.0, 200 + i * 66, &events, opts);
            w.push(&rgb, i * 66).expect("push");
        }
        w.finish(400).expect("finish");
        let len = std::fs::metadata(&path).unwrap().len();
        assert!(len > 500, "gif 过小: {}", len);
        println!("record pipeline: {} bytes, {} frames", len, w.frames());
    }
}
