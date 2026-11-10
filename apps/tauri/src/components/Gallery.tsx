// 时间线/网格（Virtuoso 虚拟滚动 + 懒加载缩略图）+ 工具栏（导入/向导/同步/再平衡/
// 一键体检）+ 相册选择与上传 + 拖拽导入 + 详情模态（分片分布）+ 历史报告重开。
import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type MouseEvent as ReactMouseEvent,
} from "react";
import { Virtuoso, VirtuosoGrid } from "react-virtuoso";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import {
  api,
  platform,
  AlbumInfo,
  BlobEntry,
  formatBytes,
  formatDate,
  ImportOutcome,
  ImportRunReport,
} from "../lib/api";
import { useRef } from "react";
import { BlobDetailModal } from "./BlobDetailModal";
import { ImportWizard } from "./ImportWizard";
import { AlbumTree } from "./AlbumTree";
import { MapView } from "./MapView";
import { SettingsPage } from "./SettingsPage";
import { StoragePage } from "./StoragePage";
import { TrashPage } from "./TrashPage";
import { HomePage } from "./HomePage";
import { Lightbox } from "./Lightbox";
import { Thumb } from "./Thumb";
import { TaskPanel, TaskProvider, useTasks } from "./TaskPanel";
import { useToast } from "./Toast";

export function Gallery({ onLock }: { onLock: () => void }) {
  const toast = useToast();
  const tasks = useTasks();
  const [blobs, setBlobs] = useState<BlobEntry[]>([]);
  const [importing, setImporting] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [healthChecking, setHealthChecking] = useState(false);
  const [results, setResults] = useState<ImportOutcome[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [wizardOpen, setWizardOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [storageOpen, setStorageOpen] = useState(false);
  const [albumTreeOpen, setAlbumTreeOpen] = useState(false);
  const [mapOpen, setMapOpen] = useState(false);
  const [detailBlob, setDetailBlob] = useState<BlobEntry | null>(null);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [history, setHistory] = useState<string[] | null>(null);
  const [historyReport, setHistoryReport] = useState<ImportRunReport | null>(null);
  const [albums, setAlbums] = useState<AlbumInfo[]>([]);
  const [selectedAlbum, setSelectedAlbum] = useState<string>("");
  const [albumBlobs, setAlbumBlobs] = useState<BlobEntry[]>([]);
  const [viewMode, setViewMode] = useState<"grid" | "list">("grid");
  const [favOnly, setFavOnly] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [trashOpen, setTrashOpen] = useState(false);
  // 首页（相册封面墙）/ 全屏查看器 / 多选模式
  const [homeOpen, setHomeOpen] = useState(true);
  const [lightboxIdx, setLightboxIdx] = useState<number | null>(null);
  const [selectMode, setSelectMode] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const listRef = useRef<any>(null);
  const gridRef = useRef<any>(null);
  // 右键菜单 / 相册选择弹窗（可批量）/ 重命名弹窗
  const [ctxMenu, setCtxMenu] = useState<{ x: number; y: number; blob: BlobEntry } | null>(null);
  const [albumPickerHashes, setAlbumPickerHashes] = useState<string[] | null>(null);
  const [renameModal, setRenameModal] = useState<BlobEntry | null>(null);
  const [renameVal, setRenameVal] = useState("");

  const refresh = useCallback(async () => {
    try {
      setBlobs(await api.listBlobs());
      setAlbums(await api.listAlbums());
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // 切换相册 → 加载该相册内容
  useEffect(() => {
    if (!selectedAlbum) {
      setAlbumBlobs([]);
      return;
    }
    api
      .listAlbumBlobs(selectedAlbum)
      .then(setAlbumBlobs)
      .catch((e) => setError(String(e)));
  }, [selectedAlbum]);

  const doImportPaths = async (paths: string[], albumId: string | null) => {
    if (paths.length === 0) return;
    setImporting(true);
    setError(null);
    setResults(null);
    const taskId = tasks.push("import", albumId ? "导入到相册" : "单文件导入");
    await api.serviceStart().catch(() => {});
    try {
      tasks.update(taskId, { progress: `${paths.length} 个文件` });
      const outcomes = await api.importFiles(paths);
      let added = 0;
      if (albumId) {
        for (const o of outcomes) {
          if (o.status === "imported" && o.blob_hash) {
            await api.albumAddBlob(albumId, o.blob_hash).catch(() => {});
            added++;
          }
        }
      }
      setResults(outcomes);
      tasks.finish(
        taskId,
        "done",
        `${outcomes.filter((o) => o.status === "imported").length} 已上传${added > 0 ? ` · ${added} 张已入相册` : ""}`
      );
      await refresh();
      if (albumId) {
        setAlbumBlobs(await api.listAlbumBlobs(albumId));
      }
    } catch (e) {
      tasks.finish(taskId, "failed", String(e));
      setError(String(e));
    } finally {
      await api.serviceStop().catch(() => {});
      setImporting(false);
    }
  };

  const doImport = async () => {
    const paths = await api.pickFiles();
    await doImportPaths(paths, null);
  };

  const doUploadToAlbum = async () => {
    if (!selectedAlbum) return;
    const paths = await api.pickFiles();
    await doImportPaths(paths, selectedAlbum);
  };

  // 拖拽导入：媒体文件拖进窗口即导入（当前选中相册则入相册）
  useEffect(() => {
    const un = getCurrentWebviewWindow().onDragDropEvent((ev) => {
      if (ev.payload.type !== "drop") return;
      const media = ev.payload.paths.filter((p) =>
        /\.(jpe?g|png|gif|webp|bmp|heic|heif|avif|tiff?|mp4|mov|m4v|avi|mkv|webm)$/i.test(p)
      );
      if (media.length > 0) doImportPaths(media, selectedAlbum || null);
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedAlbum]);

  const doSync = async () => {
    setSyncing(true);
    setError(null);
    const taskId = tasks.push("sync", "多设备同步");
    try {
      const r = await api.syncNow();
      toast.success(`同步完成：拉取 ${r.pulled_objects} · 应用 ${r.applied_objects}`);
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
        toast.success(`再平衡完成：迁移 ${r.moved} 个 shard`);
      } else if (r.failed.length > 0) {
        toast.error(`${r.failed.length} 个 shard 无法搬迁，请先体检+修复`);
      } else {
        toast.info(`再平衡完成：布局已符合策略`);
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
    const taskId = tasks.push("health", "一键体检（全部照片）");
    try {
      const results = await api.verifyAll();
      const ok = results.filter((r) => r.ok).length;
      const bad = results.length - ok;
      if (bad > 0) {
        toast.error(`体检：${bad} 张照片存在缺失/损坏分片，可在照片详情里修复`);
      } else {
        toast.success(`体检：全部 ${results.length} 张照片健康`);
      }
      tasks.finish(taskId, "done", `${ok}/${results.length} 健康`);
    } catch (e) {
      tasks.finish(taskId, "failed", String(e));
      setError(`体检失败：${String(e)}`);
    } finally {
      setHealthChecking(false);
    }
  };

  const doExport = async (blob: BlobEntry) => {
    setError(null);
    try {
      const dest = await api.pickSavePath(blob.file_name);
      if (!dest) return;
      await api.exportBlob(blob.blob_hash, dest);
      toast.success(`已还原并通过 SHA-256 校验：${dest}`);
    } catch (e) {
      setError(`导出失败：${String(e)}`);
    }
  };

  const doPreview = async (blob: BlobEntry) => {
    setError(null);
    try {
      const result = await api.previewBlob(blob.blob_hash);
      await api.openPathSystem(result.path);
    } catch (e) {
      setError(`预览失败：${String(e)}`);
    }
  };

  const doLock = async () => {
    await api.lock();
    onLock();
  };

  // 刷新主列表 + 当前相册内容
  const reloadAll = useCallback(async () => {
    await refresh();
    if (selectedAlbum) {
      const list = await api.listAlbumBlobs(selectedAlbum).catch(() => []);
      setAlbumBlobs(list);
    }
  }, [refresh, selectedAlbum]);

  // ---------------- 右键菜单 ----------------
  const openCtx = (e: ReactMouseEvent, blob: BlobEntry) => {
    e.preventDefault();
    e.stopPropagation();
    setCtxMenu({
      x: Math.min(e.clientX, window.innerWidth - 200),
      y: Math.min(e.clientY, window.innerHeight - 340),
      blob,
    });
  };

  const ctxToggleFav = async (b: BlobEntry) => {
    setCtxMenu(null);
    try {
      await api.setBlobFavorite(b.blob_hash, !b.favorite);
      await reloadAll();
    } catch (e) {
      setError(String(e));
    }
  };

  const ctxRemoveFromAlbum = async (b: BlobEntry) => {
    setCtxMenu(null);
    try {
      await api.albumRemoveBlob(selectedAlbum, b.blob_hash);
      toast.success(`已从相册移除`);
      await reloadAll();
    } catch (e) {
      setError(String(e));
    }
  };

  const ctxDelete = async (b: BlobEntry) => {
    setCtxMenu(null);
    if (!window.confirm(`把「${b.file_name}」移入回收站吗？碎片仍保留，可随时恢复。`)) return;
    try {
      await api.deleteBlob(b.blob_hash);
      toast.success("已移入回收站");
      await reloadAll();
    } catch (e) {
      setError(String(e));
    }
  };

  const startRename = (b: BlobEntry) => {
    setCtxMenu(null);
    setRenameVal(b.file_name);
    setRenameModal(b);
  };

  const doRenameModal = async () => {
    if (!renameModal || !renameVal.trim()) return;
    try {
      await api.renameBlob(renameModal.blob_hash, renameVal.trim());
      setRenameModal(null);
      await reloadAll();
    } catch (e) {
      setError(String(e));
    }
  };

  // Esc 关闭右键菜单 / 更多菜单
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setCtxMenu(null);
        setMenuOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

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

  // 展示集：选中相册时为相册内容，否则为全部；可叠加「只看收藏」
  const displayBlobs = useMemo(() => {
    const base = selectedAlbum ? albumBlobs : blobs;
    return favOnly ? base.filter((b) => b.favorite) : base;
  }, [selectedAlbum, albumBlobs, blobs, favOnly]);

  // 按月分组（时间线导航芯片条：跳转到该月第一张）
  const months = useMemo(() => {
    const out: { key: string; label: string; firstIdx: number; count: number }[] = [];
    let cur: { key: string; label: string; firstIdx: number; count: number } | null = null;
    displayBlobs.forEach((b, i) => {
      const d = new Date(b.taken_at * 1000);
      const key = `${d.getFullYear()}-${d.getMonth() + 1}`;
      const label = `${d.getFullYear()} 年 ${d.getMonth() + 1} 月`;
      if (cur && cur.key === key) {
        cur.count++;
      } else {
        cur = { key, label, firstIdx: i, count: 1 };
        out.push(cur);
      }
    });
    return out;
  }, [displayBlobs]);

  const jumpToMonth = (firstIdx: number) => {
    (gridRef.current ?? listRef.current)?.scrollToIndex?.({ index: firstIdx, align: "start" });
  };

  // ---------------- 多选批量操作 ----------------
  const toggleSelect = (hash: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(hash)) next.delete(hash);
      else next.add(hash);
      return next;
    });
  };

  const exitSelectMode = () => {
    setSelectMode(false);
    setSelected(new Set());
  };

  const batchFavorite = async () => {
    const hashes = [...selected];
    let ok = 0;
    for (const h of hashes) {
      await api.setBlobFavorite(h, true).then(() => ok++).catch(() => {});
    }
    toast.success(`已收藏 ${ok} 张`);
    exitSelectMode();
    await reloadAll();
  };

  const batchDownload = async () => {
    const dir = await api.pickDirectory();
    if (!dir) return;
    const hashes = [...selected];
    let ok = 0;
    const failed: string[] = [];
    for (const h of hashes) {
      const blob = displayBlobs.find((b) => b.blob_hash === h);
      if (!blob) continue;
      try {
        await api.exportBlob(h, `${dir}\\${blob.file_name}`);
        ok++;
      } catch {
        failed.push(blob.file_name);
      }
    }
    if (failed.length > 0) {
      toast.error(`导出 ${ok} 张，失败 ${failed.length}：${failed.slice(0, 3).join("、")}`);
    } else {
      toast.success(`已导出 ${ok} 张到所选目录`);
    }
    exitSelectMode();
  };

  const batchDelete = async () => {
    if (!window.confirm(`把选中的 ${selected.size} 张照片移入回收站吗？碎片保留，可恢复。`)) return;
    for (const h of selected) {
      await api.deleteBlob(h).catch(() => {});
    }
    toast.success(`已移入回收站 ${selected.size} 张`);
    exitSelectMode();
    await reloadAll();
  };

  // 点击网格/列表条目的统一入口
  const openOrToggle = (b: BlobEntry, index: number) => {
    if (selectMode) {
      toggleSelect(b.blob_hash);
    } else {
      setLightboxIdx(index);
    }
  };

  // 按拍摄/修改时间倒序 + 日期分组（列表视图用；blob 项携带 displayBlobs 索引供灯箱跳转）
  const items = useMemo(() => {
    const out: Array<
      | { type: "header"; key: string; label: string }
      | { type: "blob"; key: string; blob: BlobEntry; idx: number }
    > = [];
    let lastDate = "";
    displayBlobs.forEach((b, idx) => {
      const key = b.taken_at ? new Date(b.taken_at * 1000).toLocaleDateString() : "未知日期";
      if (key !== lastDate) {
        out.push({ type: "header", key: `h-${key}`, label: key });
        lastDate = key;
      }
      out.push({ type: "blob", key: b.blob_hash, blob: b, idx });
    });
    return out;
  }, [displayBlobs]);

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

  if (storageOpen) {
    return (
      <StoragePage
        onDone={() => {
          setStorageOpen(false);
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

  if (trashOpen) {
    return (
      <TrashPage
        onDone={() => {
          setTrashOpen(false);
          refresh();
        }}
      />
    );
  }

  if (homeOpen) {
    return (
      <HomePage
        albums={albums}
        total={blobs.length}
        latestHash={blobs[0]?.blob_hash ?? null}
        onOpen={(albumId) => {
          setSelectedAlbum(albumId ?? "");
          setHomeOpen(false);
        }}
      />
    );
  }

  return (
    <div className="gallery">
      <header className="toolbar">
        <div className="toolbar-row">
          <span className="brand">牧心相册</span>
          <button onClick={() => setHomeOpen(true)}>🏠 首页</button>
          <select
            className="album-select"
            value={selectedAlbum}
            onChange={(e) => setSelectedAlbum(e.target.value)}
          >
            <option value="">全部照片（{blobs.length}）</option>
            {albums.map((a) => (
              <option key={a.id} value={a.id}>
                📁 {a.name}（{a.blob_count}）
              </option>
            ))}
          </select>
          <button
            onClick={() => setFavOnly(!favOnly)}
            className={favOnly ? "fav-on" : "secondary"}
            title="只看收藏"
          >
            {favOnly ? "♥ 收藏" : "♡ 收藏"}
          </button>
          <button onClick={() => setViewMode(viewMode === "grid" ? "list" : "grid")}>
            {viewMode === "grid" ? "▦ 网格" : "☰ 列表"}
          </button>
          <button
            onClick={() => (selectMode ? exitSelectMode() : setSelectMode(true))}
            className={selectMode ? "fav-on" : ""}
          >
            {selectMode ? "✕ 退出选择" : "☑ 选择"}
          </button>
          <span className="spacer" />
          <button onClick={doLock} className="secondary">
            锁定
          </button>
        </div>
        <div className="toolbar-row toolbar-buttons">
          <button className="primary-btn" onClick={doImport} disabled={importing}>
            {importing ? "导入中…" : "导入照片 / 视频"}
          </button>
          {platform.canImportWizard && <button onClick={() => setWizardOpen(true)}>导入目录</button>}
          <button onClick={() => setAlbumTreeOpen(true)}>相册管理</button>
          <button onClick={() => setMapOpen(true)}>地图</button>
          <button onClick={() => setSettingsOpen(true)}>设置</button>
          <div className="dropdown-wrap">
            <button onClick={() => setMenuOpen(!menuOpen)}>更多 ▾</button>
            {menuOpen && (
              <div className="dropdown-menu" onMouseLeave={() => setMenuOpen(false)}>
                <button onClick={() => { setMenuOpen(false); doSync(); }} disabled={syncing}>
                  同步多设备
                </button>
                {platform.canRebalance && (
                  <button onClick={() => { setMenuOpen(false); doRebalance(); }} disabled={syncing}>
                    再平衡分片
                  </button>
                )}
                {platform.canVerify && (
                  <button onClick={() => { setMenuOpen(false); doHealthCheck(); }} disabled={healthChecking}>
                    {healthChecking ? "体检中…" : "一键体检"}
                  </button>
                )}
                <button onClick={() => { setMenuOpen(false); setTrashOpen(true); }}>回收站</button>
                {platform.canStorageManage && (
                  <button onClick={() => { setMenuOpen(false); setStorageOpen(true); }}>存储管理</button>
                )}
                {platform.canImportWizard && (
                  <button onClick={() => { setMenuOpen(false); openHistory(); }}>导入历史</button>
                )}
                <button onClick={() => { setMenuOpen(false); refresh(); }}>刷新</button>
              </div>
            )}
          </div>
        </div>
      </header>

      {selectedAlbum && (
        <div className="notice album-bar">
          <strong>📁 {albums.find((a) => a.id === selectedAlbum)?.name ?? "相册"}</strong>
          <span className="hint" style={{ marginLeft: 8 }}>{displayBlobs.length} 张</span>
          <button style={{ marginLeft: 12 }} onClick={doUploadToAlbum} disabled={importing}>
            {importing ? "上传中…" : "上传照片到此相册"}
          </button>
        </div>
      )}
      {results && (
        <div className="notice">
          <strong>本次导入：</strong>
          <button className="notice-close" onClick={() => setResults(null)}>✕</button>
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
      {error && (
        <div className="notice error">
          {error}
          <button className="notice-close" onClick={() => setError(null)}>✕</button>
        </div>
      )}

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

      {months.length > 1 && displayBlobs.length > 0 && (
        <div className="month-bar">
          {months.map((m) => (
            <button key={m.key} onClick={() => jumpToMonth(m.firstIdx)}>
              {m.label}（{m.count}）
            </button>
          ))}
        </div>
      )}

      {displayBlobs.length === 0 ? (
        <div className="empty">
          {selectedAlbum ? (
            <>
              <p>该相册还没有照片。</p>
              <p className="hint">点上方「上传照片到此相册」，或把文件直接拖进窗口。</p>
            </>
          ) : (
            <>
              <p>相册还是空的。</p>
              <p className="hint">点「导入照片 / 视频」选择文件，或把文件直接拖进窗口。导入的照片会被切片、AES-256-GCM 加密并以 n=4/m=2 纠删冗余存储。</p>
            </>
          )}
        </div>
      ) : viewMode === "grid" ? (
        <VirtuosoGrid
          ref={gridRef}
          style={{ height: "calc(100vh - 240px)", paddingTop: 8 }}
          totalCount={displayBlobs.length}
          listClassName="photo-grid"
          itemClassName="photo-cell"
          itemContent={(index: number) => {
            const b = displayBlobs[index];
            return (
              <div
                className={`photo-cell-inner ${selectMode && selected.has(b.blob_hash) ? "cell-selected" : ""}`}
                onClick={() => openOrToggle(b, index)}
                onContextMenu={(e) => openCtx(e, b)}
                title={`${b.file_name} · ${formatBytes(b.total_len)}`}
              >
                <Thumb blobHash={b.blob_hash} mime={b.mime} />
                <span className="cell-label">
                  {b.favorite && <span style={{ color: "#f0b45c" }}>♥ </span>}
                  {b.file_name}
                </span>
                {selectMode && (
                  <span className={`cell-check ${selected.has(b.blob_hash) ? "on" : ""}`}>
                    {selected.has(b.blob_hash) ? "✓" : ""}
                  </span>
                )}
              </div>
            );
          }}
        />
      ) : (
        <Virtuoso
          ref={listRef}
          style={{ height: "calc(100vh - 240px)", paddingTop: 8 }}
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
            return (
              <div
                className={`blob-row ${selectMode && selected.has(b.blob_hash) ? "cell-selected" : ""}`}
                style={{ cursor: "pointer" }}
                onClick={() => openOrToggle(b, item.idx)}
                onContextMenu={(e) => openCtx(e, b)}
              >
                <Thumb blobHash={b.blob_hash} mime={b.mime} />
                <div className="blob-main">
                  <span className="name" title={b.file_name}>
                    {b.favorite && <span style={{ color: "#f0b45c" }}>♥ </span>}
                    {b.file_name}
                  </span>
                  <span className="meta">
                    {formatBytes(b.total_len)} · {formatDate(b.taken_at)}
                  </span>
                </div>
                <div className="blob-actions" onClick={(e) => e.stopPropagation()}>
                  <button onClick={() => doPreview(b)}>预览</button>
                  <button onClick={() => doExport(b)}>下载</button>
                </div>
              </div>
            );
          }}
        />
      )}

      {/* 多选批量操作条 */}
      {selectMode && (
        <div className="selection-bar">
          <span className="sel-count">已选 {selected.size} 张</span>
          <button onClick={() => selected.size === displayBlobs.length ? setSelected(new Set()) : setSelected(new Set(displayBlobs.map((b) => b.blob_hash)))}>
            {selected.size === displayBlobs.length ? "取消全选" : "全选"}
          </button>
          <button onClick={batchFavorite} disabled={selected.size === 0}>♥ 收藏</button>
          <button
            disabled={selected.size === 0}
            onClick={() => setAlbumPickerHashes([...selected])}
          >
            📁 添加到相册
          </button>
          <button onClick={batchDownload} disabled={selected.size === 0}>⬇ 下载</button>
          <button className="danger" onClick={batchDelete} disabled={selected.size === 0}>🗑 删除</button>
          <button onClick={exitSelectMode}>退出</button>
        </div>
      )}

      {/* 全屏查看器 */}
      {lightboxIdx !== null && displayBlobs.length > 0 && (
        <Lightbox
          blobs={displayBlobs}
          index={Math.min(lightboxIdx, displayBlobs.length - 1)}
          onClose={() => setLightboxIdx(null)}
          onNavigate={setLightboxIdx}
          onInfo={(b) => {
            setLightboxIdx(null);
            setDetailBlob(b);
          }}
        />
      )}

      {detailBlob && (
        <BlobDetailModal
          blob={detailBlob}
          onClose={() => setDetailBlob(null)}
          onExport={doExport}
          onPreview={doPreview}
          onChanged={(b) => {
            setDetailBlob(b);
            reloadAll();
          }}
          onDeleted={() => {
            setDetailBlob(null);
            reloadAll();
          }}
        />
      )}

      {/* 照片右键菜单 */}
      {ctxMenu && (
        <>
          <div
            className="menu-overlay"
            onClick={() => setCtxMenu(null)}
            onContextMenu={(e) => {
              e.preventDefault();
              setCtxMenu(null);
            }}
          />
          <div className="ctx-menu" style={{ left: ctxMenu.x, top: ctxMenu.y }}>
            <button onClick={() => { setCtxMenu(null); doPreview(ctxMenu.blob); }}>🔍 预览</button>
            <button onClick={() => ctxToggleFav(ctxMenu.blob)}>
              {ctxMenu.blob.favorite ? "♥ 取消收藏" : "♡ 收藏"}
            </button>
            <button onClick={() => { setAlbumPickerHashes([ctxMenu.blob.blob_hash]); setCtxMenu(null); }}>
              📁 添加到相册…
            </button>
            {selectedAlbum && (
              <button onClick={() => ctxRemoveFromAlbum(ctxMenu.blob)}>➖ 从本相册移除</button>
            )}
            <button onClick={() => startRename(ctxMenu.blob)}>✏️ 重命名</button>
            <button onClick={() => { setCtxMenu(null); doExport(ctxMenu.blob); }}>⬇ 下载 / 分享</button>
            <button className="danger" onClick={() => ctxDelete(ctxMenu.blob)}>🗑 移入回收站</button>
          </div>
        </>
      )}

      {/* 添加到相册弹窗（支持批量：右键单张或多选操作条） */}
      {albumPickerHashes && (
        <div className="modal-backdrop" onClick={() => setAlbumPickerHashes(null)}>
          <div className="modal" style={{ width: 420 }} onClick={(e) => e.stopPropagation()}>
            <header className="modal-head">
              <span className="name">
                添加到相册（{albumPickerHashes.length} 张）
              </span>
              <span className="spacer" />
              <button onClick={() => setAlbumPickerHashes(null)}>✕</button>
            </header>
            {albums.length === 0 ? (
              <p className="hint">还没有相册。请先到「相册管理」创建一个。</p>
            ) : (
              <ul className="album-list">
                {albums.map((a) => (
                  <li key={a.id} className="album-row">
                    <span className="name">📁 {a.name}</span>
                    <button
                      onClick={async () => {
                        let ok = 0;
                        for (const h of albumPickerHashes) {
                          await api.albumAddBlob(a.id, h).then(() => ok++).catch(() => {});
                        }
                        toast.success(`已添加 ${ok} 张到「${a.name}」`);
                        await reloadAll();
                      }}
                    >
                      添加
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>
      )}

      {/* 重命名弹窗 */}
      {renameModal && (
        <div className="modal-backdrop" onClick={() => setRenameModal(null)}>
          <div className="modal" style={{ width: 420 }} onClick={(e) => e.stopPropagation()}>
            <header className="modal-head">
              <span className="name">重命名照片</span>
              <span className="spacer" />
              <button onClick={() => setRenameModal(null)}>✕</button>
            </header>
            <div className="settings-card">
              <input
                value={renameVal}
                autoFocus
                style={{ width: "100%" }}
                onChange={(e) => setRenameVal(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && doRenameModal()}
              />
              <button className="primary" onClick={doRenameModal} disabled={!renameVal.trim()}>
                确定
              </button>
              <button style={{ marginLeft: 8 }} onClick={() => setRenameModal(null)}>
                取消
              </button>
            </div>
          </div>
        </div>
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
