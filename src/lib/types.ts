export type Rect = { x: number; y: number; w: number; h: number };

export type MonitorInfo = {
  id: string;
  index: number;
  x: number;
  y: number;
  w: number;
  h: number;
  scale: number;
  primary: boolean;
};

export type MonitorShot = MonitorInfo & { path: string };

export type WinPeek = {
  hwnd: number;
  x: number;
  y: number;
  w: number;
  h: number;
  title: string;
  class: string;
};

export type OverlayBootstrap = {
  mode: "shot" | "record";
  shots: MonitorShot[];
  /** 启动时光标的物理坐标（x, y），用来决定中央提示落在哪块屏 */
  cursor: [number, number];
};

export type ShotResult = { path: string; bytes: number };

export type RecordResult = { path: string; bytes: number; duration_ms: number; frames: number; dropped: boolean };

export type RecordOpts = {
  x: number;
  y: number;
  w: number;
  h: number;
  fps?: number;
  max_width?: number;
  quality?: string;
  cursor?: boolean;
  ripple?: boolean;
  key_hud?: boolean;
  max_seconds?: number;
};

export type Config = {
  record: {
    fps: number;
    max_width: number;
    quality: string;
    cursor: boolean;
    click_ripple: boolean;
    key_hud: boolean;
    max_seconds: number;
  };
  output: { dir: string; copy_after: boolean; gif_clipboard: string };
  hotkeys: { shot: string; record: string };
  app: { language: string; magic_capture: boolean; auto_start: boolean };
};

export type Obj =
  | { id: string; type: "rect" | "ellipse"; x: number; y: number; w: number; h: number; color: string; width: number; fill?: string }
  | { id: string; type: "arrow"; x1: number; y1: number; x2: number; y2: number; color: string; width: number }
  | { id: string; type: "pen" | "marker"; pts: [number, number][]; color: string; width: number }
  | { id: string; type: "text"; x: number; y: number; text: string; size: number; color: string }
  | { id: string; type: "step"; x: number; y: number; n: number; color: string; r: number }
  | { id: string; type: "mosaic"; x: number; y: number; w: number; h: number; block: number; blur: boolean };
