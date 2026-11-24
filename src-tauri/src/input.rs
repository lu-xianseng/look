//! 全局键鼠采集：低层钩子线程 + 事件缓冲。

use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DestroyWindow, DispatchMessageW, GetMessageW, KBDLLHOOKSTRUCT,
    KillTimer, MSLLHOOKSTRUCT, MSG, SetTimer, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, HWND_MESSAGE, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

#[derive(Clone, Debug)]
pub enum Ev {
    KeyDown(String),
    KeyUp,
    Move(i32, i32),
    Down(u8, i32, i32),
    Up(u8, i32, i32),
    Wheel(i16, i32, i32),
}

#[derive(Clone, Debug)]
pub struct InputEvent {
    /// 录制开始后的毫秒
    pub t: u32,
    pub ev: Ev,
}

pub struct InputCtx {
    start: Instant,
    events: Mutex<Vec<InputEvent>>,
    last_move: Mutex<u32>,
    /// 停止录制用的组合键（vk + 修饰位），在钩子内直接识别，
    /// 因为录制期间系统 WM_HOTKEY 可能被低层钩子影响而收不到
    stop_vk: u32,
    stop_mods: u32,
    hit: AtomicBool,
    mod_ctrl: AtomicBool,
    mod_alt: AtomicBool,
    mod_shift: AtomicBool,
    mod_win: AtomicBool,
}

/// 解析 "Ctrl+Alt+R" 这类快捷键文本 → (vk, 修饰位)；无法解析时返回 None。
/// 修饰位：1=Alt 2=Ctrl 4=Shift 8=Win
pub fn parse_combo(text: &str) -> Option<(u32, u32)> {
    let mut mods = 0u32;
    let mut vk = None;
    for part in text.split('+') {
        let p = part.trim().to_ascii_lowercase();
        match p.as_str() {
            "ctrl" | "control" => mods |= 2,
            "alt" => mods |= 1,
            "shift" => mods |= 4,
            "win" | "super" | "meta" => mods |= 8,
            "" => {}
            other => {
                if other.len() == 1 {
                    let b = other.as_bytes()[0];
                    if b.is_ascii_alphabetic() {
                        vk = Some(b.to_ascii_uppercase() as u32);
                    } else if b.is_ascii_digit() {
                        vk = Some(b as u32);
                    } else {
                        return None;
                    }
                } else if let Some(n) = other.strip_prefix('f') {
                    let n: u32 = n.parse().ok()?;
                    if (1..=12).contains(&n) {
                        vk = Some(0x70 + n - 1);
                    } else {
                        return None;
                    }
                } else {
                    return None;
                }
            }
        }
    }
    vk.map(|v| (v, mods))
}

/// 当前采集上下文。必须每次 start 覆盖：用 OnceCell 会导致第二次录制仍写入首次的
/// 旧缓冲，表现为「录制中键帽/涟漪不出现、停止热键永不命中」。
static CTX: OnceCell<parking_lot::RwLock<Option<Arc<InputCtx>>>> = OnceCell::new();

fn ctx_slot() -> &'static parking_lot::RwLock<Option<Arc<InputCtx>>> {
    CTX.get_or_init(|| parking_lot::RwLock::new(None))
}

fn current_ctx() -> Option<Arc<InputCtx>> {
    ctx_slot().read().clone()
}

pub struct InputHandle {
    ctx: Arc<InputCtx>,
    stop: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl InputHandle {
    pub fn elapsed_ms(&self) -> u32 {
        self.ctx.start.elapsed().as_millis() as u32
    }

    /// 钩子内命中停止热键
    pub fn stop_hit(&self) -> bool {
        self.ctx.hit.load(Ordering::Relaxed)
    }

    pub fn snapshot(&self) -> Vec<InputEvent> {
        self.ctx.events.lock().clone()
    }

    /// 累计采集到的事件条数，用于诊断钩子是否真的在回调。
    pub fn total(&self) -> usize {
        self.ctx.events.lock().len()
    }

    /// 只取最近 1.6 秒的事件，避免每帧全量克隆。
    pub fn recent(&self, now: u32) -> Vec<InputEvent> {
        let list = self.ctx.events.lock();
        let mut out: Vec<InputEvent> = Vec::new();
        for e in list.iter().rev() {
            if now >= e.t && now - e.t > 1600 {
                break;
            }
            out.push(e.clone());
        }
        out.reverse();
        out
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

pub fn start(stop_combo: Option<(u32, u32)>) -> InputHandle {
    let (stop_vk, stop_mods) = stop_combo.unwrap_or((0, 0));
    let ctx = Arc::new(InputCtx {
        start: Instant::now(),
        events: Mutex::new(Vec::with_capacity(2048)),
        last_move: Mutex::new(0),
        stop_vk,
        stop_mods,
        hit: AtomicBool::new(false),
        mod_ctrl: AtomicBool::new(false),
        mod_alt: AtomicBool::new(false),
        mod_shift: AtomicBool::new(false),
        mod_win: AtomicBool::new(false),
    });
    let _ = &ctx;
    *ctx_slot().write() = Some(ctx.clone());

    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = stop.clone();
    let join = std::thread::spawn(move || unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb_proc), module, 0);
        let ms = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0);
        if kb.is_null() || ms.is_null() {
            crate::dlog!(
                "[look] 全局钩子安装失败 kb={:?} ms={:?} err={:?}",
                kb,
                ms,
                std::io::Error::last_os_error()
            );
        } else {
            crate::dlog!("[look] 全局钩子已安装 kb={:?} ms={:?}", kb, ms);
        }
        // 低层钩子的回调要求安装线程真正泵消息。实测：仅 GetMessageW 的裸循环在本机收不到
        // 任何回调（input_events 恒为 0），必须有属于该线程的窗口并走 GetMessage/DispatchMessage。
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let title: Vec<u16> = "LookMsg\0".encode_utf16().collect();
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            module,
            std::ptr::null(),
        );
        let timer = if hwnd.is_null() {
            0
        } else {
            SetTimer(hwnd, 1, 100, None)
        };
        let mut msg: MSG = std::mem::zeroed();
        loop {
            let r = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
            if r <= 0 {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }
        }
        if timer != 0 {
            KillTimer(hwnd, 1);
        }
        if !hwnd.is_null() {
            DestroyWindow(hwnd);
        }
        if !kb.is_null() {
            UnhookWindowsHookEx(kb);
        }
        if !ms.is_null() {
            UnhookWindowsHookEx(ms);
        }
    });

    InputHandle {
        ctx,
        stop,
        join: Some(join),
    }
}

fn push(ctx: &InputCtx, ev: Ev) {
    let t = ctx.start.elapsed().as_millis() as u32;
    let mut list = ctx.events.lock();
    if list.len() > 20000 {
        // 防止超长录制把内存吃满，丢最旧的一半
        let cut = list.len() / 2;
        list.drain(0..cut);
    }
    list.push(InputEvent { t, ev });
}

unsafe extern "system" fn kb_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        if let Some(ctx) = current_ctx() {
            let kb = &*(lparam as *const KBDLLHOOKSTRUCT);
            let vk = kb.vkCode;
            let msg = wparam as u32;
            let down = matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN);
            let up = matches!(msg, WM_KEYUP | WM_SYSKEYUP);
            if down || up {
                match vk {
                    0x11 | 0xA2 | 0xA3 => ctx.mod_ctrl.store(down, Ordering::Relaxed),
                    0x12 | 0xA4 | 0xA5 => ctx.mod_alt.store(down, Ordering::Relaxed),
                    0x10 | 0xA0 | 0xA1 => ctx.mod_shift.store(down, Ordering::Relaxed),
                    0x5B | 0x5C => ctx.mod_win.store(down, Ordering::Relaxed),
                    _ => {}
                }
            }
            if down {
                // 录制期间系统 WM_HOTKEY 不可靠，直接在钩子里识别停止热键
                if ctx.stop_vk != 0 && vk == ctx.stop_vk {
                    let cur = (ctx.mod_alt.load(Ordering::Relaxed) as u32)
                        | ((ctx.mod_ctrl.load(Ordering::Relaxed) as u32) << 1)
                        | ((ctx.mod_shift.load(Ordering::Relaxed) as u32) << 2)
                        | ((ctx.mod_win.load(Ordering::Relaxed) as u32) << 3);
                    if cur == ctx.stop_mods {
                        crate::dlog!("[look] 钩子命中停止热键 vk={} mods={}", vk, cur);
                        ctx.hit.store(true, Ordering::Relaxed);
                    }
                }
                push(&ctx, Ev::KeyDown(key_label(vk)));
            } else if up {
                push(&ctx, Ev::KeyUp);
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        if let Some(ctx) = current_ctx() {
            let ms = &*(lparam as *const MSLLHOOKSTRUCT);
            let x = ms.pt.x;
            let y = ms.pt.y;
            let msg = wparam as u32;
            match msg {
                WM_MOUSEMOVE => {
                    let mut last = ctx.last_move.lock();
                    let now = ctx.start.elapsed().as_millis() as u32;
                    if now.saturating_sub(*last) >= 25 {
                        *last = now;
                        drop(last);
                        push(&ctx, Ev::Move(x, y));
                    }
                }
                WM_LBUTTONDOWN => push(&ctx, Ev::Down(0, x, y)),
                WM_LBUTTONUP => push(&ctx, Ev::Up(0, x, y)),
                WM_RBUTTONDOWN => push(&ctx, Ev::Down(1, x, y)),
                WM_RBUTTONUP => push(&ctx, Ev::Up(1, x, y)),
                WM_MBUTTONDOWN => push(&ctx, Ev::Down(2, x, y)),
                WM_MBUTTONUP => push(&ctx, Ev::Up(2, x, y)),
                WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                    let delta = ((ms.mouseData >> 16) as i16).wrapping_div(120);
                    push(&ctx, Ev::Wheel(delta, x, y));
                }
                _ => {}
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

pub fn key_label(vk: u32) -> String {
    match vk {
        0x08 => "Backspace".into(),
        0x09 => "Tab".into(),
        0x0D => "Enter".into(),
        0x1B => "Esc".into(),
        0x20 => "Space".into(),
        0x25 => "←".into(),
        0x26 => "↑".into(),
        0x27 => "→".into(),
        0x28 => "↓".into(),
        0x10 | 0xA0 | 0xA1 => "Shift".into(),
        0x11 | 0xA2 | 0xA3 => "Ctrl".into(),
        0x12 | 0xA4 | 0xA5 => "Alt".into(),
        0x5B | 0x5C => "Win".into(),
        0x14 => "CapsLock".into(),
        0x2E => "Del".into(),
        0x24 => "Home".into(),
        0x23 => "End".into(),
        0x21 => "PgUp".into(),
        0x22 => "PgDn".into(),
        0x2D => "Ins".into(),
        0x30..=0x39 => ((vk as u8) as char).to_string(),
        0x41..=0x5A => ((vk as u8) as char).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0xBA => ";".into(),
        0xBB => "=".into(),
        0xBC => ",".into(),
        0xBD => "-".into(),
        0xBE => ".".into(),
        0xBF => "/".into(),
        0xC0 => "`".into(),
        0xDB => "[".into(),
        0xDC => "\\".into(),
        0xDD => "]".into(),
        0xDE => "'".into(),
        _ => format!("VK{:02X}", vk),
    }
}
