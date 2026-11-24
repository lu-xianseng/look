//! 前端可调用的命令。

use crate::app::{self, AppState};
use crate::model::{Config, MonitorShot, Rect};
use crate::record::{self, RecordOptions, RecordResult};
use crate::win32;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

#[derive(Serialize)]
pub struct OverlayBootstrap {
    pub mode: String,
    pub shots: Vec<MonitorShot>,
    /// 启动那一刻的光标位置（物理坐标）。前端用它把中央提示放到光标所在那块屏，
    /// 而不是虚拟桌面正中间（双屏时正好落在两块屏的交界处）。
    pub cursor: (i32, i32),
}

/// 浮层把冻结截图铺好后调用：这时才显示窗口，避免先露出黑底（用户看到「闪一下黑」）
#[tauri::command]
pub fn overlay_ready(win: tauri::WebviewWindow) -> Result<(), String> {
    app::show_overlay(&win);
    Ok(())
}

#[derive(Serialize)]
pub struct ShotResult {
    pub path: String,
    pub bytes: u64,
}

#[derive(Serialize)]
pub struct RecordStatus {
    pub recording: bool,
    pub paused: bool,
}

/// 前端把诊断信息写进日志文件（%APPDATA%\Look\look.log）。
#[tauri::command]
pub fn ui_log(msg: String) {
    crate::dlog!("{}", msg);
}

#[tauri::command]
pub fn config_get() -> Config {

    Config::load()
}

#[tauri::command]
pub fn config_set(app: AppHandle, cfg: Config) -> Result<Vec<String>, String> {
    cfg.save()?;
    let _ = crate::win32::set_autostart(cfg.app.auto_start);
    Ok(app::register_hotkeys(&app, &cfg))
}

#[tauri::command]
pub fn overlay_bootstrap(state: State<'_, AppState>) -> OverlayBootstrap {
    OverlayBootstrap {
        mode: state.mode.lock().clone(),
        shots: state.shots.lock().clone(),
        cursor: crate::win32::cursor_pos(),
    }
}

/// toast 窗口挂载后取要显示的那句话
#[tauri::command]
pub fn toast_text(state: State<'_, AppState>) -> String {
    state.toast_msg.lock().clone()
}

/// 前端把 toast 画好了：这时才显示窗口，避免先闪出一个空框
#[tauri::command]
pub fn toast_ready(win: tauri::WebviewWindow) -> Result<(), String> {
    win.show().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn toast_close(app: AppHandle) {
    if let Some(w) = app.get_webview_window("toast") {
        let _ = w.close();
    }
}

#[tauri::command]
pub fn window_at(x: i32, y: i32, state: State<'_, AppState>) -> Option<win32::WindowPeek> {
    let excludes = state.excludes.lock().clone();
    win32::window_at_point(x, y, &excludes)
}

/// 返回指定显示器的冻结帧 base64（前端以 data URL 加载，避免跨源污染画布导致 toDataURL 失败）。
#[tauri::command]
pub fn frame_b64(index: i32, state: State<'_, AppState>) -> Result<String, String> {
    let shots = state.shots.lock().clone();
    let shot = shots
        .iter()
        .find(|s| s.mon.index == index)
        .or_else(|| shots.first())
        .ok_or_else(|| "没有冻结帧".to_string())?;
    let bytes = std::fs::read(&shot.path).map_err(|e| format!("读取冻结帧失败: {}", e))?;
    Ok(STANDARD.encode(bytes))
}

#[tauri::command]
pub fn capture_close(app: AppHandle) {
    app::close_overlays(&app);
}

#[tauri::command]
pub fn open_capture_ui(app: AppHandle, mode: String) -> Result<(), String> {
    app::open_capture(&app, &mode)
}

/// 截断成文件名，toast 里只放得下一行短句
pub(crate) fn short_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

#[tauri::command]
pub fn shot_export(
    app: AppHandle,
    png_b64: String,
    copy: bool,
    save: bool,
    region: Option<Rect>,
) -> Result<ShotResult, String> {
    let png = STANDARD.decode(png_b64).map_err(|e| e.to_string())?;
    let img = image::load_from_memory(&png)
        .map_err(|e| format!("PNG 解码失败: {}", e))?
        .to_rgba8();
    let (w, h) = img.dimensions();
    let rgba = img.into_raw();

    let mut path_str = String::new();
    if save {
        let cfg = Config::load();
        let dir = cfg.ensure_output_dir();
        let p = dir.join(crate::model::timestamp_name("Shot", "png"));
        std::fs::write(&p, &png).map_err(|e| format!("保存截图失败: {}", e))?;
        path_str = p.to_string_lossy().to_string();
    }

    if copy {
        let files: Vec<&str> = if path_str.is_empty() {
            Vec::new()
        } else {
            vec![path_str.as_str()]
        };
        win32::write_clipboard(win32::ClipOptions {
            png: Some(&png),
            rgba: Some((rgba.as_slice(), w, h)),
            gif: None,
            text: None,
            files,
        })?;
    }

    if let Some(state) = app.try_state::<AppState>() {
        if let Some(r) = region {
            *state.last_region.lock() = Some(r);
        }
    }

    // 结果反馈：浮层紧接着就关了，没有这句话用户不知道到底复制没复制、存到哪了
    let msg = if copy && save {
        format!("已保存并复制：{}", short_name(&path_str))
    } else if copy {
        "已复制到剪贴板".to_string()
    } else if save {
        format!("已保存：{}", short_name(&path_str))
    } else {
        String::new()
    };
    if !msg.is_empty() {
        app::show_toast(&app, &msg);
    }

    Ok(ShotResult {
        path: path_str,
        bytes: png.len() as u64,
    })
}

#[tauri::command]
pub fn pin_png(app: AppHandle, png_b64: String) -> Result<String, String> {
    let png = STANDARD.decode(png_b64).map_err(|e| e.to_string())?;
    let img = image::load_from_memory(&png).map_err(|e| format!("PNG 解码失败: {}", e))?;
    let (w, h) = (img.width(), img.height());
    crate::dlog!("[pin] 收到: {}x{} bytes={}", w, h, png.len());

    // 立刻返回：WebviewWindowBuilder::build() 不能在 IPC 命令线程里同步调用，
    // 否则会卡住命令（日志到「收到」就停），前端也会被 busy 卡死。
    // 关浮层 + 建贴图窗口都排到主线程队列异步执行。
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(180));
        let h_inner = handle.clone();
        let _ = handle.run_on_main_thread(move || {
            app::close_overlays(&h_inner);
            let h_next = h_inner.clone();
            let _ = h_next.clone().run_on_main_thread(move || match app::create_pin(&h_next, &png, w, h) {
                Ok(label) => crate::dlog!("[pin] 成功: {}", label),
                Err(e) => crate::dlog!("[pin] 失败: {}", e),
            });
        });
    });

    Ok("queued".to_string())
}

/// Windows 自带 OCR 的脚本随 exe 走，运行时释放到缓存目录再跑：
/// 用户机器不一定装了 Tesseract，系统 OCR 是唯一天生可用的方案。
const OCR_PS1: &str = include_str!("../../tools/ocr.ps1");

fn run_ocr(img: &std::path::Path, out: &std::path::Path) -> Result<String, String> {
    let dir = crate::model::cache_dir();
    let _ = std::fs::create_dir_all(&dir);
    let ps1 = dir.join("ocr.ps1");
    // 每次都覆盖：装新版后缓存里的旧脚本不能残留
    std::fs::write(&ps1, OCR_PS1).map_err(|e| format!("释放 OCR 脚本失败: {}", e))?;

    let sysroot = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    let powershell = std::path::Path::new(&sysroot).join("System32\\WindowsPowerShell\\v1.0\\powershell.exe");
    let mut cmd = std::process::Command::new(powershell);
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-WindowStyle",
        "Hidden",
        "-File",
    ])
    .arg(&ps1)
    .arg("-Path")
    .arg(img)
    .arg("-Out")
    .arg(out);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let st = cmd.status().map_err(|e| format!("启动 OCR 失败: {}", e))?;
    let text = std::fs::read_to_string(out).unwrap_or_default();
    if let Some(e) = text.strip_prefix("ERR:") {
        return Err(e.trim().to_string());
    }
    if !st.success() {
        return Err(format!("OCR 进程退出码 {:?}", st.code()));
    }
    Ok(text)
}

/// 系统 OCR 会在汉字之间插空格（「创 建 时 间」），中文里这些空格要抹掉才好读。
fn tidy_ocr(text: &str) -> String {
    let is_cjk = |c: char| {
        matches!(c, '\u{4e00}'..='\u{9fff}' | '\u{3000}'..='\u{303f}' | '\u{ff00}'..='\u{ffef}')
    };
    text.split('\n')
        .map(|line| {
            let cs: Vec<char> = line.chars().collect();
            let mut out = String::with_capacity(line.len());
            for (i, c) in cs.iter().enumerate() {
                if *c == ' '
                    && i > 0
                    && i + 1 < cs.len()
                    && is_cjk(cs[i - 1])
                    && is_cjk(cs[i + 1])
                {
                    continue;
                }
                out.push(*c);
            }
            out.trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 点「OCR」按钮：把选区图交给系统 OCR，识别完写剪贴板并用 toast 报结果。
/// 识别要 1-2 秒，所以立刻返回，结果走 toast，不让前端干等。
#[tauri::command]
pub fn ocr_text(app: AppHandle, png_b64: String) -> Result<String, String> {
    let mut png = STANDARD.decode(png_b64).map_err(|e| e.to_string())?;
    let dir = crate::model::cache_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建缓存目录失败: {}", e))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let img = dir.join(format!("ocr_{}.png", stamp));
    let out = dir.join(format!("ocr_{}.txt", stamp));

    // 系统 OCR 对 1.25 缩放下的小字识别很差，小图先放大 2 倍再送去识别，
    // 正确率提升很明显（放大失败就退回原图，不影响可用性）
    if let Ok(decoded) = image::load_from_memory(&png) {
        let (w, h) = (decoded.width(), decoded.height());
        if w < 1200 && h < 1200 && w > 0 && h > 0 {
            let up = image::imageops::resize(
                &decoded.to_rgba8(),
                w * 2,
                h * 2,
                image::imageops::FilterType::Triangle,
            );
            let mut buf = Vec::new();
            if image::DynamicImage::ImageRgba8(up)
                .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
                .is_ok()
                && !buf.is_empty()
            {
                png = buf;
            }
        }
    }

    std::fs::write(&img, &png).map_err(|e| format!("写截图失败: {}", e))?;
    crate::dlog!("[ocr] 收到 {} bytes -> {}", png.len(), img.display());

    std::thread::spawn(move || {
        match run_ocr(&img, &out) {
            Ok(t) => {
                let text = tidy_ocr(&t.trim().replace("\r\n", "\n"));
                if text.is_empty() {
                    app::show_toast(&app, "没识别到文字");
                } else {
                    let n = text.chars().count();
                    match win32::write_clipboard(win32::ClipOptions {
                        text: Some(&text),
                        ..Default::default()
                    }) {
                        Ok(_) => app::show_toast(&app, &format!("已识别 {} 字，已复制到剪贴板", n)),
                        Err(e) => app::show_toast(&app, &format!("识别到 {} 字，但写剪贴板失败：{}", n, e)),
                    }
                }
            }
            Err(e) => {
                crate::dlog!("[ocr] 失败: {}", e);
                app::show_toast(&app, &format!("文字识别失败：{}", e));
            }
        }
        let _ = std::fs::remove_file(&img);
        let _ = std::fs::remove_file(&out);
    });

    Ok("queued".to_string())
}

/// 开始长截图：建预览窗 + 起抓帧线程，之后由用户自己滚轮，点按钮才结束
#[tauri::command]
pub fn scroll_start(app: AppHandle, rect: Rect) -> Result<(), String> {
    crate::scroll::start(app.clone(), rect)?;
    // 预览窗不能在本命令线程里同步 build（会把这个命令卡住），排到主线程队列
    let h = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        let h_inner = h.clone();
        let _ = h.run_on_main_thread(move || {
            if let Err(e) = crate::app::show_scroll_frame(&h_inner, rect) {
                crate::dlog!("[scroll] 选区边框失败: {}", e);
            }
            if let Err(e) = crate::app::show_scroll_preview(&h_inner, rect) {
                crate::dlog!("[scroll] 预览窗失败: {}", e);
            }
        });
    });
    Ok(())
}

/// 结束长截图会话并输出；copy/save 由预览窗上的按钮决定
#[tauri::command]
pub fn scroll_finish(app: AppHandle, copy: bool, save: bool) -> Result<String, String> {
    let msg = crate::scroll::finish(&app, copy, save)?;
    crate::app::show_toast(&app, &msg);
    Ok(msg)
}

/// 丢弃这次长截图
#[tauri::command]
pub fn scroll_cancel(app: AppHandle) -> Result<(), String> {
    crate::scroll::cancel(&app);
    Ok(())
}

#[tauri::command]
pub fn record_begin(app: AppHandle, opts: RecordOptions) -> Result<(), String> {
    let region = Rect::new(opts.x, opts.y, opts.w, opts.h);

    // 立刻返回 Ok：本命令内部要关掉发起调用的浮层窗口，
    // 窗口一旦销毁 invoke 的响应就送不回去，前端会永远卡在 await 上
    // （表现就是「点开始录制毫无反应」）。所以关窗口、建控制条、开录
    // 全部排到主线程队列里异步执行。
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(180));
        let h_inner = handle.clone();
        let _ = handle.run_on_main_thread(move || {
            app::close_overlays(&h_inner);
            // 再排一轮，给浮层窗口真正销毁留出时间，避免控制条建在残壳上
            let h_next = h_inner.clone();
            let _ = h_next.clone().run_on_main_thread(move || {
                if let Err(e) = app::start_recording(&h_next, region) {
                    crate::dlog!("[record_begin] start_recording 失败: {}", e);
                }
            });
        });
    });

    Ok(())
}

/// 请求停止录制：立即返回，结果通过 `rec://saved` / `rec://state` 事件与系统通知给出。
#[tauri::command]
pub fn record_stop() -> Result<(), String> {
    record::request_stop();
    Ok(())
}

#[tauri::command]
pub fn record_cancel() -> Result<(), String> {
    record::request_cancel();
    Ok(())
}

#[tauri::command]
pub fn record_set_pause(paused: bool) {
    record::set_pause(paused);
}

#[tauri::command]
pub fn record_status() -> RecordStatus {
    RecordStatus {
        recording: record::is_recording(),
        paused: record::is_paused(),
    }
}

#[tauri::command]
pub fn output_dir_open(app: AppHandle) {
    app::open_output_dir(&app);
}

#[tauri::command]
pub fn settings_show(app: AppHandle) {
    app::show_main(&app);
}

#[tauri::command]
pub fn app_exit(app: AppHandle) {
    app.exit(0);
}
