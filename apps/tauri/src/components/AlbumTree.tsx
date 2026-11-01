// §10 相册树：列表 + 拖拽排序（HTML5 DnD）+ 封面设置 + 重命名。
import { useCallback, useEffect, useState } from "react";
import { api, AlbumInfo } from "../lib/api";

export function AlbumTree({ onDone }: { onDone: () => void }) {
  const [albums, setAlbums] = useState<AlbumInfo[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [dragId, setDragId] = useState<string | null>(null);
  const [dropTarget, setDropTarget] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setAlbums(await api.listAlbums());
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const doRename = async (albumId: string) => {
    if (!renameValue.trim()) return;
    setBusy(true);
    try {
      await api.albumRename(albumId, renameValue.trim());
      setRenaming(null);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const doCreate = async () => {
    const name = prompt("相册名称：");
    if (!name) return;
    setBusy(true);
    try {
      await api.albumCreate(name, null);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  // HTML5 拖拽：dragstart 记录源，drop 计算新序号并调用 albumSetSort
  const handleDrop = async (target: AlbumInfo) => {
    if (!dragId || dragId === target.id) return;
    const sorted = [...albums].sort((a, b) => a.sort_order - b.sort_order);
    const fromIdx = sorted.findIndex((a) => a.id === dragId);
    const toIdx = sorted.findIndex((a) => a.id === target.id);
    if (fromIdx < 0 || toIdx < 0) return;
    // 计算目标 sort_order（取相邻中值或首尾）
    const isAfter = fromIdx < toIdx;
    const prev = isAfter ? sorted[toIdx] : sorted[toIdx - 1];
    const next = isAfter ? sorted[toIdx + 1] : sorted[toIdx];
    const newSort = next
      ? Math.floor(((prev?.sort_order ?? 0) + next.sort_order) / 2)
      : sorted[sorted.length - 1].sort_order + 1;
    setBusy(true);
    try {
      await api.albumSetSort(dragId, newSort, null);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      setDragId(null);
      setDropTarget(null);
    }
  };

  const setCover = async (albumId: string) => {
    // 封面：取相册内第一个 blob 的 hash（从列表提示用户选择）
    // 简化实现：让用户输入 blob hash 前 8 位；M1.5 换图片选择器
    const hash = prompt("输入封面 blob 的 SHA-256（可在详情页复制完整 hash）：");
    if (!hash || hash.length !== 64) {
      if (hash !== null) setError("需要 64 位 SHA-256");
      return;
    }
    setBusy(true);
    try {
      await api.albumSetCover(albumId, hash);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const sorted = [...albums].sort((a, b) => a.sort_order - b.sort_order);

  return (
    <div className="wizard">
      <header className="toolbar">
        <span className="brand">相册树</span>
        <span className="spacer" />
        <button onClick={doCreate} disabled={busy}>新建相册</button>
        <button onClick={onDone}>返回</button>
      </header>
      {error && <div className="notice error">{error}</div>}
      {sorted.length === 0 ? (
        <div className="empty"><p>暂无相册。导入目录或手动创建。</p></div>
      ) : (
        <ul className="album-list">
          {sorted.map((a) => (
            <li
              key={a.id}
              className={`album-row ${dragId === a.id ? "dragging" : ""} ${dropTarget === a.id ? "drop-target" : ""}`}
              draggable={!busy && renaming !== a.id}
              onDragStart={() => setDragId(a.id)}
              onDragOver={(e) => { e.preventDefault(); setDropTarget(a.id); }}
              onDragEnd={() => { setDragId(null); setDropTarget(null); }}
              onDrop={(e) => { e.preventDefault(); handleDrop(a); }}
            >
              <span className="drag-handle">⠿</span>
              {renaming === a.id ? (
                <input
                  value={renameValue}
                  autoFocus
                  onChange={(e) => setRenameValue(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && doRename(a.id)}
                  onBlur={() => setRenaming(null)}
                />
              ) : (
                <span
                  className="name"
                  onDoubleClick={() => { setRenaming(a.id); setRenameValue(a.name); }}
                  title="双击重命名"
                >
                  {a.cover_hash ? "📸 " : ""}{a.name}
                </span>
              )}
              <span className="meta">{a.blob_count} 个文件</span>
              <div className="blob-actions">
                <button onClick={() => { setRenaming(a.id); setRenameValue(a.name); }}>重命名</button>
                <button onClick={() => setCover(a.id)} disabled={a.blob_count === 0}>设封面</button>
              </div>
            </li>
          ))}
        </ul>
      )}
      <p className="hint">拖拽排序 · 双击重命名 · 设封面需输入 blob 的 SHA-256</p>
    </div>
  );
}
