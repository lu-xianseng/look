import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ipc } from "../lib/ipc";
import type { MonitorShot, Obj, Rect, WinPeek } from "../lib/types";
import { drawObject, renderCrop, toBase64 } from "./draw";

type Tool = "rect" | "ellipse" | "arrow" | "pen" | "marker" | "text" | "step" | "mosaic";
type Phase = "select" | "annotate";

const COLORS = ["#ff3b30", "#ffcc00", "#34c759", "#2b6cff", "#ffffff", "#1c1c1e"];
// 长截图体验还没打磨好，先屏蔽入口（按钮与快捷键 L）。改成 true 即恢复
const SCROLL_ON = false;
const TOOLS: Tool[] = ["rect", "ellipse", "arrow", "pen", "marker", "text", "step", "mosaic"];
const TOOL_LABEL: Record<Tool, string> = {
  rect: "矩形",
  ellipse: "椭圆",
  arrow: "箭头",
  pen: "画笔",
  marker: "荧光",
  text: "文字",
  step: "序号",
  mosaic: "马赛克",
};

// 工具按钮改用图标：8 个中文标签并排要 450px，图标只要 240px，工具条能短一大截
const TOOL_ICON: Record<Tool, React.ReactNode> = {
  rect: (
    <svg viewBox="0 0 16 16" className="tb-ic">
      <rect x="2.5" y="3.5" width="11" height="9" rx="1.2" fill="none" stroke="currentColor" strokeWidth="1.3" />
    </svg>
  ),
  ellipse: (
    <svg viewBox="0 0 16 16" className="tb-ic">
      <ellipse cx="8" cy="8" rx="5.5" ry="4.4" fill="none" stroke="currentColor" strokeWidth="1.3" />
    </svg>
  ),
  arrow: (
    <svg viewBox="0 0 16 16" className="tb-ic">
      <path d="M3.2 12.8 11.6 4.4" fill="none" stroke="currentColor" strokeWidth="1.3" />
      <path d="M13.4 2.6l-1 4.8-3.8-3.8z" fill="currentColor" />
    </svg>
  ),
  pen: (
    <svg viewBox="0 0 16 16" className="tb-ic">
      <path d="M3 13l.9-3 7.4-7.4 2.6 2.6-7.4 7.4-3 .9z" fill="none" stroke="currentColor" strokeWidth="1.3" />
    </svg>
  ),
  marker: (
    <svg viewBox="0 0 16 16" className="tb-ic">
      <rect x="2" y="5.4" width="8.4" height="4.6" rx="1" transform="rotate(-35 6.2 7.7)" fill="none" stroke="currentColor" strokeWidth="1.3" />
      <path d="M9.4 12.6h4.2" fill="none" stroke="currentColor" strokeWidth="1.3" />
    </svg>
  ),
  text: (
    <svg viewBox="0 0 16 16" className="tb-ic">
      <path d="M4 3.4h8M8 3.4v9.2M6 12.6h4" fill="none" stroke="currentColor" strokeWidth="1.3" />
    </svg>
  ),
  step: (
    <svg viewBox="0 0 16 16" className="tb-ic">
      <circle cx="8" cy="8" r="5.2" fill="none" stroke="currentColor" strokeWidth="1.3" />
      <path d="M7 5.4l1.5-1v6.4M6.3 10.8h4.4" fill="none" stroke="currentColor" strokeWidth="1.2" />
    </svg>
  ),
  mosaic: (
    <svg viewBox="0 0 16 16" className="tb-ic">
      <rect x="2.4" y="2.4" width="5" height="5" rx="0.8" fill="currentColor" opacity="0.75" />
      <rect x="8.6" y="2.4" width="5" height="5" rx="0.8" fill="none" stroke="currentColor" strokeWidth="1.2" />
      <rect x="2.4" y="8.6" width="5" height="5" rx="0.8" fill="none" stroke="currentColor" strokeWidth="1.2" />
      <rect x="8.6" y="8.6" width="5" height="5" rx="0.8" fill="currentColor" opacity="0.75" />
    </svg>
  ),
};

const norm = (x1: number, y1: number, x2: number, y2: number): Rect => ({
  x: Math.min(x1, x2),
  y: Math.min(y1, y2),
  w: Math.abs(x2 - x1),
  h: Math.abs(y2 - y1),
});

/// 自绘下拉：原生 <select> 在 Windows 上弹出的是系统白底列表，和深色工具条完全不搭，
/// 所以改成按钮 + 深色浮层菜单，样式与工具条同一套。
function ChipSelect({
  icon,
  value,
  options,
  unit,
  onChange,
}: {
  icon: React.ReactNode;
  value: string | number;
  options: { v: string | number; label?: string }[];
  unit?: string;
  onChange: (v: any) => void;
}) {
  const [open, setOpen] = useState(false);
  const [up, setUp] = useState(false);
  const ref = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!open) return;
    const close = (e: PointerEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", close, true);
    return () => document.removeEventListener("pointerdown", close, true);
  }, [open]);

  const toggle = () => {
    const el = ref.current;
    if (el) {
      const r = el.getBoundingClientRect();
      setUp(r.bottom + 120 > window.innerHeight);
    }
    setOpen((o) => !o);
  };

  const cur = options.find((o) => o.v === value);
  return (
    <div className="tb-select" ref={ref}>
      <button className={"tb-chip " + (open ? "open" : "")} onClick={toggle} onPointerDown={(e) => e.stopPropagation()}>
        {icon}
        <span>{cur?.label ?? value}</span>
        {unit ? <span className="tb-unit">{unit}</span> : null}
        <svg viewBox="0 0 16 16" className="tb-chev" aria-hidden>
          <path d="M5 6.4l3 3 3-3" fill="none" stroke="currentColor" strokeWidth="1.3" />
        </svg>
      </button>
      {open && (
        <div className={"tb-menu " + (up ? "up" : "")} onPointerDown={(e) => e.stopPropagation()}>
          {options.map((o) => (
            <button
              key={String(o.v)}
              className={"tb-menu-item " + (o.v === value ? "on" : "")}
              onClick={() => {
                onChange(o.v);
                setOpen(false);
              }}
            >
              {o.label ?? o.v}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export default function CaptureOverlay({ label }: { label: string }) {
  const index = useMemo(() => Number(label.split("_").pop() || 0), [label]);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const dragRef = useRef<{ x: number; y: number; on: boolean } | null>(null);
  const hoverTs = useRef(0);

  const [shot, setShot] = useState<MonitorShot | null>(null);
  const [shots, setShots] = useState<MonitorShot[]>([]);
  const [cur0, setCur0] = useState<{ x: number; y: number } | null>(null);
  const [img, setImg] = useState<CanvasImageSource | null>(null);
  const [k, setK] = useState(1);
  const [mode, setMode] = useState<"shot" | "record">("shot");
  const [phase, setPhase] = useState<Phase>("select");
  const [sel, setSel] = useState<Rect | null>(null);
  const [hover, setHover] = useState<Rect | null>(null);
  const [hoverTitle, setHoverTitle] = useState("");
  const [ptr, setPtr] = useState<{ x: number; y: number } | null>(null);
  const [objects, setObjects] = useState<Obj[]>([]);
  const [redoStack, setRedoStack] = useState<Obj[]>([]);
  const [draft, setDraft] = useState<Obj | null>(null);
  const [tool, setTool] = useState<Tool>("rect");
  const [color, setColor] = useState(COLORS[0]);
  const [lw, setLw] = useState(3);
  const [textAt, setTextAt] = useState<{ x: number; y: number } | null>(null);
  const [textValue, setTextValue] = useState("");
  const [stepNo, setStepNo] = useState(1);
  const [fps, setFps] = useState(15);
  const [quality, setQuality] = useState("medium");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const busyTimer = useRef<number | null>(null);
  const textRef = useRef<HTMLInputElement | null>(null);

  // 输入框挂载后主动聚焦：WebView 里 autoFocus 常常不生效，用户就会觉得「点了没反应」
  useEffect(() => {
    if (!textAt) return;
    const t = window.setTimeout(() => textRef.current?.focus(), 40);
    return () => window.clearTimeout(t);
  }, [textAt]);

  /* busy 兜底：后端命令若卡住不返回（创建窗口阻塞等），12 秒后自动解锁，
     否则所有带 busy 闸门的按钮会永久静默失效——点了没任何反应 */
  const beginBusy = useCallback(() => {
    setBusy(true);
    if (busyTimer.current) window.clearTimeout(busyTimer.current);
    busyTimer.current = window.setTimeout(() => {
      busyTimer.current = null;
      setBusy(false);
      ipc.uiLog(`[overlay_${index}] busy 超时自动解除（后端未返回）`);
    }, 12000);
  }, [index]);

  const endBusy = useCallback(() => {
    if (busyTimer.current) {
      window.clearTimeout(busyTimer.current);
      busyTimer.current = null;
    }
    setBusy(false);
  }, []);

  const tbLog = useCallback(
    (name: string) =>
      ipc.uiLog(
        `[overlay_${index}] btn=${name} phase=${phase} mode=${mode} busy=${busy} sel=${sel ? `${Math.round(sel.w)}x${Math.round(sel.h)}` : "none"}`,
      ),
    [index, phase, mode, busy, sel],
  );

  /* ---------- 初始化 ---------- */
  useEffect(() => {
    document.body.classList.add("overlay");
    let disposed = false;
    ipc
      .overlayBootstrap()
      .then(async (b) => {
        if (disposed) return;
        setMode(b.mode === "record" ? "record" : "shot");
        setShots(b.shots);
        if (b.cursor) setCur0({ x: b.cursor[0], y: b.cursor[1] });
        if (!b.shots.length) {
          setErr("没有可用的显示器");
          return;
        }
        // 窗口盖的是整个虚拟桌面，所以把所有显示器的冻结帧拼成一张大图，
        // 每屏按它在虚拟桌面里的偏移落位——跨屏框选才是连续的。
        const x0 = Math.min(...b.shots.map((s) => s.x));
        const y0 = Math.min(...b.shots.map((s) => s.y));
        const x1 = Math.max(...b.shots.map((s) => s.x + s.w));
        const y1 = Math.max(...b.shots.map((s) => s.y + s.h));
        const vb = { x: x0, y: y0, w: Math.max(1, x1 - x0), h: Math.max(1, y1 - y0) };
        ipc.uiLog(
          `[overlay_${index}] bootstrap ok mode=${b.mode} shots=${b.shots.length} vb=${vb.w}x${vb.h}@${vb.x},${vb.y} 明细=${b.shots
            .map((s) => `#${s.index} ${s.w}x${s.h}@${s.x},${s.y} scale=${s.scale}`)
            .join(" | ")}`,
        );

        let loaded: { s: MonitorShot; im: HTMLImageElement }[] = [];
        try {
          loaded = await Promise.all(
            b.shots.map(
              (s) =>
                new Promise<{ s: MonitorShot; im: HTMLImageElement }>((res, rej) => {
                  const im = new Image();
                  im.onload = () => res({ s, im });
                  im.onerror = () => rej(new Error(`显示器 ${s.index} 冻结帧解码失败`));
                  // 走 IPC 取 base64 + data URL：asset 协议跨源会把画布污染，导出时 toDataURL 会抛错
                  ipc
                    .frameB64(s.index)
                    .then((b64) => {
                      ipc.uiLog(`[overlay_${index}] frame_b64#${s.index} len=${b64.length}`);
                      im.src = `data:image/png;base64,${b64}`;
                    })
                    .catch(rej);
                }),
            ),
          );
        } catch (e) {
          if (!disposed) {
            setErr(`冻结帧读取失败: ${String(e)}`);
            ipc.uiLog(`[overlay_${index}] frame load failed: ${String(e)}`);
          }
          return;
        }
        if (disposed) return;

        // 拼到离屏画布：主画布的绘制/导出逻辑完全不用改，底图就是整块虚拟桌面
        const off = document.createElement("canvas");
        off.width = vb.w;
        off.height = vb.h;
        const octx = off.getContext("2d");
        if (!octx) {
          setErr("离屏画布创建失败");
          return;
        }
        for (const { s, im } of loaded) {
          octx.drawImage(im, s.x - vb.x, s.y - vb.y, s.w, s.h);
        }

        setShot({
          id: "virtual",
          index: 0,
          x: vb.x,
          y: vb.y,
          w: vb.w,
          h: vb.h,
          scale: 1,
          primary: true,
          path: "",
        });
        setImg(off);

        const el = canvasRef.current;
        if (el) {
          const kk = vb.w / (el.getBoundingClientRect().width || vb.w);
          setK(kk);
          const r = el.getBoundingClientRect();
          ipc.uiLog(
            `[overlay_${index}] 合成完成 ${vb.w}x${vb.h} canvasRect=${Math.round(r.width)}x${Math.round(r.height)} k=${kk}`,
          );
        }
        // 画面准备好了才让后端把窗口显示出来，否则会先露出黑底（屏幕闪一下黑）
        requestAnimationFrame(() =>
          requestAnimationFrame(() => {
            if (!disposed) void ipc.overlayReady().catch(() => undefined);
          }),
        );
      })
      .catch((e) => {
        if (disposed) return;
        setErr(`bootstrap 失败: ${String(e)}`);
        ipc.uiLog(`[overlay_${index}] bootstrap failed: ${String(e)}`);
      });
    return () => {
      disposed = true;
      document.body.classList.remove("overlay");
    };
  }, [index]);

  const phys = useCallback(
    (e: React.PointerEvent<HTMLCanvasElement>) => {
      const el = canvasRef.current!;
      const r = el.getBoundingClientRect();
      const cur = shot ? shot.w / (r.width || shot.w) : 1;
      return { x: (e.clientX - r.left) * cur, y: (e.clientY - r.top) * cur };
    },
    [shot],
  );

  /* ---------- 绘制 ---------- */
  useEffect(() => {
    const cv = canvasRef.current;
    if (!cv || !shot) return;
    if (cv.width !== shot.w) cv.width = shot.w;
    if (cv.height !== shot.h) cv.height = shot.h;
    const ctx = cv.getContext("2d");
    if (!ctx) return;

    ctx.clearRect(0, 0, shot.w, shot.h);
    if (img) ctx.drawImage(img, 0, 0, shot.w, shot.h);
    else {
      ctx.fillStyle = "#0e1014";
      ctx.fillRect(0, 0, shot.w, shot.h);
    }

    const hasSel = !!sel && sel.w > 2 && sel.h > 2;

    // 选区外压暗
    ctx.fillStyle = phase === "annotate" ? "rgba(8,10,14,0.5)" : "rgba(8,10,14,0.38)";
    if (hasSel && sel) {
      ctx.fillRect(0, 0, shot.w, sel.y);
      ctx.fillRect(0, sel.y + sel.h, shot.w, shot.h - sel.y - sel.h);
      ctx.fillRect(0, sel.y, sel.x, sel.h);
      ctx.fillRect(sel.x + sel.w, sel.y, shot.w - sel.x - sel.w, sel.h);
    } else {
      ctx.fillRect(0, 0, shot.w, shot.h);
    }

    // 标注对象
    if (hasSel && sel) {
      ctx.save();
      for (const o of objects) drawObject(ctx, o, img);
      if (draft) drawObject(ctx, draft, img);
      ctx.restore();
    }

    // 选区边框 + 手柄
    if (hasSel && sel) {
      ctx.strokeStyle = mode === "record" ? "#ff4d4f" : "#2b6cff";
      ctx.lineWidth = Math.max(1, 2 * k);
      ctx.strokeRect(sel.x, sel.y, sel.w, sel.h);

      const hs = Math.max(4, 4 * k);
      ctx.fillStyle = "#ffffff";
      const pts = [
        [sel.x, sel.y],
        [sel.x + sel.w, sel.y],
        [sel.x, sel.y + sel.h],
        [sel.x + sel.w, sel.y + sel.h],
        [sel.x + sel.w / 2, sel.y],
        [sel.x + sel.w / 2, sel.y + sel.h],
        [sel.x, sel.y + sel.h / 2],
        [sel.x + sel.w, sel.y + sel.h / 2],
      ];
      for (const [hx, hy] of pts) ctx.fillRect(hx - hs / 2, hy - hs / 2, hs, hs);

      // 尺寸徽标
      const labelTxt = `${Math.round(sel.w)} × ${Math.round(sel.h)}`;
      const fs = Math.round(12 * k);
      ctx.font = `600 ${fs}px "Segoe UI", "Microsoft YaHei", sans-serif`;
      const tw = ctx.measureText(labelTxt).width;
      const bx = sel.x;
      const by = sel.y - fs * 1.9 > 0 ? sel.y - fs * 1.9 : sel.y + fs * 0.5;
      ctx.fillStyle = "rgba(18,20,26,0.92)";
      ctx.fillRect(bx, by, tw + 12 * k, fs * 1.6);
      ctx.fillStyle = "#ffffff";
      ctx.textBaseline = "middle";
      ctx.fillText(labelTxt, bx + 6 * k, by + fs * 0.8);
      ctx.textBaseline = "top";
    }

    // 悬停窗口框
    if (phase === "select" && hover) {
      ctx.save();
      ctx.setLineDash([6 * k, 4 * k]);
      ctx.strokeStyle = "#ffd166";
      ctx.lineWidth = Math.max(1, 2 * k);
      ctx.strokeRect(hover.x, hover.y, hover.w, hover.h);
      ctx.strokeStyle = "rgba(0,0,0,0.45)";
      ctx.lineWidth = Math.max(1, 1 * k);
      ctx.strokeRect(hover.x, hover.y, hover.w, hover.h);
      ctx.restore();

      if (hoverTitle) {
        const fs = Math.round(12 * k);
        ctx.font = `600 ${fs}px "Segoe UI", "Microsoft YaHei", sans-serif`;
        const txt = hoverTitle.length > 46 ? hoverTitle.slice(0, 46) + "…" : hoverTitle;
        const tw = ctx.measureText(txt).width;
        let bx = hover.x + 2 * k;
        let by = hover.y + hover.h + 6 * k;
        if (by + fs * 1.8 > shot.h) by = hover.y - fs * 1.9;
        if (bx + tw + 12 * k > shot.w) bx = shot.w - tw - 12 * k;
        ctx.fillStyle = "rgba(18,20,26,0.92)";
        ctx.fillRect(bx, by, tw + 12 * k, fs * 1.7);
        ctx.fillStyle = "#ffd166";
        ctx.textBaseline = "middle";
        ctx.fillText(txt, bx + 6 * k, by + fs * 0.85);
        ctx.textBaseline = "top";
      }
    }

    // 放大镜（框选阶段、未拖拽时）
    if (phase === "select" && ptr && !dragRef.current?.on) {
      const R = 64 * k;
      const zoom = 5;
      let lx = ptr.x + 22 * k;
      let ly = ptr.y + 22 * k;
      if (lx + R * 2 > shot.w) lx = ptr.x - 22 * k - R * 2;
      if (ly + R * 2 > shot.h) ly = ptr.y - 22 * k - R * 2;
      const s = R / zoom;

      ctx.save();
      ctx.beginPath();
      ctx.arc(lx + R, ly + R, R, 0, Math.PI * 2);
      ctx.clip();
      ctx.fillStyle = "#000";
      ctx.fillRect(lx, ly, R * 2, R * 2);
      if (img) {
        ctx.imageSmoothingEnabled = false;
        ctx.drawImage(img, ptr.x - s, ptr.y - s, s * 2, s * 2, lx, ly, R * 2, R * 2);
        ctx.imageSmoothingEnabled = true;
      }
      ctx.restore();

      ctx.beginPath();
      ctx.arc(lx + R, ly + R, R, 0, Math.PI * 2);
      ctx.strokeStyle = "rgba(255,255,255,0.85)";
      ctx.lineWidth = Math.max(1, k);
      ctx.stroke();

      // 中心十字
      ctx.strokeStyle = "#ff4d4f";
      ctx.lineWidth = Math.max(1, k);
      ctx.beginPath();
      ctx.moveTo(lx + R - 6 * k, ly + R);
      ctx.lineTo(lx + R + 6 * k, ly + R);
      ctx.moveTo(lx + R, ly + R - 6 * k);
      ctx.lineTo(lx + R, ly + R + 6 * k);
      ctx.stroke();

      const fs = Math.round(11 * k);
      ctx.font = `500 ${fs}px "Segoe UI", sans-serif`;
      const t = `${Math.round(ptr.x)}, ${Math.round(ptr.y)}`;
      ctx.fillStyle = "rgba(18,20,26,0.92)";
      ctx.fillRect(lx, ly + R * 2 + 4 * k, ctx.measureText(t).width + 10 * k, fs * 1.5);
      ctx.fillStyle = "#d8dbe4";
      ctx.textBaseline = "middle";
      ctx.fillText(t, lx + 5 * k, ly + R * 2 + 4 * k + fs * 0.8);
      ctx.textBaseline = "top";
    }
  }, [shot, img, sel, hover, hoverTitle, phase, objects, draft, ptr, k, mode]);

  /* ---------- 导出与动作 ---------- */
  const regionOf = useCallback(
    (r: Rect): Rect | null => {
      if (!shot) return null;
      return { x: shot.x + Math.round(r.x), y: shot.y + Math.round(r.y), w: Math.round(r.w), h: Math.round(r.h) };
    },
    [shot],
  );

  const buildPng = useCallback((): string | null => {
    if (!img || !sel || sel.w < 1 || sel.h < 1) return null;
    return toBase64(renderCrop(img, objects, sel));
  }, [img, sel, objects]);

  const closeAll = useCallback(() => {
    ipc.captureClose().catch(() => {});
  }, []);

  const doShot = useCallback(
    async (copy: boolean, save: boolean) => {
      if (busy) {
        ipc.uiLog(`[overlay_${index}] shot 被 busy 拦下（等后端）`);
        return;
      }
      const b64 = buildPng();
      const region = sel ? regionOf(sel) : null;
      if (!b64) {
        ipc.uiLog(`[overlay_${index}] shot 无内容（img/sel 未就绪）→ 直接关闭`);
        closeAll();
        return;
      }
      beginBusy();
      try {
        ipc.uiLog(
          `[overlay_${index}] shot_export 开始 copy=${copy} save=${save} region=${region ? `${region.x},${region.y},${region.w}x${region.h}` : "full"} b64len=${b64.length}`,
        );
        const r = await ipc.shotExport(b64, copy, save, region);
        ipc.uiLog(`[overlay_${index}] shot_export 成功 ${r.path} ${r.bytes}B`);
      } catch (e) {
        ipc.uiLog(`[overlay_${index}] shot_export 失败: ${String(e)}`);
        setErr(`导出失败：${String(e)}`);
        console.error(e);
      } finally {
        endBusy();
        closeAll();
      }
    },
    [busy, buildPng, regionOf, closeAll, sel, index, beginBusy, endBusy],
  );

  // OCR：把选区图交给后端（系统自带识别），识别完写剪贴板并用 toast 报结果
  const doOcr = useCallback(async () => {
    if (busy) return;
    const b64 = buildPng();
    if (!b64) {
      closeAll();
      return;
    }
    ipc.uiLog(`[overlay_${index}] ocr 发出 b64len=${b64.length}`);
    // 先发识别命令再关浮层：反过来的话 capture_close 会抢在前面把窗口销毁，
    // 正在路上的 invoke 就送不到后端了（表现是「点了 OCR 毫无反应」）
    void ipc.ocrText(b64).then(
      (r) => ipc.uiLog(`[overlay_${index}] ocr 已排队 ${r}`),
      (e) => ipc.uiLog(`[overlay_${index}] ocr 失败: ${String(e)}`),
    );
    closeAll();
  }, [busy, buildPng, closeAll, index]);

  // 长截图：进「滚动截长图」会话（预览窗由后端弹出），关掉浮层后由用户自己滚轮，
  // 点预览窗上的复制/复制并保存才结束
  const doScrollShot = useCallback(async () => {
    if (!sel || busy) {
      if (!sel) setErr("请先框选要滚动截取的区域");
      return;
    }
    const region = regionOf(sel);
    if (!region) return;
    ipc.uiLog(
      `[overlay_${index}] scroll_start 发出 ${region.x},${region.y} ${region.w}x${region.h}`,
    );
    // 先发命令再关浮层（理由同 OCR：close 会先销毁窗口，命令就丢了）
    void ipc.scrollStart(region).then(
      () => ipc.uiLog(`[overlay_${index}] scroll_start 成功`),
      (e) => ipc.uiLog(`[overlay_${index}] scroll_start 失败: ${String(e)}`),
    );
    closeAll();
  }, [sel, busy, regionOf, closeAll, index]);

  const doPin = useCallback(async () => {
    if (busy) return;
    const b64 = buildPng();
    if (!b64) {
      ipc.uiLog(`[overlay_${index}] pin 无内容（img/sel 未就绪）→ 直接关闭`);
      closeAll();
      return;
    }
    // 只发命令，不自己关浮层：贴图要新建窗口，后端建好后会顺手关掉本浮层
    ipc.uiLog(`[overlay_${index}] pin 发出 b64len=${b64.length}`);
    void ipc.pinPng(b64).then(
      (id) => ipc.uiLog(`[overlay_${index}] pin 成功 id=${id}`),
      (e) => ipc.uiLog(`[overlay_${index}] pin 失败: ${String(e)}`),
    );
  }, [busy, buildPng, index]);

  const startRecord = useCallback(async () => {
    if (!sel || busy) {
      ipc.uiLog(`[overlay_${index}] record 被拦下 sel=${sel ? "有" : "无"} busy=${busy}`);
      if (!sel) setErr("请先按住左键拖拽，选出要录制的区域");
      return;
    }
    const region = regionOf(sel);
    if (!region) return;
    // 不 await：后端会先把这个浮层关掉，响应很可能送不回来（await 会永久挂起，
    // 之后所有带 busy 闸门的按钮就全部失效）
    ipc.uiLog(`[overlay_${index}] record_begin 发出 ${region.x},${region.y} ${region.w}x${region.h} fps=${fps} q=${quality}`);
    void ipc.recordBegin({ ...region, fps, quality }).then(
      () => ipc.uiLog(`[overlay_${index}] record_begin 已返回`),
      (e) => ipc.uiLog(`[overlay_${index}] record_begin 返回错误: ${String(e)}`),
    );
  }, [sel, busy, regionOf, fps, quality, index]);

  const enterAnnotate = useCallback(() => {
    setPhase("annotate");
    setObjects([]);
    setRedoStack([]);
  }, []);

  /* 兜底：指针在窗口外松开 / 被系统取消时，也把 canvas 的指针捕获放掉 */
  useEffect(() => {
    const release = (ev: PointerEvent) => {
      const cv = canvasRef.current;
      if (!cv) return;
      try {
        if (cv.hasPointerCapture(ev.pointerId)) cv.releasePointerCapture(ev.pointerId);
      } catch {
        /* 已释放 */
      }
    };
    window.addEventListener("pointerup", release);
    window.addEventListener("pointercancel", release);
    return () => {
      window.removeEventListener("pointerup", release);
      window.removeEventListener("pointercancel", release);
    };
  }, []);

  /* ---------- 指针 ---------- */
  const onDown = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const pp = phys(e);
    ipc.uiLog(`[overlay_${index}] down phase=${phase} btn=${e.button} at ${pp.x},${pp.y} sel=${sel ? `${sel.x},${sel.y},${sel.w}x${sel.h}` : "none"}`);
    if (e.button !== 0) return;
    const p = phys(e);
    canvasRef.current?.setPointerCapture(e.pointerId);

    if (phase === "select") {
      dragRef.current = { x: p.x, y: p.y, on: true };
      setSel({ x: p.x, y: p.y, w: 0, h: 0 });
      return;
    }

    if (!sel) return;
    const inside =
      p.x >= sel.x && p.x <= sel.x + sel.w && p.y >= sel.y && p.y <= sel.y + sel.h;
    if (!inside) return;

    if (tool === "text") {
      // canvas 正持有指针捕获时，新出现的输入框拿不到焦点，表现为「点了文字没反应」。
      // 先把指针放开，再让输入框在下一帧聚焦。
      try {
        if (canvasRef.current?.hasPointerCapture(e.pointerId)) {
          canvasRef.current.releasePointerCapture(e.pointerId);
        }
      } catch {
        /* 指针已不在 canvas 上 */
      }
      setTextAt({ x: p.x, y: p.y });
      setTextValue("");
      ipc.uiLog(`[overlay_${index}] text 输入框出现 at ${p.x},${p.y}`);
      return;
    }
    if (tool === "step") {
      const o: Obj = { id: crypto.randomUUID(), type: "step", x: p.x, y: p.y, n: stepNo, color, r: 16 * k };
      setObjects((v) => [...v, o]);
      setStepNo((n) => n + 1);
      return;
    }
    dragRef.current = { x: p.x, y: p.y, on: true };
    const wid = Math.max(1, Math.round(lw * k));
    if (tool === "rect" || tool === "ellipse" || tool === "mosaic") {
      setDraft({
        id: "draft",
        type: tool,
        x: p.x,
        y: p.y,
        w: 0,
        h: 0,
        color,
        width: wid,
        block: tool === "mosaic" ? Math.max(3, 8 * k) : undefined,
        blur: false,
      } as Obj);
    } else if (tool === "arrow") {
      setDraft({ id: "draft", type: "arrow", x1: p.x, y1: p.y, x2: p.x, y2: p.y, color, width: wid });
    } else {
      setDraft({ id: "draft", type: tool, pts: [[p.x, p.y]], color, width: wid } as Obj);
    }
  };

  const onMove = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const p = phys(e);
    setPtr(p);

    if (dragRef.current?.on) {
      if (phase === "select") {
        setSel(norm(dragRef.current.x, dragRef.current.y, p.x, p.y));
      } else if (draft) {
        if (draft.type === "rect" || draft.type === "ellipse" || draft.type === "mosaic") {
          setDraft({ ...draft, ...norm(dragRef.current.x, dragRef.current.y, p.x, p.y) } as Obj);
        } else if (draft.type === "arrow") {
          setDraft({ ...draft, x2: p.x, y2: p.y });
        } else if (draft.type === "pen" || draft.type === "marker") {
          setDraft({ ...draft, pts: [...draft.pts, [p.x, p.y]] });
        }
      }
      return;
    }

    // 悬停窗口命中（框选阶段、无选区）
    if (phase === "select" && shot) {
      const now = performance.now();
      if (now - hoverTs.current > 90) {
        hoverTs.current = now;
        ipc
          .windowAt(shot.x + Math.round(p.x), shot.y + Math.round(p.y))
          .then((w: WinPeek | null) => {
            if (!w) {
              setHover(null);
              setHoverTitle("");
              return;
            }
            const r: Rect = {
              x: Math.max(0, w.x - shot.x),
              y: Math.max(0, w.y - shot.y),
              w: Math.min(w.w, shot.w - Math.max(0, w.x - shot.x)),
              h: Math.min(w.h, shot.h - Math.max(0, w.y - shot.y)),
            };
            setHover(r);
            setHoverTitle(w.title || w.class || "");
          })
          .catch(() => {});
      }
    }
  };

  const onUp = (e: React.PointerEvent<HTMLCanvasElement>) => {
    // 必须显式释放指针捕获：否则松手后 canvas 会继续独占指针，
    // 工具条按钮、标注面板全都收不到点击（表现为「点按钮完全没反应」）
    try {
      const cv = canvasRef.current;
      if (cv && cv.hasPointerCapture(e.pointerId)) cv.releasePointerCapture(e.pointerId);
    } catch {
      /* 已释放 */
    }
    const p = phys(e);
    const wasOn = dragRef.current?.on;
    dragRef.current = null;

    if (phase === "select") {
      if (!wasOn) return;
      if (!sel || sel.w < 6 * k || sel.h < 6 * k) {
        // 视为单击：吸附悬停窗口
        if (hover && hover.w > 8) {
          setSel(hover);
          return;
        }
        setSel(null);
        return;
      }
      return;
    }

    if (draft) {
      if (draft.type === "rect" || draft.type === "ellipse" || draft.type === "mosaic") {
        if (Math.abs(draft.w) > 4 && Math.abs(draft.h) > 4) {
          setObjects((v) => [...v, { ...draft, id: crypto.randomUUID() }]);
        }
      } else if (draft.type === "arrow") {
        if (Math.hypot(draft.x2 - draft.x1, draft.y2 - draft.y1) > 6) {
          setObjects((v) => [...v, { ...draft, id: crypto.randomUUID() }]);
        }
      } else if (draft.type === "pen" || draft.type === "marker") {
        if (draft.pts.length > 1) {
          setObjects((v) => [...v, { ...draft, id: crypto.randomUUID() }]);
        }
      }
      setDraft(null);
    }
    void p;
  };

  const commitText = () => {
    ipc.uiLog(`[overlay_${index}] text commit 进入 textAt=${!!textAt} value="${textValue}"`);
    if (textAt && textValue.trim()) {
      const o: Obj = {
        id: crypto.randomUUID(),
        type: "text",
        x: textAt.x,
        y: textAt.y,
        text: textValue,
        size: Math.round(20 * k),
        color,
      };
      setObjects((v) => [...v, o]);
      ipc.uiLog(`[overlay_${index}] text 已添加 objects=${"+"}1`);
    }
    setTextAt(null);
    setTextValue("");
  };

  const undo = useCallback(() => {
    setObjects((v) => {
      if (v.length === 0) return v;
      const last = v[v.length - 1];
      setRedoStack((r) => [...r, last]);
      return v.slice(0, -1);
    });
  }, []);

  const redo = useCallback(() => {
    setRedoStack((r) => {
      if (r.length === 0) return r;
      const last = r[r.length - 1];
      setObjects((v) => [...v, last]);
      return r.slice(0, -1);
    });
  }, []);

  /* ---------- 键盘 ---------- */
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // 输入框里已经处理并 preventDefault 的按键（比如文字标注的 Enter），
      // 不要再当成「截图/开录」的快捷键执行一遍
      if (e.defaultPrevented) return;
      const tag = (e.target as HTMLElement)?.tagName;
      if (tag === "INPUT" || tag === "SELECT" || tag === "TEXTAREA") return;

      if (e.key === "Escape") {
        e.preventDefault();
        if (phase === "annotate") {
          setPhase("select");
          setObjects([]);
          setRedoStack([]);
          setTextAt(null);
        } else {
          closeAll();
        }
        return;
      }
      if (e.key === "Tab") {
        e.preventDefault();
        setMode((m) => (m === "shot" ? "record" : "shot"));
        return;
      }
      if (e.key === "Enter") {
        e.preventDefault();
        ipc.uiLog(
          `[overlay_${index}] key=Enter phase=${phase} mode=${mode} sel=${sel ? `${Math.round(sel.w)}x${Math.round(sel.h)}` : "none"}`,
        );
        if (phase === "annotate") {
          void doShot(true, false);
        } else if (mode === "shot") {
          if (sel && sel.w > 8) void doShot(true, false);
          else setErr("请先按住左键拖拽，选出要截取的区域");
        } else if (sel && sel.w > 8) {
          void startRecord();
        } else {
          setErr("请先按住左键拖拽，选出要录制的区域");
        }
        return;
      }
      // 单键快捷操作（浮层里不必再去精确点按钮）
      if (phase === "select" && !e.ctrlKey && !e.metaKey && !e.altKey) {
        const k = e.key.toLowerCase();
        if (mode === "shot" && k === "c") {
          e.preventDefault();
          tbLog("复制(快捷键C)");
          void doShot(true, false);
          return;
        }
        if (mode === "shot" && k === "s") {
          e.preventDefault();
          tbLog("复制并保存(快捷键S)");
          void doShot(true, true);
          return;
        }
        if (mode === "shot" && k === "p") {
          e.preventDefault();
          tbLog("贴图(快捷键P)");
          void doPin();
          return;
        }
        if (mode === "shot" && k === "o") {
          e.preventDefault();
          tbLog("OCR(快捷键O)");
          void doOcr();
          return;
        }
        if (SCROLL_ON && mode === "shot" && k === "l") {
          e.preventDefault();
          tbLog("长截图(快捷键L)");
          void doScrollShot();
          return;
        }
        if (k === "a") {
          e.preventDefault();
          tbLog("标注(快捷键A)");
          enterAnnotate();
          return;
        }
      }
      // 标注态：单键切工具，省得去点小按钮
      if (phase === "annotate" && !e.ctrlKey && !e.metaKey && !e.altKey) {
        const toolMap: Record<string, Tool> = {
          r: "rect",
          e: "ellipse",
          x: "arrow",
          b: "pen",
          h: "marker",
          t: "text",
          n: "step",
          m: "mosaic",
        };
        const t = toolMap[e.key.toLowerCase()];
        if (t) {
          e.preventDefault();
          setTool(t);
          ipc.uiLog(`[overlay_${index}] 工具切换 ${t}`);
          return;
        }
      }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "z") {
        e.preventDefault();
        if (e.shiftKey) redo();
        else undo();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [phase, mode, sel, closeAll, doShot, startRecord, undo, redo]);

  /* ---------- 工具条定位 ---------- */
  // 工具条宽度随内容变化（截图态两行、录屏态一行），猜一个估算值容易让工具条
  // 在贴边选区时伸出屏幕右边把「取消」切掉，所以实测一次再定位。
  const tbRef = useRef<HTMLDivElement | null>(null);
  const [tbW, setTbW] = useState(0);
  const tb = useMemo(() => {
    if (!sel || sel.w < 8 || sel.h < 8) return null;
    const x = sel.x / k;
    const y = sel.y / k;
    const w = sel.w / k;
    const h = sel.h / k;
    const barH = mode === "shot" ? 44 : 40;
    const W = window.innerWidth;
    const H = window.innerHeight;
    const estW = tbW || (mode === "shot" ? 800 : 470);
    let left = Math.max(8, Math.min(x + w - estW, W - estW - 8));
    if (W < estW + 16) left = 8;
    let top = y + h + 8;
    if (top + barH > H - 8) top = Math.max(8, y - barH - 8);
    return { left, top };
  }, [sel, k, mode, tbW]);
  useEffect(() => {
    const el = tbRef.current;
    if (el && el.offsetWidth && el.offsetWidth !== tbW) setTbW(el.offsetWidth);
  }, [mode, phase, sel, tbW]);

  const hasSel = !!sel && sel.w > 8 && sel.h > 8;

  /* 中央提示要落在「光标所在的那块屏」的中央。
     双屏下虚拟桌面的正中正好是两块屏的接缝，提示会劈在缝上，两边各露一半。
     光标移到另一块屏时，提示跟着过去。 */
  const hintPos = useMemo(() => {
    if (!shot || !shots.length) return null;
    const p = ptr ?? cur0;
    if (!p) return null;
    const mon =
      shots.find(
        (s) => p.x >= s.x && p.x < s.x + s.w && p.y >= s.y && p.y < s.y + s.h,
      ) ?? shots.find((s) => s.primary) ?? shots[0];
    return {
      left: (mon.x - shot.x + mon.w / 2) / k,
      top: (mon.y - shot.y + mon.h / 2) / k,
    };
  }, [shot, shots, ptr, cur0, k]);

  return (
    <div className="overlay-root">
      {err ? (
        <div className="ov-err" style={hintPos ? { left: hintPos.left } : undefined}>
          {err}
        </div>
      ) : null}

      {!hasSel && phase === "select" && (
        <div
          className="ov-hint"
          style={hintPos ? { left: hintPos.left, top: hintPos.top } : undefined}
        >
          <div className="ov-hint-main">
            按住鼠标左键拖拽，选出{mode === "shot" ? "截图" : "录制"}区域
          </div>
          <div className="ov-hint-sub">
            松开即可；<b>Enter</b> {mode === "shot" ? "截图并复制" : "开始录制"}　·　<b>Tab</b> 切换{mode === "shot" ? "录屏" : "截图"}　·　<b>Esc</b> 退出
          </div>
        </div>
      )}

      <canvas
        ref={canvasRef}
        style={{ width: "100%", height: "100%", cursor: phase === "annotate" && tool === "text" ? "text" : "crosshair" }}
        onPointerDown={onDown}
        onPointerMove={onMove}
        onPointerUp={onUp}
        onPointerLeave={() => setPtr(null)}
        onDoubleClick={() => {
          if (phase === "select" && mode === "shot" && hasSel) void doShot(true, false);
        }}
      />

      {tb && (
        <div
          ref={tbRef}
          className="overlay-toolbar"
          style={{ left: tb.left, top: tb.top }}
          onPointerDown={(e) => e.stopPropagation()}
        >
          <button
            className={"tb-btn " + (mode === "shot" ? "active" : "")}
            onClick={() => {
              tbLog("截图");
              setMode("shot");
            }}
          >
            截图
          </button>
          <button
            className={"tb-btn " + (mode === "record" ? "active" : "")}
            onClick={() => {
              tbLog("录屏");
              setMode("record");
            }}
          >
            录屏
          </button>
          <div className="tb-sep" />
          {mode === "shot" ? (
            <>
              {/* 标注工具直接摆在主工具条上：原来要先点「标注」再展开，常用项藏进了二级，
                  用户每标注一次都得多点一下。点任一工具即进入标注态。 */}
              {TOOLS.map((t) => (
                <button
                  key={t}
                  className={"tb-icon " + (phase === "annotate" && tool === t ? "active" : "")}
                  title={TOOL_LABEL[t]}
                  onClick={() => {
                    tbLog(`工具:${t}`);
                    setTool(t);
                    setPhase("annotate");
                  }}
                >
                  {TOOL_ICON[t]}
                </button>
              ))}
              <div className="tb-sep" />
              {COLORS.map((c) => (
                <button
                  key={c}
                  className="tb-color"
                  style={{ background: c, outline: color === c ? "2px solid #2b6cff" : "none" }}
                  onClick={() => setColor(c)}
                />
              ))}
              <input
                type="range"
                min={1}
                max={10}
                value={lw}
                onChange={(e) => setLw(Number(e.target.value))}
                style={{ width: 58 }}
                title="线宽"
              />
              {/* 工具图标化后总宽约 760px，单行能放下，不再强制换行 */}
              <div className="tb-sep" />
              <button className="tb-icon" onClick={undo} title="撤销 (Ctrl+Z)">
                <svg viewBox="0 0 16 16" className="tb-ic">
                  <path d="M4 7.2h6.1a3.2 3.2 0 0 1 0 6.4H4.4" fill="none" stroke="currentColor" strokeWidth="1.3" />
                  <path d="M6.7 4.2 4 7.2l2.7 2.6" fill="none" stroke="currentColor" strokeWidth="1.3" />
                </svg>
              </button>
              <button className="tb-icon" onClick={redo} title="重做 (Ctrl+Y)">
                <svg viewBox="0 0 16 16" className="tb-ic">
                  <path d="M12 7.2H5.9a3.2 3.2 0 0 0 0 6.4h5.7" fill="none" stroke="currentColor" strokeWidth="1.3" />
                  <path d="M9.3 4.2 12 7.2l-2.7 2.6" fill="none" stroke="currentColor" strokeWidth="1.3" />
                </svg>
              </button>
              <div className="tb-sep" />
              <button
                className="tb-btn"
                onClick={() => {
                  tbLog("复制并保存");
                  void doShot(true, true);
                }}
              >
                复制并保存
              </button>
              <button
                className="tb-btn"
                onClick={() => {
                  tbLog("复制");
                  void doShot(true, false);
                }}
              >
                复制
              </button>
              <button
                className="tb-btn"
                onClick={() => {
                  tbLog("贴图");
                  void doPin();
                }}
              >
                贴图
              </button>
              <button
                className="tb-btn"
                title="识别选区里的文字并复制（用系统自带 OCR）"
                onClick={() => {
                  tbLog("OCR");
                  void doOcr();
                }}
              >
                OCR
              </button>
              {/* 长截图体验还没打磨好，入口先屏蔽（代码保留，SCROLL_ON 打开即恢复） */}
              {SCROLL_ON && (
                <button
                  className="tb-btn"
                  title="滚动截取整页（选区要盖住可滚动的窗口内容）"
                  onClick={() => {
                    tbLog("长截图");
                    void doScrollShot();
                  }}
                >
                  长截图
                </button>
              )}
              {phase === "annotate" && (
                <button
                  className="tb-btn"
                  onClick={() => {
                    tbLog("重选");
                    setPhase("select");
                    setObjects([]);
                    setRedoStack([]);
                  }}
                >
                  重选
                </button>
              )}
            </>
          ) : (
            <>
              <ChipSelect
                icon={
                  <svg viewBox="0 0 16 16" aria-hidden>
                    <rect x="1.6" y="3.6" width="12.8" height="8.8" rx="1.6" fill="none" stroke="currentColor" strokeWidth="1.2" />
                    <path d="M4.4 3.6v8.8M8 3.6v8.8M11.6 3.6v8.8" stroke="currentColor" strokeWidth="1.1" opacity="0.55" />
                  </svg>
                }
                value={fps}
                options={[{ v: 10 }, { v: 15 }, { v: 20 }]}
                unit="fps"
                onChange={(v) => setFps(Number(v))}
              />
              <ChipSelect
                icon={
                  <svg viewBox="0 0 16 16" aria-hidden>
                    <path d="M2.4 11.2a5.6 5.6 0 0 1 11.2 0" fill="none" stroke="currentColor" strokeWidth="1.2" />
                    <path d="M8 11.2 10.6 7" stroke="currentColor" strokeWidth="1.3" />
                    <circle cx="8" cy="11.2" r="1" fill="currentColor" />
                  </svg>
                }
                value={quality}
                options={[
                  { v: "low", label: "小体积" },
                  { v: "medium", label: "均衡" },
                  { v: "high", label: "高清晰" },
                ]}
                onChange={(v) => setQuality(v as typeof quality)}
              />
              <button
                className="tb-rec"
                onClick={() => {
                  tbLog("开始录制");
                  void startRecord();
                }}
              >
                <span className="tb-rec-dot" />
                录制
              </button>
            </>
          )}
          <div className="tb-sep" />
          <button
            className="tb-btn"
            onClick={() => {
              tbLog("取消");
              closeAll();
            }}
          >
            取消
          </button>
        </div>
      )}

      {textAt && (
        <div
          className="text-input-wrap"
          style={{ left: textAt.x / k, top: textAt.y / k }}
          onPointerDown={(e) => e.stopPropagation()}
        >
          <input
            ref={textRef}
            value={textValue}
            onChange={(e) => setTextValue(e.target.value)}
            onKeyDown={(e) => {
              e.stopPropagation();
              ipc.uiLog(`[overlay_${index}] input key=${e.key} code=${e.keyCode} target=${(e.target as HTMLElement)?.tagName}`);
              if (e.key === "Enter" || e.keyCode === 13) {
                e.preventDefault();
                commitText();
                return;
              }
              if (e.key === "Escape" || e.keyCode === 27) {
                e.preventDefault();
                setTextAt(null);
                setTextValue("");
              }
            }}
            // 空内容失焦不立刻销毁，避免刚弹出就被自己的 blur 关掉
            onBlur={() => {
              if (textValue.trim()) commitText();
            }}
            placeholder="输入文字后回车"
          />
          <button className="text-ok" onClick={commitText} title="确定">
            确定
          </button>
        </div>
      )}
    </div>
  );
}
