// §10 照片详情：大图/在线播放 + 操作栏（预览/收藏/重命名/分享导出/删除）。
// 加密分片等技术咨询折叠在底部（相册优先，技术隐藏）。
import { useEffect, useState } from "react";
import { api, BlobDetail, BlobEntry, formatBytes, formatDate } from "../lib/api";

/// §10 大图预览：1024px 加密缓存缩略图（thumbnail_large 命令，未命中时按需生成）。
function LargePreview({ blobHash }: { blobHash: string }) {
  const [src, setSrc] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .thumbnailLarge(blobHash)
      .then(setSrc)
      .catch(() => setError("大图生成失败（可能不是图片或源数据已损坏）"));
  }, [blobHash]);

  if (error) return <p className="hint">{error}</p>;
  if (!src) return <p className="hint">生成大图预览中…</p>;
  return <img src={src} alt="" style={{ maxWidth: "100%", borderRadius: 8 }} />;
}

const STORAGE_COLORS = ["#5b8def", "#7ed6a0", "#f0b45c", "#c792ea", "#ef8f6a", "#6ad0ef"];

export function BlobDetailModal({
  blob,
  onClose,
  onExport,
  onDeleted,
  onChanged,
  onPreview,
}: {
  blob: BlobEntry;
  onClose: () => void;
  onExport: (b: BlobEntry) => void;
  onDeleted: () => void;
  onChanged: (b: BlobEntry) => void;
  onPreview: (b: BlobEntry) => void;
}) {
  const [detail, setDetail] = useState<BlobDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [renaming, setRenaming] = useState(false);
  const [newName, setNewName] = useState(blob.file_name);
  const [favorite, setFavorite] = useState(blob.favorite);
  const [busy, setBusy] = useState(false);
  const [health, setHealth] = useState<string | null>(null);

  const doVerify = async () => {
    try {
      const r = await api.verifyBlob(blob.blob_hash);
      setHealth(
        r.ok
          ? `✔ 全部 ${r.shard_count} 个分片健康`
          : r.unrecoverable
            ? `✘ 不可恢复（可用 ${r.available}/${r.shard_count}）`
            : `⚠ 降级（可用 ${r.available}/${r.shard_count}），可点「修复」`
      );
    } catch (e) {
      setError(String(e));
    }
  };

  const doRepair = async () => {
    try {
      const r = await api.repairBlob(blob.blob_hash);
      setHealth(
        r.unrecoverable.length > 0
          ? `✘ ${r.unrecoverable.length} 个分片组损坏过重，无法自动修复`
          : `已修复 ${r.repaired.length} 个分片`
      );
    } catch (e) {
      setError(String(e));
    }
  };

  useEffect(() => {
    api
      .blobDetail(blob.blob_hash)
      .then(setDetail)
      .catch((e) => setError(String(e)));
  }, [blob.blob_hash]);

  const doRename = async () => {
    setBusy(true);
    try {
      await api.renameBlob(blob.blob_hash, newName);
      setRenaming(false);
      onChanged({ ...blob, file_name: newName });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const doFavorite = async () => {
    const next = !favorite;
    try {
      await api.setBlobFavorite(blob.blob_hash, next);
      setFavorite(next);
      onChanged({ ...blob, favorite: next });
    } catch (e) {
      setError(String(e));
    }
  };

  const doDelete = async () => {
    if (!window.confirm(`把「${blob.file_name}」移入回收站吗？\n\n碎片仍保留在 Storage，可在回收站恢复。`)) {
      return;
    }
    setBusy(true);
    try {
      await api.deleteBlob(blob.blob_hash);
      onDeleted();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };

  const doPreview = () => onPreview(blob);

  const storageIds = detail?.per_storage.map(([s]) => s) ?? [];
  const colorOf = (sid: string) =>
    STORAGE_COLORS[Math.max(0, storageIds.indexOf(sid)) % STORAGE_COLORS.length];

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <header className="modal-head">
          <span className="name" title={blob.file_name}>
            {favorite && <span style={{ color: "#f0b45c" }}>♥ </span>}
            {blob.file_name}
          </span>
          <span className="spacer" />
          <button onClick={onClose}>✕</button>
        </header>

        {error && <div className="notice error">{error} <button onClick={() => setError(null)}>✕</button></div>}
        {!detail && !error && <p className="hint">加载中…</p>}

        {detail && (
          <div className="detail-body">
            {detail.mime.startsWith("image/") && (
              <LargePreview blobHash={detail.blob_hash} />
            )}
            {detail.mime.startsWith("video/") && (
              <>
                <video
                  controls
                  style={{ width: "100%", borderRadius: 8, maxHeight: 400 }}
                  src={`heartshard-video://localhost/${detail.blob_hash}`}
                />
                <p className="hint">边下边播，无需等待完整下载。</p>
              </>
            )}

            <div className="wizard-actions" style={{ marginTop: 14, flexWrap: "wrap" }}>
              <button className="primary" onClick={doPreview}>
                预览
              </button>
              <button onClick={doFavorite}>{favorite ? "♥ 取消收藏" : "♡ 收藏"}</button>
              <button onClick={() => setRenaming(true)}>重命名</button>
              <button onClick={() => onExport(blob)}>下载 / 分享</button>
              <button onClick={doDelete} disabled={busy} style={{ color: "var(--destructive)" }}>
                删除
              </button>
            </div>

            {renaming && (
              <div className="settings-card">
                <label className="setting-row">
                  <span>新名称</span>
                  <input
                    value={newName}
                    autoFocus
                    onChange={(e) => setNewName(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && doRename()}
                  />
                </label>
                <button className="primary" onClick={doRename} disabled={busy || !newName.trim()}>
                  确定
                </button>
                <button style={{ marginLeft: 8 }} onClick={() => { setRenaming(false); setNewName(blob.file_name); }}>
                  取消
                </button>
              </div>
            )}

            <h3>信息</h3>
            <table className="meta-table">
              <tbody>
                <tr><td>名称</td><td>{detail.file_name}</td></tr>
                <tr><td>类型</td><td>{detail.mime}</td></tr>
                <tr><td>大小</td><td>{formatBytes(detail.total_len)}</td></tr>
                <tr><td>拍摄时间</td><td>{formatDate(detail.taken_at)}</td></tr>
              </tbody>
            </table>

            <details className="tech-details">
              <summary>高级详情（加密与分片）</summary>
              <div className="wizard-actions" style={{ marginBottom: 8 }}>
                <button onClick={doVerify}>体检</button>
                <button onClick={doRepair}>修复</button>
                {health && <span className="hint" style={{ alignSelf: "center", marginLeft: 8 }}>{health}</span>}
              </div>
              <table className="meta-table">
                <tbody>
                  <tr><td>分片策略</td><td>chunk {formatBytes(detail.chunk_size)} · n={detail.n} / m={detail.m}</td></tr>
                  <tr><td>chunk 数</td><td>{detail.chunks}</td></tr>
                  <tr><td>blob SHA-256</td><td><code title={detail.blob_hash}>{detail.blob_hash.slice(0, 16)}…</code></td></tr>
                </tbody>
              </table>

              <h3>分片分布（{detail.shards.length} 个 shard）</h3>
              <div className="legend">
                {detail.per_storage.map(([sid, count]) => (
                  <span key={sid} className="legend-item">
                    <span className="dot" style={{ background: colorOf(sid) }} />
                    {sid} × {count}
                  </span>
                ))}
              </div>
              <div className="shard-grid">
                {detail.shards.map((s) => (
                  <div
                    key={`${s.chunk_index}-${s.shard_index}`}
                    className="shard-cell"
                    style={{ background: colorOf(s.storage_id) }}
                    title={`chunk ${s.chunk_index} shard ${s.shard_index}\n${s.storage_id}\n${s.remote_key}\n${formatBytes(s.size)}`}
                  >
                    {s.chunk_index}·{s.shard_index}
                  </div>
                ))}
              </div>
              <p className="hint">
                恢复阈值 m={detail.m}：任意 {detail.m} 个分片即可还原整个文件。
              </p>
            </details>
          </div>
        )}
      </div>
    </div>
  );
}
