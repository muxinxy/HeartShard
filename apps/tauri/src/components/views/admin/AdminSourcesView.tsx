// 同步源（网页端 viewAdminSources 同款）：列表 + 添加表单（kind 动态字段）。
import { useCallback, useEffect, useState } from "react";
import { api, SourceInfo } from "../../../lib/api";
import { usePhotoActions } from "../../PhotoActions";
import { Ic } from "../../icons";
import { useToast } from "../../Toast";

const KIND_LABEL: Record<string, string> = { local: "本地目录", s3: "S3 兼容", webdav: "WebDAV", openlist: "OpenList" };

export function AdminSourcesView() {
  const actions = usePhotoActions();
  const toast = useToast();
  const [sources, setSources] = useState<SourceInfo[]>([]);
  const [id, setId] = useState("");
  const [kind, setKind] = useState("local");
  const [root, setRoot] = useState("");
  const [endpoint, setEndpoint] = useState("");
  const [bucket, setBucket] = useState("");
  const [user, setUser] = useState("");
  const [pass, setPass] = useState("");
  const [token, setToken] = useState("");
  const [base, setBase] = useState("");
  const [strategy, setStrategy] = useState("append");
  const [delMode, setDelMode] = useState("keep");

  const reload = useCallback(() => {
    api
      .adminSources()
      .then((s) => setSources(Array.isArray(s) ? s : []))
      .catch((e) => toast.error(String(e)));
  }, [toast]);

  useEffect(() => {
    reload();
  }, [reload, actions.dataVersion]);

  const add = async () => {
    if (!id.trim()) return;
    const payload: SourceInfo = { id: id.trim(), kind, strategy, delete_mode: delMode };
    if (kind === "local") payload.root = root;
    if (kind === "s3") Object.assign(payload, { endpoint, bucket, access_key: user, secret_key: pass });
    if (kind === "webdav") Object.assign(payload, { base_url: endpoint, username: user, password: pass });
    if (kind === "openlist") Object.assign(payload, { base_url: endpoint, token, base_path: base || "/heartshard" });
    try {
      await api.saveAdminSources([...sources, payload]);
      toast.success("同步源已添加（按计划轮询）");
      setId("");
      setRoot("");
      setEndpoint("");
      setBucket("");
      setUser("");
      setPass("");
      setToken("");
      setBase("");
      reload();
    } catch (e) {
      toast.error(String(e));
    }
  };

  const del = (s: SourceInfo) => {
    actions.confirmAsk("删除同步源", `删除同步源「${s.id}」？（已同步的照片保留）`, async () => {
      try {
        await api.saveAdminSources(sources.filter((x) => x.id !== s.id));
        toast.success("已删除");
        reload();
      } catch (e) {
        toast.error(String(e));
      }
    }, true);
  };

  return (
    <div>
      <div className="view-head">
        <h2>同步源</h2>
        <span className="badge">{sources.length} 个</span>
        <span className="hint">按 server.toml 的 sync_interval_secs 轮询；凭据加密存储。</span>
      </div>

      {sources.map((s) => (
        <div className="form-card" key={s.id}>
          <strong>{s.id}</strong>
          <span className="badge">{KIND_LABEL[s.kind] ?? s.kind}</span>
          <span className="hint">{s.root || s.endpoint || ""}</span>
          <span className="badge">{s.strategy ?? "append"}</span>
          <span className="badge">删除 {s.delete_mode === "trash" ? "入回收站" : "保留"}</span>
          <span style={{ flex: 1 }} />
          <button className="danger" onClick={() => del(s)}>
            删除
          </button>
        </div>
      ))}

      <div className="form-card" style={{ alignItems: "flex-start", flexDirection: "column" }}>
        <strong>
          <Ic name="i-sync" /> 添加同步源
        </strong>
        <div className="row">
          <input placeholder="源 ID（如 photos）" value={id} onChange={(e) => setId(e.target.value)} style={{ width: 180 }} />
          <select value={kind} onChange={(e) => setKind(e.target.value)}>
            {Object.entries(KIND_LABEL).map(([k, v]) => (
              <option key={k} value={k}>
                {v}
              </option>
            ))}
          </select>
          {kind === "local" && <input placeholder="本地目录路径" value={root} onChange={(e) => setRoot(e.target.value)} style={{ flex: 1 }} />}
          {kind !== "local" && <input placeholder="端点地址" value={endpoint} onChange={(e) => setEndpoint(e.target.value)} style={{ flex: 1 }} />}
          {kind === "s3" && <input placeholder="bucket" value={bucket} onChange={(e) => setBucket(e.target.value)} />}
          {kind === "openlist" && <input placeholder="令牌 token" value={token} onChange={(e) => setToken(e.target.value)} style={{ flex: 1 }} />}
          {kind === "openlist" && <input placeholder="base_path（/heartshard）" value={base} onChange={(e) => setBase(e.target.value)} />}
        </div>
        <div className="row">
          {(kind === "s3" || kind === "webdav") && (
            <>
              <input placeholder="用户 / Access Key" value={user} onChange={(e) => setUser(e.target.value)} />
              <input type="password" placeholder="密码 / Secret Key" value={pass} onChange={(e) => setPass(e.target.value)} />
            </>
          )}
          <label>
            策略
            <select value={strategy} onChange={(e) => setStrategy(e.target.value)}>
              <option value="append">append（仅新增）</option>
              <option value="mirror">mirror（完全同步）</option>
            </select>
          </label>
          <label>
            删除
            <select value={delMode} onChange={(e) => setDelMode(e.target.value)}>
              <option value="keep">保留</option>
              <option value="trash">入回收站</option>
            </select>
          </label>
          <button className="primary" onClick={add} disabled={!id.trim()}>
            添加
          </button>
        </div>
      </div>
    </div>
  );
}
