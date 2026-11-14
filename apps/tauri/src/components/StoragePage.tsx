// §10 存储管理：列表 / 新增本地目录 / S3 / OpenList / WebDAV / FTPS。
// 每种类型支持「测试连接」——探针通过后才允许添加；OpenList 使用令牌（Token）。
import { useCallback, useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, StorageStatus } from "../lib/api";
import { useToast } from "./Toast";

type AddMode = "none" | "local" | "s3" | "openlist" | "webdav" | "ftp";

const KIND_LABEL: Record<string, string> = {
  "local-dir": "本地目录",
  s3: "S3 兼容",
  openlist: "OpenList",
  webdav: "WebDAV",
  ftp: "FTPS",
};

export function StoragePage({ onDone }: { onDone: () => void }) {
  const toast = useToast();
  const [storages, setStorages] = useState<StorageStatus[]>([]);
  const [mode, setMode] = useState<AddMode>("none");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [probeMsg, setProbeMsg] = useState<string | null>(null);
  const [probeOk, setProbeOk] = useState(false);

  // S3 表单
  const [s3Endpoint, setS3Endpoint] = useState("");
  const [s3Bucket, setS3Bucket] = useState("");
  const [s3Key, setS3Key] = useState("");
  const [s3Secret, setS3Secret] = useState("");
  const [s3Region, setS3Region] = useState("us-east-1");
  const [s3PathStyle, setS3PathStyle] = useState(true);

  // OpenList 表单（令牌优先；兼容可选账密）
  const [olUrl, setOlUrl] = useState("");
  const [olToken, setOlToken] = useState("");
  const [olPath, setOlPath] = useState("/heartshard");

  // WebDAV 表单
  const [wdUrl, setWdUrl] = useState("");
  const [wdUser, setWdUser] = useState("");
  const [wdPass, setWdPass] = useState("");

  // FTP(S) 表单
  const [ftpHost, setFtpHost] = useState("");
  const [ftpPort, setFtpPort] = useState("990");
  const [ftpUser, setFtpUser] = useState("");
  const [ftpPass, setFtpPass] = useState("");
  const [ftpSecure, setFtpSecure] = useState(true);
  const [ftpPath, setFtpPath] = useState("/heartshard");

  const refresh = useCallback(async () => {
    try {
      setStorages(await api.storageStatus());
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

  // 当前表单的探针配置（config → probeStorage / add*）
  const formConfig = useMemo<Record<string, unknown> | null>(() => {
    switch (mode) {
      case "s3":
        return { endpoint: s3Endpoint, bucket: s3Bucket, access_key: s3Key, secret_key: s3Secret, region: s3Region, path_style: s3PathStyle };
      case "openlist":
        return { base_url: olUrl, token: olToken, base_path: olPath };
      case "webdav":
        return { base_url: wdUrl, username: wdUser, password: wdPass };
      case "ftp":
        return { host: ftpHost, port: Number(ftpPort) || 990, username: ftpUser, password: ftpPass, secure: ftpSecure, base_path: ftpPath };
      default:
        return null;
    }
  }, [mode, s3Endpoint, s3Bucket, s3Key, s3Secret, s3Region, s3PathStyle, olUrl, olToken, olPath, wdUrl, wdUser, wdPass, ftpHost, ftpPort, ftpUser, ftpPass, ftpSecure, ftpPath]);

  const formConfigJson = formConfig ? JSON.stringify(formConfig) : "";

  const resetProbe = () => {
    setProbeMsg(null);
    setProbeOk(false);
  };
  // 任意字段变更后需重新测试
  useEffect(() => {
    resetProbe();
  }, [formConfigJson]);

  const doTest = async () => {
    if (!formConfig) return;
    setBusy(true);
    try {
      const msg = await api.probeStorage(mode, formConfig);
      setProbeMsg(`✓ ${msg}`);
      setProbeOk(true);
    } catch (e) {
      setProbeMsg(`✗ ${String(e)}`);
      setProbeOk(false);
    } finally {
      setBusy(false);
    }
  };

  const doAdd = async () => {
    if (!formConfig || !probeOk) return;
    setBusy(true);
    setError(null);
    try {
      switch (mode) {
        case "s3":
          await api.addStorageS3(formConfig);
          break;
        case "openlist":
          await api.addStorageOpenList(formConfig);
          break;
        case "webdav":
          await api.addStorageWebdav(formConfig);
          break;
        case "ftp":
          await api.addStorageFtp(formConfig);
          break;
        default:
          return;
      }
      toast.success("存储已添加");
      setMode("none");
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

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

  const doRemove = async (id: string) => {
    if (!window.confirm(`确定删除 Storage「${id}」吗？\n\n已有数据不会被删除，但该 Storage 将不再参与后续上传和体检。`)) {
      return;
    }
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

  const TestAddButtons = ({ addLabel }: { addLabel: string }) => (
    <>
      <div className="row" style={{ marginTop: 12, justifyContent: "space-between" }}>
        <button onClick={doTest} disabled={busy || !formConfigJson || formConfigJson === "{}"}>
          测试连接
        </button>
        <button className="primary" onClick={doAdd} disabled={busy || !probeOk} title={probeOk ? undefined : "请先通过「测试连接」"}>
          {addLabel}
        </button>
      </div>
      {probeMsg && (
        <p className={probeOk ? "hint" : "error"} style={{ marginTop: 8 }}>
          {probeMsg}
        </p>
      )}
      {!probeOk && !probeMsg && <p className="hint" style={{ marginTop: 8 }}>点击「测试连接」通过后才能添加。</p>}
    </>
  );

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
              <span className="meta" style={{ marginLeft: "auto", marginRight: 8 }}>{KIND_LABEL[s.kind] ?? s.kind}</span>
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
        <button onClick={() => setMode(mode === "webdav" ? "none" : "webdav")}>WebDAV</button>
        <button onClick={() => setMode(mode === "ftp" ? "none" : "ftp")}>FTPS</button>
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
          <TestAddButtons addLabel="添加 S3 Storage" />
        </div>
      )}

      {mode === "openlist" && (
        <div className="settings-card">
          <label className="setting-row"><span>Base URL</span>
            <input value={olUrl} onChange={(e) => setOlUrl(e.target.value)} placeholder="http://127.0.0.1:5244" /></label>
          <label className="setting-row"><span>令牌 Token</span>
            <input type="password" value={olToken} onChange={(e) => setOlToken(e.target.value)} placeholder="OpenList 管理后台「个人资料 → 令牌」生成" /></label>
          <label className="setting-row"><span>基础路径</span>
            <input value={olPath} onChange={(e) => setOlPath(e.target.value)} placeholder="/heartshard" /></label>
          <TestAddButtons addLabel="添加 OpenList Storage" />
          <p className="hint">使用令牌认证（与网页端一致）；不再使用用户名密码。</p>
        </div>
      )}

      {mode === "webdav" && (
        <div className="settings-card">
          <label className="setting-row"><span>Base URL</span>
            <input value={wdUrl} onChange={(e) => setWdUrl(e.target.value)} placeholder="https://dav.example.com/dav" /></label>
          <label className="setting-row"><span>用户名</span>
            <input value={wdUser} onChange={(e) => setWdUser(e.target.value)} /></label>
          <label className="setting-row"><span>密码</span>
            <input type="password" value={wdPass} onChange={(e) => setWdPass(e.target.value)} /></label>
          <TestAddButtons addLabel="添加 WebDAV Storage" />
          <p className="hint">兼容 nginx dav / Nextcloud / Alist / OpenList 的 WebDAV 入口（Basic 认证）。</p>
        </div>
      )}

      {mode === "ftp" && (
        <div className="settings-card">
          <label className="setting-row"><span>主机</span>
            <input value={ftpHost} onChange={(e) => setFtpHost(e.target.value)} placeholder="ftp.example.com" /></label>
          <label className="setting-row"><span>端口</span>
            <input value={ftpPort} onChange={(e) => setFtpPort(e.target.value)} placeholder="990" /></label>
          <label className="setting-row"><span>用户名</span>
            <input value={ftpUser} onChange={(e) => setFtpUser(e.target.value)} /></label>
          <label className="setting-row"><span>密码</span>
            <input type="password" value={ftpPass} onChange={(e) => setFtpPass(e.target.value)} /></label>
          <label className="setting-row"><span>FTPS（AUTH TLS）</span>
            <input type="checkbox" checked={ftpSecure} onChange={(e) => setFtpSecure(e.target.checked)} /></label>
          <label className="setting-row"><span>基础路径</span>
            <input value={ftpPath} onChange={(e) => setFtpPath(e.target.value)} placeholder="/heartshard" /></label>
          <TestAddButtons addLabel="添加 FTPS Storage" />
          <p className="hint">仅支持 FTPS；明文 FTP 因凭据明文传输默认禁用。</p>
        </div>
      )}
    </div>
  );
}
