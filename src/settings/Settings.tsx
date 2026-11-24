import React, { useEffect, useState } from "react";
import { ipc } from "../lib/ipc";
import type { Config } from "../lib/types";

export default function Settings() {
  const [cfg, setCfg] = useState<Config | null>(null);
  const [conflicts, setConflicts] = useState<string[]>([]);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    ipc.configGet().then(setCfg).catch(() => {});
  }, []);

  if (!cfg) return <div className="settings">加载中…</div>;

  const patch = (fn: (c: Config) => void) => {
    const next = JSON.parse(JSON.stringify(cfg)) as Config;
    fn(next);
    setCfg(next);
    setSaved(false);
  };

  const save = async () => {
    try {
      setConflicts(await ipc.configSet(cfg));
      setSaved(true);
    } catch (e) {
      console.error(e);
    }
  };

  return (
    <div className="settings">
      <h1>Look 录刻</h1>
      <div className="sub"></div>

      <div className="card">
        <h2>通用</h2>
        <div className="row">
          <label>开机自启</label>
          <input type="checkbox" checked={cfg.app.auto_start} onChange={(e) => patch((c) => (c.app.auto_start = e.target.checked))} />
          <span className="hint">开机后静默驻留托盘，不弹主窗口</span>
        </div>
      </div>

      <div className="card">
        <h2>输出</h2>
        <div className="row">
          <label>保存目录</label>
          <input type="text" value={cfg.output.dir} onChange={(e) => patch((c) => (c.output.dir = e.target.value))} />
          <button className="ghost" onClick={() => ipc.outputDirOpen().catch(() => {})}>打开</button>
        </div>
        <div className="row">
          <label>完成后自动复制</label>
          <input type="checkbox" checked={cfg.output.copy_after} onChange={(e) => patch((c) => (c.output.copy_after = e.target.checked))} />
        </div>
        <div className="row">
          <label>GIF 进剪贴板</label>
          <select value={cfg.output.gif_clipboard} onChange={(e) => patch((c) => (c.output.gif_clipboard = e.target.value))}>
            <option value="gif">GIF 动画（可直接粘贴进聊天工具）</option>
            <option value="file">文件路径</option>
          </select>
        </div>
      </div>

      <div className="card">
        <h2>录制</h2>
        <div className="row">
          <label>默认帧率</label>
          <input type="number" min={4} max={30} value={cfg.record.fps} onChange={(e) => patch((c) => (c.record.fps = Number(e.target.value)))} />
        </div>
        <div className="row">
          <label>最大宽度 (px)</label>
          <input type="number" min={320} max={2560} value={cfg.record.max_width} onChange={(e) => patch((c) => (c.record.max_width = Number(e.target.value)))} />
        </div>
        <div className="row">
          <label>色板</label>
          <select value={cfg.record.quality} onChange={(e) => patch((c) => (c.record.quality = e.target.value))}>
            <option value="low">小体积（128 色）</option>
            <option value="medium">均衡（192 色）</option>
            <option value="high">高清晰（256 色）</option>
          </select>
        </div>
        <div className="row">
          <label>最长时长 (秒)</label>
          <input type="number" min={5} max={3600} value={cfg.record.max_seconds} onChange={(e) => patch((c) => (c.record.max_seconds = Number(e.target.value)))} />
        </div>
        <div className="row">
          <label>录制光标</label>
          <input type="checkbox" checked={cfg.record.cursor} onChange={(e) => patch((c) => (c.record.cursor = e.target.checked))} />
        </div>
        <div className="row">
          <label>点击涟漪</label>
          <input type="checkbox" checked={cfg.record.click_ripple} onChange={(e) => patch((c) => (c.record.click_ripple = e.target.checked))} />
        </div>
        <div className="row">
          <label>键盘 HUD</label>
          <input type="checkbox" checked={cfg.record.key_hud} onChange={(e) => patch((c) => (c.record.key_hud = e.target.checked))} />
        </div>
      </div>

      <div className="card">
        <h2>快捷键</h2>
        <div className="row">
          <label>截图</label>
          <input type="text" value={cfg.hotkeys.shot} onChange={(e) => patch((c) => (c.hotkeys.shot = e.target.value))} />
        </div>
        <div className="row">
          <label>录屏</label>
          <input type="text" value={cfg.hotkeys.record} onChange={(e) => patch((c) => (c.hotkeys.record = e.target.value))} />
        </div>
      </div>

      <div className="actions">
        <button className="primary" onClick={save}>保存设置</button>
        <button className="ghost" onClick={() => ipc.appExit().catch(() => {})}>退出 Look</button>
      </div>

      {saved && <div className="hint">已保存。{conflicts.length ? `以下快捷键注册失败：${conflicts.join("、")}` : "快捷键已生效。"}</div>}
      <div className="hint">
        快捷键写法：Ctrl+Alt+A、Ctrl+Shift+1 …。框选后按 Tab 切换截图/录屏，Enter 确认，Esc 退出。<br />
        录制中按 Ctrl+Alt+R 结束并保存；录制完成后 GIF 已在剪贴板。<br />
        项目地址：https://github.com/lu-xianseng/look
      </div>
    </div>
  );
}
