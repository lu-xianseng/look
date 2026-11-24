use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Rect { x, y, w, h }
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }

    pub fn clamp_inside(&self, outer: Rect) -> Rect {
        let x1 = self.x.max(outer.x);
        let y1 = self.y.max(outer.y);
        let x2 = (self.x + self.w).min(outer.x + outer.w);
        let y2 = (self.y + self.h).min(outer.y + outer.h);
        Rect::new(x1, y1, (x2 - x1).max(0), (y2 - y1).max(0))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MonitorInfo {
    pub id: String,
    pub index: i32,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub scale: f64,
    pub primary: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MonitorShot {
    #[serde(flatten)]
    pub mon: MonitorInfo,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordConfig {
    pub fps: u32,
    pub max_width: u32,
    pub quality: String,
    pub cursor: bool,
    pub click_ripple: bool,
    pub key_hud: bool,
    pub max_seconds: u32,
}

impl Default for RecordConfig {
    fn default() -> Self {
        RecordConfig {
            fps: 15,
            max_width: 1280,
            quality: "medium".into(),
            cursor: true,
            click_ripple: true,
            key_hud: true,
            max_seconds: 300,
        }
    }
}

impl RecordConfig {
    /// 调色板颜色数，按画质档位取值。
    pub fn colors(&self) -> usize {
        match self.quality.as_str() {
            "low" => 128,
            "high" => 256,
            _ => 192,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputConfig {
    pub dir: String,
    pub copy_after: bool,
    /// gif | file | frame
    pub gif_clipboard: String,
}

impl Default for OutputConfig {
    fn default() -> Self {
        OutputConfig {
            dir: default_output_dir(),
            copy_after: true,
            gif_clipboard: "gif".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeyConfig {
    pub shot: String,
    pub record: String,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        HotkeyConfig {
            shot: "Ctrl+Alt+A".into(),
            record: "Ctrl+Alt+R".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub language: String,
    pub magic_capture: bool,
    /// 开机自启：写 HKCU\...\Run 项。默认开启。
    pub auto_start: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            language: "zh-CN".into(),
            magic_capture: false,
            auto_start: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub record: RecordConfig,
    pub output: OutputConfig,
    pub hotkeys: HotkeyConfig,
    pub app: AppConfig,
}

pub fn default_output_dir() -> String {
    match std::env::var("USERPROFILE") {
        Ok(home) => format!("{}\\Pictures\\Look", home),
        Err(_) => ".\\Look".to_string(),
    }
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    let dir = PathBuf::from(base).join("Look");
    let _ = std::fs::create_dir_all(&dir);
    // 从旧名 ScreenKit 迁移一次配置，避免改名后热键、输出目录设置被重置
    let old_cfg = PathBuf::from(std::env::var("APPDATA").unwrap_or_default())
        .join("ScreenKit")
        .join("config.json");
    let new_cfg = dir.join("config.json");
    if old_cfg.exists() && !new_cfg.exists() {
        // 顺手把输出目录里的旧名换掉，历史文件仍留在 Pictures\ScreenKit 不受影响
        if let Ok(text) = std::fs::read_to_string(&old_cfg) {
            let fixed = text.replace(
                r"\\Pictures\\ScreenKit",
                r"\\Pictures\\Look",
            );
            let _ = std::fs::write(&new_cfg, fixed);
        }
    }
    dir
}

pub fn cache_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("Look");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// 运行日志：追加写到 %APPDATA%\Look\look.log。
/// release 版双击运行时没有控制台，排查只能靠这个文件。
pub fn log(msg: &str) {
    use std::io::Write;
    let path = data_dir().join("look.log");
    // 超过 512KB 就截断重来，避免无限增长
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > 512 * 1024 {
            let _ = std::fs::remove_file(&path);
        }
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{}", msg);
    }
    eprintln!("{}", msg);
}

impl Config {
    fn path() -> PathBuf {
        data_dir().join("config.json")
    }

    pub fn load() -> Config {
        match std::fs::read_to_string(Config::path()) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Config::default(),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(Config::path(), text).map_err(|e| e.to_string())
    }

    pub fn ensure_output_dir(&self) -> PathBuf {
        let dir = PathBuf::from(&self.output.dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }
}

pub fn timestamp_name(prefix: &str, ext: &str) -> String {
    let now = chrono_like_now();
    format!("{}_{}.{}", prefix, now, ext)
}

/// 本地时间字符串 yyyy-MM-dd_HH-mm-ss，避免额外依赖。
fn chrono_like_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    // 东八区
    let local = secs + 8 * 3600;
    let days = local / 86400;
    let sod = local % 86400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
        y,
        m,
        d,
        sod / 3600,
        (sod % 3600) / 60,
        sod % 60
    )
}

/// Howard Hinnant 的 days -> (y, m, d) 算法。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
