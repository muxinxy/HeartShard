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
      .thumbnail(blobHash)
      .then((s) => {
        if (!cancelled) setSrc(s);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [visible, blobHash]);

  const isImage = mime.startsWith("image/");
  return (
    <div ref={ref} className="thumb">
      {src ? (
        <img src={src} alt="" />
      ) : (
        <div className="thumb-placeholder">{isImage ? "🖼" : "📄"}</div>
      )}
    </div>
  );
}
