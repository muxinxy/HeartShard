// 分享列表（网页端 viewShares 同款）：状态列 / 内容预览 / 复制链接 / 取消分享。
import { useCallback, useEffect, useMemo, useState } from "react";
import { api, remoteUrl, ShareInfo, ShareMeta } from "../../lib/api";
import { usePhotoActions } from "../PhotoActions";
import { Ic } from "../icons";
import { useToast } from "../Toast";

const PAGE_SIZE = 10;

export function SharesView() {
  const actions = usePhotoActions();
  const toast = useToast();
  const [shares, setShares] = useState<ShareInfo[]>([]);
  const [page, setPage] = useState(0);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [content, setContent] = useState<ShareMeta | null>(null);

  const reload = useCallback(() => {
    api
      .listShares()
      .then((list) => setShares([...list].sort((a, b) => (b.created_at ?? 0) - (a.created_at ?? 0))))
      .catch((e) => toast.error(String(e)));
  }, [toast]);

  useEffect(() => {
    reload();
  }, [reload, actions.dataVersion]);

  const expired = (s: ShareInfo) => s.expires_at != null && Date.now() / 1000 >= s.expires_at;
  const pages = Math.max(1, Math.ceil(shares.length / PAGE_SIZE));
  const rows = useMemo(() => shares.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE), [shares, page]);

  const copyLink = async (s: ShareInfo) => {
    try {
      await navigator.clipboard.writeText(`${remoteUrl()}/s/${s.id}`);
      toast.success("链接已复制");
    } catch {
      toast.error("复制失败");
    }
  };

  const cancel = (s: ShareInfo) => {
    actions.confirmAsk("取消分享", `取消分享「${s.album_name ?? s.name ?? s.id}」？链接将立即失效。`, async () => {
      try {
        await api.cancelShare(s.id);
        toast.success("已取消分享");
        reload();
      } catch (e) {
        toast.error(String(e));
      }
    }, true);
  };

  const showContent = async (s: ShareInfo) => {
    try {
      const meta = await api.shareMeta(s.id);
      setContent(meta);
    } catch (e) {
      toast.error(String(e));
    }
  };

  const toggle = (id: string) => {
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
        <h2>分享</h2>
        <span className="badge">{shares.length} 个</span>
        {selected.size > 0 && (
          <>
            <button
              onClick={async () => {
                for (const id of selected) {
                  await navigator.clipboard.writeText(`${remoteUrl()}/s/${id}`).catch(() => {});
                }
                toast.success("已复制选中链接");
              }}
            >
              <Ic name="i-shares" /> 复制选中链接
            </button>
            <button
              className="danger"
              onClick={() => {
                actions.confirmAsk("取消分享", `取消选中的 ${selected.size} 个分享？`, async () => {
                  for (const id of selected) await api.cancelShare(id).catch(() => {});
                  toast.success("已取消分享");
                  setSelected(new Set());
                  reload();
                }, true);
              }}
            >
              <Ic name="i-trash" /> 取消分享选中
            </button>
          </>
        )}
      </div>

      <table className="admin">
        <thead>
          <tr>
            <th style={{ width: 36 }}>
              <input
                type="checkbox"
                checked={rows.length > 0 && rows.every((s) => selected.has(s.id))}
                onChange={(e) =>
                  setSelected(e.target.checked ? new Set(rows.map((s) => s.id)) : new Set())
                }
              />
            </th>
            <th>名称</th>
            <th>类型</th>
            <th>数量</th>
            <th>分享码</th>
            <th>过期</th>
            <th>状态</th>
            <th>内容</th>
            <th style={{ width: 190 }}>操作</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((s) => (
            <tr key={s.id}>
              <td>
                <input type="checkbox" checked={selected.has(s.id)} onChange={() => toggle(s.id)} />
              </td>
              <td>{s.album_name ?? s.name ?? s.id.slice(0, 8)}</td>
              <td>{s.kind === "album" ? "相册" : "照片"}</td>
              <td>{s.blob_hashes.length}</td>
              <td>{s.access_code_hash ? "有" : "—"}</td>
              <td>{s.expires_at ? new Date(s.expires_at * 1000).toLocaleString() : "永久"}</td>
              <td>
                <span className={`badge ${expired(s) ? "bad" : "ok"}`}>{expired(s) ? "已失效" : "有效"}</span>
              </td>
              <td>
                <button onClick={() => showContent(s)}>查看内容</button>
              </td>
              <td>
                <button onClick={() => copyLink(s)}>复制链接</button>{" "}
                {!expired(s) && (
                  <button className="danger" onClick={() => cancel(s)}>
                    取消分享
                  </button>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      {pages > 1 && (
        <div className="row" style={{ justifyContent: "center", marginTop: 14 }}>
          <button disabled={page === 0} onClick={() => setPage(page - 1)}>
            ‹ 上一页
          </button>
          <span className="hint">
            {page + 1} / {pages}
          </span>
          <button disabled={page >= pages - 1} onClick={() => setPage(page + 1)}>
            下一页 ›
          </button>
        </div>
      )}

      {/* 分享内容预览 */}
      {content && (
        <div className="modal-backdrop" onClick={() => setContent(null)}>
          <div className="modal" style={{ width: 560 }} onClick={(e) => e.stopPropagation()}>
            <h3>分享内容{content.album ? `：${content.album}` : ""}</h3>
            {(content.files || []).length === 0 ? (
              <p className="hint">分享已失效或内容为空。</p>
            ) : (
              <table className="admin">
                <tbody>
                  {content.files.map((it) => (
                    <tr key={it.hash}>
                      <td>{it.name ?? it.hash.slice(0, 10)}</td>
                      <td>{it.taken_at ? new Date(it.taken_at * 1000).toLocaleDateString() : ""}</td>
                      <td>
                        <span className={`badge ${it.sanitized ? "ok" : "warn"}`}>
                          {it.sanitized ? "已去元数据" : "含元数据"}
                        </span>
                      </td>
                      <td>
                        {it.mime && (it.mime.startsWith("image/") || it.mime.startsWith("video/")) && (
                          <button
                            onClick={() => {
                              setContent(null);
                              actions.openViewer(
                                [
                                  {
                                    blob_hash: it.hash,
                                    mime: it.mime || "image/jpeg",
                                    file_name: it.name || it.hash,
                                    favorite: false,
                                    total_len: 0,
                                    taken_at: it.taken_at || 0,
                                    gps_lat: null,
                                    gps_lon: null,
                                  },
                                ],
                                0
                              );
                            }}
                          >
                            预览
                          </button>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
            <div className="row" style={{ justifyContent: "flex-end", marginTop: 12 }}>
              <button className="primary" onClick={() => setContent(null)}>
                关闭
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
