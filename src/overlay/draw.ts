import type { Obj, Rect } from "../lib/types";

export function drawArrow(
  ctx: CanvasRenderingContext2D,
  x1: number,
  y1: number,
  x2: number,
  y2: number,
  color: string,
  width: number,
) {
  const dx = x2 - x1;
  const dy = y2 - y1;
  const len = Math.hypot(dx, dy) || 1;
  const ux = dx / len;
  const uy = dy / len;
  const head = Math.max(12, width * 3.2);

  ctx.strokeStyle = color;
  ctx.lineWidth = width;
  ctx.lineCap = "round";
  ctx.beginPath();
  ctx.moveTo(x1, y1);
  ctx.lineTo(x2 - ux * head * 0.75, y2 - uy * head * 0.75);
  ctx.stroke();

  const px = -uy;
  const py = ux;
  ctx.beginPath();
  ctx.moveTo(x2, y2);
  ctx.lineTo(x2 - ux * head + px * head * 0.5, y2 - uy * head + py * head * 0.5);
  ctx.lineTo(x2 - ux * head - px * head * 0.5, y2 - uy * head - py * head * 0.5);
  ctx.closePath();
  ctx.fillStyle = color;
  ctx.fill();
}

export function roundRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
) {
  const rr = Math.min(r, Math.abs(w) / 2, Math.abs(h) / 2);
  ctx.beginPath();
  ctx.moveTo(x + rr, y);
  ctx.lineTo(x + w - rr, y);
  ctx.arcTo(x + w, y, x + w, y + rr, rr);
  ctx.lineTo(x + w, y + h - rr);
  ctx.arcTo(x + w, y + h, x + w - rr, y + h, rr);
  ctx.lineTo(x + rr, y + h);
  ctx.arcTo(x, y + h, x, y + h - rr, rr);
  ctx.lineTo(x, y + rr);
  ctx.arcTo(x, y, x + rr, y, rr);
  ctx.closePath();
}

/** 在 ctx 当前坐标系统内绘制一个标注对象；mosaic 需要底图。 */
export function drawObject(
  ctx: CanvasRenderingContext2D,
  o: Obj,
  img?: CanvasImageSource | null,
) {
  switch (o.type) {
    case "rect": {
      if (o.fill) {
        ctx.fillStyle = o.fill;
        ctx.fillRect(o.x, o.y, o.w, o.h);
      }
      ctx.strokeStyle = o.color;
      ctx.lineWidth = o.width;
      ctx.strokeRect(o.x, o.y, o.w, o.h);
      break;
    }
    case "ellipse": {
      ctx.strokeStyle = o.color;
      ctx.lineWidth = o.width;
      ctx.beginPath();
      ctx.ellipse(o.x + o.w / 2, o.y + o.h / 2, Math.abs(o.w / 2), Math.abs(o.h / 2), 0, 0, Math.PI * 2);
      if (o.fill) {
        ctx.fillStyle = o.fill;
        ctx.fill();
      }
      ctx.stroke();
      break;
    }
    case "arrow":
      drawArrow(ctx, o.x1, o.y1, o.x2, o.y2, o.color, o.width);
      break;
    case "pen":
    case "marker": {
      ctx.strokeStyle = o.color;
      ctx.lineWidth = o.type === "marker" ? o.width * 2.4 : o.width;
      ctx.lineJoin = "round";
      ctx.lineCap = "round";
      ctx.globalAlpha = o.type === "marker" ? 0.45 : 1;
      ctx.beginPath();
      o.pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      ctx.stroke();
      ctx.globalAlpha = 1;
      break;
    }
    case "text": {
      ctx.fillStyle = o.color;
      ctx.font = `600 ${o.size}px "Microsoft YaHei", "Segoe UI", sans-serif`;
      ctx.textBaseline = "top";
      const lines = o.text.split("\n");
      lines.forEach((line, i) => ctx.fillText(line, o.x, o.y + i * (o.size * 1.3)));
      break;
    }
    case "step": {
      const r = o.r;
      ctx.beginPath();
      ctx.arc(o.x, o.y, r, 0, Math.PI * 2);
      ctx.fillStyle = o.color;
      ctx.fill();
      ctx.beginPath();
      ctx.arc(o.x, o.y, r, 0, Math.PI * 2);
      ctx.strokeStyle = "#ffffff";
      ctx.lineWidth = 2;
      ctx.stroke();
      ctx.fillStyle = "#ffffff";
      ctx.font = `700 ${Math.round(r * 1.05)}px "Segoe UI", sans-serif`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(String(o.n), o.x, o.y + 1);
      ctx.textAlign = "left";
      ctx.textBaseline = "top";
      break;
    }
    case "mosaic": {
      const w = Math.abs(o.w);
      const h = Math.abs(o.h);
      const sx = Math.min(o.x, o.x + o.w);
      const sy = Math.min(o.y, o.y + o.h);
      if (img && w > 1 && h > 1) {
        const blk = Math.max(3, o.block);
        const tw = Math.max(1, Math.round(w / blk));
        const th = Math.max(1, Math.round(h / blk));
        const tmp = document.createElement("canvas");
        tmp.width = tw;
        tmp.height = th;
        const tctx = tmp.getContext("2d")!;
        tctx.drawImage(img, sx, sy, w, h, 0, 0, tw, th);
        ctx.imageSmoothingEnabled = false;
        ctx.drawImage(tmp, 0, 0, tw, th, sx, sy, w, h);
        ctx.imageSmoothingEnabled = true;
      } else {
        ctx.fillStyle = "rgba(20,22,28,0.9)";
        ctx.fillRect(sx, sy, w, h);
      }
      break;
    }
  }
}

/** 把底图裁剪 + 标注合成为离屏画布。坐标按裁剪框平移。 */
export function renderCrop(
  img: CanvasImageSource,
  objects: Obj[],
  crop: Rect,
): HTMLCanvasElement {
  const cw = Math.max(1, Math.round(crop.w));
  const ch = Math.max(1, Math.round(crop.h));
  const c = document.createElement("canvas");
  c.width = cw;
  c.height = ch;
  const ctx = c.getContext("2d")!;
  ctx.drawImage(img, crop.x, crop.y, crop.w, crop.h, 0, 0, cw, ch);
  ctx.save();
  ctx.translate(-crop.x, -crop.y);
  for (const o of objects) drawObject(ctx, o, img);
  ctx.restore();
  return c;
}

export function toBase64(c: HTMLCanvasElement): string {
  const url = c.toDataURL("image/png");
  return url.slice(url.indexOf(",") + 1);
}
