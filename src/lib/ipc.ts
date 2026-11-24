import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import type { Config, MonitorShot, OverlayBootstrap, Rect, RecordOpts, RecordResult, ShotResult, WinPeek } from "./types";

export const ipc = {
  configGet: () => invoke<Config>("config_get"),
  configSet: (cfg: Config) => invoke<string[]>("config_set", { cfg }),
  overlayBootstrap: () => invoke<OverlayBootstrap>("overlay_bootstrap"),
  overlayReady: () => invoke<void>("overlay_ready"),
  frameB64: (index: number) => invoke<string>("frame_b64", { index }),
  windowAt: (x: number, y: number) => invoke<WinPeek | null>("window_at", { x, y }),
  captureClose: () => invoke<void>("capture_close"),
  toastText: () => invoke<string>("toast_text"),
  toastReady: () => invoke<void>("toast_ready"),
  toastClose: () => invoke<void>("toast_close"),
  openCaptureUi: (mode: "shot" | "record") => invoke<void>("open_capture_ui", { mode }),
  shotExport: (pngB64: string, copy: boolean, save: boolean, region: Rect | null) =>
    invoke<ShotResult>("shot_export", { pngB64, copy, save, region }),
  pinPng: (pngB64: string) => invoke<string>("pin_png", { pngB64 }),
  ocrText: (pngB64: string) => invoke<string>("ocr_text", { pngB64 }),
  scrollStart: (rect: Rect) => invoke<void>("scroll_start", { rect }),
  scrollFinish: (copy: boolean, save: boolean) => invoke<string>("scroll_finish", { copy, save }),
  scrollCancel: () => invoke<void>("scroll_cancel"),
  recordBegin: (opts: RecordOpts) => invoke<void>("record_begin", { opts }),
  recordStop: () => invoke<RecordResult | null>("record_stop"),
  recordCancel: () => invoke<void>("record_cancel"),
  recordSetPause: (paused: boolean) => invoke<void>("record_set_pause", { paused }),
  recordStatus: () => invoke<{ recording: boolean; paused: boolean }>("record_status"),
  outputDirOpen: () => invoke<void>("output_dir_open"),
  settingsShow: () => invoke<void>("settings_show"),
  appExit: () => invoke<void>("app_exit"),
  uiLog: (msg: string) => invoke<void>("ui_log", { msg }).catch(() => undefined),
};

export const assetUrl = (p: string) => convertFileSrc(p);

export type { MonitorShot };
