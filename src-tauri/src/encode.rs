//! GIF 流式编码：自研中位切分调色板 + 帧差合并 + 1 帧前瞻定延迟。
//! 内存恒定，不随录制时长增长。

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

pub struct GifWriter {
    w: u16,
    h: u16,
    max_colors: usize,
    file: Option<BufWriter<File>>,
    enc: Option<gif::Encoder<BufWriter<File>>>,
    palette: Vec<[u8; 3]>,
    lut: Vec<u8>,
    warm: Vec<(Vec<u8>, u32)>,
    pending: Option<(Vec<u8>, u32)>,
    /// 上一帧的量化索引；下一帧只写和它不同的像素，其余留透明
    prev: Vec<u8>,
    /// 透明色索引，调色板只用到 0..=254，255 专门留作"这像素没变"
    trans: u8,
    frames: u32,
}

const WARMUP_FRAMES: usize = 6;

impl GifWriter {
    pub fn new(path: &Path, w: u32, h: u32, max_colors: usize) -> Result<GifWriter, String> {
        let file = File::create(path).map_err(|e| format!("创建 GIF 失败: {}", e))?;
        Ok(GifWriter {
            w: w as u16,
            h: h as u16,
            max_colors: max_colors.clamp(8, 255),
            file: Some(BufWriter::with_capacity(1 << 16, file)),
            enc: None,
            palette: Vec::new(),
            lut: vec![255u8; 32768],
            warm: Vec::with_capacity(WARMUP_FRAMES),
            pending: None,
            prev: Vec::new(),
            trans: 255,
            frames: 0,
        })
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    pub fn push(&mut self, rgb: &[u8], t_ms: u32) -> Result<(), String> {
        if self.enc.is_none() {
            self.warm.push((rgb.to_vec(), t_ms));
            if self.warm.len() >= WARMUP_FRAMES {
                self.init_encoder()?;
            }
            return Ok(());
        }
        match self.pending.take() {
            None => {
                self.pending = Some((rgb.to_vec(), t_ms));
            }
            Some((prev, prev_t)) => {
                if prev.as_slice() == rgb {
                    // 静态画面：保持 pending，等下一帧到来时把时长合并进去
                    self.pending = Some((prev, prev_t));
                } else {
                    let delay = delay_cs(t_ms.saturating_sub(prev_t));
                    if self.write_frame(&prev, delay)? {
                        self.pending = Some((rgb.to_vec(), t_ms));
                    } else {
                        // 这一帧跟上上帧没差别，没写出去：时间起点保持不变，让延迟继续累加
                        self.pending = Some((rgb.to_vec(), prev_t));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn finish(&mut self, t_ms: u32) -> Result<(), String> {
        if self.enc.is_none() {
            if self.warm.is_empty() {
                return Err("没有采集到任何帧".into());
            }
            self.init_encoder()?;
        }
        if let Some((rgb, prev_t)) = self.pending.take() {
            let delay = delay_cs(t_ms.saturating_sub(prev_t));
            let _ = self.write_frame(&rgb, delay)?;
        }
        self.enc.take(); // drop -> flush
        Ok(())
    }

    fn init_encoder(&mut self) -> Result<(), String> {
        let palette = build_palette(&self.warm, self.max_colors);
        // 显式补到 256 项：第 255 项是 transparent 占位，缺了它索引会错位
        let mut pal_bytes: Vec<u8> = Vec::with_capacity(256 * 3);
        for c in &palette {
            pal_bytes.extend_from_slice(c);
        }
        while pal_bytes.len() < 256 * 3 {
            pal_bytes.push(0);
        }
        let writer = self.file.take().ok_or("GIF 文件句柄缺失")?;
        let mut enc =
            gif::Encoder::new(writer, self.w, self.h, &pal_bytes).map_err(|e| e.to_string())?;
        let _ = enc.set_repeat(gif::Repeat::Infinite);

        self.palette = palette;
        self.lut = vec![255u8; 32768];
        self.prev.clear();
        self.enc = Some(enc);

        let warm = std::mem::take(&mut self.warm);
        if warm.len() == 1 {
            self.pending = Some((warm[0].0.clone(), warm[0].1));
            return Ok(());
        }
        for i in 0..warm.len() - 1 {
            let delay = delay_cs(warm[i + 1].1.saturating_sub(warm[i].1));
            let _ = self.write_frame(&warm[i].0, delay)?;
        }
        if let Some((rgb, t)) = warm.last() {
            self.pending = Some((rgb.clone(), *t));
        }
        Ok(())
    }

    fn write_frame(&mut self, rgb: &[u8], delay: u16) -> Result<bool, String> {
        let w = self.w as usize;
        let h = self.h as usize;
        let n = w * h;
        let indices = self.quantize(rgb);
        let first = self.prev.len() != n;

        // 帧间差分：只算一遍包围盒，后面只在这块里取像素
        let (x0, y0, x1, y1) = if first {
            (0usize, 0usize, w, h)
        } else {
            let mut bx0 = w;
            let mut by0 = h;
            let mut bx1 = 0usize;
            let mut by1 = 0usize;
            let mut changed = 0usize;
            for y in 0..h {
                let row = y * w;
                let mut row_hit = false;
                for x in 0..w {
                    let i = row + x;
                    if indices[i] != self.prev[i] {
                        changed += 1;
                        row_hit = true;
                        if x < bx0 {
                            bx0 = x;
                        }
                        if x >= bx1 {
                            bx1 = x + 1;
                        }
                    }
                }
                if row_hit {
                    if y < by0 {
                        by0 = y;
                    }
                    if y >= by1 {
                        by1 = y + 1;
                    }
                }
            }
            if changed == 0 {
                self.prev = indices;
                return Ok(false);
            }
            (bx0, by0, bx1, by1)
        };

        let fw = x1 - x0;
        let fh = y1 - y0;
        // 没变的像素一律写透明索引，LZW 对成片相同字节压得极短
        let mut buf = vec![self.trans; fw * fh];
        for y in y0..y1 {
            let src_row = y * w;
            let dst_row = (y - y0) * fw;
            for x in x0..x1 {
                let i = src_row + x;
                if first || indices[i] != self.prev[i] {
                    buf[dst_row + (x - x0)] = indices[i];
                }
            }
        }

        let mut frame = gif::Frame::default();
        frame.left = x0 as u16;
        frame.top = y0 as u16;
        frame.width = fw as u16;
        frame.height = fh as u16;
        frame.buffer = std::borrow::Cow::Owned(buf);
        frame.delay = delay;
        // Keep：差分帧叠在上一帧上，透明像素自然露出底下的内容
        frame.dispose = gif::DisposalMethod::Keep;
        if !first {
            frame.transparent = Some(self.trans);
        }
        let enc = self.enc.as_mut().ok_or("编码器未初始化")?;
        enc.write_frame(&frame).map_err(|e| e.to_string())?;
        self.prev = indices;
        self.frames += 1;
        Ok(true)
    }

    fn quantize(&mut self, rgb: &[u8]) -> Vec<u8> {
        let n = (self.w as usize) * (self.h as usize);
        let mut out = vec![0u8; n];
        let palette = &self.palette;
        for i in 0..n {
            let p = i * 3;
            let r = rgb[p] as u32;
            let g = rgb[p + 1] as u32;
            let b = rgb[p + 2] as u32;
            let key = (((r >> 3) << 10) | ((g >> 3) << 5) | (b >> 3)) as usize;
            let mut idx = self.lut[key];
            if idx == 255 {
                idx = nearest(palette, r, g, b);
                self.lut[key] = idx;
            }
            out[i] = idx;
        }
        out
    }
}

fn delay_cs(ms: u32) -> u16 {
    // 1/100 秒为单位，最小 2（20ms），规避旧解码器把过小延迟当 100ms 的老坑
    ((ms / 10).clamp(2, 600)) as u16
}

fn nearest(palette: &[[u8; 3]], r: u32, g: u32, b: u32) -> u8 {
    let mut best = 0usize;
    let mut best_d = u32::MAX;
    for (i, c) in palette.iter().enumerate() {
        let dr = r as i32 - c[0] as i32;
        let dg = g as i32 - c[1] as i32;
        let db = b as i32 - c[2] as i32;
        let d = (dr * dr + dg * dg + db * db) as u32;
        if d < best_d {
            best_d = d;
            best = i;
            if d == 0 {
                break;
            }
        }
    }
    best as u8
}

/// 中位切分：先在 5-5-5 直方图上聚合，再按方差最大的通道二分。
fn build_palette(frames: &[(Vec<u8>, u32)], max_colors: usize) -> Vec<[u8; 3]> {
    let mut counts = vec![0u32; 32768];
    let mut sum_r = vec![0u32; 32768];
    let mut sum_g = vec![0u32; 32768];
    let mut sum_b = vec![0u32; 32768];

    for (rgb, _) in frames {
        let n = rgb.len() / 3;
        let step = if n > 200_000 { 3 } else { 1 };
        let mut i = 0usize;
        while i < n {
            let p = i * 3;
            let r = rgb[p] as u32;
            let g = rgb[p + 1] as u32;
            let b = rgb[p + 2] as u32;
            let key = (((r >> 3) << 10) | ((g >> 3) << 5) | (b >> 3)) as usize;
            counts[key] += 1;
            sum_r[key] += r;
            sum_g[key] += g;
            sum_b[key] += b;
            i += step;
        }
    }

    let used: Vec<usize> = (0..32768).filter(|&k| counts[k] > 0).collect();
    if used.is_empty() {
        return vec![[0, 0, 0]];
    }
    if used.len() <= max_colors {
        return used
            .iter()
            .map(|&k| {
                let c = counts[k];
                [
                    (sum_r[k] / c) as u8,
                    (sum_g[k] / c) as u8,
                    (sum_b[k] / c) as u8,
                ]
            })
            .collect();
    }

    let mut boxes: Vec<Vec<usize>> = vec![used];

    while boxes.len() < max_colors {
        // 选一个可继续分裂、且跨度 * 像素数最大的盒子
        let mut target: Option<usize> = None;
        let mut best_score = -1f64;
        for (bi, bx) in boxes.iter().enumerate() {
            if bx.len() < 2 {
                continue;
            }
            let mut min = [255u32; 3];
            let mut max = [0u32; 3];
            let mut total = 0u32;
            for &k in bx {
                let c = counts[k];
                let avg = [sum_r[k] / c, sum_g[k] / c, sum_b[k] / c];
                total += c;
                for ch in 0..3 {
                    min[ch] = min[ch].min(avg[ch]);
                    max[ch] = max[ch].max(avg[ch]);
                }
            }
            let span = (max[0] - min[0]).max(max[1] - min[1]).max(max[2] - min[2]);
            let score = span as f64 * (total as f64).ln().max(1.0);
            if score > best_score {
                best_score = score;
                target = Some(bi);
            }
        }
        let Some(bi) = target else { break };
        let bx = boxes.swap_remove(bi);
        let mut min = [255u32; 3];
        let mut max = [0u32; 3];
        for &k in &bx {
            let c = counts[k];
            let avg = [sum_r[k] / c, sum_g[k] / c, sum_b[k] / c];
            for ch in 0..3 {
                min[ch] = min[ch].min(avg[ch]);
                max[ch] = max[ch].max(avg[ch]);
            }
        }
        let channel = (0..3)
            .max_by_key(|&ch| max[ch] - min[ch])
            .unwrap_or(0);
        let mut sorted = bx;
        sorted.sort_by_key(|&k| {
            let c = counts[k].max(1);
            match channel {
                0 => sum_r[k] / c,
                1 => sum_g[k] / c,
                _ => sum_b[k] / c,
            }
        });
        let total: u32 = sorted.iter().map(|&k| counts[k]).sum();
        let mut acc = 0u32;
        let mut split_at = sorted.len() / 2;
        for (i, &k) in sorted.iter().enumerate() {
            acc += counts[k];
            if acc * 2 >= total {
                split_at = (i + 1).min(sorted.len() - 1);
                break;
            }
        }
        let (left, right) = sorted.split_at(split_at);
        boxes.push(left.to_vec());
        boxes.push(right.to_vec());
    }

    let mut palette: Vec<[u8; 3]> = Vec::with_capacity(boxes.len());
    for bx in boxes {
        let mut r = 0u64;
        let mut g = 0u64;
        let mut b = 0u64;
        let mut total = 0u64;
        for &k in &bx {
            let c = counts[k] as u64;
            r += sum_r[k] as u64;
            g += sum_g[k] as u64;
            b += sum_b[k] as u64;
            total += c;
        }
        if total > 0 {
            palette.push([
                (r / total).min(255) as u8,
                (g / total).min(255) as u8,
                (b / total).min(255) as u8,
            ]);
        }
    }
    if palette.is_empty() {
        palette.push([0, 0, 0]);
    }
    palette
}

/// 面积法降采样（RGB8）。
pub fn resize_rgb(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    if sw == dw && sh == dh {
        return src.to_vec();
    }
    let mut out = vec![0u8; (dw as usize) * (dh as usize) * 3];
    for dy in 0..dh {
        let y0 = (dy as u64 * sh as u64 / dh as u64) as u32;
        let y1 = (((dy as u64 + 1) * sh as u64 / dh as u64) as u32).max(y0 + 1).min(sh);
        for dx in 0..dw {
            let x0 = (dx as u64 * sw as u64 / dw as u64) as u32;
            let x1 = (((dx as u64 + 1) * sw as u64 / dw as u64) as u32).max(x0 + 1).min(sw);
            let mut r = 0u32;
            let mut g = 0u32;
            let mut b = 0u32;
            let mut n = 0u32;
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = ((y * sw + x) * 3) as usize;
                    r += src[p] as u32;
                    g += src[p + 1] as u32;
                    b += src[p + 2] as u32;
                    n += 1;
                }
            }
            let o = ((dy * dw + dx) * 3) as usize;
            out[o] = (r / n.max(1)) as u8;
            out[o + 1] = (g / n.max(1)) as u8;
            out[o + 2] = (b / n.max(1)) as u8;
        }
    }
    out
}

/// BGRA(top-down) -> RGB
pub fn bgra_to_rgb(src: &[u8], out: &mut Vec<u8>) {
    let n = src.len() / 4;
    out.resize(n * 3, 0);
    for i in 0..n {
        let s = i * 4;
        let d = i * 3;
        out[d] = src[s + 2];
        out[d + 1] = src[s + 1];
        out[d + 2] = src[s];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(w: u32, h: u32, shift: u32) -> Vec<u8> {
        let mut rgb = vec![0u8; (w * h * 3) as usize];
        for y in 0..h {
            for x in 0..w {
                let p = ((y * w + x) * 3) as usize;
                if x >= shift && x < shift + 12 && y >= 10 && y < 26 {
                    rgb[p] = 255;
                    rgb[p + 1] = 40;
                    rgb[p + 2] = 40;
                } else {
                    rgb[p] = (x % 200) as u8;
                    rgb[p + 1] = (y % 200) as u8;
                    rgb[p + 2] = 180;
                }
            }
        }
        rgb
    }

    #[test]
    fn gif_encoder_roundtrip() {
        let dir = std::env::temp_dir().join("look_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("enc_test.gif");
        let (w, h) = (64u32, 48u32);
        let mut gw = GifWriter::new(&path, w, h, 32).unwrap();
        for i in 0..12u32 {
            gw.push(&frame(w, h, i * 4), i * 66).unwrap();
        }
        gw.finish(12 * 66).unwrap();
        let len = path.metadata().unwrap().len();
        assert!(len > 800, "gif too small: {} bytes", len);

        let data = std::fs::read(&path).unwrap();
        let mut opts = gif::DecodeOptions::new();
        opts.set_color_output(gif::ColorOutput::RGBA);
        let mut dec = opts.read_info(std::io::Cursor::new(&data)).unwrap();
        assert_eq!(dec.width(), w as u16);
        assert_eq!(dec.height(), h as u16);
        let mut frames = 0u32;
        while let Some(_f) = dec.read_next_frame().unwrap() {
            frames += 1;
        }
        assert!(frames >= 3, "decoded frames={}", frames);
        println!("encoded {} frames, {} bytes", gw.frames(), len);
    }

    #[test]
    fn palette_handles_few_colors() {
        let mut frames = Vec::new();
        for i in 0..3u32 {
            frames.push((frame(32, 24, i * 4), i * 66));
        }
        let pal = build_palette(&frames, 4);
        assert!(pal.len() <= 4 && !pal.is_empty());
    }
}

