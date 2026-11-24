//! Win32 底层：DPI、显示器枚举、抓屏、窗口探测、光标、剪贴板。

use crate::model::{MonitorInfo, Rect};
use std::ffi::c_void;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows_sys::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
};
use windows_sys::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateRoundRectRgn, DeleteDC, DeleteObject,
    EnumDisplayMonitors, GetDC, GetDIBits, GetMonitorInfoW, ReleaseDC, SelectObject, SetWindowRgn,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, HDC, HGDIOBJ, HMONITOR,
    MONITORINFOEXW, SRCCOPY,
};
use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetClassNameW, GetCursorPos, GetWindowTextW, IsWindowVisible, SetWindowDisplayAffinity,
    WindowFromPoint, GA_ROOT,
};

const CF_DIB: u32 = 8;
const CF_UNICODETEXT: u32 = 13;
const CF_HDROP: u32 = 15;
const WDA_EXCLUDEFROMCAPTURE: u32 = 0x11;
const MONITORINFOF_PRIMARY: u32 = 1;

/// 进程级 DPI 感知必须最早调用，否则所有坐标都是缩放后的逻辑像素。
pub fn init_dpi_awareness() {
    use windows_sys::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

/// 开机自启：写/删 HKCU\Software\Microsoft\Windows\CurrentVersion\Run 下的 "Look" 项。
/// 用注册表而不是启动文件夹快捷方式，省掉 COM 依赖，删掉就是真删掉。
/// debug 版不写：它的路径在 target\debug 下，写进去只会让开机跑调试版。
pub fn set_autostart(enable: bool) -> Result<(), String> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegSetValueExW, HKEY_CURRENT_USER,
        KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
    };
    if cfg!(debug_assertions) {
        return Ok(());
    }
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    let sub = wide(r"Software\Microsoft\Windows\CurrentVersion\Run");
    let name = wide("Look");
    unsafe {
        let mut hkey: *mut std::ffi::c_void = std::ptr::null_mut();
        let rc = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            sub.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut hkey,
            std::ptr::null_mut(),
        );
        if rc != 0 {
            return Err(format!("打开自启注册表项失败 code={}", rc));
        }
        let rc = if enable {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let val = format!("\"{}\"", exe.display());
            // REG_SZ 存的是 UTF-16LE 宽字符，必须按 2 字节/字符写，不能直接塞 UTF-8 字节
            let wval: Vec<u16> = val.encode_utf16().collect();
            RegSetValueExW(
                hkey,
                name.as_ptr(),
                0,
                REG_SZ,
                wval.as_ptr() as *const u8,
                (wval.len() * 2) as u32,
            )
        } else {
            RegDeleteValueW(hkey, name.as_ptr())
        };
        let _ = RegCloseKey(hkey);
        // 2 = ERROR_FILE_NOT_FOUND：本来就没这一项，删除也算成功
        if rc != 0 && !(rc == 2 && !enable) {
            return Err(format!("写入开机自启失败 code={}", rc));
        }
    }
    Ok(())
}

/// 把窗口强制推到前台并拿到键盘焦点。
/// Tauri 的 set_focus() 只是请求，Windows 前台锁定会让它静默失败——
/// 表现就是浮层能框选、但键盘 Enter / Esc / Tab 全都没反应。
pub fn force_foreground(hwnd: HWND) -> bool {
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
    };
    unsafe {
        let fg = GetForegroundWindow();
        let mut fg_pid: u32 = 0;
        let fg_tid = if fg.is_null() {
            0
        } else {
            GetWindowThreadProcessId(fg, &mut fg_pid)
        };
        let my_tid = GetCurrentThreadId();
        let attached = !fg.is_null() && fg_tid != 0 && fg_tid != my_tid;
        if attached {
            let _ = AttachThreadInput(my_tid, fg_tid, 1);
        }
        let _ = BringWindowToTop(hwnd);
        let ok = SetForegroundWindow(hwnd) != 0;
        if attached {
            let _ = AttachThreadInput(my_tid, fg_tid, 0);
        }
        ok
    }
}

unsafe extern "system" fn monitor_enum_proc(
    hmon: HMONITOR,
    _hdc: HDC,
    _clip: *mut RECT,
    data: LPARAM,
) -> windows_sys::Win32::Foundation::BOOL {
    let list = &mut *(data as *mut Vec<MonitorInfo>);
    let mut info: MONITORINFOEXW = std::mem::zeroed();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(hmon, &mut info.monitorInfo as *mut _ as *mut _) != 0 {
        let mut dx: u32 = 96;
        let mut dy: u32 = 96;
        let _ = GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let r = info.monitorInfo.rcMonitor;
        let id = String::from_utf16_lossy(&info.szDevice)
            .trim_end_matches('\0')
            .to_string();
        list.push(MonitorInfo {
            id: if id.is_empty() { format!("mon{}", list.len()) } else { id },
            index: list.len() as i32,
            x: r.left,
            y: r.top,
            w: r.right - r.left,
            h: r.bottom - r.top,
            scale: (dx as f64 / 96.0).max(1.0),
            primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
        });
    }
    1
}

pub fn enum_monitors() -> Vec<MonitorInfo> {
    let mut list: Vec<MonitorInfo> = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            null_mut(),
            std::ptr::null(),
            Some(monitor_enum_proc),
            &mut list as *mut _ as LPARAM,
        );
    }
    list
}

/// 可复用的区域抓屏器：一次建 DC/位图，逐帧 BitBlt。
pub struct Grabber {
    screen: HDC,
    mem: HDC,
    bmp: windows_sys::Win32::Graphics::Gdi::HBITMAP,
    old: HGDIOBJ,
    rect: Rect,
    buf: Vec<u8>,
    bi: BITMAPINFO,
}

impl Grabber {
    pub fn new(rect: Rect) -> Result<Grabber, String> {
        if rect.is_empty() {
            return Err("抓屏区域为空".into());
        }
        unsafe {
            let screen = GetDC(null_mut());
            if screen.is_null() {
                return Err("GetDC 失败".into());
            }
            let mem = CreateCompatibleDC(screen);
            let bmp = CreateCompatibleBitmap(screen, rect.w, rect.h);
            let old = SelectObject(mem, bmp);
            let mut bi: BITMAPINFO = std::mem::zeroed();
            bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bi.bmiHeader.biWidth = rect.w;
            bi.bmiHeader.biHeight = -rect.h;
            bi.bmiHeader.biPlanes = 1;
            bi.bmiHeader.biBitCount = 32;
            bi.bmiHeader.biCompression = BI_RGB;
            let buf = vec![0u8; (rect.w as usize) * (rect.h as usize) * 4];
            Ok(Grabber {
                screen,
                mem,
                bmp,
                old,
                rect,
                buf,
                bi,
            })
        }
    }

    /// 抓一帧，返回 BGRA（top-down）。
    pub fn grab(&mut self) -> Result<&[u8], String> {
        unsafe {
            SelectObject(self.mem, self.bmp);
            let ok = BitBlt(
                self.mem,
                0,
                0,
                self.rect.w,
                self.rect.h,
                self.screen,
                self.rect.x,
                self.rect.y,
                SRCCOPY | CAPTUREBLT,
            );
            SelectObject(self.mem, self.old);
            if ok == 0 {
                return Err("BitBlt 失败".into());
            }
            let lines = GetDIBits(
                self.mem,
                self.bmp,
                0,
                self.rect.h as u32,
                self.buf.as_mut_ptr() as *mut c_void,
                &mut self.bi,
                DIB_RGB_COLORS,
            );
            if lines != self.rect.h {
                return Err(format!("GetDIBits 失败（{}）", lines));
            }
        }
        Ok(&self.buf)
    }
}

impl Drop for Grabber {
    fn drop(&mut self) {
        unsafe {
            if !self.old.is_null() {
                SelectObject(self.mem, self.old);
            }
            if !self.bmp.is_null() {
                DeleteObject(self.bmp);
            }
            if !self.mem.is_null() {
                DeleteDC(self.mem);
            }
            if !self.screen.is_null() {
                ReleaseDC(null_mut(), self.screen);
            }
        }
    }
}

/// 一次性抓取区域（BGRA）。
pub fn grab_once(rect: Rect) -> Result<Vec<u8>, String> {
    let mut g = Grabber::new(rect)?;
    Ok(g.grab()?.to_vec())
}

/// 找 prev 末尾若干行 与 cur 开头若干行 的最大重叠行数。
/// 只逐行抽样比较字节，够快也够准（滚动前后同一行像素是一样的）。
pub(crate) fn overlap_rows(prev: &[u8], cur: &[u8], row: usize, h: usize) -> usize {
    // 至少要有 20 行是新的，否则大片纯色会「匹配」出无意义的重复拼接
    let maxk = h.saturating_sub(20);
    if maxk == 0 {
        return 0;
    }
    let step = (row / 48).max(1);
    for k in (1..=maxk).rev() {
        let a = (h - k) * row;
        let mut ok = true;
        for r in 0..k {
            let pa = &prev[a + r * row..a + (r + 1) * row];
            let pb = &cur[r * row..(r + 1) * row];
            let mut diff = 0u32;
            let mut x = 0usize;
            while x + 1 < row {
                if pa[x] != pb[x] || pa[x + 1] != pb[x + 1] {
                    diff += 1;
                }
                x += step;
            }
            if diff > 0 {
                ok = false;
                break;
            }
        }
        if ok {
            return k;
        }
    }
    0
}


#[derive(Debug, Clone, serde::Serialize)]
pub struct WindowPeek {
    pub hwnd: i64,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub title: String,
    pub class: String,
}

fn wide_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

fn is_ignored_class(class: &str) -> bool {
    matches!(
        class,
        "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "Windows.UI.Core.CoreWindow"
    )
}

/// 命中测试：返回鼠标下最外层可见窗口的真实视觉边界。
pub fn window_at_point(x: i32, y: i32, ignore: &[isize]) -> Option<WindowPeek> {
    unsafe {
        let hwnd = WindowFromPoint(POINT { x, y });
        if hwnd.is_null() {
            return None;
        }
        if ignore.contains(&(hwnd as isize)) {
            return None;
        }
        let root: HWND = GetAncestor(hwnd, GA_ROOT);
        if root.is_null() || ignore.contains(&(root as isize)) {
            return None;
        }
        if IsWindowVisible(root) == 0 {
            return None;
        }

        let mut cloaked: u32 = 0;
        let hr = DwmGetWindowAttribute(
            root,
            DWMWA_CLOAKED as u32,
            &mut cloaked as *mut _ as *mut c_void,
            std::mem::size_of::<u32>() as u32,
        );
        if hr == 0 && cloaked != 0 {
            return None;
        }

        let mut rect: RECT = std::mem::zeroed();
        let hr = DwmGetWindowAttribute(
            root,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            &mut rect as *mut _ as *mut c_void,
            std::mem::size_of::<RECT>() as u32,
        );
        if hr != 0 {
            use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;
            if GetWindowRect(root, &mut rect) == 0 {
                return None;
            }
        }

        let w = rect.right - rect.left;
        let h = rect.bottom - rect.top;
        if w < 40 || h < 40 {
            return None;
        }

        let mut title_buf = [0u16; 512];
        GetWindowTextW(root, title_buf.as_mut_ptr(), title_buf.len() as i32);
        let title = wide_to_string(&title_buf);

        let mut class_buf = [0u16; 256];
        GetClassNameW(root, class_buf.as_mut_ptr(), class_buf.len() as i32);
        let class = wide_to_string(&class_buf);
        if is_ignored_class(&class) {
            return None;
        }

        Some(WindowPeek {
            hwnd: root as isize as i64,
            x: rect.left,
            y: rect.top,
            w,
            h,
            title,
            class,
        })
    }
}

pub fn cursor_pos() -> (i32, i32) {
    unsafe {
        let mut p: POINT = std::mem::zeroed();
        if GetCursorPos(&mut p) != 0 {
            (p.x, p.y)
        } else {
            (0, 0)
        }
    }
}

/// 让指定窗口不进入截屏/录屏画面。
pub fn exclude_from_capture(hwnd: isize) -> bool {
    unsafe { SetWindowDisplayAffinity(hwnd as isize as HWND, WDA_EXCLUDEFROMCAPTURE) != 0 }
}

/// 把窗口裁成圆角矩形。
/// WebView2 在不透明环境下圆角外侧会露出窗口底色，必须由窗口本身的区域来裁，
/// 这样圆角在 Win10 / 远程桌面上都能真实显示（代价是边缘没有抗锯齿）。
pub fn set_round_window_region(hwnd: isize, w: i32, h: i32, radius: i32) -> bool {
    if w <= 0 || h <= 0 {
        return false;
    }
    unsafe {
        // 右/下边界要比窗口大 1，否则最后一列像素会被裁掉
        let rgn = CreateRoundRectRgn(0, 0, w + 1, h + 1, radius * 2, radius * 2);
        if rgn.is_null() {
            return false;
        }
        // 成功后 region 归系统所有，不能再 DeleteObject
        SetWindowRgn(hwnd as HWND, rgn, 1) != 0
    }
}

// ---------------------------------------------------------------- clipboard

fn open_clipboard_retry() -> Result<(), String> {
    for _ in 0..10 {
        unsafe {
            if OpenClipboard(null_mut()) != 0 {
                return Ok(());
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Err("剪贴板被其他程序占用".into())
}

unsafe fn set_bytes(format: u32, bytes: &[u8]) -> bool {
    let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1));
    if h.is_null() {
        return false;
    }
    let p = GlobalLock(h);
    if p.is_null() {
        return false;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), p as *mut u8, bytes.len());
    GlobalUnlock(h);
    let r = SetClipboardData(format, h as *mut std::ffi::c_void);
    !r.is_null()
}

fn register_format(name: &str) -> u32 {
    use windows_sys::Win32::System::DataExchange::RegisterClipboardFormatW;
    let mut utf16: Vec<u16> = name.encode_utf16().collect();
    utf16.push(0);
    unsafe { RegisterClipboardFormatW(utf16.as_ptr()) }
}

/// 32bpp bottom-up DIB（CF_DIB），Alpha 通道保留但按 CF_DIB 语义忽略。
fn build_dib(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(40 + (w * h * 4) as usize);
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(w * h * 4).to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for y in (0..h).rev() {
        let row = (y * w * 4) as usize;
        for x in 0..w {
            let i = row + (x * 4) as usize;
            let (r, g, b, a) = (rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]);
            if a == 0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                out.extend_from_slice(&[b, g, r, a]);
            }
        }
    }
    out
}

fn build_hdrop(paths: &[&str]) -> Vec<u8> {
    let header_size = 20usize;
    let mut list: Vec<u16> = Vec::new();
    for p in paths {
        list.extend(p.encode_utf16());
        list.push(0);
    }
    list.push(0);
    let mut out = Vec::with_capacity(header_size + list.len() * 2);
    out.extend_from_slice(&(header_size as u32).to_le_bytes()); // pFiles
    out.extend_from_slice(&0u32.to_le_bytes()); // pt
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // fNC
    out.extend_from_slice(&1u32.to_le_bytes()); // fWide
    for u in list {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out
}

#[derive(Default)]
pub struct ClipOptions<'a> {
    pub png: Option<&'a [u8]>,
    pub rgba: Option<(&'a [u8], u32, u32)>,
    pub gif: Option<&'a [u8]>,
    /// 纯文本（OCR 结果），按 CF_UNICODETEXT 写入
    pub text: Option<&'a str>,
    pub files: Vec<&'a str>,
}

/// 一次打开剪贴板写入多种格式，尽量兼容不同应用。
pub fn write_clipboard(opts: ClipOptions) -> Result<(), String> {
    open_clipboard_retry()?;
    unsafe {
        EmptyClipboard();
        if let Some(t) = opts.text {
            let mut w: Vec<u16> = t.encode_utf16().collect();
            w.push(0);
            let bytes = std::slice::from_raw_parts(w.as_ptr() as *const u8, w.len() * 2);
            set_bytes(CF_UNICODETEXT, bytes);
        }
        if let Some(png) = opts.png {
            let fmt = register_format("PNG");
            if fmt != 0 {
                set_bytes(fmt, png);
            }
        }
        if let Some((rgba, w, h)) = opts.rgba {
            let dib = build_dib(rgba, w, h);
            set_bytes(CF_DIB, &dib);
        }
        if let Some(gif) = opts.gif {
            let fmt = register_format("GIF");
            if fmt != 0 {
                set_bytes(fmt, gif);
            }
        }
        if !opts.files.is_empty() {
            let drop = build_hdrop(&opts.files);
            set_bytes(CF_HDROP, &drop);
        }
        CloseClipboard();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_enumeration_works() {
        init_dpi_awareness();
        let mons = enum_monitors();
        assert!(!mons.is_empty(), "没有枚举到显示器");
        let m = &mons[0];
        assert!(m.w > 0 && m.h > 0);
    }

    #[test]
    fn capture_matches_monitor_size() {
        init_dpi_awareness();
        let mons = enum_monitors();
        let m = &mons[0];
        let rect = Rect::new(m.x, m.y, m.w, m.h);
        let px = grab_once(rect).expect("抓屏失败");
        assert_eq!(px.len(), (m.w as usize) * (m.h as usize) * 4);
        let lit = px
            .chunks(4)
            .filter(|p| p[0].max(p[1]).max(p[2]) != 0)
            .count();
        assert!(lit > 0, "抓到的画面全黑（屏幕锁定或 BitBlt 被拒）");
    }

    #[test]
    fn dib_header_is_wellformed() {
        let w = 4u32;
        let h = 3u32;
        let rgba = vec![255u8; (w * h * 4) as usize];
        let dib = build_dib(&rgba, w, h);
        assert_eq!(dib.len(), 40 + (w * h * 4) as usize);
        assert_eq!(u32::from_le_bytes(dib[0..4].try_into().unwrap()), 40);
        assert_eq!(i32::from_le_bytes(dib[4..8].try_into().unwrap()), w as i32);
        assert_eq!(i32::from_le_bytes(dib[8..12].try_into().unwrap()), h as i32);
        assert_eq!(u16::from_le_bytes(dib[12..14].try_into().unwrap()), 1);
        assert_eq!(u16::from_le_bytes(dib[14..16].try_into().unwrap()), 32);
        assert_eq!(u32::from_le_bytes(dib[16..20].try_into().unwrap()), 0);
        // 白色像素在 BGRA 顺序下为 FF FF FF FF
        assert_eq!(&dib[40..44], &[255, 255, 255, 255]);
    }

    #[test]
    fn hdrop_layout_is_wellformed() {
        let drop = build_hdrop(&["C:\\a\\b.gif"]);
        assert_eq!(u32::from_le_bytes(drop[0..4].try_into().unwrap()), 20);
        assert_eq!(u32::from_le_bytes(drop[16..20].try_into().unwrap()), 1);
        let utf16: Vec<u16> = drop[20..]
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(String::from_utf16_lossy(&utf16[..10]), "C:\\a\\b.gif");
        assert_eq!(utf16[10], 0);
        assert_eq!(utf16[11], 0);
    }

    #[test]
    fn capture_to_gif_roundtrip() {
        use crate::encode::{bgra_to_rgb, GifWriter};
        init_dpi_awareness();
        let rect = Rect::new(0, 0, 320, 200);
        let mut g = Grabber::new(rect).expect("Grabber 创建失败");
        let dir = std::env::temp_dir().join("look_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cap.gif");
        let mut w = GifWriter::new(&path, 320, 200, 64).unwrap();
        let mut rgb = Vec::new();
        for i in 0..6u32 {
            let f = g.grab().unwrap();
            bgra_to_rgb(f, &mut rgb);
            w.push(&rgb, i * 100).unwrap();
        }
        w.finish(600).unwrap();
        let data = std::fs::read(&path).unwrap();
        assert!(data.len() > 100, "gif 过小: {}", data.len());
        let mut opts = gif::DecodeOptions::new();
        opts.set_color_output(gif::ColorOutput::RGBA);
        let mut dec = opts
            .read_info(std::io::Cursor::new(&data))
            .expect("GIF 解码失败");
        assert_eq!((dec.width(), dec.height()), (320, 200));
        let mut n = 0;
        while dec.read_next_frame().unwrap().is_some() {
            n += 1;
        }
        assert!(n >= 1, "解码帧数为 0");
        println!("capture->gif: {} bytes, {} frames", data.len(), n);
    }
}

