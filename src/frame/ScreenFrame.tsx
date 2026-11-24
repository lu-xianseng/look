/**
 * 区域边框：一个点击穿透的透明窗口，只负责画一圈虚线。
 * 边框画在 region 外侧 2px，抓帧只抓 region 内的像素，所以不会进画面。
 * - rec：红色，录制进行中
 * - scroll：蓝色，滚动截长图进行中（让用户看清自己正在截哪一块）
 */
import { useEffect } from "react";

export default function ScreenFrame({ tone = "rec" }: { tone?: "rec" | "scroll" }) {
  useEffect(() => {
    // 透明窗口不能有任何页面底色
    document.documentElement.classList.add("frame");
    document.body.classList.add("frame");
  }, []);

  return (
    <div className={tone === "scroll" ? "rec-frame sc" : "rec-frame"}>
      <div className="rec-frame-edge" />
      <div className="rec-frame-corner tl" />
      <div className="rec-frame-corner tr" />
      <div className="rec-frame-corner bl" />
      <div className="rec-frame-corner br" />
    </div>
  );
}
