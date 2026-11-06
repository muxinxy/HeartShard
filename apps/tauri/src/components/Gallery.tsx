// 时间线（Virtuoso 虚拟滚动 + 懒加载缩略图）+ 工具栏（导入/向导/同步/再平衡/
// 一键体检）+ 详情模态（分片分布）+ 历史报告重开。
import { useCallback, useEffect, useMemo, useState } from "react";
import { Virtuoso } from "react-virtuoso";
import {
  api,
  BlobEntry,
  formatBytes,
  formatDate,
  ImportOutcome,
  ImportRunReport,
  shortHash,
  StorageStatus,
  SyncOutcome,
  VerifyReport,
} from "../lib/api";
import { BlobDetailModal } from "./BlobDetailModal";
import { ImportWizard } from "./ImportWizard";
import { AlbumTree } from "./AlbumTree";
import { MapView } from "./MapView";
import { SettingsPage } from "./SettingsPage";
import { Thumb } from "./Thumb";
import { TaskPanel, TaskProvider, useTasks } from "./TaskPanel";

interface VerifyState {
  report: VerifyReport;
  at: number;
}

interface HealthSummary {
  total: number;
  ok: number;
  bad: number;
  results: VerifyReport[];
}

export function Gallery({ onLock }: { onLock: () => void }) {
  const tasks = useTasks();
  const [blobs, setBlobs] = useState<BlobEntry[]>([]);
  const [importing, setImporting] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [healthChecking, setHealthChecking] = useState(false);
  const [results, setResults] = useState<ImportOutcome[] | null>(null);
  const [syncResult, setSyncResult] = useState<SyncOutcome | null>(null);
  const [storages, setStorages] = useState<StorageStatus[]>([]);
  const [health, setHealth] = useState<HealthSummary | null>(null);
  const [verify, setVerify] = useState<Record<string, VerifyState>>({});
  const [error, setError] = useState<string | null>(null);
  const [wizardOpen, setWizardOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [albumTreeOpen, setAlbumTreeOpen] = useState(false);
  const [mapOpen, setMapOpen] = useState(false);
  const [detailBlob, setDetailBlob] = useState<BlobEntry | null>(null);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [history, setHistory] = useState<string[] | null>(null);
  const [historyReport, setHistoryReport] = useState<ImportRunReport | null>(null);

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
    const taskId = tasks.push("import", "单文件导入");
    await api.serviceStart().catch(() => {});
    try {
      const paths = await api.pickFiles();
      if (paths.length === 0) {
        tasks.finish(taskId, "done", "已取消");
        return;
      }
      tasks.update(taskId, { progress: `${paths.length} 个文件` });
      const outcomes = await api.importFiles(paths);
      setResults(outcomes);
      tasks.finish(taskId, "done", `${outcomes.filter((o) => o.status === "imported").length} 已上传`);
      await refresh();
    } catch (e) {
      tasks.finish(taskId, "failed", String(e));
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
    const taskId = tasks.push("sync", "多设备同步");
    try {
      const r = await api.syncNow();
      setSyncResult(r);
      tasks.finish(taskId, "done", `时钟 ${r.published_lamport} · 应用 ${r.applied_objects}`);
      await refresh();
    } catch (e) {
      tasks.finish(taskId, "failed", String(e));
      setError(`同步失败：${String(e)}`);
    } finally {
      setSyncing(false);
    }
  };

  const doRebalance = async () => {
    setSyncing(true);
    setError(null);
    const taskId = tasks.push("rebalance", "再平衡");
    try {
      const r = await api.rebalanceAll();
      tasks.finish(taskId, "done", `迁移 ${r.moved} 个 shard`);
      if (r.moved > 0) {
        alert(`再平衡完成：检查 ${r.blobs_checked} 个 blob，迁移 ${r.moved} 个 shard。`);
      } else if (r.failed.length > 0) {
        alert(`再平衡发现无法搬迁的 shard：\n${r.failed.join("\n")}`);
      } else {
        alert(`再平衡完成：检查 ${r.blobs_checked} 个 blob，布局已符合策略。`);
      }
      await refresh();
    } catch (e) {
      tasks.finish(taskId, "failed", String(e));
      setError(`再平衡失败：${String(e)}`);
    } finally {
      setSyncing(false);
    }
  };

  const doHealthCheck = async () => {
    setHealthChecking(true);
    setError(null);
    const taskId = tasks.push("health", "一键体检（全部 blob）");
    try {
      const results = await api.verifyAll();
      const ok = results.filter((r) => r.ok).length;
      setHealth({ total: results.length, ok, bad: results.length - ok, results });
      // 顺带刷新逐 blob 徽章
      const map: Record<string, VerifyState> = {};
      for (const r of results) map[r.blob_hash] = { report: r, at: Date.now() };
      setVerify((prev) => ({ ...prev, ...map }));
      tasks.finish(taskId, "done", `${ok}/${results.length} 健康`);
    } catch (e) {
      tasks.finish(taskId, "failed", String(e));
      setError(`体检失败：${String(e)}`);
    } finally {
      setHealthChecking(false);
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
    const taskId = tasks.push("repair", `修复 ${blob.file_name}`);
    try {
      const report = await api.repairBlob(blob.blob_hash);
      tasks.finish(
        taskId,
        report.unrecoverable.length > 0 ? "failed" : "done",
        report.unrecoverable.length > 0 ? "健康 shard 不足" : `修复 ${report.repaired.length} 个 shard`
      );
      if (report.repaired.length > 0) {
        alert(`已修复 ${report.repaired.length} 个 shard：\n${report.repaired.join("\n")}`);
      } else if (report.unrecoverable.length > 0) {
        alert(`chunk ${report.unrecoverable.join(", ")} 损坏过重，无法自动修复。`);
      } else {
        alert("一切正常，无需修复。");
      }
      const after = await api.verifyBlob(blob.blob_hash);
      setVerify((prev) => ({ ...prev, [blob.blob_hash]: { report: after, at: Date.now() } }));
    } catch (e) {
      tasks.finish(taskId, "failed", String(e));
      setError(`修复失败：${String(e)}`);
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

  const doLock = async () => {
    await api.lock();
    onLock();
  };

  const applyTheme = () => {
    api.getSettings().then((s) => {
      document.body.className = s.theme === "light" ? "theme-light" : "";
    }).catch(() => {});
  };

  const openHistory = async () => {
    const names = await api.importHistory().catch(() => []);
    setHistory(names.length > 0 ? names : []);
    setHistoryReport(null);
    setHistoryOpen(true);
  };

  const reopenReport = async (file: string) => {
    try {
      setHistoryReport(await api.importReportGet(file));
    } catch (e) {
      setError(`报告读取失败：${String(e)}`);
    }
  };

  // 按拍摄/修改时间倒序 + 日期分组（Virtuoso flat list + 组头 item）
  const items = useMemo(() => {
    const out: Array<{ type: "header"; key: string; label: string } | { type: "blob"; key: string; blob: BlobEntry }> = [];
    let lastDate = "";
    for (const b of blobs) {
      const key = b.taken_at ? new Date(b.taken_at * 1000).toLocaleDateString() : "未知日期";
      if (key !== lastDate) {
        out.push({ type: "header", key: `h-${key}`, label: key });
        lastDate = key;
      }
      out.push({ type: "blob", key: b.blob_hash, blob: b });
    }
    return out;
  }, [blobs]);

  const healthBadge = health
    ? `${health.ok}/${health.total} 健康`
    : null;

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

  if (settingsOpen) {
    return (
      <SettingsPage
        onDone={() => {
          setSettingsOpen(false);
          applyTheme();
          refresh();
        }}
      />
    );
  }

  if (albumTreeOpen) {
    return (
      <AlbumTree
        onDone={() => {
          setAlbumTreeOpen(false);
          refresh();
        }}
      />
    );
  }

  if (mapOpen) {
    return <MapView blobs={blobs} onDone={() => setMapOpen(false)} />;
  }

  return (
    <div className="gallery">
      <header className="toolbar">
        <span className="brand">HeartShard · 牧心相册</span>
        <span className="storages" title={storages.map((s) => `${s.id} (${s.kind})`).join(", ")}>
          {healthBadge ?? (storages.length > 0 ? `${storages.length} 个 Storage` : "")}
        </span>
        <span className="spacer" />
        <button onClick={doImport} disabled={importing}>
          {importing ? "导入中…" : "导入照片 / 视频"}
        </button>
        <button onClick={() => setWizardOpen(true)}>导入目录（向导）</button>
        <button onClick={() => setAlbumTreeOpen(true)}>相册树</button>
        <button onClick={() => setMapOpen(true)}>地图</button>
        <button onClick={() => setSettingsOpen(true)}>设置</button>
        <button onClick={doSync} disabled={syncing}>
          {syncing ? "同步中…" : "同步"}
        </button>
        <button onClick={doRebalance} disabled={syncing}>
          再平衡
        </button>
        <button onClick={doHealthCheck} disabled={healthChecking}>
          {healthChecking ? "体检中…" : "一键体检"}
        </button>
        <button onClick={openHistory}>历史</button>
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
      {syncResult && (
        <div className="notice">
          <strong>同步完成：</strong>
          时钟 {syncResult.published_lamport} · 拉取 {syncResult.pulled_objects} · 应用{" "}
          {syncResult.applied_objects}
          {syncResult.stores_failed.length > 0 && (
            <span className="error"> 失败：{syncResult.stores_failed.join("; ")}</span>
          )}
        </div>
      )}
      {health && health.bad > 0 && (
        <div className="notice error">
          一键体检：{health.bad} 个 blob 存在缺失/损坏 shard——可逐个「修复」。
        </div>
      )}
      {error && <div className="notice error">{error}</div>}

      {historyOpen && (
        <div className="notice">
          <strong>导入历史报告（点击重开）：</strong>
          {history && history.length > 0 ? (
            history.map((f) => (
              <div key={f}>
                <button className="link" onClick={() => reopenReport(f)}>
                  {f}
                </button>
              </div>
            ))
          ) : (
            <p className="hint">暂无历史报告。</p>
          )}
          {historyReport && (
            <div className="report">
              <li>会话 {historyReport.session_id}</li>
              <li>新建相册 {historyReport.albums_created}（复用 {historyReport.albums_reused}）</li>
              <li>上传 {historyReport.imported} · 去重 {historyReport.skipped_duplicate} · 失败 {historyReport.failed.length}</li>
              {historyReport.paused && <li className="error">该次导入已暂停，可再次向导续传</li>}
            </div>
          )}
          <button onClick={() => setHistoryOpen(false)}>收起</button>
        </div>
      )}

      {blobs.length === 0 ? (
        <div className="empty">
          <p>相册还是空的。</p>
          <p className="hint">导入的照片会被切片、AES-256-GCM 加密并以 n=4/m=2 纠删冗余存储。</p>
        </div>
      ) : (
        <Virtuoso
          style={{ height: "calc(100vh - 64px)" }}
          totalCount={items.length}
          itemContent={(index: number) => {
            const item = items[index];
            if (item.type === "header") {
              return (
                <h2 className="day-header" style={{ margin: "18px 0 8px", color: "#9aa0aa", fontSize: 14 }}>
                  {item.label}
                </h2>
              );
            }
            const b = item.blob;
            const v = verify[b.blob_hash];
            return (
              <div
                className="blob-row"
                style={{ cursor: "pointer" }}
                onClick={() => setDetailBlob(b)}
              >
                <Thumb blobHash={b.blob_hash} mime={b.mime} />
                <div className="blob-main">
                  <span className="name" title={b.file_name}>
                    {b.file_name}
                  </span>
                  <span className="meta">
                    {formatBytes(b.total_len)} · {b.mime} · {formatDate(b.taken_at)} ·{" "}
                    <code title={b.blob_hash}>{shortHash(b.blob_hash)}</code>
                  </span>
                </div>
                <div className="blob-actions" onClick={(e) => e.stopPropagation()}>
                  {v && (
                    <span
                      className={`badge ${v.report.ok ? "ok" : "bad"}`}
                      title={`shards: ${v.report.shard_count}, missing: ${v.report.missing.length}, corrupted: ${v.report.corrupted.length}`}
                    >
                      {v.report.ok
                        ? `✔ (${v.report.shard_count} shards)`
                        : `✘ 缺 ${v.report.missing.length} / 坏 ${v.report.corrupted.length}`}
                    </span>
                  )}
                  <button onClick={() => doVerify(b)}>体检</button>
                  <button onClick={() => doRepair(b)}>修复</button>
                  <button onClick={() => doExport(b)}>导出</button>
                </div>
              </div>
            );
          }}
        />
      )}

      {detailBlob && (
        <BlobDetailModal
          blob={detailBlob}
          onClose={() => setDetailBlob(null)}
          onExport={doExport}
        />
      )}
    </div>
  );
}

/// 挂 TaskProvider（任务中心需要 context）。
export function GalleryWithTasks({ onLock }: { onLock: () => void }) {
  return (
    <TaskProvider>
      <Gallery onLock={onLock} />
      <TaskPanel />
    </TaskProvider>
  );
}
