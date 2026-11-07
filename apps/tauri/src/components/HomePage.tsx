// 首页：相册封面墙（Imnich 风格落地页）。
// 封面优先取相册设置的 cover_hash，未设置则取相册第一张照片。
import { useEffect, useState } from "react";
import { api, AlbumInfo } from "../lib/api";
import { Thumb } from "./Thumb";

export function HomePage({
  albums,
  total,
  latestHash,
  onOpen,
}: {
  albums: AlbumInfo[];
  total: number;
  latestHash: string | null;
  onOpen: (albumId: string | null) => void;
}) {
  const [covers, setCovers] = useState<Record<string, string>>({});

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const m: Record<string, string> = {};
      for (const a of albums) {
        if (a.cover_hash) {
          m[a.id] = a.cover_hash;
          continue;
        }
        const list = await api.listAlbumBlobs(a.id).catch(() => []);
        if (list.length > 0) m[a.id] = list[0].blob_hash;
      }
      if (!cancelled) setCovers(m);
    })();
    return () => {
      cancelled = true;
    };
  }, [albums]);

  return (
    <div className="gallery">
      <header className="toolbar">
        <div className="toolbar-row">
          <span className="brand">牧心相册</span>
        </div>
      </header>

      <div className="album-wall">
        <div className="album-card" onClick={() => onOpen(null)}>
          <div className="album-cover">
            {latestHash ? (
              <Thumb blobHash={latestHash} mime="image/jpeg" />
            ) : (
              <div className="thumb-placeholder">🖼</div>
            )}
          </div>
          <div className="album-card-name">
            全部照片<span className="album-card-count">{total}</span>
          </div>
        </div>

        {albums.map((a) => (
          <div key={a.id} className="album-card" onClick={() => onOpen(a.id)}>
            <div className="album-cover">
              {covers[a.id] ? (
                <Thumb blobHash={covers[a.id]} mime="image/jpeg" />
              ) : (
                <div className="thumb-placeholder">📁</div>
              )}
            </div>
            <div className="album-card-name">
              {a.name}
              <span className="album-card-count">{a.blob_count}</span>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
