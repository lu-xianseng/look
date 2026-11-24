//! 应用壳：托盘、全局热键、浮层窗口、控制条、贴图窗口。

use crate::encode::bgra_to_rgb;
use crate::model::{cache_dir, Config, MonitorShot, Rect};
use crate::record::{self, RecordOptions};
use crate::win32;
use parking_lot::Mutex;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::utils::config::Color;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub struct AppState {
    pub shots: Mutex<Vec<MonitorShot>>,
    pub excludes: Mutex<Vec<isize>>,
    pub mode: Mutex<String>,
    pub last_region: Mutex<Option<Rect>>,
    pub pin_seq: Mutex<u32>,
    /// 每轮捕获递增，用于生成唯一浮层 label，避免与上一轮尚未销毁的窗口撞名
    pub overlay_seq: Mutex<u32>,
    /// 当前生效的两个全局快捷键（由热键统一回调按 shortcut 分发）
    pub shot_key: Mutex<Option<Shortcut>>,
    pub rec_key: Mutex<Option<Shortcut>>,
    /// toast 窗口要显示的那句话（窗口每次新建，前端挂载后取一次）
    pub toast_msg: Mutex<String>,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            shots: Mutex::new(Vec::new()),
            excludes: Mutex::new(Vec::new()),
            mode: Mutex::new("shot".into()),
            last_region: Mutex::new(None),
            pin_seq: Mutex::new(0),
            overlay_seq: Mutex::new(0),
            shot_key: Mutex::new(None),
            rec_key: Mutex::new(None),
            toast_msg: Mutex::new(String::new()),
        }
    }
}

/// 冻结所有显示器，落 PNG 到缓存目录。
pub fn freeze_all() -> Result<Vec<MonitorShot>, String> {
    let mut list = Vec::new();
    for mon in win32::enum_monitors() {
        let rect = Rect::new(mon.x, mon.y, mon.w, mon.h);
        let bgra = win32::grab_once(rect)?;
        let mut rgb = Vec::new();
        bgra_to_rgb(&bgra, &mut rgb);
        let path = cache_dir().join(format!("freeze_{}.png", mon.index));
        image::save_buffer(
            &path,
            &rgb,
            mon.w as u32,
            mon.h as u32,
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|e| format!("保存冻结帧失败: {}", e))?;
        list.push(MonitorShot {
            mon,
            path: path.to_string_lossy().to_string(),
        });
    }
    Ok(list)
}

pub fn close_overlays(app: &AppHandle) {
    for (label, win) in app.webview_windows() {
        if label.starts_with("overlay_") {
            let _ = win.close();
        }
    }
    if let Some(state) = app.try_state::<AppState>() {
        state.excludes.lock().clear();
    }
}

/// 唤起捕获浮层（截图/录屏共用）。
pub fn open_capture(app: &AppHandle, mode: &str) -> Result<(), String> {
    if record::is_recording() {
        return Ok(());
    }
    close_overlays(app);

    let shots = freeze_all()?;
    let state = app.state::<AppState>();
    *state.mode.lock() = mode.to_string();
    let seq = {
        let mut s = state.overlay_seq.lock();
        *s += 1;
        *s
    };
    let shots_clone = shots.clone();
    *state.shots.lock() = shots;

    let mut excludes: Vec<isize> = Vec::new();
    let mut pending: Vec<String> = Vec::new();

    if shots_clone.is_empty() {
        return Err("没有可用的显示器".into());
    }
    // 逐屏打一行：多屏问题时只看「屏数」不够，布局（谁在左、分辨率、缩放）才是关键
    for s in shots_clone.iter() {
        crate::dlog!(
            "[look]   屏#{}: {}x{} @ ({}, {}) scale={} primary={}",
            s.mon.index,
            s.mon.w,
            s.mon.h,
            s.mon.x,
            s.mon.y,
            s.mon.scale,
            s.mon.primary
        );
    }
    // 所有显示器并成一块虚拟桌面，用**一个**浮层窗口盖住它。
    // 之前是「一个显示器一个窗口」，跨屏拖拽时鼠标一移出当前窗口，pointermove 就断了，
    // 选区被钉在屏幕边缘——双屏下表现为「只能框选到一个屏幕的边缘」。
    let mut x0 = i32::MAX;
    let mut y0 = i32::MAX;
    let mut x1 = i32::MIN;
    let mut y1 = i32::MIN;
    for s in shots_clone.iter() {
        x0 = x0.min(s.mon.x);
        y0 = y0.min(s.mon.y);
        x1 = x1.max(s.mon.x + s.mon.w);
        y1 = y1.max(s.mon.y + s.mon.h);
    }
    let vb_w = (x1 - x0).max(1);
    let vb_h = (y1 - y0).max(1);

    let label = format!("overlay_{}", seq);
    let win = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title("Look")
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        // 先隐藏建窗：直接 visible(true) 时 WebView 还没画出画面，
        // 系统先显示窗口底色（一片黑），用户看到的就是「启动截图时屏幕闪一下黑」。
        // 等前端把冻结的截图铺好（img onload）再调 overlay_ready 显示。
        .visible(false)
        .focused(false)
        .build()
        .map_err(|e| format!("创建浮层失败: {}", e))?;

    let _ = win.set_position(PhysicalPosition::new(x0, y0));
    let _ = win.set_size(PhysicalSize::new(vb_w as u32, vb_h as u32));
    let _ = win.set_always_on_top(true);

    if let Ok(hwnd) = win.hwnd() {
        excludes.push(hwnd.0 as isize);
    }
    pending.push(label.clone());
    crate::dlog!(
        "[look] 浮层已创建（覆盖虚拟桌面）: {}x{} at ({}, {}) 屏数={} scale={}",
        vb_w,
        vb_h,
        x0,
        y0,
        shots_clone.len(),
        win.scale_factor().unwrap_or(-1.0),
    );

    // 兜底：万一前端因为异常没能上报就绪，1.8 秒后强制显示，避免用户完全看不到浮层
    if !pending.is_empty() {
        let app2 = app.clone();
        let labels = pending.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(1800));
            for l in labels {
                if let Some(w) = app2.get_webview_window(&l) {
                    if !w.is_visible().unwrap_or(false) {
                        crate::dlog!("[look] 浮层 {} 超时未就绪，兜底显示", l);
                        show_overlay(&w);
                    }
                }
            }
        });
    }

    // 默认不做 WDA_EXCLUDEFROMCAPTURE。
    // 理由：被标记排除的窗口在某些显示通道（远程桌面/远控、部分虚拟显示）里不参与合成，
    // 用户会看到「只有鼠标变成十字、浮层一片空白」。而浮层在开始录制前就关掉了、
    // 控制条又位于选区外侧，本来就不会进录制画面，这里不需要排除。
    // 需要时用 SCREENKIT_EXCLUDE=1 打开。
    if std::env::var("SCREENKIT_EXCLUDE").is_ok() {
        for e in excludes.iter() {
            win32::exclude_from_capture(*e);
        }
    }
    *state.excludes.lock() = excludes;
    let _ = app.emit("capture://opened", serde_json::json!({"mode": mode}));
    Ok(())
}

/// 显示已建好的浮层并把焦点抢过来（前端铺完冻结截图后调用）。
pub fn show_overlay(win: &tauri::WebviewWindow) {
    let _ = win.show();
    let _ = win.set_focus();
    // Tauri 的 set_focus 可能被 Windows 前台锁定吞掉；再走一遍 Win32 强制置前，
    // 否则用户能拖选但按 Enter / Esc / Tab 全无反应。
    let fg_ok = match win.hwnd() {
        Ok(h) => win32::force_foreground(h.0 as isize as _),
        Err(_) => false,
    };
    crate::dlog!(
        "[look] 浮层就绪并显示: label={} {:.0}x{:.0} visible={} foreground={}",
        win.label(),
        win.outer_size().map(|s| s.width as f64).unwrap_or(-1.0),
        win.outer_size().map(|s| s.height as f64).unwrap_or(-1.0),
        win.is_visible().unwrap_or(false),
        fg_ok
    );
}

fn urlenc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        let c = b as char;
        if c.is_ascii_alphanumeric() || "-_.~/".contains(c) {
            out.push(c);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

fn monitor_scale_at(x: i32, y: i32) -> f64 {
    for m in win32::enum_monitors() {
        if x >= m.x && x < m.x + m.w && y >= m.y && y < m.y + m.h {
            return m.scale;
        }
    }
    1.0
}

/// 长截图的预览小窗：放在选区外侧（右 → 左 → 下方），绝不压住选区，
/// 否则自己会被抓进滚动画面。
pub fn show_scroll_preview(app: &AppHandle, region: Rect) -> Result<(), String> {
    let label = "scroll_preview";
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.close();
    }
    let scale = monitor_scale_at(region.x, region.y);
    let log_w = 262.0f64;
    let log_h = 438.0f64;    let pw = (log_w * scale).round() as i32;
    let ph = (log_h * scale).round() as i32;
    let gap = 12i32;

    let mon = win32::enum_monitors()
        .into_iter()
        .find(|m| region.x >= m.x && region.x < m.x + m.w);

    // 先放右侧，放不下放左侧，两边都放不下再压到选区下方
    let mut x = region.x + region.w + gap;
    let mut y = region.y;
    if let Some(m) = mon.as_ref() {
        if x + pw > m.x + m.w - 8 {
            let left = region.x - pw - gap;
            x = if left >= m.x + 8 {
                left
            } else {
                (m.x + m.w - 8 - pw).max(m.x + 8)
            };
        }
        if y + ph > m.y + m.h - 8 {
            y = (m.y + m.h - 8 - ph).max(m.y + 8);
        }
        if y < m.y + 8 {
            y = m.y + 8;
        }
    }

    let win = WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .title("Look Scroll")
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .background_color(Color(0, 0, 0, 0))
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(true)
        .position(x as f64 / scale, y as f64 / scale)
        .inner_size(log_w, log_h)
        .build()
        .map_err(|e| format!("创建预览窗失败: {}", e))?;

    let _ = win.set_size(tauri::PhysicalSize::new(pw as u32, ph as u32));
    let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
    let _ = win.set_always_on_top(true);
    if let Ok(hwnd) = win.hwnd() {
        let radius = (10.0 * scale).round() as i32;
        win32::set_round_window_region(hwnd.0 as isize, pw, ph, radius);
    }
    crate::dlog!("[scroll] 预览窗: {}x{} @({}, {})", pw, ph, x, y);
    Ok(())
}

/// 录制控制条：贴在选区右下角，且不进入录屏画面。
pub fn show_control_bar(app: &AppHandle, region: Rect) -> Result<(), String> {
    let label = "control";
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.close();
    }
    let scale = monitor_scale_at(region.x, region.y);
    let bar_w = 232.0f64;
    let bar_h = 40.0f64;
    let w_px = (bar_w * scale).round() as i32;
    let h_px = (bar_h * scale).round() as i32;

    let mut x = region.x + region.w - w_px;
    if x < region.x {
        x = region.x;
    }
    let mut y = region.y + region.h + 16;
    // 超出屏幕则放到选区上方
    if let Some(mon) = win32::enum_monitors()
        .into_iter()
        .find(|m| region.x >= m.x && region.x < m.x + m.w)
    {
        if y + h_px > mon.y + mon.h - 8 {
            // 下方放不下就挪到选区上方外侧；上方也不够才退回屏幕底部，
            // 绝不放进选区内部 —— 那样控制条会被录进 GIF 画面
            let above = region.y - h_px - 16;
            y = if above >= mon.y + 8 {
                above
            } else {
                mon.y + mon.h - 8 - h_px
            };
        }
        // 右端不出屏
        if x + w_px > mon.x + mon.w - 8 {
            x = mon.x + mon.w - 8 - w_px;
        }
        if x < mon.x + 8 {
            x = mon.x + 8;
        }
    }

    // 直接在 build 时给定位置与尺寸并 visible(true)：Win10 上 visible(false)+show() 会留下不可见窗口
    let win = WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .title("Look Recorder")
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        // 透明窗口不能开 DWM 阴影：Windows 上它会把 WebView2 的透明区域填成不透明矩形，
        // 圆角就被直角遮挡。阴影交给 CSS 画，窗口四周留出足够的 padding。
        .shadow(false)
        // 关键：WebView2 自己的背景也要透明，否则圆角外侧是一圈黑底
        .background_color(Color(0, 0, 0, 0))
        .visible(true)
        .position(x as f64 / scale, y as f64 / scale)
        .inner_size(bar_w, bar_h)
        .build()
        .map_err(|e| format!("创建控制条失败: {}", e))?;

    // build 时的逻辑坐标在不同 DPI 下会漂，创建后再用物理坐标钉一次，保证贴住选区外侧
    let _ = win.set_size(tauri::PhysicalSize::new(w_px as u32, h_px as u32));
    let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
    if let Ok(p) = win.outer_position() {
        crate::dlog!("[look] 控制条实际位置: ({}, {}) 目标: ({}, {})", p.x, p.y, x, y);
    }

    let _ = win.set_always_on_top(true);

    // WebView2 的底不透明，圆角只能靠窗口区域裁出来
    if let Ok(hwnd) = win.hwnd() {
        let radius = (9.0 * scale).round() as i32;
        win32::set_round_window_region(hwnd.0 as isize, w_px, h_px, radius);
    }

    // 控制条保持在选区外侧，默认不做排除（同 open_capture 的理由）
    if std::env::var("SCREENKIT_EXCLUDE").is_ok() {
        if let Ok(hwnd) = win.hwnd() {
            win32::exclude_from_capture(hwnd.0 as isize);
        }
    }
    crate::dlog!("[look] 控制条已创建: {}x{} at ({}, {})", w_px, h_px, x, y);
    Ok(())
}

pub fn close_control_bar(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("control") {
        let _ = w.close();
    }
}

/// 录制进行中的虚线边框：一个点击穿透的透明浮窗，只画边。
/// 录制线程结束时会关掉它。边框画在选区外侧 2px 上，不占录制画面。
pub fn show_record_frame(app: &AppHandle, region: Rect) -> Result<(), String> {
    show_frame_window(app, "rec_frame", "Look Frame", region)
}

/// 长截图进行中的选区边框（蓝色）：选区和滚动截图的抓帧区域一致，
/// 全程显示，用户滚轮时能看清自己在截哪一块。同样点击穿透。
pub fn show_scroll_frame(app: &AppHandle, region: Rect) -> Result<(), String> {
    show_frame_window(app, "scroll_frame", "Look Scroll Frame", region)
}

pub fn close_scroll_frame(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("scroll_frame") {
        let _ = w.close();
    }
}

fn show_frame_window(
    app: &AppHandle,
    label: &str,
    title: &str,
    region: Rect,
) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.close();
    }
    let scale = monitor_scale_at(region.x, region.y);
    let pad = 2i32;
    let x = region.x - pad;
    let y = region.y - pad;
    let px_w = region.w + pad * 2 + 4;
    let px_h = region.h + pad * 2 + 4;

    let win = WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .title(title)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .transparent(true)
        .visible(true)
        .position(x as f64 / scale, y as f64 / scale)
        .inner_size(px_w as f64 / scale, px_h as f64 / scale)
        .build()
        .map_err(|e| format!("创建边框失败: {}", e))?;

    // 同控制条：物理坐标钉一次，保证虚线框和选区严格对齐
    let _ = win.set_size(tauri::PhysicalSize::new(px_w as u32, px_h as u32));
    let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
    if let Ok(p) = win.outer_position() {
        crate::dlog!(
            "[look] 边框实际位置: ({}, {}) 目标: ({}, {})",
            p.x,
            p.y,
            x,
            y
        );
    }

    // 完全点击穿透：用户能照常操作被录的窗口
    let _ = win.set_ignore_cursor_events(true);
    let _ = win.set_always_on_top(true);
    crate::dlog!("[look] 边框已创建: {} {}x{} at ({}, {})", label, px_w, px_h, x, y);
    Ok(())
}

pub fn close_record_frame(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("rec_frame") {
        let _ = w.close();
    }
}

/// 贴图：把一张 PNG 钉在屏幕上。
pub fn create_pin(app: &AppHandle, png: &[u8], px_w: u32, px_h: u32) -> Result<String, String> {
    let state = app.state::<AppState>();
    let mut seq = state.pin_seq.lock();
    *seq += 1;
    let label = format!("pin_{}", *seq);
    drop(seq);

    let path = cache_dir().join(format!("{}.png", label));
    std::fs::write(&path, png).map_err(|e| e.to_string())?;

    let (cx, cy) = win32::cursor_pos();
    let scale = monitor_scale_at(cx, cy);
    let logical_w = (px_w as f64 / scale).round().max(120.0);
    let logical_h = (px_h as f64 / scale).round().max(80.0);

    let url = format!("index.html?pin={}", urlenc(&path.to_string_lossy()));
    let win = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .title("Look Pin")
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(true)
        .shadow(true)
        .visible(false)
        .focused(true)
        .build()
        .map_err(|e| format!("创建贴图失败: {}", e))?;

    let _ = win.set_position(PhysicalPosition::new(
        (cx - px_w as i32 / 2).max(0),
        (cy - px_h as i32 / 2).max(0),
    ));
    let _ = win.set_size(PhysicalSize::new(
        (logical_w * scale) as u32,
        (logical_h * scale) as u32,
    ));
    let _ = win.show();
    let _ = win.set_focus();
    Ok(label)
}

pub fn toggle_record(app: &AppHandle) {
    if record::is_recording() {
        // 只置停止标志：保存、通知、关闭控制条都由录制线程收尾，避免主线程 join 造成死锁
        record::request_stop();
        return;
    }
    // 一律先弹框选：用户明确要求「按 Ctrl+Alt+R 也要先选区域」，
    // 之前有 last_region 就直接开录，会让人以为热键漏了框选这一步。
    let _ = open_capture(app, "record");
}

/// 轻量 toast：截图/录屏收尾时给一句结果反馈。
/// 用系统通知（NotificationExt）在没注册 AppUserModelID 的机器上经常静默不弹，
/// 所以改成自绘的小窗口：无边框、透明、置顶、不抢焦点、点击穿透，2 秒后自己消失。
pub fn show_toast(app: &AppHandle, text: &str) {
    let a = app.clone();
    let msg = text.to_string();
    crate::dlog!("[look] toast: {}", msg);
    std::thread::spawn(move || {
        // 稍等再建：让浮层 / 控制条先关掉，避免 toast 被它们压住
        std::thread::sleep(std::time::Duration::from_millis(320));
        let h = a.clone();
        let _ = a.run_on_main_thread(move || {
            let (cx, cy) = win32::cursor_pos();
            let mons = win32::enum_monitors();
            let mon = mons
                .iter()
                .find(|m| cx >= m.x && cx < m.x + m.w && cy >= m.y && cy < m.y + m.h)
                .or_else(|| mons.iter().find(|m| m.primary))
                .cloned();
            let sc = mon.as_ref().map(|m| m.scale).unwrap_or(1.0).max(0.5);
            let w = (300.0 * sc) as i32;
            let hgt = (66.0 * sc) as i32;
            let (px, py) = match &mon {
                Some(m) => (
                    m.x + m.w - w - (28.0 * sc) as i32,
                    m.y + m.h - hgt - (96.0 * sc) as i32,
                ),
                None => (cx - w / 2, cy + (40.0 * sc) as i32),
            };

            // 复用已经在的 toast：关掉再建的话，旧窗口是异步销毁的，
            // 新窗口会在同一位置叠上去，看起来像两个 toast 重影。
            if let Some(win) = h.get_webview_window("toast") {
                if let Some(st) = h.try_state::<AppState>() {
                    *st.toast_msg.lock() = msg.clone();
                }
                let _ = win.set_position(PhysicalPosition::new(px, py));
                let _ = win.set_size(PhysicalSize::new(w as u32, hgt as u32));
                let _ = win.set_ignore_cursor_events(true);
                if let Ok(hwnd) = win.hwnd() {
                    let radius = (10.0 * sc).round() as i32;
                    win32::set_round_window_region(hwnd.0 as isize, w, hgt, radius);
                }
                let _ = win.emit("toast://text", &msg);
                let _ = win.show();
                // 重新计一次兜底关闭，防止前端异常导致第二次提示长留
                let h2 = h.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(3000));
                    let h3 = h2.clone();
                    let _ = h2.run_on_main_thread(move || {
                        if let Some(w) = h3.get_webview_window("toast") {
                            let _ = w.close();
                        }
                    });
                });
                return;
            }

            match WebviewWindowBuilder::new(&h, "toast", WebviewUrl::App("index.html".into()))
                .title("Look Toast")
                .decorations(false)
                .transparent(true)
                .background_color(Color(0, 0, 0, 0))
                .inner_size(300.0, 66.0)
                .always_on_top(true)
                .skip_taskbar(true)
                .shadow(false)
                .resizable(false)
                .focused(false)
                .visible(false)
                .build()
            {
                Ok(win) => {
                    let _ = win.set_position(PhysicalPosition::new(px, py));
                    let _ = win.set_size(PhysicalSize::new(w as u32, hgt as u32));
                    let _ = win.set_ignore_cursor_events(true);
                    // 同控制条：WebView2 的底不透明，圆角只能靠窗口区域裁出来
                    if let Ok(hwnd) = win.hwnd() {
                        let radius = (10.0 * sc).round() as i32;
                        win32::set_round_window_region(hwnd.0 as isize, w, hgt, radius);
                    }
                    if let Some(st) = h.try_state::<AppState>() {
                        *st.toast_msg.lock() = msg;
                    }
                    // 前端渲染好后会调 toast_ready 显示；这里再兜底关掉，防前端异常窗口长留
                    let h2 = h.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(3000));
                        let h3 = h2.clone();
                        let _ = h2.run_on_main_thread(move || {
                            if let Some(w) = h3.get_webview_window("toast") {
                                let _ = w.close();
                            }
                        });
                    });
                }
                Err(e) => crate::dlog!("[look] toast 创建失败: {}", e),
            }
        });
    });
}

pub fn notify(app: &AppHandle, body: &str) {
    show_toast(app, body);
}

pub fn start_recording(app: &AppHandle, rect: Rect) -> Result<(), String> {
    let cfg = Config::load();
    let state = app.state::<AppState>();
    *state.last_region.lock() = Some(rect);

    // 控制条必须先建、再开录：WebView 窗口创建会同步等待主线程，若录制已经开跑、
    // 主线程被逐帧 emit 占用，build() 会一直卡住（既无成功日志也无失败日志）。
    show_control_bar(app, rect).map_err(|e| {
        crate::dlog!("[look] 控制条创建失败: {}", e);
        e
    })?;

    // 虚线边框（失败不影响录制）
    if let Err(e) = show_record_frame(app, rect) {
        crate::dlog!("[look] 录制边框创建失败: {}", e);
    }

    record::start(
        app.clone(),
        cfg,
        RecordOptions {
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: rect.h,
            fps: None,
            max_width: None,
            quality: None,
            cursor: None,
            ripple: None,
            key_hud: None,
            max_seconds: None,
        },
    )?;
    Ok(())
}

pub fn register_hotkeys(app: &AppHandle, cfg: &Config) -> Vec<String> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let mut conflicts = Vec::new();

    // 只用 gs.register 注册，分发给 lib.rs 里 with_handler 的统一回调，
    // 避免 on_shortcut 逐个注册时后一次覆盖前一次的处理器
    let state = app.state::<AppState>();
    for (slot, text) in [
        (0, cfg.hotkeys.shot.clone()),
        (1, cfg.hotkeys.record.clone()),
    ] {
        if text.trim().is_empty() {
            if slot == 0 {
                *state.shot_key.lock() = None;
            } else {
                *state.rec_key.lock() = None;
            }
            continue;
        }
        match text.parse::<Shortcut>() {
            Ok(sc) => {
                if slot == 0 {
                    *state.shot_key.lock() = Some(sc.clone());
                } else {
                    *state.rec_key.lock() = Some(sc.clone());
                }
                if gs.register(sc).is_err() {
                    conflicts.push(text.clone());
                }
            }
            Err(_) => conflicts.push(text.clone()),
        }
    }
    conflicts
}

/// 全局快捷键统一回调：按注册时的 Shortcut 分发到截图 / 录制。
pub fn handle_shortcut(app: &AppHandle, shortcut: &Shortcut, pressed: bool) {
    if !pressed {
        return;
    }
    let state = app.state::<AppState>();
    let is_shot = state
        .shot_key
        .lock()
        .as_ref()
        .map(|k| k == shortcut)
        .unwrap_or(false);
    let is_rec = state
        .rec_key
        .lock()
        .as_ref()
        .map(|k| k == shortcut)
        .unwrap_or(false);
    crate::dlog!(
        "[look] 热键触发 shortcut={} shot={} rec={}",
        shortcut, is_shot, is_rec
    );
    if is_shot {
        if let Err(e) = open_capture(app, "shot") {
            crate::dlog!("[look] open_capture 失败: {}", e);
        }
    } else if is_rec {
        toggle_record(app);
    }
}

pub fn build_tray(app: &AppHandle) -> Result<(), String> {
    let shot = MenuItem::with_id(app, "shot", "截图", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let rec = MenuItem::with_id(app, "record", "录屏", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let settings = MenuItem::with_id(app, "settings", "设置…", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let output = MenuItem::with_id(app, "output", "打开输出目录", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)
        .map_err(|e| e.to_string())?;

    let menu = Menu::with_items(app, &[&shot, &rec, &settings, &output, &quit])
        .map_err(|e| e.to_string())?;

    let mut builder = TrayIconBuilder::with_id("look-tray")
        .menu(&menu)
        .tooltip("Look 录刻")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "shot" => {
                let _ = open_capture(app, "shot");
            }
            "record" => toggle_record(app),
            "settings" => show_main(app),
            "output" => open_output_dir(app),
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = open_capture(tray.app_handle(), "shot");
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn show_main(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

pub fn open_output_dir(_app: &AppHandle) {
    let cfg = Config::load();
    let dir = cfg.ensure_output_dir();
    let _ = std::process::Command::new("explorer.exe")
        .arg(dir.to_string_lossy().to_string())
        .spawn();
}
