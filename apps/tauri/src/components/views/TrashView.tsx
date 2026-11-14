// 回收站（网页端 viewTrash 同款）：表格 + 缩略图 + 恢复 / 彻底删除。
import { useCallback, useEffect, useState } from "react";
import { api, BlobEntry } from "../../lib/api";
import { usePhotoActions } from "../PhotoActions";
import { formatBytes } from "../../lib/api";
import { Ic } from "../icons";

export function TrashView() {
  const actions = usePhotoActions();
  const [items, setItems] = useState<BlobEntry[]>([]);

  const reload = useCallback(() => {
    api
      .listTrashed()
      .then(setItems)
      .catch(() => setItems([]));
  }, []);

  useEffect(() => {
    reload();
  }, [actions.dataVersion, reload]);

  const restore = async (b: BlobEntry) => {
    try {
      await api.restoreBlob(b.blob_hash);
      actions.onChanged();
    } catch {
      actions.onChanged();
    }
  };

  const purge = (b: BlobEntry) => {
    actions.confirmAsk("彻底删除", `彻底删除「${b.file_name}」？此操作无法恢复。`, () => {
      api.purgeBlob(b.blob_hash).then(actions.onChanged).catch(() => {});
    }, true);
  };

  return (
    <div>
      <div className="view-head">
        <h2>回收站</h2>
        <span className="badge">{items.length} 张</span>
        <span className="hint">回收站中的照片碎片仍占用存储；彻底删除后才释放。</span>
      </div>
      {items.length === 0 ? (
        <p className="hint" style={{ marginTop: 60, textAlign: "center" }}>
          回收站是空的。
        </p>
      ) : (
        <table className="admin">
          <thead>
            <tr>
              <th style={{ width: 60 }}></th>
              <th>名称</th>
              <th>大小</th>
              <th>拍摄时间</th>
              <th style={{ width: 180 }}>操作</th>
            </tr>
          </thead>
          <tbody>
            {items.map((b) => (
              <tr key={b.blob_hash}>
                <td>
                  <div style={{ cursor: "pointer" }} onClick={() => actions.detail(b)}>
                    <Thumb40 hash={b.blob_hash} mime={b.mime} />
                  </div>
                </td>
                <td>
                  {b.mime.startsWith("video/") && <Ic name="i-video" />} {b.file_name}
                </td>
                <td>{formatBytes(b.total_len)}</td>
                <td>{b.taken_at ? new Date(b.taken_at * 1000).toLocaleString() : "—"}</td>
                <td>
                  <button onClick={() => restore(b)}>恢复</button>{" "}
                  <button className="danger" onClick={() => purge(b)}>
                    彻底删除
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

function Thumb40({ hash, mime }: { hash: string; mime: string }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    api.thumbnailUrl(hash).then(setSrc).catch(() => {});
  }, [hash]);
  return src ? (
    <img src={src} alt="" />
  ) : (
    <div style={{ width: 44, height: 44, background: "var(--input)", borderRadius: 8, display: "grid", placeItems: "center" }}>
      {mime.startsWith("video/") ? "🎬" : "🖼"}
    </div>
  );
}
