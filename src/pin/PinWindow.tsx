import React, { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { PhysicalSize } from "@tauri-apps/api/dpi";
import { assetUrl } from "../lib/ipc";

export default function PinWindow({ label }: { label: string }) {
  const [src, setSrc] = useState("");
  const [sel, setSel] = useState(false);
  const baseRef = useRef<{ w: number; h: number } | null>(null);

  useEffect(() => {
    const raw = new URLSearchParams(window.location.search).get("pin");
    if (!raw) return;
    setSrc(assetUrl(decodeURIComponent(raw)));
    getCurrentWindow()
      .innerSize()
      .then((s) => (baseRef.current = { w: s.width, h: s.height }))
      .catch(() => {});
  }, []);

  useEffect(() => {
    let un: (() => void) | undefined;
    getCurrentWindow()
      .onFocusChanged(({ payload }) => setSel(payload))
      .then((u) => (un = u))
      .catch(() => {});
    return () => un?.();
  }, []);

  // 只有「选中」的那张贴图能被关掉：Esc 由当前有焦点的窗口自己收，别的贴图收不到键。
  // 右键关闭已经去掉，避免误右键就丢掉一张图。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        void getCurrentWindow().close().catch(() => {});
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const onWheel = async (e: React.WheelEvent) => {
    e.preventDefault();
    const w = getCurrentWindow();
    const cur = await w.innerSize();
    const f = e.deltaY < 0 ? 1.15 : 1 / 1.15;
    const nw = Math.max(80, Math.round(cur.width * f));
    const nh = Math.max(60, Math.round(cur.height * f));
    await w
      .setSize(new PhysicalSize(nw, nh))
      .catch(() => {});
  };

  const onDown = async (e: React.MouseEvent) => {
    if (e.button === 0) {
      await getCurrentWindow().startDragging().catch(() => {});
    }
  };

  return (
    <div
      className={"pin-root" + (sel ? " sel" : "")}
      onMouseDown={onDown}
      onWheel={onWheel}
      onContextMenu={(e) => e.preventDefault()}
    >
      {src ? <img src={src} draggable={false} alt={label} /> : null}
    </div>
  );
}
