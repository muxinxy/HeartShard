// §11 懒加载缩略图：进入视口才请求（缓存命中→data URL；未命中→占位符）。
import { useEffect, useRef, useState } from "react";
import { api } from "../lib/api";

export function Thumb({ blobHash, mime }: { blobHash: string; mime: string }) {
  const [src, setSrc] = useState<string | null>(null);
  const [visible, setVisible] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          setVisible(true);
          io.disconnect();
        }
      },
      { rootMargin: "200px" }
    );
    io.observe(el);
    return () => io.disconnect();
  }, []);

  useEffect(() => {
    if (!visible) return;
    let cancelled = false;
    api
      .thumbnailUrl(blobHash)
      .then((s) => {
        if (!cancelled) setSrc(s);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [visible, blobHash]);

  const isImage = mime.startsWith("image/");
  const isVideo = mime.startsWith("video/");
  return (
    <div ref={ref} className="thumb">
      {isVideo ? (
        // 视频：预载首帧作缩略图（自定义协议支持 Range，#t=0.1 定位到开头）
        <video
          src={`heartshard-video://localhost/${blobHash}#t=0.1`}
          preload="metadata"
          muted
          playsInline
          style={{ width: "100%", height: "100%", objectFit: "cover" }}
        />
      ) : src ? (
        <img src={src} alt="" />
      ) : (
        <div className="thumb-placeholder"><svg className="ic" style={{ width: 22, height: 22 }}><use href={isImage ? "#i-image" : "#i-box"} /></svg></div>
      )}
    </div>
  );
}
