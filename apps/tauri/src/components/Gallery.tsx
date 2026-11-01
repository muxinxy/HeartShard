import { useCallback, useEffect, useMemo, useState } from "react";
import {
  api,
  BlobEntry,
  formatBytes,
  formatDate,
  ImportOutcome,
  RebalanceReport,
  shortHash,
  StorageStatus,
  SyncOutcome,
  VerifyReport,
} from "../lib/api";
import { ImportWizard } from "./ImportWizard";

interface VerifyState {
  report: VerifyReport;
  at: number;
}

export function Gallery({ onLock }: { onLock: () => void }) {
  const [blobs, setBlobs] = useState<BlobEntry[]>([]);
  const [importing, setImporting] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [results, setResults] = useState<ImportOutcome[] | null>(null);
  const [syncResult, setSyncResult] = useState<SyncOutcome | null>(null);
  const [storages, setStorages] = useState<StorageStatus[]>([]);
  const [verify, setVerify] = useState<Record<string, VerifyState>>({});
  const [error, setError] = useState<string | null>(null);
  const [wizardOpen, setWizardOpen] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setBlobs(await api.listBlobs());
      setStorages(await api.storageStatus());
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
    // M3 后台传输：批量导入期间保持前台服务，防止系统回收进程（桌面端静默成功）
    await api.serviceStart().catch(() => {});
    try {
      const paths = await api.pickFiles();
      if (paths.length === 0) return;
      const outcomes = await api.importFiles(paths);
      setResults(outcomes);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      await api.serviceStop().catch(() => {});
      setImporting(false);
    }
  };

  const doSync = async () => {
    setSyncing(true);
    setError(null);
    setSyncResult(null);
    try {
      setSyncResult(await api.syncNow());
      await refresh();
    } catch (e) {
      setError(`同步失败：${String(e)}`);
    } finally {
      setSyncing(false);
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

  const doRepair = async (blob: BlobEntry) => {
    setError(null);
    try {
      const report = await api.repairBlob(blob.blob_hash);
      if (report.repaired.length > 0) {
        alert(`已修复 ${report.repaired.length} 个 shard：\n${report.repaired.join("\n")}`);
      } else if (report.unrecoverable.length > 0) {
        alert(`chunk ${report.unrecoverable.join(", ")} 损坏过重，无法自动修复（健康 shard 不足）。`);
      } else {
        alert("一切正常，无需修复。");
      }
      const after = await api.verifyBlob(blob.blob_hash);
      setVerify((prev) => ({ ...prev, [blob.blob_hash]: { report: after, at: Date.now() } }));
    } catch (e) {
      setError(`修复失败：${String(e)}`);
    }
  };

  const doLock = async () => {
    await api.lock();
    onLock();
  };

  const doRebalance = async () => {
    setSyncing(true);
    setError(null);
    try {
      const report: RebalanceReport = await api.rebalanceAll();
      if (report.moved > 0) {
        alert(`再平衡完成：检查 ${report.blobs_checked} 个 blob，迁移 ${report.moved} 个 shard。`);
      } else if (report.failed.length > 0) {
        alert(`再平衡发现无法搬迁的 shard：\n${report.failed.join("\n")}`);
      } else {
        alert(`再平衡完成：检查 ${report.blobs_checked} 个 blob，布局已符合策略，无需迁移。`);
      }
      await refresh();
    } catch (e) {
      setError(`再平衡失败：${String(e)}`);
    } finally {
      setSyncing(false);
    }
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

  if (wizardOpen) {
    return (
      <ImportWizard
        onDone={() => {
          setWizardOpen(false);
          refresh();
        }}
      />
    );
  }

  return (
    <div className="gallery">
      <header className="toolbar">
        <span className="brand">HeartShard · 牧心相册</span>
        <span className="storages" title={storages.map((s) => `${s.id} (${s.kind})`).join(", ")}>
          {storages.length > 0 ? `${storages.length} 个 Storage` : ""}
        </span>
        <span className="spacer" />
        <button onClick={doImport} disabled={importing}>
          {importing ? "导入中…" : "导入照片 / 视频"}
        </button>
        <button onClick={() => setWizardOpen(true)}>导入目录（向导）</button>
        <button onClick={doSync} disabled={syncing}>
          {syncing ? "同步中…" : "同步"}
        </button>
        <button onClick={doRebalance} disabled={syncing}>
          再平衡
        </button>
        <button onClick={refresh}>刷新</button>
        <button onClick={doLock} className="secondary">
          锁定
        </button>
      </header>

      {syncResult && (
        <div className="notice">
          <strong>同步完成：</strong>
          时钟 {syncResult.published_lamport} · 拉取 {syncResult.pulled_objects} · 应用{" "}
          {syncResult.applied_objects} · 发布到{" "}
          {syncResult.stores_ok.join(", ") || "（无）"}
          {syncResult.stores_failed.length > 0 && (
            <span className="error"> 失败：{syncResult.stores_failed.join("; ")}</span>
          )}
        </div>
      )}

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
                      <button onClick={() => doRepair(b)}>修复</button>
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
