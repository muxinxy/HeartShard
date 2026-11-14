// 系统状态（重排版）：图表化仪表盘 —— 库内统计柱状图 + 媒体构成环形图 + 明细表 + 立即同步。
import { useCallback, useEffect, useState } from "react";
import {
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  Legend,
  Pie,
  PieChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { api, AdminStatus, BlobEntry } from "../../../lib/api";
import { usePhotoActions } from "../../PhotoActions";
import { Ic } from "../../icons";
import { useToast } from "../../Toast";

const C = {
  primary: "#5b8def",
  success: "#7ed6a0",
  warn: "#f0b45c",
  danger: "#ef6a6a",
  muted: "#8a8f98",
};

export function AdminStatusView() {
  const actions = usePhotoActions();
  const toast = useToast();
  const [st, setSt] = useState<AdminStatus | null>(null);
  const [blobs, setBlobs] = useState<BlobEntry[]>([]);
  const [out, setOut] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);

  const reload = useCallback(() => {
    api
      .adminStatus()
      .then(setSt)
      .catch((e) => toast.error(String(e)));
    api
      .listBlobs()
      .then(setBlobs)
      .catch(() => {});
  }, [toast]);

  useEffect(() => {
    reload();
  }, [reload, actions.dataVersion]);

  const runSync = async () => {
    setBusy(true);
    try {
      const r = (await api.syncNow()) as unknown as {
        sources?: { source?: string; added?: number; moved?: number; deleted?: number }[];
        pulled_objects?: number;
        applied_objects?: number;
      };
      if (r.sources && Array.isArray(r.sources) && r.sources.length > 0) {
        setOut(r.sources.map((s) => `${s.source ?? "?"}：新增 ${s.added ?? 0} · 移动 ${s.moved ?? 0} · 删除 ${s.deleted ?? 0}`));
      } else {
        setOut([`拉取 ${r.pulled_objects ?? 0} · 应用 ${r.applied_objects ?? 0}`]);
      }
      toast.success("同步完成");
      reload();
    } catch (e) {
      toast.error(`同步失败：${String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  // 图表数据
  const n = (k: string) => Number(st?.[k] ?? 0);
  const barData = st
    ? [
        { name: "用户", 值: n("users") },
        { name: "用户组", 值: n("groups") },
        { name: "相册", 值: n("albums") },
        { name: "照片", 值: n("blobs") ?? n("photos") },
        { name: "回收站", 值: n("trash") },
        { name: "分享", 值: n("shares") },
        { name: "同步源", 值: n("sources") },
      ]
    : [];
  const photos = blobs.filter((b) => b.mime.startsWith("image/") && !b.is_motion).length;
  const videos = blobs.filter((b) => b.mime.startsWith("video/")).length;
  const motions = blobs.filter((b) => b.is_motion).length;
  const pieData = [
    { name: "照片", 值: photos, color: C.primary },
    { name: "视频", 值: videos, color: C.warn },
    { name: "动态照片", 值: motions, color: C.success },
  ].filter((d) => d.值 > 0);
  const total = photos + videos + motions;
  const gb = blobs.reduce((s, b) => s + b.total_len, 0);

  const tooltipStyle = {
    background: "var(--card)",
    border: "1px solid var(--border)",
    borderRadius: 10,
    color: "var(--fg)",
    fontSize: 13,
  };

  return (
    <div>
      <div className="view-head">
        <h2>系统状态</h2>
        {total > 0 && (
          <span className="badge">
            库内 {total} 个文件 · {(gb / 1024 / 1024 / 1024).toFixed(2)} GB
          </span>
        )}
        <span style={{ flex: 1 }} />
        <button className="primary" onClick={runSync} disabled={busy}>
          <Ic name="i-sync" /> {busy ? "同步中…" : "立即同步"}
        </button>
      </div>

      {st ? (
        <>
          <div className="status-charts">
            <div className="status-card">
              <h3>库内统计</h3>
              <ResponsiveContainer width="100%" height={230}>
                <BarChart data={barData} margin={{ top: 8, right: 8, left: -16, bottom: 0 }}>
                  <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" vertical={false} />
                  <XAxis dataKey="name" tick={{ fontSize: 12, fill: C.muted }} axisLine={false} tickLine={false} />
                  <YAxis tick={{ fontSize: 12, fill: C.muted }} axisLine={false} tickLine={false} allowDecimals={false} />
                  <Tooltip cursor={{ fill: "rgba(128,128,128,.12)" }} contentStyle={tooltipStyle} />
                  <Bar dataKey="值" radius={[6, 6, 0, 0]} fill={C.primary} barSize={28} />
                </BarChart>
              </ResponsiveContainer>
            </div>
            <div className="status-card">
              <h3>媒体构成</h3>
              {pieData.length > 0 ? (
                <ResponsiveContainer width="100%" height={230}>
                  <PieChart>
                    <Pie data={pieData} dataKey="值" nameKey="name" innerRadius={55} outerRadius={85} paddingAngle={3}>
                      {pieData.map((d) => (
                        <Cell key={d.name} fill={d.color} />
                      ))}
                    </Pie>
                    <Legend wrapperStyle={{ fontSize: 13, color: C.muted }} />
                    <Tooltip contentStyle={tooltipStyle} />
                  </PieChart>
                </ResponsiveContainer>
              ) : (
                <p className="hint" style={{ textAlign: "center", marginTop: 90 }}>
                  暂无照片数据
                </p>
              )}
            </div>
          </div>

          <table className="admin" style={{ marginTop: 16 }}>
            <tbody>
              {barData.map((d) => (
                <tr key={d.name}>
                  <td style={{ color: "var(--muted)", width: 180 }}>{d.name}</td>
                  <td>{d.值}</td>
                </tr>
              ))}
              {st.sync_interval_secs !== undefined && (
                <tr>
                  <td style={{ color: "var(--muted)" }}>同步间隔（秒）</td>
                  <td>{String(st.sync_interval_secs)}</td>
                </tr>
              )}
            </tbody>
          </table>
        </>
      ) : (
        <p className="hint">加载中…</p>
      )}

      {out.length > 0 && (
        <div className="form-card" style={{ flexDirection: "column", alignItems: "flex-start" }}>
          <strong>本次同步</strong>
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
