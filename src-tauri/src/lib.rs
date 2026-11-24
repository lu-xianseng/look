mod app;
mod commands;
mod composite;
mod encode;
mod input;
mod model;
mod record;
mod scroll;
mod win32;

/// 同时写 stderr 与 %APPDATA%\Look\look.log（release 双击运行时没有控制台）
#[macro_export]
macro_rules! dlog {
    ($($arg:tt)*) => {
        crate::model::log(&format!($($arg)*))
    };
}

pub fn run() {
    win32::init_dpi_awareness();

    let mut builder = tauri::Builder::default();
    // SCREENKIT_MULTI=1 时跳过单实例限制，便于开发期并存多个实例做验收
    if std::env::var("SCREENKIT_MULTI").is_err() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            let _ = app::open_capture(app, "shot");
        }));
    }
    let builder = builder
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    app::handle_shortcut(
                        app,
                        shortcut,
                        event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed,
                    );
                })
                .build(),
        )
        .manage(app::AppState::new())
        .setup(|app| {
            let handle = app.handle().clone();
            crate::dlog!("[look] setup 进入");
            let exe_path = std::env::current_exe()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "?".into());
            crate::dlog!(
                "[look] exe={} debug_build={}（true=debug 版，前端走 devUrl，必须配 npm run dev）",
                exe_path,
                cfg!(debug_assertions)
            );
            crate::dlog!(
                "[look] log={}",
                model::data_dir().join("look.log").display()
            );
            let first_run = !model::data_dir().join("config.json").exists();
            let mut cfg = model::Config::load();
            crate::dlog!("[look] 配置已读取 first_run={}", first_run);
            // 开机自启跟着配置走：首次运行（默认开启）就会写进注册表
            match crate::win32::set_autostart(cfg.app.auto_start) {
                Ok(()) => {
                    crate::dlog!("[look] 开机自启已同步 auto_start={}", cfg.app.auto_start)
                }
                Err(e) => crate::dlog!("[look] 开机自启同步失败: {}", e),
            }
            // 开发期可用环境变量覆盖快捷键，避免与研究机/其它实例抢占同一组合键
            if let Ok(s) = std::env::var("SCREENKIT_HOTKEY_SHOT") {
                cfg.hotkeys.shot = s;
            }
            if let Ok(s) = std::env::var("SCREENKIT_HOTKEY_REC") {
                cfg.hotkeys.record = s;
            }
            if let Err(e) = app::build_tray(&handle) {
                crate::dlog!("托盘创建失败: {}", e);
            }
            crate::dlog!("[look] 托盘就绪");
            let conflicts = app::register_hotkeys(&handle, &cfg);
            crate::dlog!(
                "[look] 热键注册完成 shot={} rec={} 冲突={:?}",
                cfg.hotkeys.shot, cfg.hotkeys.record, conflicts
            );
            if first_run {
                let _ = cfg.save();
                app::show_main(&handle);
            }
            // 开发自检：SCREENKIT_SELFTEST=1 时不依赖任何 GUI 操作，直接跑一遍录制链路并打印结果
            if std::env::var("SCREENKIT_AUTOQUIT_SEC").is_ok() {
                let secs: u64 = std::env::var("SCREENKIT_AUTOQUIT_SEC")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(120);
                let h = handle.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(secs));
                    crate::dlog!("[look] 自动退出（{} 秒）", secs);
                    h.exit(0);
                });
            }
            if std::env::var("SCREENKIT_SELFTEST").is_ok() {
                let h = handle.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(1200));
                    // 走 start_recording（而不是 record::start），这样控制条也会被创建，
                    // 自检能覆盖「录制 + 控制条」整条链路，且全程不需要模拟鼠标键盘。
                    match crate::app::start_recording(
                        &h,
                        crate::model::Rect::new(100, 100, 600, 400),
                    ) {
                        Ok(()) => crate::dlog!("[selftest] 录制已启动"),
                        Err(e) => crate::dlog!("[selftest] 启动失败: {}", e),
                    }
                    std::thread::sleep(std::time::Duration::from_secs(4));
                    record::request_stop();
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    crate::dlog!(
                        "[selftest] 停止后 is_recording={}（false 表示线程已收尾）",
                        record::is_recording()
                    );
                    let dir = model::Config::load().ensure_output_dir();
                    if let Ok(rd) = std::fs::read_dir(&dir) {
                        let mut newest: Option<(std::time::SystemTime, String, u64)> = None;
                        for e in rd.flatten() {
                            let p = e.path();
                            if p.extension().map(|x| x == "gif").unwrap_or(false) {
                                let m = e.metadata().ok();
                                let t = m.as_ref().and_then(|m| m.modified().ok());
                                let len = m.map(|m| m.len()).unwrap_or(0);
                                if let Some(t) = t {
                                    if newest.as_ref().map(|n| t > n.0).unwrap_or(true) {
                                        newest = Some((t, p.to_string_lossy().to_string(), len));
                                    }
                                }
                            }
                        }
                        if let Some((_, path, len)) = newest {
                            crate::dlog!("[selftest] 最新 GIF: {} ({} 字节)", path, len);
                        }
                    }
                    h.exit(0);
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::config_get,
            commands::config_set,
            commands::overlay_bootstrap,
            commands::frame_b64,
            commands::window_at,
            commands::capture_close,
            commands::open_capture_ui,
            commands::shot_export,
            commands::pin_png,
            commands::ocr_text,
            commands::scroll_start,
            commands::scroll_finish,
            commands::scroll_cancel,
            commands::record_begin,
            commands::record_stop,
            commands::record_cancel,
            commands::record_set_pause,
            commands::record_status,
            commands::output_dir_open,
            commands::settings_show,
            commands::app_exit,
            commands::overlay_ready,
            commands::toast_text,
            commands::toast_ready,
            commands::toast_close,
            commands::ui_log
        ]);

    builder
        .run(tauri::generate_context!())
        .expect("Look 启动失败");
}
