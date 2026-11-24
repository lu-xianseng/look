import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { ipc } from "../lib/ipc";

/** 截图 / 录屏收尾的小提示：无边框透明窗，2 秒后自己关。
 *  连续操作时后端复用同一个窗口并 emit `toast://text`，这里重新取文案、重新计时。 */
export default function Toast() {
  const [msg, setMsg] = useState("");
  const timer = useRef<number | null>(null);
  const seq = useRef(0);

  useEffect(() => {
    document.body.classList.add("toast");
    // html 也要摘掉默认底色，否则圆角外会留一圈方形深底
    document.documentElement.classList.add("toast");

    const show = async () => {
      const id = ++seq.current;
      try {
        const t = await ipc.toastText();
        setMsg(t || "完成");
        // 文字有了才让后端显示窗口，否则会先闪出一个空框
        void ipc.toastReady().catch(() => undefined);
      } catch {
        void ipc.toastClose().catch(() => undefined);
        return;
      }
      if (timer.current) window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => {
        if (seq.current === id) void ipc.toastClose().catch(() => undefined);
      }, 2000);
    };

    void show();
    let un: (() => void) | undefined;
    listen<string>("toast://text", () => void show())
      .then((u) => (un = u))
      .catch(() => {});

    return () => {
      un?.();
      if (timer.current) window.clearTimeout(timer.current);
      document.body.classList.remove("toast");
      document.documentElement.classList.remove("toast");
    };
  }, []);

  return (
    <div className="toast">
      <span className="toast-dot" />
      <span className="toast-text">{msg}</span>
    </div>
  );
}
