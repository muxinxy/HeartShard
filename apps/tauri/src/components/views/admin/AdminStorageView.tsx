// 存储管理（网页端 viewAdminStorage 同款）：列表 + 测连通 + 删除 + 添加表单。
import { useCallback, useEffect, useState } from "react";
import { api, StorageStatus } from "../../../lib/api";
import { usePhotoActions } from "../../PhotoActions";
import { Ic } from "../../icons";
import { useToast } from "../../Toast";

const KIND_LABEL: Record<string, string> = { local: "本地目录", s3: "S3 兼容", webdav: "WebDAV", openlist: "OpenList" };

export function AdminStorageView() {
  const actions = usePhotoActions();
  const toast = useToast();
  const [storages, setStorages] = useState<StorageStatus[]>([]);
  const [kind, setKind] = useState("local");
  const [id, setId] = useState("");
  const [root, setRoot] = useState("");
  const [endpoint, setEndpoint] = useState("");
  const [bucket, setBucket] = useState("");
  const [region, setRegion] = useState("");
  const [user, setUser] = useState("");
  const [pass, setPass] = useState("");
  const [token, setToken] = useState("");
  const [base, setBase] = useState("/heartshard");

  const reload = useCallback(() => {
    api
      .storageStatus()
      .then(setStorages)
      .catch((e) => toast.error(String(e)));
  }, [toast]);

  useEffect(() => {
    reload();
  }, [reload, actions.dataVersion]);

  const verify = async (sid: string) => {
    try {
      const msg = await api.verifyStorageAdmin(sid);
      toast.success(`${sid}：${msg || "连通正常"}`);
    } catch (e) {
      toast.error(`${sid}：${String(e)}`);
    }
  };

  const del = (sid: string) => {
    actions.confirmAsk("删除存储", `删除存储「${sid}」？（已存分片保留，可后续迁移）`, async () => {
      try {
        await api.removeStorage(sid);
        toast.success("已删除");
        reload();
      } catch (e) {
        toast.error(String(e));
      }
    }, true);
  };

  const add = async () => {
    if (!id.trim()) return;
    const cfg: Record<string, unknown> = {};
    try {
      if (kind === "local") {
        await api.addStorageLocal(id.trim(), root);
      } else if (kind === "s3") {
        Object.assign(cfg, { endpoint, bucket, access_key: user, secret_key: pass, region });
        await api.addStorageS3(cfg);
      } else if (kind === "webdav") {
        Object.assign(cfg, { base_url: endpoint, username: user, password: pass });
        await api.addStorageWebdav(cfg);
      } else if (kind === "openlist") {
        Object.assign(cfg, { base_url: endpoint, token, base_path: base });
        await api.addStorageOpenList(cfg);
      }
      toast.success(`存储「${id.trim()}」已添加`);
      setId("");
      setRoot("");
      setEndpoint("");
      setBucket("");
      setUser("");
      setPass("");
      setToken("");
      reload();
    } catch (e) {
      toast.error(String(e));
    }
  };

  return (
    <div>
      <div className="view-head">
        <h2>存储管理</h2>
        <span className="badge">{storages.length} 个</span>
        <span className="hint">同一 blob 的分片会强制落在不同存储。</span>
      </div>

      <table className="admin">
        <thead>
          <tr>
            <th>ID</th>
            <th>类型</th>
            <th style={{ width: 200 }}>操作</th>
          </tr>
        </thead>
        <tbody>
          {storages.map((s) => (
            <tr key={s.id}>
              <td>{s.id}</td>
              <td>
                <span className="badge">{KIND_LABEL[s.kind] ?? s.kind}</span>
              </td>
              <td>
                <button onClick={() => verify(s.id)}>
                  <Ic name="i-preview" /> 测连通
                </button>{" "}
                <button className="danger" onClick={() => del(s.id)}>
                  <Ic name="i-trash" /> 删除
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      <div className="form-card" style={{ alignItems: "flex-start", flexDirection: "column", marginTop: 20 }}>
        <strong>
          <Ic name="i-storage" /> 添加存储
        </strong>
        <div className="row">
          <select value={kind} onChange={(e) => setKind(e.target.value)}>
            {Object.entries(KIND_LABEL).map(([k, v]) => (
              <option key={k} value={k}>
                {v}
              </option>
            ))}
          </select>
          <input placeholder="存储 ID" value={id} onChange={(e) => setId(e.target.value)} style={{ width: 160 }} />
          {kind === "local" && <input placeholder="本地目录" value={root} onChange={(e) => setRoot(e.target.value)} style={{ flex: 1 }} />}
          {kind !== "local" && <input placeholder="端点地址" value={endpoint} onChange={(e) => setEndpoint(e.target.value)} style={{ flex: 1 }} />}
          {kind === "s3" && <input placeholder="bucket" value={bucket} onChange={(e) => setBucket(e.target.value)} />}
          {kind === "s3" && <input placeholder="region（可选）" value={region} onChange={(e) => setRegion(e.target.value)} />}
        </div>
        <div className="row">
          {(kind === "s3" || kind === "webdav") && (
            <>
              <input placeholder="用户 / Access Key" value={user} onChange={(e) => setUser(e.target.value)} />
              <input type="password" placeholder="密码 / Secret Key" value={pass} onChange={(e) => setPass(e.target.value)} />
            </>
          )}
          {kind === "openlist" && (
            <>
              <input placeholder="令牌 token" value={token} onChange={(e) => setToken(e.target.value)} style={{ flex: 1 }} />
              <input placeholder="base_path" value={base} onChange={(e) => setBase(e.target.value)} />
            </>
          )}
          <button className="primary" onClick={add} disabled={!id.trim()}>
            添加
          </button>
        </div>
      </div>
    </div>
  );
}
