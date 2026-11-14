// 时间线（网页端 viewPhotos 同款）：日分组网格 + 年/月导航轨 + 常显多选 +
// 批量操作条 + 右键菜单；本地模式附导入/同步/体检工具行 + 拖拽导入。
import { useCallback, useEffect, useMemo, useState, type MouseEvent as ReactMouseEvent } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { api, BlobEntry, platform } from "../../lib/api";
import { CtxMenu, type MenuItem } from "../CtxMenu";
import { PhotoGrid, groupByDay } from "../PhotoGrid";
import { usePhotoActions } from "../PhotoActions";
import { Ic } from "../icons";
import { useToast } from "../Toast";

export function TimelineView({ searchQ }: { searchQ: string }) {
  const toast = useToast();
  const actions = usePhotoActions();
  const [blobs, setBlobs] = useState<BlobEntry[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [selectedDays, setSelectedDays] = useState<Set<string>>(new Set());
  const [ctx, setCtx] = useState<{ x: number; y: number; blob: BlobEntry } | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    api
      .listBlobs()
      .then(setBlobs)
      .catch((e) => toast.error(String(e)));
  }, [actions.dataVersion, toast]);

  // 搜索过滤（网页端 filteredBlobs：文件名子串）
  const filtered = useMemo(() => {
    const q = searchQ.trim().toLowerCase();
    const base = [...blobs].sort((a, b) => b.taken_at - a.taken_at);
    return q ? base.filter((b) => b.file_name.toLowerCase().includes(q)) : base;
  }, [blobs, searchQ]);

  // 年/月导航轨（年标题在上，月份缩进其下，倒序）
  const rail = useMemo(() => {
    const out: { y: number; m?: number }[] = [];
    for (const g of groupByDay(filtered)) {
      const last = out[out.length - 1];
      if (!last || last.y !== g.year) {
        out.push({ y: g.year });
        out.push({ y: g.year, m: g.month });
      } else if (last.m !== undefined && last.m !== g.month) {
        out.push({ y: g.year, m: g.month });
      }
    }
    return out;
  }, [filtered]);

  const scrollToAnchor = (y: number, m?: number) => {
    const el = m
      ? document.querySelector(`[data-anchor-m="${y}-${m}"]`)
      : document.querySelector(`[data-anchor-y="${y}"]`);
    el?.scrollIntoView({ behavior: "smooth", block: "start" });
  };

  const blobMap = useMemo(() => new Map(blobs.map((b) => [b.blob_hash, b])), [blobs]);
  const filteredMap = useMemo(() => new Map(filtered.map((b, i) => [b.blob_hash, i])), [filtered]);

  const toggleSel = useCallback((hash: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(hash)) next.delete(hash);
      else next.add(hash);
      return next;
    });
  }, []);

  const toggleDay = useCallback(
    (dayKey: string, select: boolean) => {
      setSelected((prev) => {
        const next = new Set(prev);
        for (const b of filtered) {
          const k = b.taken_at ? new Date(b.taken_at * 1000).toDateString() : "unknown";
          if (k === dayKey) {
            if (select) next.add(b.blob_hash);
            else next.delete(b.blob_hash);
          }
        }
        return next;
      });
      setSelectedDays((prev) => {
        const next = new Set(prev);
        if (select) next.add(dayKey);
        else next.delete(dayKey);
        return next;
      });
    },
    [filtered]
  );

  // 同步日复选框勾选态
  useEffect(() => {
    setSelectedDays((prev) => {
      const next = new Set<string>();
      for (const key of prev) {
        const inDay = filtered.filter(
          (b) => (b.taken_at ? new Date(b.taken_at * 1000).toDateString() : "unknown") === key
        );
        if (inDay.length > 0 && inDay.every((b) => selected.has(b.blob_hash))) next.add(key);
      }
      return next.size === prev.size ? prev : next;
    });
  }, [selected, filtered]);

  const selBlobs = useMemo(
    () => [...selected].map((h) => blobMap.get(h)).filter((b): b is BlobEntry => Boolean(b)),
    [selected, blobMap]
  );

  // ---------------- 右键菜单（网页端 photoMenuItems 同款） ----------------
  const openCtx = (e: ReactMouseEvent, b: BlobEntry) => {
    e.preventDefault();
    e.stopPropagation();
    setCtx({ x: e.clientX, y: e.clientY, blob: b });
  };

  const menuItems = (b: BlobEntry): MenuItem[] => {
    const items: MenuItem[] = [
      { label: "预览", icon: "i-preview", onClick: () => actions.openViewer(filtered, filteredMap.get(b.blob_hash) ?? 0) },
      { label: b.favorite ? "取消收藏" : "收藏", icon: "i-heart", onClick: () => actions.fav(b) },
      { label: "重命名", icon: "i-edit", onClick: () => actions.rename(b) },
      { label: "下载原图", icon: "i-download", onClick: () => actions.downloadBlob(b) },
    ];
    if (platform.isRemote) items.push({ label: "分享", icon: "i-shares", onClick: () => actions.share([b.blob_hash]) });
    items.push(
      { label: "详情", icon: "i-box", onClick: () => actions.detail(b) },
      { label: "移动到相册", icon: "i-albums", onClick: () => actions.pickAlbum([b.blob_hash], "move") },
      { label: "复制到相册", icon: "i-folder-plus", onClick: () => actions.pickAlbum([b.blob_hash], "add") },
      { label: "移入回收站", icon: "i-trash", danger: true, onClick: () => actions.trash(b) }
    );
    return items;
  };

  // ---------------- 批量操作 ----------------
  const clearSel = () => {
    setSelected(new Set());
    setSelectedDays(new Set());
  };

  const batchFav = async () => {
    let ok = 0;
    for (const h of selected) await api.setBlobFavorite(h, true).then(() => ok++).catch(() => {});
    toast.success(`已收藏 ${ok} 张`);
    clearSel();
    actions.onChanged();
  };

  const batchDownload = async () => {
    const dir = await api.pickDirectory();
    if (!dir) return;
    let ok = 0;
    const failed: string[] = [];
    for (const b of selBlobs) {
      try {
        await api.exportBlob(b.blob_hash, `${dir}\\${b.file_name}`);
        ok++;
      } catch {
        failed.push(b.file_name);
      }
    }
    if (failed.length > 0) toast.error(`导出 ${ok} 张，失败 ${failed.length}：${failed.slice(0, 3).join("、")}`);
    else toast.success(`已导出 ${ok} 张到所选目录`);
    clearSel();
  };

  const batchDetail = () => {
    if (selBlobs.length === 1) {
      actions.detail(selBlobs[0]);
      return;
    }
    const total = selBlobs.reduce((s, b) => s + b.total_len, 0);
    const videos = selBlobs.filter((b) => b.mime.startsWith("video/")).length;
    toast.info(`共 ${selBlobs.length} 张 · ${total / 1024 / 1024 < 1024 ? `${(total / 1024 / 1024).toFixed(1)} MB` : `${(total / 1024 / 1024 / 1024).toFixed(2)} GB`} · 视频 ${videos} 个`);
  };

  // ---------------- 本地工具行：同步 / 体检 / 再平衡 ----------------
  const doSync = async () => {
    setBusy("sync");
    try {
      const r = await api.syncNow();
      toast.success(`同步完成：拉取 ${r.pulled_objects} · 应用 ${r.applied_objects}`);
      actions.onChanged();
    } catch (e) {
      toast.error(`同步失败：${String(e)}`);
    } finally {
      setBusy(null);
    }
  };
  const doHealth = async () => {
    setBusy("health");
    try {
      const rs = await api.verifyAll();
      const ok = rs.filter((r) => r.ok).length;
      const bad = rs.length - ok;
      if (bad > 0) toast.error(`体检：${bad} 张存在缺失/损坏分片，可在详情里修复`);
      else toast.success(`体检：全部 ${rs.length} 张健康`);
    } catch (e) {
      toast.error(`体检失败：${String(e)}`);
    } finally {
      setBusy(null);
    }
  };
  const doRebalance = async () => {
    setBusy("rebalance");
    try {
      const r = await api.rebalanceAll();
      if (r.moved > 0) toast.success(`再平衡完成：迁移 ${r.moved} 个 shard`);
      else if (r.failed.length > 0) toast.error(`${r.failed.length} 个 shard 无法搬迁，请先体检+修复`);
      else toast.info("再平衡完成：布局已符合策略");
    } catch (e) {
      toast.error(`再平衡失败：${String(e)}`);
    } finally {
      setBusy(null);
    }
  };

  // 拖拽导入（媒体文件拖进窗口）
  useEffect(() => {
    const un = getCurrentWebviewWindow().onDragDropEvent((ev) => {
      if (ev.payload.type !== "drop") return;
      const media = ev.payload.paths.filter((p) =>
        /\.(jpe?g|png|gif|webp|bmp|heic|heif|avif|tiff?|mp4|mov|m4v|avi|mkv|webm)$/i.test(p)
      );
      if (media.length > 0) {
        api
          .importFiles(media)
          .then((rs) => {
            const ok = rs.filter((r) => r.status === "imported").length;
            toast.success(`导入完成：${ok} 张加密上传`);
            actions.onChanged();
          })
          .catch((e) => toast.error(`导入失败：${String(e)}`));
      }
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, [actions, toast]);

  return (
    <div>
      <div className="view-head">
        <h2>时间线</h2>
        <span className="badge">{filtered.length} 张</span>
        <span style={{ flex: 1 }} />
        {!platform.isRemote && (
          <>
            <button onClick={() => (window.location.hash = "#/import")}>
              <Ic name="i-folder-plus" /> 导入目录
            </button>
            <button onClick={doSync} disabled={busy !== null}>
              <Ic name="i-sync" /> {busy === "sync" ? "同步中…" : "同步多设备"}
            </button>
            <button onClick={doHealth} disabled={busy !== null}>
              {busy === "health" ? "体检中…" : "一键体检"}
            </button>
            <button onClick={doRebalance} disabled={busy !== null}>
              再平衡
            </button>
          </>
        )}
      </div>

      <PhotoGrid
        blobs={filtered}
        selected={selected}
        onToggleSel={toggleSel}
        onOpen={(_b, idx) => actions.openViewer(filtered, idx)}
        onCtx={openCtx}
        daySelectable
        selectedDays={selectedDays}
        onToggleDay={toggleDay}
        emptyHint={searchQ ? `没有匹配「${searchQ}」的照片。` : "相册还是空的。点「导入照片 / 视频」选择文件，或把文件直接拖进窗口。"}
      />

      {rail.length > 1 && (
        <div className="years">
          {rail.map((r, i) => (
            <div
              key={i}
              className={r.m === undefined ? "rail-year" : "rail-month"}
              onClick={() => scrollToAnchor(r.y, r.m)}
            >
              {r.m === undefined ? `${r.y} 年` : `${r.m} 月`}
            </div>
          ))}
        </div>
      )}

      {selected.size > 0 && (
        <div className="selbar">
          <span className="sel-count">已选 {selected.size} 张</span>
          <button onClick={batchFav}>
            <Ic name="i-heart" /> 收藏
          </button>
          {selected.size === 1 && (
            <button onClick={() => selBlobs[0] && actions.rename(selBlobs[0])}>
              <Ic name="i-edit" /> 重命名
            </button>
          )}
          {platform.isRemote && (
            <button onClick={() => actions.share([...selected])}>
              <Ic name="i-shares" /> 分享
            </button>
          )}
          <button onClick={batchDetail}>
            <Ic name="i-box" /> 详情
          </button>
          <button onClick={batchDownload}>
            <Ic name="i-download" /> 下载
          </button>
          <button onClick={() => actions.pickAlbum([...selected], "move")}>
            <Ic name="i-albums" /> 移动到
          </button>
          <button onClick={() => actions.pickAlbum([...selected], "add")}>
            <Ic name="i-folder-plus" /> 复制到
          </button>
          <button className="danger" onClick={() => actions.trash(selBlobs)}>
            <Ic name="i-trash" /> 回收站
          </button>
          <button className="primary" onClick={clearSel}>
            取消
          </button>
        </div>
      )}

      {ctx && <CtxMenu pos={{ x: ctx.x, y: ctx.y }} items={menuItems(ctx.blob)} onClose={() => setCtx(null)} />}
    </div>
  );
}
