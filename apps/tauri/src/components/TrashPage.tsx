// 回收站：墓碑照片列表，可恢复或彻底清除碎片。
import { useCallback, useEffect, useState } from "react";
import { api, BlobEntry, formatBytes, formatDate } from "../lib/api";

export function TrashPage({ onDone }: { onDone: () => void }) {
  const [items, setItems] = useState<BlobEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setItems(await api.listTrashed());
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const doPreview = async (b: BlobEntry) => {
    setError(null);
    try {
      const result = await api.previewBlob(b.blob_hash);
      await api.openPathSystem(result.path);
    } catch (e) {
      setError(String(e));
    }
  };

  const doRestore = async (b: BlobEntry) => {
    setBusy(true);
    try {
      await api.restoreBlob(b.blob_hash);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const doPurge = async (b: BlobEntry) => {
    if (!window.confirm(`彻底删除「${b.file_name}」？\n\n将从所有 Storage 清除加密碎片，之后无法恢复（记录保留墓碑防止其他设备同步复活）。`)) {
      return;
    }
    setBusy(true);
    try {
      await api.purgeBlob(b.blob_hash);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="wizard">
      <header className="toolbar">
        <span className="brand">回收站</span>
        <span className="spacer" />
        <button onClick={onDone}>返回</button>
      </header>

      {error && <div className="notice error">{error} <button onClick={() => setError(null)}>✕</button></div>}

      {items.length === 0 ? (
        <div className="empty">
          <p>回收站是空的。</p>
          <p className="hint">删除的照片会先移入这里（碎片保留），确认后才彻底清除。</p>
        </div>
      ) : (
        <ul className="album-list">
          {items.map((b) => (
            <li key={b.blob_hash} className="blob-row">
              <div className="blob-main">
                <span className="name">{b.file_name}</span>
                <span className="meta">
                  {formatBytes(b.total_len)} · {formatDate(b.taken_at)}
                </span>
              </div>
              <div className="blob-actions">
                <button onClick={() => doPreview(b)} disabled={busy}>预览</button>
                <button onClick={() => doRestore(b)} disabled={busy}>恢复</button>
                <button onClick={() => doPurge(b)} disabled={busy} style={{ color: "var(--destructive)" }}>
                  彻底删除
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
