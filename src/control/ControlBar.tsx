import React, { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { ipc } from "../lib/ipc";

function fmt(ms: number) {
  const s = Math.floor(ms / 1000);
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

/** 16×16 线性图标，和截图工具条同一套画法 */
const ICON = {
  pause: <path d="M6 4v8M10 4v8" />,
  play: <path d="M6.5 4.2 11.6 8l-5.1 3.8z" />,
  stop: <rect x="5" y="5" width="6" height="6" rx="1" />,
  trash: <path d="M4.5 5.5h7M6.5 5.5V4h3v1.5M5.5 5.5l.5 6h4l.5-6" />,
};

function Ico({ d }: { d: keyof typeof ICON }) {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.4"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      {ICON[d]}
    </svg>
  );
}

export default function ControlBar() {
  const [ms, setMs] = useState(0);
  const [frames, setFrames] = useState(0);
  const [paused, setPaused] = useState(false);
  const [over, setOver] = useState(false);

  useEffect(() => {
    let un1: (() => void) | undefined;
    let un2: (() => void) | undefined;
    listen<{ ms: number; frames: number }>("rec://tick", (e) => {
      setMs(e.payload.ms);
      setFrames(e.payload.frames);
    }).then((u) => (un1 = u));
    listen<{ state: string; error?: string }>("rec://state", (e) => {
      if (e.payload.state === "done" || e.payload.state === "error") setOver(true);
    }).then((u) => (un2 = u));
    ipc.recordStatus().then((s) => setPaused(s.paused)).catch(() => {});
    return () => {
      un1?.();
      un2?.();
    };
  }, []);

  const toggle = async () => {
    const next = !paused;
    setPaused(next);
    await ipc.recordSetPause(next).catch(() => {});
  };

  return (
    <div className="control-bar" data-tauri-drag-region>
      <span className={"cb-dot" + (paused ? " paused" : "")} />
      <span className="cb-time">{fmt(ms)}</span>
      <span className="cb-meta">{frames} 帧</span>
      <span className="cb-sep" />
      {over ? (
        <button
          className="cb-ico"
          title="关闭控制条"
          onClick={() => ipc.recordCancel().catch(() => {})}
        >
          <Ico d="stop" />
        </button>
      ) : (
        <>
          <button className="cb-ico" title={paused ? "继续录制" : "暂停录制"} onClick={toggle}>
            <Ico d={paused ? "play" : "pause"} />
          </button>
          <button
            className="cb-ico"
            title="停止并保存"
            onClick={() => ipc.recordStop().catch(() => {})}
          >
            <Ico d="stop" />
          </button>
          <button
            className="cb-ico danger"
            title="丢弃这段录制"
            onClick={() => ipc.recordCancel().catch(() => {})}
          >
            <Ico d="trash" />
          </button>
        </>
      )}
    </div>
  );
}
