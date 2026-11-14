// 全屏查看器（网页端 #viewer 同款）：
// 顶栏：收藏/重命名/分享/详情/下载/移动到/复制到/回收站/关闭（+ 动态照片「播放动态」）
// 左下工具胶囊：＋/－/旋转/水平翻转/垂直翻转/1:1；滚轮缩放、拖拽平移、双指捏合、键盘。
import { useCallback, useEffect, useRef, useState } from "react";
import type {
  WheelEvent as ReactWheelEvent,
  MouseEvent as ReactMouseEvent,
  TouchEvent as ReactTouchEvent,
} from "react";
import { api, BlobEntry, platform } from "../lib/api";
import { usePhotoActions } from "./PhotoActions";

export function Lightbox({
  blobs,
  index,
  onClose,
  onNavigate,
}: {
  blobs: BlobEntry[];
  index: number;
  onClose: () => void;
  onNavigate: (i: number) => void;
}) {
  const actions = usePhotoActions();
  const blob = blobs[index];
  const [src, setSrc] = useState<string | null>(null);
  const [playingMotion, setPlayingMotion] = useState(false);
  const [zoom, setZoom] = useState(1);
  const [rotate, setRotate] = useState(0);
  const [flipH, setFlipH] = useState(false);
  const [flipV, setFlipV] = useState(false);
  const [off, setOff] = useState({ x: 0, y: 0 });
  const drag = useRef<{ x: number; y: number; ox: number; oy: number } | null>(null);
  const pinch = useRef<{ dist: number; zoom: number } | null>(null);

  useEffect(() => {
    setSrc(null);
    setZoom(1);
    setRotate(0);
    setFlipH(false);
    setFlipV(false);
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
      else if (e.key === "i" || e.key === "I") actions.detail(blob);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [go, onClose, actions, blob]);

  const onWheel = (e: ReactWheelEvent) => {
    e.preventDefault();
    setZoom((z) => Math.min(8, Math.max(0.2, e.deltaY < 0 ? z * 1.2 : z / 1.2)));
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
      setZoom(Math.min(8, Math.max(0.2, pinch.current.zoom * ratio)));
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

  const resetView = () => {
    setZoom(1);
    setRotate(0);
    setFlipH(false);
    setFlipV(false);
    setOff({ x: 0, y: 0 });
  };

  const transform = `translate(${off.x}px, ${off.y}px) scale(${zoom}) rotate(${rotate}deg) scaleX(${flipH ? -1 : 1}) scaleY(${flipV ? -1 : 1})`;

  return (
    <div className="lightbox" onClick={onClose}>
      <div className="lb-bar" onClick={(e) => e.stopPropagation()}>
        <span className="lb-title">{blob.file_name}</span>
        <span className="lightbox-pos">
          {index + 1} / {blobs.length}
        </span>
        <span style={{ flex: 1 }} />
        <button onClick={() => actions.fav(blob)}>{blob.favorite ? "已收藏" : "收藏"}</button>
        <button onClick={() => actions.rename(blob)}>重命名</button>
        {platform.isRemote && <button onClick={() => actions.share([blob.blob_hash])}>分享</button>}
        <button onClick={() => actions.detail(blob)}>详情</button>
        <button onClick={() => actions.downloadBlob(blob)}>下载</button>
        <button onClick={() => actions.pickAlbum([blob.blob_hash], "move")}>移动到</button>
        <button onClick={() => actions.pickAlbum([blob.blob_hash], "add")}>复制到</button>
        <button className="danger" onClick={() => actions.trash(blob)}>
          回收站
        </button>
        <button onClick={onClose}>✕</button>
      </div>

      <div
        className="lb-stage"
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
          <img src={src} alt="" draggable={false} style={{ transform }} />
        ) : (
          <p className="hint" style={{ color: "#888" }}>加载中…</p>
        )}

        {blob.is_motion && blob.motion_video_hash && (
          <button className="lb-motion-play" onClick={() => setPlayingMotion(true)}>
            ▶ 播放动态
          </button>
        )}
        {playingMotion && blob.motion_video_hash && (
          <div className="lb-motion-overlay" onClick={() => setPlayingMotion(false)}>
            <video controls autoPlay src={`heartshard-video://localhost/${blob.motion_video_hash}`} />
            <button className="lb-motion-close" onClick={() => setPlayingMotion(false)}>
              ✕ 返回照片
            </button>
          </div>
        )}

        <div className="lb-tools" onClick={(e) => e.stopPropagation()}>
          <button onClick={() => setZoom((z) => Math.min(8, z * 1.25))} title="放大">＋</button>
          <button onClick={() => setZoom((z) => Math.max(0.2, z / 1.25))} title="缩小">－</button>
          <button onClick={() => setRotate((r) => r + 90)} title="旋转">⟳</button>
          <button onClick={() => setFlipH((f) => !f)} title="水平翻转">⇋</button>
          <button onClick={() => setFlipV((f) => !f)} title="垂直翻转">⇵</button>
          <button onClick={resetView} title="重置">1:1</button>
        </div>
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
