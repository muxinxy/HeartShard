// 共享照片操作层（对齐网页端 window.* 全局函数）：
// 收藏 / 下载 / 回收站 / 重命名 / 移动到·复制到相册 / 分享 / 详情 / 灯箱。
// 弹窗集中在此渲染；视图经 usePhotoActions() 调用，dataVersion 变更触发重取。
import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import {
  api,
  remoteUrl,
  AlbumInfo,
  BlobEntry,
  platform,
} from "../lib/api";
import { invoke } from "@tauri-apps/api/core";
import { BlobDetailModal } from "./BlobDetailModal";
import { Lightbox } from "./Lightbox";
import { Ic } from "./icons";
import { useToast } from "./Toast";

export interface PhotoActions {
  /** 任一照片数据变更后 +1；视图监听它重取数据 */
  dataVersion: number;
  albums: AlbumInfo[];
  reloadAlbums(): Promise<void>;
  fav(b: BlobEntry): Promise<void>;
  downloadBlob(b: BlobEntry): Promise<void>;
  trash(b: BlobEntry | BlobEntry[]): void;
  rename(b: BlobEntry): void;
  pickAlbum(hashes: string[], mode: "add" | "move"): void;
  /** 通用确认弹窗（替代 window.confirm） */
  confirmAsk(title: string, msg: string, onOk: () => void, danger?: boolean): void;
  share(hashes: string[]): void;
  shareAlbum(album: AlbumInfo): void;
  detail(b: BlobEntry): void;
  openViewer(blobs: BlobEntry[], index: number): void;
  closeViewer(): void;
  importDialog(albumId?: string | null): Promise<void>;
  preview(b: BlobEntry): Promise<void>;
  /** 重命名 / 删除 / 移动后的回调（视图刷新自身数据） */
  onChanged(): void;
}

const Ctx = createContext<PhotoActions | null>(null);
export function usePhotoActions(): PhotoActions {
  const v = useContext(Ctx);
  if (!v) throw new Error("PhotoActionsProvider 缺失");
  return v;
}

type Confirm = { title: string; msg: string; danger?: boolean; onOk: () => void } | null;
type AlbumPick = { hashes: string[]; mode: "add" | "move" } | null;
type ShareDlg = { hashes: string[] } | { album: AlbumInfo } | null;

export function PhotoActionsProvider({ children }: { children: ReactNode }) {
  const toast = useToast();
  const [dataVersion, setDataVersion] = useState(0);
  const [albums, setAlbums] = useState<AlbumInfo[]>([]);
  const [confirm, setConfirm] = useState<Confirm>(null);
  const [albumPick, setAlbumPick] = useState<AlbumPick>(null);
  const [shareDlg, setShareDlg] = useState<ShareDlg>(null);
  const [renameTarget, setRenameTarget] = useState<BlobEntry | null>(null);
  const [renameVal, setRenameVal] = useState("");
  const [detailBlob, setDetailBlob] = useState<BlobEntry | null>(null);
  const [viewer, setViewer] = useState<{ blobs: BlobEntry[]; index: number } | null>(null);

  const bump = useCallback(() => setDataVersion((v) => v + 1), []);
  const reloadAlbums = useCallback(async () => {
    setAlbums(await api.listAlbums().catch(() => []));
  }, []);
  const onChanged = useCallback(() => {
    bump();
    reloadAlbums();
  }, [bump, reloadAlbums]);

  const fav = useCallback(
    async (b: BlobEntry) => {
      try {
        await api.setBlobFavorite(b.blob_hash, !b.favorite);
        onChanged();
      } catch (e) {
        toast.error(String(e));
      }
    },
    [onChanged, toast]
  );

  const downloadBlob = useCallback(
    async (b: BlobEntry) => {
      try {
        const dest = await api.pickSavePath(b.file_name);
        if (!dest) return;
        await api.exportBlob(b.blob_hash, dest);
        toast.success(`已还原并通过 SHA-256 校验：${dest}`);
        // 动态照片：同时导出配对视频段（双文件）
        if (b.is_motion && b.motion_video_hash) {
          const dot = dest.lastIndexOf(".");
          const videoDest = dot >= 0 ? `${dest.slice(0, dot)}-motion.mp4` : `${dest}-motion.mp4`;
          try {
            await api.exportBlob(b.motion_video_hash, videoDest);
            toast.success(`动态视频已一并导出：${videoDest}`);
          } catch (e) {
            toast.error(`动态视频导出失败：${String(e)}`);
          }
        }
      } catch (e) {
        toast.error(`导出失败：${String(e)}`);
      }
    },
    [toast]
  );

  const trash = useCallback((b: BlobEntry | BlobEntry[]) => {
    const list = Array.isArray(b) ? b : [b];
    if (list.length === 0) return;
    setConfirm({
      title: "移入回收站",
      msg: `把${list.length > 1 ? `选中的 ${list.length} 张` : `「${list[0].file_name}」`}移入回收站吗？碎片仍保留，可随时恢复。`,
      onOk: async () => {
        for (const x of list) await api.deleteBlob(x.blob_hash).catch(() => {});
        toast.success(`已移入回收站 ${list.length} 张`);
        onChanged();
      },
    });
  }, [onChanged, toast]);

  const rename = useCallback((b: BlobEntry) => {
    setRenameVal(b.file_name);
    setRenameTarget(b);
  }, []);

  const pickAlbum = useCallback((hashes: string[], mode: "add" | "move") => {
    setAlbumPick({ hashes, mode });
    reloadAlbums();
  }, [reloadAlbums]);

  const confirmAsk = useCallback(
    (title: string, msg: string, onOk: () => void, danger?: boolean) => {
      setConfirm({ title, msg, onOk, danger });
    },
    []
  );

  const doRename = useCallback(async () => {
    if (!renameTarget || !renameVal.trim()) return;
    try {
      await api.renameBlob(renameTarget.blob_hash, renameVal.trim());
      setRenameTarget(null);
      toast.success("已重命名");
      onChanged();
    } catch (e) {
      toast.error(String(e));
    }
  }, [renameTarget, renameVal, onChanged, toast]);

  const doAlbumPick = useCallback(
    async (albumId: string, albumName: string) => {
      if (!albumPick) return;
      const { hashes, mode } = albumPick;
      let ok = 0;
      for (const h of hashes) {
        if (mode === "move") {
          await api.albumRemoveBlob(albumId, h).catch(() => {});
          await api.albumAddBlob(albumId, h).then(() => ok++).catch(() => {});
        } else {
          await api.albumAddBlob(albumId, h).then(() => ok++).catch(() => {});
        }
      }
      setAlbumPick(null);
      toast.success(`已${mode === "move" ? "移动" : "添加"} ${ok} 张到「${albumName}」`);
      onChanged();
    },
    [albumPick, onChanged, toast]
  );

  const importDialog = useCallback(
    async (albumId?: string | null) => {
      const paths = await api.pickFiles();
      if (paths.length === 0) return;
      if (platform.isRemote) {
        // 远端：读本地路径 → multipart 上传
        let ok = 0;
        let fail = 0;
        for (const p of paths) {
          try {
            const bytes = await invoke<number[]>("read_local_file", { path: p });
            const name = p.split(/[\\/]/).pop() || "upload.bin";
            const file = new File([new Uint8Array(bytes)], name);
            const r = await api.uploadFiles([file], albumId ?? null);
            if (r.some((x) => x.status === "imported")) ok++;
            else fail++;
          } catch {
            fail++;
          }
        }
        toast.success(`上传完成：${ok} 成功${fail ? `，${fail} 失败` : ""}`);
        onChanged();
        return;
      }
      // 本地：importFiles（任务中心跟踪）
      try {
        const outcomes = await api.importFiles(paths);
        const okN = outcomes.filter((o) => o.status === "imported").length;
        toast.success(`导入完成：${okN} 张加密上传`);
        if (albumId) {
          for (const o of outcomes) {
            if (o.status === "imported" && o.blob_hash) await api.albumAddBlob(albumId, o.blob_hash).catch(() => {});
          }
        }
        onChanged();
      } catch (e) {
        toast.error(`导入失败：${String(e)}`);
      }
    },
    [onChanged, toast]
  );

  const preview = useCallback(
    async (b: BlobEntry) => {
      try {
        const result = await api.previewBlob(b.blob_hash);
        await api.openPathSystem(result.path);
      } catch (e) {
        toast.error(`预览失败：${String(e)}`);
      }
    },
    [toast]
  );

  const ctxValue = useMemo<PhotoActions>(
    () => ({
      dataVersion,
      albums,
      reloadAlbums,
      fav,
      downloadBlob,
      trash,
      rename,
      pickAlbum,
      confirmAsk,
      share: (hashes: string[]) => setShareDlg({ hashes }),
      shareAlbum: (album: AlbumInfo) => setShareDlg({ album }),
      detail: (b: BlobEntry) => setDetailBlob(b),
      openViewer: (blobs: BlobEntry[], index: number) => setViewer({ blobs, index }),
      closeViewer: () => setViewer(null),
      importDialog,
      preview,
      onChanged,
    }),
    [dataVersion, albums, reloadAlbums, fav, downloadBlob, trash, rename, pickAlbum, confirmAsk, importDialog, preview, onChanged]
  );

  return (
    <Ctx.Provider value={ctxValue}>
      {children}

      {/* 确认弹窗 */}
      {confirm && (
        <div className="modal-backdrop" onClick={() => setConfirm(null)}>
          <div className="modal" style={{ width: 420 }} onClick={(e) => e.stopPropagation()}>
            <h3>{confirm.title}</h3>
            <p style={{ fontSize: 14, color: "var(--muted)" }}>{confirm.msg}</p>
            <div className="row" style={{ justifyContent: "flex-end", marginTop: 16 }}>
              <button onClick={() => setConfirm(null)}>取消</button>
              <button
                className="primary"
                style={confirm.danger ? { background: "var(--destructive)" } : undefined}
                onClick={() => {
                  setConfirm(null);
                  confirm.onOk();
                }}
              >
                确定
              </button>
            </div>
          </div>
        </div>
      )}

      {/* 重命名弹窗 */}
      {renameTarget && (
        <div className="modal-backdrop" onClick={() => setRenameTarget(null)}>
          <div className="modal" style={{ width: 420 }} onClick={(e) => e.stopPropagation()}>
            <h3>重命名</h3>
            <input
              value={renameVal}
              autoFocus
              style={{ width: "100%" }}
              onChange={(e) => setRenameVal(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && doRename()}
            />
            <div className="row" style={{ justifyContent: "flex-end", marginTop: 14 }}>
              <button onClick={() => setRenameTarget(null)}>取消</button>
              <button className="primary" onClick={doRename} disabled={!renameVal.trim()}>
                确定
              </button>
            </div>
          </div>
        </div>
      )}

      {/* 移动到 / 复制到相册 */}
      {albumPick && (
        <div className="modal-backdrop" onClick={() => setAlbumPick(null)}>
          <div className="modal" style={{ width: 420 }} onClick={(e) => e.stopPropagation()}>
            <h3>{albumPick.mode === "move" ? "移动到相册" : "复制到相册"}（{albumPick.hashes.length} 张）</h3>
            {albums.length === 0 ? (
              <p className="hint">还没有相册，请先到「相册」页创建。</p>
            ) : (
              <ul className="album-list">
                {albums.map((a) => (
                  <li key={a.id} className="album-row" style={{ cursor: "pointer" }} onClick={() => doAlbumPick(a.id, a.name)}>
                    <span className="name">
                      <Ic name="i-albums" /> {a.name}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>
      )}

      {/* 详情 */}
      {detailBlob && (
        <BlobDetailModal
          blob={detailBlob}
          onClose={() => setDetailBlob(null)}
          onExport={downloadBlob}
          onPreview={preview}
          onChanged={(b) => {
            setDetailBlob(b);
            onChanged();
          }}
          onDeleted={() => {
            setDetailBlob(null);
            onChanged();
          }}
        />
      )}

      {/* 灯箱（全屏查看器） */}
      {viewer && (
        <Lightbox
          blobs={viewer.blobs}
          index={Math.min(viewer.index, viewer.blobs.length - 1)}
          onClose={() => setViewer(null)}
          onNavigate={(i) => setViewer({ blobs: viewer.blobs, index: i })}
        />
      )}

      {/* 分享弹窗（远端） */}
      {shareDlg && <ShareDialog dlg={shareDlg} onClose={() => setShareDlg(null)} />}
    </Ctx.Provider>
  );
}

// ---------------- 分享弹窗（有效期 / 分享码，网页端 shareDialog 同款） ----------------

function ShareDialog({ dlg, onClose }: { dlg: NonNullable<ShareDlg>; onClose: () => void }) {
  const toast = useToast();
  const [mode, setMode] = useState<"agg" | "each">("agg");
  const [exp, setExp] = useState("");
  const [code, setCode] = useState("");
  const [urls, setUrls] = useState<string[] | null>(null);
  const [busy, setBusy] = useState(false);

  const create = async () => {
    setBusy(true);
    try {
      const expires = exp ? Number(exp) : null;
      const access = code.trim() || null;
      // 链接是后台地址（桌面端 origin 是 tauri://，浏览者要打开后台）
      const base = remoteUrl();
      const out: string[] = [];
      if ("album" in dlg) {
        const data = await api.albumShare(dlg.album.id, expires, access);
        out.push(`${base}${data.url}${access ? `（分享码：${access}）` : ""}`);
      } else if (mode === "agg") {
        const data = await api.createShare(dlg.hashes, expires, access);
        out.push(`${base}${data.url}${access ? `（分享码：${access}）` : ""}`);
      } else {
        for (const h of dlg.hashes) {
          const data = await api.createShare([h], expires, access);
          out.push(`${base}${data.url}${access ? `（分享码：${access}）` : ""}`);
        }
      }
      setUrls(out);
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  };

  const copyAll = async () => {
    if (!urls) return;
    try {
      await navigator.clipboard.writeText(urls.join("\n"));
      toast.success("已复制全部链接");
    } catch {
      toast.error("复制失败，请手动选择文本复制");
    }
  };

  const isAlbum = "album" in dlg;
  const title = isAlbum ? `分享相册「${dlg.album.name}」` : `分享 ${dlg.hashes.length} 张照片`;

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" style={{ width: 460 }} onClick={(e) => e.stopPropagation()}>
        <h3>{title}</h3>
        {!urls ? (
          <>
            {!isAlbum && (
              <div className="row" style={{ margin: "8px 0" }}>
                <label>
                  <input type="radio" name="share-mode" checked={mode === "agg"} onChange={() => setMode("agg")} /> 聚合分享（一个链接）
                </label>
                <label>
                  <input type="radio" name="share-mode" checked={mode === "each"} onChange={() => setMode("each")} /> 逐个分享（每张一个链接）
                </label>
              </div>
            )}
            <div className="row">
              <select value={exp} onChange={(e) => setExp(e.target.value)}>
                <option value="">永久有效</option>
                <option value="1">1 天后过期</option>
                <option value="7">7 天后过期</option>
                <option value="30">30 天后过期</option>
              </select>
              <input
                placeholder="分享码（可选）"
                value={code}
                onChange={(e) => setCode(e.target.value)}
                style={{ flex: 1 }}
              />
            </div>
            <p className="hint" style={{ marginTop: 10 }}>
              链接默认去除 EXIF/GPS 元数据，无需登录即可查看。
            </p>
            <div className="row" style={{ justifyContent: "flex-end", marginTop: 14 }}>
              <button onClick={onClose}>取消</button>
              <button className="primary" onClick={create} disabled={busy}>
                {busy ? "创建中…" : "创建链接"}
              </button>
            </div>
          </>
        ) : (
          <>
            <textarea readOnly value={urls.join("\n")} style={{ width: "100%", minHeight: 120, marginTop: 8 }} />
            <div className="row" style={{ justifyContent: "flex-end", marginTop: 12 }}>
              <button className="primary" onClick={copyAll}>
                <Ic name="i-shares" /> 复制全部
              </button>
              <button onClick={onClose}>关闭</button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
