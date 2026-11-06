// §10 存储管理：列表 / 新增本地目录 / S3 / OpenList / 删除。
import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, StorageStatus } from "../lib/api";

type AddMode = "none" | "local" | "s3" | "openlist";

export function StoragePage({ onDone }: { onDone: () => void }) {
  const [storages, setStorages] = useState<StorageStatus[]>([]);
  const [mode, setMode] = useState<AddMode>("none");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // S3 表单
  const [s3Endpoint, setS3Endpoint] = useState("");
  const [s3Bucket, setS3Bucket] = useState("");
  const [s3Key, setS3Key] = useState("");
  const [s3Secret, setS3Secret] = useState("");
  const [s3Region, setS3Region] = useState("us-east-1");
  const [s3PathStyle, setS3PathStyle] = useState(true);

  // OpenList 表单
  const [olUrl, setOlUrl] = useState("");
  const [olUser, setOlUser] = useState("");
  const [olPass, setOlPass] = useState("");
  const [olPath, setOlPath] = useState("/heartshard");

  const refresh = useCallback(async () => {
    try {
      setStorages(await api.storageStatus());
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

  const doPickDir = async () => {
    const picked = await open({ directory: true, title: "选择本地存储目录" });
    if (!picked || typeof picked !== "string") return;
    const id = picked.split(/[\\/]/).filter(Boolean).pop() ?? "local";
    setBusy(true);
    try {
      await api.addStorageLocal(id, picked);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const doAddS3 = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.addStorageS3({
        endpoint: s3Endpoint,
        bucket: s3Bucket,
        access_key: s3Key,
        secret_key: s3Secret,
        region: s3Region,
        path_style: s3PathStyle,
      });
      setMode("none");
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const doAddOpenList = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.addStorageOpenList({
        base_url: olUrl,
        username: olUser,
        password: olPass,
        base_path: olPath,
      });
      setMode("none");
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const doRemove = async (id: string) => {
    setBusy(true);
    try {
      await api.removeStorage(id);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="wizard">
      <header className="toolbar">
        <span className="brand">存储管理</span>
        <span className="spacer" />
        <button onClick={onDone}>返回</button>
      </header>

      {error && <div className="notice error">{error}</div>}

      <h2>已配置的 Storage（{storages.length}）</h2>
      {storages.length === 0 ? (
        <p className="hint">尚无 Storage。导入前至少需要一个。</p>
      ) : (
        <ul className="album-list">
          {storages.map((s) => (
            <li key={s.id} className="album-row">
              <span className="name">{s.id}</span>
              <span className="meta" style={{ marginLeft: "auto", marginRight: 8 }}>{s.kind}</span>
              <button onClick={() => doRemove(s.id)} disabled={busy}>删除</button>
            </li>
          ))}
        </ul>
      )}

      <h2 style={{ marginTop: 24 }}>添加 Storage</h2>
      <div className="wizard-actions" style={{ marginBottom: 16 }}>
        <button onClick={() => setMode(mode === "local" ? "none" : "local")}>本地目录</button>
        <button onClick={() => setMode(mode === "s3" ? "none" : "s3")}>S3 兼容</button>
        <button onClick={() => setMode(mode === "openlist" ? "none" : "openlist")}>OpenList</button>
      </div>

      {mode === "local" && (
        <div className="settings-card">
          <p className="hint">选择一个本地目录作为 Storage（分片将存入该目录下的子路径）。</p>
          <button onClick={doPickDir} disabled={busy}>选择目录…</button>
        </div>
      )}

      {mode === "s3" && (
        <div className="settings-card">
          <label className="setting-row"><span>Endpoint</span>
            <input value={s3Endpoint} onChange={(e) => setS3Endpoint(e.target.value)} placeholder="https://s3.amazonaws.com" /></label>
          <label className="setting-row"><span>Bucket</span>
            <input value={s3Bucket} onChange={(e) => setS3Bucket(e.target.value)} placeholder="my-bucket" /></label>
          <label className="setting-row"><span>Access Key</span>
            <input value={s3Key} onChange={(e) => setS3Key(e.target.value)} /></label>
          <label className="setting-row"><span>Secret Key</span>
            <input type="password" value={s3Secret} onChange={(e) => setS3Secret(e.target.value)} /></label>
          <label className="setting-row"><span>Region</span>
            <input value={s3Region} onChange={(e) => setS3Region(e.target.value)} /></label>
          <label className="setting-row"><span>Path Style（MinIO/R2 勾选）</span>
            <input type="checkbox" checked={s3PathStyle} onChange={(e) => setS3PathStyle(e.target.checked)} /></label>
          <button className="primary" onClick={doAddS3} disabled={busy || !s3Endpoint || !s3Bucket}>
            添加 S3 Storage
          </button>
          <p className="hint">添加前可先用 s3-verify 工具验证连通性。</p>
        </div>
      )}

      {mode === "openlist" && (
        <div className="settings-card">
          <label className="setting-row"><span>Base URL</span>
            <input value={olUrl} onChange={(e) => setOlUrl(e.target.value)} placeholder="http://127.0.0.1:5244" /></label>
          <label className="setting-row"><span>用户名</span>
            <input value={olUser} onChange={(e) => setOlUser(e.target.value)} /></label>
          <label className="setting-row"><span>密码</span>
            <input type="password" value={olPass} onChange={(e) => setOlPass(e.target.value)} /></label>
          <label className="setting-row"><span>基础路径</span>
            <input value={olPath} onChange={(e) => setOlPath(e.target.value)} placeholder="/heartshard" /></label>
          <button className="primary" onClick={doAddOpenList} disabled={busy || !olUrl}>
            添加 OpenList Storage
          </button>
        </div>
      )}
    </div>
  );
}
