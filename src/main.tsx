import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import CaptureOverlay from "./overlay/CaptureOverlay";
import ControlBar from "./control/ControlBar";
import PinWindow from "./pin/PinWindow";
import ScreenFrame from "./frame/ScreenFrame";
import ScrollPreview from "./scroll/ScrollPreview";
import Settings from "./settings/Settings";
import Toast from "./toast/Toast";
import { ipc } from "./lib/ipc";
import "./styles.css";

// 任何未捕获异常都直接画到页面上，便于在没有 devtools 的窗口里定位问题；
// 同时写进 %APPDATA%\Look\look.log，release 双击运行时也能事后查。
function showError(msg: string) {
  ipc.uiLog(`[${label}] ERR: ${msg}`);
  const el = document.getElementById("root");
  if (!el) return;
  el.innerHTML = `<pre style="margin:0;padding:8px;color:#c00;background:#fff;font:12px/1.4 monospace;white-space:pre-wrap">ERR: ${msg}</pre>`;
}
window.addEventListener("error", (e) => showError(e.message));
window.addEventListener("unhandledrejection", (e) =>
  showError(String((e as PromiseRejectionEvent).reason)),
);

const label = getCurrentWindow().label;
ipc.uiLog(`[${label}] 页面已加载 ${location.href} ${innerWidth}x${innerHeight} dpr=${devicePixelRatio}`);

let view: React.ReactElement;
if (label.startsWith("overlay_")) {
  view = <CaptureOverlay label={label} />;
} else if (label === "control") {
  document.body.classList.add("control");
  view = <ControlBar />;
} else if (label === "scroll_preview") {
  document.body.classList.add("scrollprev");
  view = <ScrollPreview />;
} else if (label === "rec_frame") {
  view = <ScreenFrame />;
} else if (label === "scroll_frame") {
  view = <ScreenFrame tone="scroll" />;
} else if (label.startsWith("pin_")) {
  view = <PinWindow label={label} />;
} else if (label === "toast") {
  view = <Toast />;
} else {
  view = <Settings />;
}

ReactDOM.createRoot(document.getElementById("root")!).render(view);
