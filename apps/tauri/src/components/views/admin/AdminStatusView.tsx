// 系统状态（网页端 viewAdminStatus 同款）：统计表 + 立即同步。
import { useCallback, useEffect, useState } from "react";
import { api, AdminStatus } from "../../../lib/api";
import { usePhotoActions } from "../../PhotoActions";
import { Ic } from "../../icons";
import { useToast } from "../../Toast";

export function AdminStatusView() {
  const actions = usePhotoActions();
  const toast = useToast();
  const [st, setSt] = useState<AdminStatus | null>(null);
  const [out, setOut] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);

  const reload = useCallback(() => {
    api
      .adminStatus()
      .then(setSt)
      .catch((e) => toast.error(String(e)));
  }, [toast]);

  useEffect(() => {
    reload();
  }, [reload, actions.dataVersion]);

  const runSync = async () => {
    setBusy(true);
    try {
      const r = (await api.syncNow()) as unknown as { sources?: { source?: string; added?: number; moved?: number; deleted?: number }[] };
      if (r.sources && Array.isArray(r.sources)) {
        setOut(r.sources.map((s) => `${s.source ?? "?"}：新增 ${s.added ?? 0} · 移动 ${s.moved ?? 0} · 删除 ${s.deleted ?? 0}`));
      } else {
        setOut([`拉取 ${(r as unknown as { pulled_objects?: number }).pulled_objects ?? 0} · 应用 ${(r as unknown as { applied_objects?: number }).applied_objects ?? 0}`]);
      }
      toast.success("同步完成");
      reload();
    } catch (e) {
      toast.error(`同步失败：${String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  const rows: [string, unknown][] = st
    ? ([
        ["用户", st.users],
        ["用户组", st.groups],
        ["相册", st.albums],
        ["照片", st.blobs ?? st.photos],
        ["回收站", st.trash],
        ["分享", st.shares],
        ["同步源", st.sources],
        ["同步间隔（秒）", st.sync_interval_secs],
      ] as [string, unknown][])
    : [];

  return (
    <div>
      <div className="view-head">
        <h2>系统状态</h2>
        <span style={{ flex: 1 }} />
        <button className="primary" onClick={runSync} disabled={busy}>
          <Ic name="i-sync" /> {busy ? "同步中…" : "立即同步"}
        </button>
      </div>
      {st ? (
        <table className="admin">
          <tbody>
            {rows.map(([k, v]) => (
              <tr key={k}>
                <td style={{ color: "var(--muted)", width: 180 }}>{k}</td>
                <td>{v === undefined || v === null ? "—" : String(v)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : (
        <p className="hint">加载中…</p>
      )}
      {out.length > 0 && (
        <div className="form-card" style={{ flexDirection: "column", alignItems: "flex-start" }}>
          {out.map((line, i) => (
            <span key={i} className="hint">
              {line}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}
