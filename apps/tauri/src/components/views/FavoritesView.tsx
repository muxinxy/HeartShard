// 收藏夹（网页端 viewFavorites 同款）：favorite 过滤 + 时间线渲染。
import { useEffect, useMemo, useState } from "react";
import { api, BlobEntry } from "../../lib/api";
import { PhotoGrid } from "../PhotoGrid";
import { usePhotoActions } from "../PhotoActions";

export function FavoritesView() {
  const actions = usePhotoActions();
  const [blobs, setBlobs] = useState<BlobEntry[]>([]);

  useEffect(() => {
    api
      .listBlobs()
      .then((all) => setBlobs(all.filter((b) => b.favorite).sort((a, b) => b.taken_at - a.taken_at)))
      .catch(() => {});
  }, [actions.dataVersion]);

  const indexed = useMemo(() => blobs, [blobs]);

  return (
    <div>
      <div className="view-head">
        <h2>收藏夹</h2>
        <span className="badge">{indexed.length} 张</span>
      </div>
      <PhotoGrid
        blobs={indexed}
        onOpen={(_b, idx) => actions.openViewer(indexed, idx)}
        onCtx={(e, b) => {
          e.preventDefault();
          actions.detail(b);
        }}
      />
    </div>
  );
}
