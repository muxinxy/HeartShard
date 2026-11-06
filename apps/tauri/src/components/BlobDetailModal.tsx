// §10 详情页：EXIF/元数据 + 分片分布可视化（每个 chunk×shard 一格，
// 按 Storage 着色）+ 健康徽章 + 导出/修复入口。
import { useEffect, useState } from "react";
import { api, BlobDetail, BlobEntry, formatBytes, formatDate } from "../lib/api";

/// §10 大图预览：1024px 加密缓存缩略图（thumbnail_large 命令）。
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
}: {
  blob: BlobEntry;
  onClose: () => void;
  onExport: (b: BlobEntry) => void;
}) {
  const [detail, setDetail] = useState<BlobDetail | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .blobDetail(blob.blob_hash)
      .then(setDetail)
      .catch((e) => setError(String(e)));
  }, [blob.blob_hash]);

  const storageIds = detail?.per_storage.map(([s]) => s) ?? [];
  const colorOf = (sid: string) =>
    STORAGE_COLORS[Math.max(0, storageIds.indexOf(sid)) % STORAGE_COLORS.length];

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <header className="modal-head">
          <span className="name">{blob.file_name}</span>
          <span className="spacer" />
          <button onClick={onClose}>✕</button>
        </header>

        {error && <div className="notice error">{error}</div>}
        {!detail && !error && <p className="hint">加载中…</p>}

        {detail && (
          <div className="detail-body">
            {detail.mime.startsWith("image/") && (
              <>
                <h3>大图预览</h3>
                <LargePreview blobHash={detail.blob_hash} />
              </>
            )}
            {detail.mime.startsWith("video/") && (
              <>
                <h3>加密视频在线播放</h3>
                <video
                  controls
                  style={{ width: "100%", borderRadius: 8, maxHeight: 400 }}
                  src={`heartshard-video://localhost/${detail.blob_hash}`}
                />
                <p className="hint">
                  流式解密播放（Range 分片拉取），不需要完整下载文件。
                </p>
              </>
            )}
            <h3>元数据（EXIF 字段，拍摄时间为文件时间）</h3>
            <table className="meta-table">
              <tbody>
                <tr><td>文件名</td><td>{detail.file_name}</td></tr>
                <tr><td>类型</td><td>{detail.mime}</td></tr>
                <tr><td>大小</td><td>{formatBytes(detail.total_len)}</td></tr>
                <tr><td>拍摄/修改时间</td><td>{formatDate(detail.taken_at)}</td></tr>
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
              恢复阈值 m={detail.n && detail.m ? detail.m : "?"}：任意 {detail.m} 个 shard 可还原整个文件。
            </p>

            <div className="wizard-actions">
              <button className="primary" onClick={() => onExport(blob)}>
                导出还原
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
