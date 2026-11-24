//! 帧合成：光标、点击涟漪、键盘 HUD。缓冲区为 RGB8（录制编码用它）。

use crate::input::{Ev, InputEvent};
use crate::model::Rect;
use crate::win32;
use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};
use once_cell::sync::OnceCell;

#[derive(Clone, Copy, Debug)]
pub struct ComposeOpts {
    pub cursor: bool,
    pub ripple: bool,
    pub key_hud: bool,
}

static FONT: OnceCell<Option<FontRef<'static>>> = OnceCell::new();

fn font() -> Option<&'static FontRef<'static>> {
    FONT.get_or_init(|| {
        for p in [
            "C:\\Windows\\Fonts\\segoeui.ttf",
            "C:\\Windows\\Fonts\\arial.ttf",
            "C:\\Windows\\Fonts\\seguisb.ttf",
        ] {
            if let Ok(bytes) = std::fs::read(p) {
                let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
                if let Ok(f) = FontRef::try_from_slice(leaked) {
                    return Some(f);
                }
            }
        }
        None
    })
    .as_ref()
}

#[inline]
fn blend(buf: &mut [u8], w: u32, h: u32, x: i32, y: i32, color: [u8; 3], a: f32) {
    if a <= 0.0 || x < 0 || y < 0 {
        return;
    }
    if x >= w as i32 || y >= h as i32 {
        return;
    }
    let idx = ((y as u32 * w + x as u32) * 3) as usize;
    let alpha = a.min(1.0);
    for c in 0..3 {
        let src = color[c] as f32;
        let dst = buf[idx + c] as f32;
        buf[idx + c] = (dst * (1.0 - alpha) + src * alpha).round().clamp(0.0, 255.0) as u8;
    }
}

#[inline]
fn sd_round_box(px: f32, py: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - (hw - r);
    let qy = (py - cy).abs() - (hh - r);
    let ax = qx.max(0.0);
    let ay = qy.max(0.0);
    (ax * ax + ay * ay).sqrt() + qx.max(qy).min(0.0) - r
}

fn round_rect(
    buf: &mut [u8],
    w: u32,
    h: u32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    radius: f32,
    color: [u8; 3],
    alpha: f32,
) {
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let cx = (x0 + x1) * 0.5;
    let cy = (y0 + y1) * 0.5;
    let hw = (x1 - x0) * 0.5;
    let hh = (y1 - y0) * 0.5;
    let r = radius.min(hw).min(hh).max(0.0);
    let x_s = x0.floor() as i32 - 1;
    let x_e = x1.ceil() as i32 + 1;
    let y_s = y0.floor() as i32 - 1;
    let y_e = y1.ceil() as i32 + 1;
    for y in y_s..y_e {
        for x in x_s..x_e {
            let sd = sd_round_box(x as f32 + 0.5, y as f32 + 0.5, cx, cy, hw, hh, r);
            let cov = (0.5 - sd).clamp(0.0, 1.0);
            if cov > 0.0 {
                blend(buf, w, h, x, y, color, alpha * cov);
            }
        }
    }
}

fn ring(
    buf: &mut [u8],
    w: u32,
    h: u32,
    cx: f32,
    cy: f32,
    radius: f32,
    thickness: f32,
    color: [u8; 3],
    alpha: f32,
) {
    let r_out = radius + thickness * 0.5;
    let x_s = (cx - r_out).floor() as i32;
    let x_e = (cx + r_out).ceil() as i32;
    let y_s = (cy - r_out).floor() as i32;
    let y_e = (cy + r_out).ceil() as i32;
    for y in y_s..y_e {
        for x in x_s..x_e {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let sd = (d - radius).abs() - thickness * 0.5;
            let cov = (0.5 - sd).clamp(0.0, 1.0);
            if cov > 0.0 {
                blend(buf, w, h, x, y, color, alpha * cov);
            }
        }
    }
}

fn fill_poly(buf: &mut [u8], w: u32, h: u32, pts: &[(f32, f32)], color: [u8; 3], alpha: f32) {
    if pts.len() < 3 {
        return;
    }
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;
    let mut min_x = f32::MAX;
    let mut max_x = f32::MIN;
    for (x, y) in pts {
        min_x = min_x.min(*x);
        max_x = max_x.max(*x);
        min_y = min_y.min(*y);
        max_y = max_y.max(*y);
    }
    for y in (min_y.floor() as i32)..=(max_y.ceil() as i32) {
        let yc = y as f32 + 0.5;
        let mut xs: Vec<f32> = Vec::with_capacity(8);
        for i in 0..pts.len() {
            let a = pts[i];
            let b = pts[(i + 1) % pts.len()];
            if (a.1 <= yc && b.1 > yc) || (b.1 <= yc && a.1 > yc) {
                let t = (yc - a.1) / (b.1 - a.1);
                xs.push(a.0 + t * (b.0 - a.0));
            }
        }
        xs.sort_by(|p, q| p.partial_cmp(q).unwrap_or(std::cmp::Ordering::Equal));
        let mut i = 0;
        while i + 1 < xs.len() {
            let x_from = xs[i] as i32;
            let x_to = xs[i + 1] as i32;
            for x in x_from..=x_to {
                blend(buf, w, h, x, y, color, alpha);
            }
            i += 2;
        }
    }
}

fn draw_text(
    buf: &mut [u8],
    w: u32,
    h: u32,
    x: f32,
    y: f32,
    size: f32,
    text: &str,
    color: [u8; 3],
    alpha: f32,
) -> f32 {
    let Some(f) = font() else {
        return 0.0;
    };
    let scaled = f.as_scaled(PxScale::from(size));
    let baseline = y + scaled.ascent();
    let mut pen = x;
    for ch in text.chars() {
        let id = scaled.glyph_id(ch);
        let glyph = id.with_scale_and_position(PxScale::from(size), point(pen, baseline));
        if let Some(outlined) = f.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, cov| {
                let px = bounds.min.x as i32 + gx as i32;
                let py = bounds.min.y as i32 + gy as i32;
                blend(buf, w, h, px, py, color, alpha * cov);
            });
        }
        pen += scaled.h_advance(id);
    }
    pen - x
}

fn measure_text(size: f32, text: &str) -> f32 {
    let Some(f) = font() else {
        return (text.chars().count() as f32) * size * 0.55;
    };
    let scaled = f.as_scaled(PxScale::from(size));
    text.chars()
        .map(|c| scaled.h_advance(scaled.glyph_id(c)))
        .sum()
}

fn draw_cursor(buf: &mut [u8], w: u32, h: u32, x: f32, y: f32) {
    let k = 1.25f32;
    let shape: [(f32, f32); 7] = [
        (0.0, 0.0),
        (0.0, 16.5),
        (4.3, 12.6),
        (6.9, 18.6),
        (9.4, 17.4),
        (6.9, 11.6),
        (11.8, 11.1),
    ];
    // 先画一圈深色描边（等比放大），再叠白色主体，浅色背景上也看得清
    let outline: Vec<(f32, f32)> = shape
        .iter()
        .map(|(dx, dy)| {
            let cx = 5.9;
            let cy = 9.3;
            (x + (dx - cx) * 1.16 * k + cx * k, y + (dy - cy) * 1.16 * k + cy * k)
        })
        .collect();
    fill_poly(buf, w, h, &outline, [16, 18, 24], 0.95);
    let body: Vec<(f32, f32)> = shape.iter().map(|(dx, dy)| (x + dx * k, y + dy * k)).collect();
    fill_poly(buf, w, h, &body, [255, 255, 255], 1.0);
}

/// 录屏水印：右下角半透明小胶囊里的「分:秒」计时，
/// 与录制时长同步，方便事后看清 GIF 从哪一秒到哪一秒。
pub fn draw_clock(buf: &mut [u8], w: u32, h: u32, ms: u32) {
    let secs = ms / 1000;
    let text = format!("{:02}:{:02}", secs / 60, secs % 60);
    let size = (h as f32 * 0.042).clamp(13.0, 30.0);
    let tw = measure_text(size, &text);
    let pad_x = (size * 0.42).round();
    let pad_y = (size * 0.26).round();
    let box_w = tw + pad_x * 2.0;
    let box_h = size + pad_y * 2.0 + 1.0;
    let m = (size * 0.55).max(7.0);
    let x1 = w as f32 - m;
    let y1 = h as f32 - m;
    let x0 = x1 - box_w;
    let y0 = y1 - box_h;
    round_rect(buf, w, h, x0, y0, x1, y1, (box_h * 0.3).min(9.0), [10, 12, 18], 0.86);
    draw_text(buf, w, h, x0 + pad_x, y0 + pad_y, size, &text, [255, 255, 255], 1.0);
}

/// 把输入事件与光标合成到 RGB 帧上。
pub fn compose(
    buf: &mut [u8],
    w: u32,
    h: u32,
    region: Rect,
    scale: f32,
    now: u32,
    events: &[InputEvent],
    opts: ComposeOpts,
) {
    let to_local = |sx: i32, sy: i32| -> (f32, f32) {
        ((sx - region.x) as f32 * scale, (sy - region.y) as f32 * scale)
    };

    if opts.ripple {
        for e in events {
            if let Ev::Down(_, sx, sy) = e.ev {
                let age = now.saturating_sub(e.t);
                if age <= 340 {
                    let p = age as f32 / 340.0;
                    let (lx, ly) = to_local(sx, sy);
                    let radius = 5.0 + 30.0 * p;
                    ring(buf, w, h, lx, ly, radius, 3.0, [255, 92, 92], 0.85 * (1.0 - p));
                    ring(buf, w, h, lx, ly, 4.0, 5.0, [255, 255, 255], 0.9 * (1.0 - p));
                }
            }
        }
    }

    if opts.cursor {
        let (cx, cy) = win32::cursor_pos();
        let (lx, ly) = to_local(cx, cy);
        draw_cursor(buf, w, h, lx, ly);
    }

    if opts.key_hud {
        draw_key_hud(buf, w, h, now, events);
    }
}

fn draw_key_hud(buf: &mut [u8], w: u32, h: u32, now: u32, events: &[InputEvent]) {
    let mut recent: Vec<(u32, String)> = Vec::new();
    for e in events {
        if let Ev::KeyDown(ref label) = e.ev {
            let age = now.saturating_sub(e.t);
            if age <= 1500 {
                recent.push((e.t, label.clone()));
            }
        }
    }
    if recent.len() > 6 {
        recent.drain(0..recent.len() - 6);
    }
    if recent.is_empty() {
        return;
    }

    let size = 15.0f32;
    let pad_x = 9.0f32;
    let cap_h = 28.0f32;
    let gap = 6.0f32;
    let widths: Vec<f32> = recent
        .iter()
        .map(|(_, s)| measure_text(size, s) + pad_x * 2.0)
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + gap * (widths.len() as f32 - 1.0);
    let bar_h = cap_h + 12.0;
    let x0 = ((w as f32 - total) * 0.5 - 10.0).max(4.0);
    let y0 = h as f32 - bar_h - 14.0;

    round_rect(
        buf,
        w,
        h,
        x0,
        y0,
        (x0 + total + 20.0).min(w as f32 - 4.0),
        y0 + bar_h,
        10.0,
        [18, 20, 26],
        0.62,
    );

    let mut x = x0 + 10.0;
    for (i, (t, label)) in recent.iter().enumerate() {
        let age = now.saturating_sub(*t);
        let alpha = if age <= 1100 {
            1.0
        } else {
            (1.0 - (age - 1100) as f32 / 400.0).clamp(0.0, 1.0)
        };
        let cw = widths[i];
        let cy = y0 + 6.0;
        round_rect(
            buf,
            w,
            h,
            x,
            cy,
            x + cw,
            cy + cap_h,
            7.0,
            [248, 249, 252],
            0.92 * alpha,
        );
        let text_y = cy + (cap_h - size) * 0.5 - 2.0;
        draw_text(
            buf,
            w,
            h,
            x + pad_x,
            text_y,
            size,
            label,
            [24, 26, 32],
            0.95 * alpha,
        );
        x += cw + gap;
    }
}
