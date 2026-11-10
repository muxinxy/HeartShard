// 全屏照片查看器：←/→ 键与箭头切换、滚轮/双击缩放、拖拽平移、双指捏合缩放、
// 视频在线播放（heartshard-video:// Range 流式解密）、ℹ 详情入口。
import { useCallback, useEffect, useRef, useState } from "react";
import type {
  WheelEvent as ReactWheelEvent,
  MouseEvent as ReactMouseEvent,
  TouchEvent as ReactTouchEvent,
} from "react";
import { api, BlobEntry } from "../lib/api";

export function Lightbox({
  blobs,
  index,
  onClose,
  onNavigate,
  onInfo,
}: {
  blobs: BlobEntry[];
  index: number;
  onClose: () => void;
  onNavigate: (i: number) => void;
  onInfo: (b: BlobEntry) => void;
}) {
  const blob = blobs[index];
  const [src, setSrc] = useState<string | null>(null);
  const [playingMotion, setPlayingMotion] = useState(false);
  const [zoom, setZoom] = useState(1);
  const [off, setOff] = useState({ x: 0, y: 0 });
  const drag = useRef<{ x: number; y: number; ox: number; oy: number } | null>(null);
  const pinch = useRef<{ dist: number; zoom: number } | null>(null);

  // 切换照片：重置视图，加载 1024px 大图（加密缓存，未命中按需生成）
  useEffect(() => {
    setSrc(null);
    setZoom(1);
    setOff({ x: 0, y: 0 });
    setPlayingMotion(false);
    if (blob.mime.startsWith("image/")) {
      api
        .thumbnailLarge(blob.blob_hash)
        .then(setSrc)
        .catch(() => setSrc(null));
    }
  }, [blob.blob_hash, blob.mime]);

  const go = useCallback(
    (delta: number) => {
      const next = index + delta;
      if (next >= 0 && next < blobs.length) onNavigate(next);
    },
    [index, blobs.length, onNavigate]
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
      else if (e.key === "ArrowLeft") go(-1);
      else if (e.key === "ArrowRight") go(1);
      else if (e.key === "i" || e.key === "I") onInfo(blob);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [go, onClose, onInfo, blob]);

  const onWheel = (e: ReactWheelEvent) => {
    e.preventDefault();
    setZoom((z) => Math.min(6, Math.max(0.5, e.deltaY < 0 ? z * 1.2 : z / 1.2)));
  };

  const onMouseDown = (e: ReactMouseEvent) => {
    drag.current = { x: e.clientX, y: e.clientY, ox: off.x, oy: off.y };
  };
  const onMouseMove = (e: ReactMouseEvent) => {
    if (!drag.current) return;
    setOff({
      x: drag.current.ox + (e.clientX - drag.current.x),
      y: drag.current.oy + (e.clientY - drag.current.y),
    });
  };
  const onMouseUp = () => {
    drag.current = null;
  };

  const touchDist = (t: ReactTouchEvent["touches"]) =>
    Math.hypot(t[0].clientX - t[1].clientX, t[0].clientY - t[1].clientY);
  const onTouchStart = (e: ReactTouchEvent) => {
    if (e.touches.length === 2) {
      pinch.current = { dist: touchDist(e.touches), zoom };
      drag.current = null;
    } else if (e.touches.length === 1) {
      drag.current = { x: e.touches[0].clientX, y: e.touches[0].clientY, ox: off.x, oy: off.y };
    }
  };
  const onTouchMove = (e: ReactTouchEvent) => {
    if (e.touches.length === 2 && pinch.current) {
      const ratio = touchDist(e.touches) / pinch.current.dist;
      setZoom(Math.min(6, Math.max(0.5, pinch.current.zoom * ratio)));
    } else if (e.touches.length === 1 && drag.current) {
      setOff({
        x: drag.current.ox + (e.touches[0].clientX - drag.current.x),
        y: drag.current.oy + (e.touches[0].clientY - drag.current.y),
      });
    }
  };
  const onTouchEnd = (e: ReactTouchEvent) => {
    if (e.touches.length < 2) pinch.current = null;
    if (e.touches.length === 0) drag.current = null;
  };

  return (
    <div className="lightbox" onClick={onClose}>
      <div className="lightbox-top" onClick={(e) => e.stopPropagation()}>
        <span className="lightbox-title">
          {blob.favorite && <span style={{ color: "#f0b45c" }}>♥ </span>}
          {blob.file_name}
        </span>
        <span className="lightbox-pos">
          {index + 1} / {blobs.length}
        </span>
        <span className="spacer" />
        <button onClick={() => onInfo(blob)}>ℹ 详情</button>
        <button onClick={onClose}>✕</button>
      </div>

      <div
        className="lightbox-stage"
        onClick={(e) => e.stopPropagation()}
        onWheel={onWheel}
        onDoubleClick={() => setZoom((z) => (z === 1 ? 2 : 1))}
        onMouseDown={onMouseDown}
        onMouseMove={onMouseMove}
        onMouseUp={onMouseUp}
        onMouseLeave={onMouseUp}
        onTouchStart={onTouchStart}
        onTouchMove={onTouchMove}
        onTouchEnd={onTouchEnd}
      >
        {blob.mime.startsWith("video/") ? (
          <video controls src={`heartshard-video://localhost/${blob.blob_hash}`} />
        ) : src ? (
          <img
            src={src}
            alt=""
            draggable={false}
            style={{ transform: `translate(${off.x}px, ${off.y}px) scale(${zoom})` }}
          />
        ) : (
          <p className="hint">加载中…</p>
        )}
        {/* M5c 动态照片：照片上叠加「播放动态」按钮切换到配对视频段 */}
        {blob.is_motion && blob.motion_video_hash && (
          <button
            className="lb-motion-play"
            onClick={(e) => {
              e.stopPropagation();
              setPlayingMotion(true);
            }}
          >
            ▶ 播放动态
          </button>
        )}
        {playingMotion && blob.motion_video_hash && (
          <div
            className="lb-motion-overlay"
            onClick={(e) => {
              e.stopPropagation();
              setPlayingMotion(false);
            }}
          >
            <video
              controls
              autoPlay
              src={`heartshard-video://localhost/${blob.motion_video_hash}`}
            />
            <button className="lb-motion-close" onClick={() => setPlayingMotion(false)}>
              ✕ 返回照片
            </button>
          </div>
        )}
      </div>

      {index > 0 && (
        <button
          className="lb-arrow lb-left"
          onClick={(e) => {
            e.stopPropagation();
            go(-1);
          }}
        >
          ‹
        </button>
      )}
      {index < blobs.length - 1 && (
        <button
          className="lb-arrow lb-right"
          onClick={(e) => {
            e.stopPropagation();
            go(1);
          }}
        >
          ›
        </button>
      )}
    </div>
  );
}
