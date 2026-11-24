import React, { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { ipc } from "../lib/ipc";

type Thumb = { b64: string; w: number; rows: number };

/// 长截图的预览窗：贴着选区外侧的深色小卡片，
/// 实时显示用户滚动时拼出来的长图缩略图，点按钮才结束流程。
export default function ScrollPreview() {
  const [thumb, setThumb] = useState<Thumb | null>(null);
  const [busy, setBusy] = useState(false);
  const stageRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let un: (() => void) | undefined;
    listen<Thumb>("scroll://thumb", (e) => setThumb(e.payload)).then((u) => (un = u));
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        void ipc.scrollCancel();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      un?.();
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  // 每拼进一段就滑到底部，用户看到的是刚接上的那一段
  useEffect(() => {
    const el = stageRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [thumb]);

  const done = (copy: boolean, save: boolean) => {
    if (busy) return;
    setBusy(true);
    // 这条命令会关掉本窗口，响应可能送不回来，所以不 await 成功与否
    void ipc.scrollFinish(copy, save).catch(() => setBusy(false));
  };

  return (
    <div className="scroll-panel">
      <div className="sp-head" data-tauri-drag-region>
        <span className="sp-dot" />
        <span className="sp-title">滚动截长图</span>
        <span className="sp-size">{thumb ? `${thumb.w} × ${thumb.rows}` : "准备中…"}</span>
      </div>
      <div className="sp-stage" ref={stageRef}>
        {thumb ? (
          <img
            className="sp-shot"
            src={`data:image/png;base64,${thumb.b64}`}
            alt=""
            draggable={false}
          />
        ) : (
          <div className="sp-empty">滚轮向下滚动，这里会实时拼出长图</div>
        )}
      </div>
      <div className="sp-hint">用鼠标滚轮滚动到想截的位置</div>
      <div className="sp-actions">
        <button className="sp-btn" disabled={busy} onClick={() => done(true, false)}>
          复制
        </button>
        <button className="sp-btn primary" disabled={busy} onClick={() => done(true, true)}>
          复制并保存
        </button>
        <button className="sp-btn ghost" disabled={busy} onClick={() => void ipc.scrollCancel()}>
          取消
        </button>
      </div>
    </div>
  );
}
