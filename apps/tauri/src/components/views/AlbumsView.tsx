// 相册（网页端 viewAlbums 同款）：封面墙 + 多选（分享/删除）+ 新建相册 +
// 相册详情（返回/权限/分享/删除）+ ACL 编辑器（远端，用户/组 × 继承/隐藏/只看/上传/管理）。
import { useCallback, useEffect, useMemo, useState } from "react";
import { api, AlbumInfo, BlobEntry, AclEntry, platform } from "../../lib/api";
import { PhotoGrid } from "../PhotoGrid";
import { CtxMenu, type MenuItem } from "../CtxMenu";
import { usePhotoActions } from "../PhotoActions";
import { Ic } from "../icons";
import { useToast } from "../Toast";

export function AlbumsView({ albumId, searchQ }: { albumId: string | null; searchQ: string }) {
  const actions = usePhotoActions();
  if (albumId) return <AlbumDetail albumId={albumId} />;
  return <AlbumWall searchQ={searchQ} actions={actions} />;
}

// ---------------- 封面墙 ----------------

function AlbumWall({
  searchQ,
  actions,
}: {
  searchQ: string;
  actions: ReturnType<typeof usePhotoActions>;
}) {
  const toast = useToast();
  const [albums, setAlbums] = useState<AlbumInfo[]>([]);
  const [covers, setCovers] = useState<Record<string, string>>({});
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [createOpen, setCreateOpen] = useState(false);
  const [newName, setNewName] = useState("");
  const [ctx, setCtx] = useState<{ x: number; y: number; album: AlbumInfo } | null>(null);
  const [aclAlbum, setAclAlbum] = useState<AlbumInfo | null>(null);
  const [coverAlbum, setCoverAlbum] = useState<AlbumInfo | null>(null);
  const [renameOpen, setRenameOpen] = useState<{ album: AlbumInfo; name: string } | null>(null);
  // 排序：创建时间（默认）/ 名称 × 升降序
  const [sortMode, setSortMode] = useState("created-desc");

  const reload = useCallback(() => {
    api
      .listAlbums()
      .then((list) => {
        setAlbums([...list].sort((a, b) => a.sort_order - b.sort_order));
        // 封面：优先 cover_hash，否则取相册第一张（网页端同款）
        (async () => {
          const m: Record<string, string> = {};
          for (const a of list) {
            if (a.cover_hash) {
              m[a.id] = a.cover_hash;
              continue;
            }
            const first = await api.listAlbumBlobs(a.id).catch(() => []);
            if (first.length > 0) m[a.id] = first[0].blob_hash;
          }
          setCovers(m);
        })();
      })
      .catch((e) => toast.error(String(e)));
  }, [toast]);

  useEffect(() => {
    reload();
  }, [reload, actions.dataVersion]);

  const filtered = useMemo(() => {
    const q = searchQ.trim().toLowerCase();
    const base = q ? albums.filter((a) => a.name.toLowerCase().includes(q)) : [...albums];
    switch (sortMode) {
      case "created-desc":
        base.sort((a, b) => b.sort_order - a.sort_order);
        break;
      case "created-asc":
        base.sort((a, b) => a.sort_order - b.sort_order);
        break;
      case "name-asc":
        base.sort((a, b) => a.name.localeCompare(b.name, "zh"));
        break;
      case "name-desc":
        base.sort((a, b) => b.name.localeCompare(a.name, "zh"));
        break;
    }
    return base;
  }, [albums, searchQ, sortMode]);

  const openAlbum = (id: string) => (window.location.hash = `#/album-${id}`);

  const create = async () => {
    if (!newName.trim()) return;
    try {
      await api.albumCreate(newName.trim(), null);
      setCreateOpen(false);
      setNewName("");
      toast.success("相册已创建");
      reload();
    } catch (e) {
      toast.error(String(e));
    }
  };

  const deleteAlbum = (a: AlbumInfo) => {
    actions.confirmAsk(
      "删除相册",
      `删除相册「${a.name}」？相册一经删除便无法恢复；照片仍保留在全部照片。`,
      async () => {
        try {
          await api.albumDelete(a.id);
          toast.success("相册已删除");
          setSelected(new Set());
          reload();
        } catch (e) {
          toast.error(String(e));
        }
      },
      true
    );
  };

  const albumMenu = (a: AlbumInfo): MenuItem[] => [
    { label: "打开相册", icon: "i-albums", onClick: () => openAlbum(a.id) },
    { label: "修改封面", icon: "i-image", onClick: () => setCoverAlbum(a) },
    ...(platform.isRemote
      ? [
          { label: "分享相册", icon: "i-shares", onClick: () => actions.shareAlbum(a) },
          { label: "权限", icon: "i-users", onClick: () => setAclAlbum(a) },
        ]
      : []),
    { label: "重命名", icon: "i-edit", onClick: () => renameAlbum(a) },
    { label: "删除相册", icon: "i-trash", danger: true, onClick: () => deleteAlbum(a) },
  ];

  const renameAlbum = (a: AlbumInfo) => {
    setRenameOpen({ album: a, name: a.name });
  };

  const doRename = async () => {
    if (!renameOpen || !renameOpen.name.trim()) return;
    try {
      await api.albumRename(renameOpen.album.id, renameOpen.name.trim());
      toast.success("已重命名");
      setRenameOpen(null);
      reload();
    } catch (e) {
      toast.error(String(e));
    }
  };

  const toggleAlbum = (id: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  return (
    <div>
      <div className="view-head">
        <h2>相册</h2>
        <span className="badge">{filtered.length} 个</span>
        <span style={{ flex: 1 }} />
        <label className="hint">
          排序
          <select value={sortMode} onChange={(e) => setSortMode(e.target.value)}>
            <option value="created-desc">创建时间 · 降序</option>
            <option value="created-asc">创建时间 · 升序</option>
            <option value="name-asc">名称 · 升序</option>
            <option value="name-desc">名称 · 降序</option>
          </select>
        </label>
        <button className="primary" onClick={() => setCreateOpen(true)}>
          <Ic name="i-folder-plus" /> 新建相册
        </button>
      </div>

      <div className="album-wall">
        {filtered.map((a) => {
          const sel = selected.has(a.id);
          return (
            <div
              key={a.id}
              className={`album-card${sel ? " sel" : ""}`}
              onClick={() => (selected.size > 0 ? toggleAlbum(a.id) : openAlbum(a.id))}
              onContextMenu={(e) => {
                e.preventDefault();
                setCtx({ x: e.clientX, y: e.clientY, album: a });
              }}
            >
              <div className="album-cover">
                {covers[a.id] ? (
                  <ThumbCover hash={covers[a.id]} />
                ) : (
                  <div className="thumb-placeholder">📁</div>
                )}
              </div>
              <div className="album-card-name">
                {a.name}
                <span className="album-card-count">{a.blob_count}</span>
              </div>
              <span
                className="check"
                onClick={(e) => {
                  e.stopPropagation();
                  toggleAlbum(a.id);
                }}
              >
                {sel ? "✓" : ""}
              </span>
            </div>
          );
        })}
      </div>

      {selected.size > 0 && (
        <div className="selbar">
          <span className="sel-count">已选 {selected.size} 个相册</span>
          {platform.isRemote && (
            <button
              onClick={() => {
                const list = albums.filter((a) => selected.has(a.id));
                (async () => {
                  for (const a of list) {
                    await actions.shareAlbum(a);
                  }
                })();
              }}
            >
              <Ic name="i-shares" /> 分享
            </button>
          )}
          <button
            className="danger"
            onClick={() => {
              const list = albums.filter((a) => selected.has(a.id));
              actions.confirmAsk(
                "删除相册",
                `删除选中的 ${list.length} 个相册？照片仍保留在全部照片。`,
                async () => {
                  for (const a of list) await api.albumDelete(a.id).catch(() => {});
                  toast.success("已删除");
                  setSelected(new Set());
                  reload();
                },
                true
              );
            }}
          >
            <Ic name="i-trash" /> 删除相册
          </button>
          <button className="primary" onClick={() => setSelected(new Set())}>
            取消
          </button>
        </div>
      )}

      {/* 新建相册 */}
      {createOpen && (
        <div className="modal-backdrop" onClick={() => setCreateOpen(false)}>
          <div className="modal" style={{ width: 400 }} onClick={(e) => e.stopPropagation()}>
            <h3>新建相册</h3>
            <input
              autoFocus
              placeholder="相册名称"
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && create()}
              style={{ width: "100%" }}
            />
            <div className="row" style={{ justifyContent: "flex-end", marginTop: 14 }}>
              <button onClick={() => setCreateOpen(false)}>取消</button>
              <button className="primary" onClick={create} disabled={!newName.trim()}>
                创建
              </button>
            </div>
          </div>
        </div>
      )}

      {/* 重命名相册 */}
      {renameOpen && (
        <div className="modal-backdrop" onClick={() => setRenameOpen(null)}>
          <div className="modal" style={{ width: 400 }} onClick={(e) => e.stopPropagation()}>
            <h3>重命名相册</h3>
            <input
              autoFocus
              value={renameOpen.name}
              onChange={(e) => setRenameOpen({ ...renameOpen, name: e.target.value })}
              onKeyDown={(e) => e.key === "Enter" && doRename()}
              style={{ width: "100%" }}
            />
            <div className="row" style={{ justifyContent: "flex-end", marginTop: 14 }}>
              <button onClick={() => setRenameOpen(null)}>取消</button>
              <button className="primary" onClick={doRename} disabled={!renameOpen.name.trim()}>
                确定
              </button>
            </div>
          </div>
        </div>
      )}

      {/* 修改封面：从相册内照片中选一张 */}
      {coverAlbum && <CoverPicker album={coverAlbum} onClose={() => setCoverAlbum(null)} />}

      {ctx && <CtxMenu pos={{ x: ctx.x, y: ctx.y }} items={albumMenu(ctx.album)} onClose={() => setCtx(null)} />}
      {aclAlbum && <AclEditor album={aclAlbum} onClose={() => setAclAlbum(null)} />}
    </div>
  );
}

// ---------------- 修改封面（右键相册 → 修改封面；从相册内照片中选） ----------------

function CoverPicker({ album, onClose }: { album: AlbumInfo; onClose: () => void }) {
  const toast = useToast();
  const [blobs, setBlobs] = useState<BlobEntry[]>([]);

  useEffect(() => {
    api
      .listAlbumBlobs(album.id)
      .then((list) => setBlobs(list.filter((b) => b.mime.startsWith("image/"))))
      .catch((e) => toast.error(String(e)));
  }, [album.id, toast]);

  const pick = async (hash: string) => {
    try {
      await api.albumSetCover(album.id, hash);
      toast.success(`「${album.name}」封面已更新`);
      onClose();
    } catch (e) {
      toast.error(String(e));
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" style={{ width: 620 }} onClick={(e) => e.stopPropagation()}>
        <h3>修改封面 · {album.name}</h3>
        {blobs.length === 0 ? (
          <p className="hint">相册内还没有照片，无法设置封面。</p>
        ) : (
          <div className="grid" style={{ maxHeight: "60vh", overflowY: "auto", marginTop: 10 }}>
            {blobs.map((b) => (
              <div key={b.blob_hash} className="cell" onClick={() => pick(b.blob_hash)} title={b.file_name}>
                <CoverThumb hash={b.blob_hash} />
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function CoverThumb({ hash }: { hash: string }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    api.thumbnailUrl(hash).then(setSrc).catch(() => {});
  }, [hash]);
  return src ? <img src={src} alt="" /> : <div className="thumb-placeholder">🖼</div>;
}

function ThumbCover({ hash }: { hash: string }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    api.thumbnailUrl(hash).then(setSrc).catch(() => {});
  }, [hash]);
  return src ? <img src={src} alt="" /> : <div className="thumb-placeholder">🖼</div>;
}

// ---------------- 相册详情 ----------------

function AlbumDetail({ albumId }: { albumId: string }) {
  const actions = usePhotoActions();
  const toast = useToast();
  const [album, setAlbum] = useState<AlbumInfo | null>(null);
  const [blobs, setBlobs] = useState<BlobEntry[]>([]);
  const [aclOpen, setAclOpen] = useState(false);

  useEffect(() => {
    api
      .listAlbums()
      .then((list) => setAlbum(list.find((a) => a.id === albumId) ?? null))
      .catch(() => {});
  }, [albumId, actions.dataVersion]);

  useEffect(() => {
    api
      .listAlbumBlobs(albumId)
      .then((list) => setBlobs([...list].sort((a, b) => b.taken_at - a.taken_at)))
      .catch(() => {});
  }, [albumId, actions.dataVersion]);

  const openCtx = (e: React.MouseEvent, b: BlobEntry) => {
    e.preventDefault();
    e.stopPropagation();
    setCtxMenu({ x: e.clientX, y: e.clientY, blob: b });
  };
  const [ctxMenu, setCtxMenu] = useState<{ x: number; y: number; blob: BlobEntry } | null>(null);

  const removeFromAlbum = async (b: BlobEntry) => {
    try {
      await api.albumRemoveBlob(albumId, b.blob_hash);
      toast.success("已从相册移除");
      actions.onChanged();
    } catch (e) {
      toast.error(String(e));
    }
  };

  const menuItems = (b: BlobEntry): MenuItem[] => {
    const base: MenuItem[] = [
      { label: "预览", icon: "i-preview", onClick: () => actions.openViewer(blobs, blobs.indexOf(b)) },
      { label: b.favorite ? "取消收藏" : "收藏", icon: "i-heart", onClick: () => actions.fav(b) },
      { label: "重命名", icon: "i-edit", onClick: () => actions.rename(b) },
      { label: "下载原图", icon: "i-download", onClick: () => actions.downloadBlob(b) },
      { label: "详情", icon: "i-box", onClick: () => actions.detail(b) },
      { label: "从本相册移除", icon: "i-trash", onClick: () => removeFromAlbum(b) },
      { label: "移入回收站", icon: "i-trash", danger: true, onClick: () => actions.trash(b) },
    ];
    return base;
  };

  return (
    <div>
      <div className="view-head">
        <a className="back-btn" onClick={() => (window.location.hash = "#/albums")}>
          ← 返回相册
        </a>
        <h2>{album?.name ?? "相册"}</h2>
        <span className="badge">{blobs.length} 张</span>
        <span style={{ flex: 1 }} />
        {platform.isRemote && (
          <>
            <button onClick={() => setAclOpen(true)}>
              <Ic name="i-users" /> 权限
            </button>
            <button onClick={() => album && actions.shareAlbum(album)}>
              <Ic name="i-shares" /> 分享相册
            </button>
            <button
              className="danger"
              onClick={() => {
                if (album) {
                  window.location.hash = "#/albums";
                  window.setTimeout(() => {
                    actions.confirmAsk(
                      "删除相册",
                      `删除相册「${album.name}」？照片仍保留在全部照片。`,
                      async () => {
                        await api.albumDelete(album.id).catch((e) => toast.error(String(e)));
                        actions.onChanged();
                      },
                      true
                    );
                  }, 50);
                }
              }}
            >
              <Ic name="i-trash" /> 删除相册
            </button>
          </>
        )}
      </div>
      <PhotoGrid
        blobs={blobs}
        onOpen={(_b, idx) => actions.openViewer(blobs, idx)}
        onCtx={openCtx}
        emptyHint="该相册还没有照片。在时间线里右键照片 →「复制到相册」，或直接上传。"
      />
      {ctxMenu && <CtxMenu pos={{ x: ctxMenu.x, y: ctxMenu.y }} items={menuItems(ctxMenu.blob)} onClose={() => setCtxMenu(null)} />}
      {aclOpen && album && <AclEditor album={album} onClose={() => setAclOpen(false)} />}
    </div>
  );
}

// ---------------- ACL 编辑器（远端） ----------------

const PERM_LABEL: Record<string, string> = { none: "隐藏", view: "只看", upload: "上传", manage: "管理" };

function AclEditor({ album, onClose }: { album: AlbumInfo; onClose: () => void }) {
  const toast = useToast();
  const [acl, setAcl] = useState<AclEntry[]>([]);
  const [users, setUsers] = useState<{ id: string; name: string }[]>([]);
  const [groups, setGroups] = useState<{ id: string; name: string }[]>([]);
  const [tab, setTab] = useState<"user" | "group">("user");

  useEffect(() => {
    api
      .getAcl(album.id)
      .then(setAcl)
      .catch(() => setAcl([]));
    api
      .adminUsers()
      .then((us) => setUsers(us.map((u) => ({ id: u.id, name: u.name }))))
      .catch(() => {});
    api
      .adminGroups()
      .then((gs) => setGroups(gs.map((g) => ({ id: g.id, name: g.name }))))
      .catch(() => {});
  }, [album.id]);

  const cur = (kind: string, id: string) => acl.find((a) => a.principal.type === kind && a.principal.id === id)?.perm ?? null;

  const set = async (kind: "user" | "group", id: string, perm: string) => {
    try {
      await api.setAcl(album.id, kind, id, perm === "" ? null : (perm as AclEntry["perm"]));
      const next = [...acl.filter((a) => !(a.principal.type === kind && a.principal.id === id))];
      if (perm !== "") next.push({ principal: { type: kind, id }, perm: perm as AclEntry["perm"] });
      setAcl(next);
      toast.success("权限已更新");
    } catch (e) {
      toast.error(String(e));
    }
  };

  const rows = tab === "user" ? users.map((u) => ({ kind: "user" as const, ...u })) : groups.map((g) => ({ kind: "group" as const, ...g }));

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" style={{ width: 560 }} onClick={(e) => e.stopPropagation()}>
        <h3>相册权限 · {album.name}</h3>
        <p className="hint">
          子相册默认继承父级；显式设置优先于继承；用户设置优先于用户组。
          {` `}根相册默认仅 family 组拥有管理权限。
        </p>
        <div className="acl-tabs">
          <button className={tab === "user" ? "active" : ""} onClick={() => setTab("user")}>
            <Ic name="i-users" /> 用户
          </button>
          <button className={tab === "group" ? "active" : ""} onClick={() => setTab("group")}>
            <Ic name="i-groups" /> 用户组
          </button>
        </div>
        <table className="admin">
          <thead>
            <tr>
              <th>对象</th>
              <th>当前</th>
              <th>设置</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => {
              const c = cur(r.kind, r.id);
              return (
                <tr key={r.id}>
                  <td>{r.kind === "user" ? "👤" : "🏷"} {r.name}</td>
                  <td>{c ? PERM_LABEL[c] || c : "继承"}</td>
                  <td>
                    <select className="perm-select" value={c ?? ""} onChange={(e) => set(r.kind, r.id, e.target.value)}>
                      <option value="">继承</option>
                      <option value="none">隐藏</option>
                      <option value="view">只看</option>
                      <option value="upload">上传</option>
                      <option value="manage">管理</option>
                    </select>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
        <div className="row" style={{ justifyContent: "flex-end", marginTop: 14 }}>
          <button className="primary" onClick={onClose}>
            完成
          </button>
        </div>
      </div>
    </div>
  );
}
