//! 长截图（滚动截图）会话。
//!
//! 交互：框选后点「长截图」→ 弹出预览小窗，之后**由用户自己滚鼠标滚轮**，
//! 程序每 220ms 抓一次选区、按重叠行拼进总图，并把缩略图实时推给预览窗；
//! 用户点「复制 / 复制并保存」才结束，点「取消」丢弃。
//!
//! 之所以不自己模拟滚轮：VDI / 安全客户端会拦合成输入（实测一帧都滚不动），
//! 而用户手动滚动既不依赖注入，也能随时停在想要的位置。

use crate::model::{Config, Rect};
use crate::win32;
use base64::{engine::general_purpose::STANDARD, Engine};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

struct Session {
    w: usize,
    rows: usize,
    /// BGRA top-down，宽 w × 高 rows
    buf: Vec<u8>,
}

static SESSION: Lazy<Mutex<Option<Session>>> = Lazy::new(|| Mutex::new(None));
static RUN: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));

/// 单张长图最多 12000 行，防止滚个不停把内存吃掉
const MAX_ROWS: usize = 12000;
const PREVIEW_LABEL: &str = "scroll_preview";

#[derive(serde::Serialize, Clone)]
struct Thumb {
    b64: String,
    w: u32,
    rows: u32,
}

/// 开始一段长截图会话（预览窗由调用方另外创建）
pub fn start(app: AppHandle, rect: Rect) -> Result<(), String> {
    if rect.w < 16 || rect.h < 16 {
        return Err("选区太小".into());
    }
    if RUN.load(Ordering::SeqCst) {
        // 上一段还没结束就再点一次：先停掉旧的
        RUN.store(false, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(120));
    }
    *SESSION.lock() = None;
    RUN.store(true, Ordering::SeqCst);
    crate::dlog!("[scroll] 会话开始 {}x{} @{},{}", rect.w, rect.h, rect.x, rect.y);
    std::thread::spawn(move || run(app, rect));
    Ok(())
}

fn run(app: AppHandle, rect: Rect) {
    // 等浮层窗口真正销毁再抓第一帧，否则会把浮层自己（暗化 + 工具条）抓进去
    std::thread::sleep(Duration::from_millis(360));
    let mut g = match win32::Grabber::new(rect) {
        Ok(g) => g,
        Err(e) => return fail(&app, &e),
    };
    let first = match g.grab() {
        Ok(b) => b.to_vec(),
        Err(e) => return fail(&app, &e),
    };
    let w = rect.w as usize;
    let row = w * 4;
    let h = rect.h as usize;
    *SESSION.lock() = Some(Session {
        w,
        rows: h,
        buf: first.clone(),
    });
    emit_thumb(&app);

    let mut prev = first;
    let mut idle = 0u32;
    while RUN.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(220));
        if !RUN.load(Ordering::SeqCst) {
            break;
        }
        let cur = match g.grab() {
            Ok(b) => b.to_vec(),
            Err(_) => break,
        };
        if cur == prev {
            // 没在滚：每 1 秒多重发一次缩略图，供刚打开的前端补上首帧
            idle += 1;
            if idle % 5 == 4 {
                emit_thumb(&app);
            }
            continue;
        }
        let mut added = false;

        // 向下滚：上一帧的底部与这一帧的顶部重叠 → 追加新内容
        let k1 = win32::overlap_rows(&prev, &cur, row, h);
        if k1 > 0 {
            let mut s = SESSION.lock();
            if let Some(s) = s.as_mut() {
                let add = h - k1;
                if s.rows + add <= MAX_ROWS {
                    s.buf.extend_from_slice(&cur[k1 * row..]);
                    s.rows += add;
                    added = true;
                } else {
                    crate::dlog!("[scroll] 达到 {} 行上限，停止拼接", MAX_ROWS);
                    RUN.store(false, Ordering::SeqCst);
                }
            }
        } else {
            // 向上滚：这一帧在上面 → 插到总图前面
            let k2 = win32::overlap_rows(&cur, &prev, row, h);
            if k2 > 0 {
                let mut s = SESSION.lock();
                if let Some(s) = s.as_mut() {
                    let add = h - k2;
                    if s.rows + add <= MAX_ROWS {
                        let mut nb = Vec::with_capacity(s.buf.len() + add * row);
                        nb.extend_from_slice(&cur[..add * row]);
                        nb.extend_from_slice(&s.buf);
                        s.buf = nb;
                        s.rows += add;
                        added = true;
                    }
                }
            }
        }
        if added {
            idle = 0;
            emit_thumb(&app);
        }
        prev = cur;
    }
    crate::dlog!("[scroll] 抓帧线程结束");
    // 线程是自己停的（比如到了行上限），把选区边框也收掉，别留个框在屏上
    if !RUN.load(Ordering::SeqCst) {
        crate::app::close_scroll_frame(&app);
    }
}

fn fail(app: &AppHandle, msg: &str) {
    crate::dlog!("[scroll] 失败: {}", msg);
    RUN.store(false, Ordering::SeqCst);
    let _ = app.emit("scroll://state", serde_json::json!({ "state": "error", "msg": msg }));
}

/// 把累计图降采样成窄缩略图推给预览窗。
/// 采样在锁内做（很快），编码也在锁内，之后立刻释放锁再走 IPC。
fn emit_thumb(app: &AppHandle) {
    let payload = {
        let s = SESSION.lock();
        match s.as_ref() {
            Some(s) => {
                let w = s.w;
                let rows = s.rows.max(1);
                let tw = 240usize.min(w).max(1);
                let step = (w / tw).max(1);
                let th = (rows / step).max(1);
                let mut rgba = Vec::with_capacity(tw * th * 4);
                for y in 0..th {
                    let sy = (y * step).min(rows - 1);
                    for x in 0..tw {
                        let sx = (x * step).min(w - 1);
                        let i = (sy * w + sx) * 4;
                        if i + 3 < s.buf.len() {
                            rgba.push(s.buf[i + 2]);
                            rgba.push(s.buf[i + 1]);
                            rgba.push(s.buf[i]);
                            rgba.push(255);
                        } else {
                            rgba.extend_from_slice(&[30, 32, 40, 255]);
                        }
                    }
                }
                match image::RgbaImage::from_raw(tw as u32, th as u32, rgba) {
                    Some(img) => {
                        let mut png = Vec::new();
                        let _ = image::DynamicImage::ImageRgba8(img)
                            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png);
                        Thumb {
                            b64: STANDARD.encode(&png),
                            w: w as u32,
                            rows: rows as u32,
                        }
                    }
                    None => return,
                }
            }
            None => return,
        }
    };
    let _ = app.emit("scroll://thumb", payload);
}

/// 结束会话并按需要输出。返回一句给用户的话。
pub fn finish(app: &AppHandle, copy: bool, save: bool) -> Result<String, String> {
    RUN.store(false, Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(90));
    let sess = SESSION.lock().take().ok_or("没有正在进行的滚动截图")?;
    if let Some(w) = app.get_webview_window(PREVIEW_LABEL) {
        let _ = w.close();
    }
    crate::app::close_scroll_frame(app);

    let mut rgba = Vec::with_capacity(sess.buf.len());
    for px in sess.buf.chunks_exact(4) {
        rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
    }
    let img = image::RgbaImage::from_raw(sess.w as u32, sess.rows as u32, rgba)
        .ok_or("图像构造失败")?;

    let path = if save {
        let dir = Config::load().ensure_output_dir();
        let p = dir.join(crate::model::timestamp_name("Scroll", "png"));
        img.save(&p).map_err(|e| format!("保存失败: {}", e))?;
        Some(p)
    } else {
        None
    };

    // 复制：优先用内存里的 PNG（不依赖临时文件）
    let mut png = Vec::new();
    let _ = image::DynamicImage::ImageRgba8(img)
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png);
    if copy {
        let mut files: Vec<&str> = Vec::new();
        let name = path.as_ref().map(|p| p.to_string_lossy().to_string());
        if let Some(n) = name.as_ref() {
            files.push(n.as_str());
        }
        win32::write_clipboard(win32::ClipOptions {
            png: Some(&png),
            files,
            ..Default::default()
        })
        .map_err(|e| format!("写剪贴板失败: {}", e))?;
    }

    crate::dlog!(
        "[scroll] 结束 {}x{} copy={} save={:?}",
        sess.w,
        sess.rows,
        copy,
        path.as_ref().map(|p| p.to_string_lossy().to_string())
    );
    Ok(match (copy, path) {
        (true, Some(p)) => format!(
            "长图已保存并复制：{}（{}×{}）",
            crate::commands::short_name(&p.to_string_lossy()),
            sess.w,
            sess.rows
        ),
        (true, None) => format!("长图已复制到剪贴板（{}×{}）", sess.w, sess.rows),
        (false, Some(p)) => format!(
            "长图已保存：{}（{}×{}）",
            crate::commands::short_name(&p.to_string_lossy()),
            sess.w,
            sess.rows
        ),
        (false, None) => format!("长图结束（{}×{}，未复制）", sess.w, sess.rows),
    })
}

/// 丢弃当前会话
pub fn cancel(app: &AppHandle) {
    RUN.store(false, Ordering::SeqCst);
    *SESSION.lock() = None;
    if let Some(w) = app.get_webview_window(PREVIEW_LABEL) {
        let _ = w.close();
    }
    crate::app::close_scroll_frame(app);
    crate::dlog!("[scroll] 已取消");
}
