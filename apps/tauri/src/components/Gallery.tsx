import { useCallback, useEffect, useMemo, useState } from "react";
import {
  api,
  BlobEntry,
  formatBytes,
  formatDate,
  ImportOutcome,
  shortHash,
  VerifyReport,
} from "../lib/api";

interface VerifyState {
  report: VerifyReport;
  at: number;
}

export function Gallery({ onLock }: { onLock: () => void }) {
  const [blobs, setBlobs] = useState<BlobEntry[]>([]);
  const [importing, setImporting] = useState(false);
  const [results, setResults] = useState<ImportOutcome[] | null>(null);
  const [verify, setVerify] = useState<Record<string, VerifyState>>({});
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setBlobs(await api.listBlobs());
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const doImport = async () => {
    setImporting(true);
    setError(null);
    setResults(null);
    try {
      const paths = await api.pickFiles();
      if (paths.length === 0) return;
      const outcomes = await api.importFiles(paths);
      setResults(outcomes);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setImporting(false);
    }
  };

  const doExport = async (blob: BlobEntry) => {
    setError(null);
    try {
      const dest = await api.pickSavePath(blob.file_name);
      if (!dest) return;
      await api.exportBlob(blob.blob_hash, dest);
      alert(`已还原并通过 SHA-256 校验：\n${dest}`);
    } catch (e) {
      setError(`导出失败：${String(e)}`);
    }
  };

  const doVerify = async (blob: BlobEntry) => {
    setError(null);
    try {
      const report = await api.verifyBlob(blob.blob_hash);
      setVerify((prev) => ({ ...prev, [blob.blob_hash]: { report, at: Date.now() } }));
    } catch (e) {
      setError(`校验失败：${String(e)}`);
    }
  };

  const doLock = async () => {
    await api.lock();
    onLock();
  };

  // 按拍摄/修改时间倒序分日分组（时间线雏形；M1.5 引入虚拟滚动）
  const groups = useMemo(() => {
    const map = new Map<string, BlobEntry[]>();
    for (const b of blobs) {
      const key = b.taken_at
        ? new Date(b.taken_at * 1000).toLocaleDateString()
        : "未知日期";
      const list = map.get(key) ?? [];
      list.push(b);
      map.set(key, list);
    }
    return [...map.entries()];
  }, [blobs]);

  return (
    <div className="gallery">
      <header className="toolbar">
        <span className="brand">HeartShard · 牧心相册</span>
        <span className="spacer" />
        <button onClick={doImport} disabled={importing}>
          {importing ? "导入中…" : "导入照片 / 视频"}
        </button>
        <button onClick={refresh}>刷新</button>
        <button onClick={doLock} className="secondary">
          锁定
        </button>
      </header>

      {results && (
        <div className="notice">
          <strong>本次导入：</strong>
          {results.map((r) => (
            <div key={r.path} className={`result ${r.status}`}>
              {r.status === "imported" && "✅ 已加密上传"}
              {r.status === "skipped_duplicate" && "⏭ 已存在（去重跳过）"}
              {r.status === "failed" && "❌ 失败"}
              <code title={r.path}>{r.path.split(/[\\/]/).pop()}</code>
              {r.error && <span className="error"> {r.error}</span>}
            </div>
          ))}
        </div>
      )}
      {error && <div className="notice error">{error}</div>}

      {blobs.length === 0 ? (
        <div className="empty">
          <p>相册还是空的。</p>
          <p className="hint">导入的照片会被切片、AES-256-GCM 加密并以 n=4/m=2 纠删冗余存到本地模拟 Storage。</p>
        </div>
      ) : (
        groups.map(([date, items]) => (
          <section key={date} className="day-group">
            <h2>{date}</h2>
            <ul>
              {items.map((b) => {
                const v = verify[b.blob_hash];
                return (
                  <li key={b.blob_hash} className="blob-row">
                    <div className="blob-main">
                      <span className="name" title={b.file_name}>
                        {b.file_name}
                      </span>
                      <span className="meta">
                        {formatBytes(b.total_len)} · {b.mime} · {formatDate(b.taken_at)} ·{" "}
                        <code title={b.blob_hash}>{shortHash(b.blob_hash)}</code>
                      </span>
                    </div>
                    <div className="blob-actions">
                      {v && (
                        <span
                          className={`badge ${v.report.ok ? "ok" : "bad"}`}
                          title={`shards: ${v.report.shard_count}, missing: ${v.report.missing.length}, corrupted: ${v.report.corrupted.length}`}
                        >
                          {v.report.ok
                            ? `✔ 冗余健康 (${v.report.shard_count} shards)`
                            : `✘ 缺失 ${v.report.missing.length} / 损坏 ${v.report.corrupted.length}`}
                        </span>
                      )}
                      <button onClick={() => doVerify(b)}>体检</button>
                      <button onClick={() => doExport(b)}>导出</button>
                    </div>
                  </li>
                );
              })}
            </ul>
          </section>
        ))
      )}
    </div>
  );
}
