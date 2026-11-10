import { useState } from "react";
import { api, currentMode, remoteUrl, setMode, type ApiMode } from "../lib/api";

export function LockScreen({ onUnlocked }: { onUnlocked: () => void }) {
  const [mode, setUiMode] = useState<ApiMode>(currentMode());
  const [password, setPassword] = useState("");
  const [serverUrl, setServerUrl] = useState(remoteUrl() || "http://localhost:8420");
  const [username, setUsername] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      if (mode === "remote") {
        if (!username || !password) {
          setError("请输入用户名和口令");
          setBusy(false);
          return;
        }
        // 后台登录：url|user|pass 合并传给 RemoteProvider.unlock
        await api.unlock(`${serverUrl}|${username}|${password}`);
      } else {
        // 本地库：Argon2id m=64MiB 首次派生需数秒
        await api.unlock(password);
      }
      setPassword("");
      onUnlocked();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const switchMode = (m: ApiMode) => {
    setMode(m);
    setUiMode(m);
    setError(null);
  };

  return (
    <div className="lock-screen">
      <div className="lock-card">
        <h1>HeartShard</h1>
        <p className="tagline">把记忆碎成片，散养在你信任的每一朵云上</p>

        <div style={{ display: "flex", gap: 8, marginBottom: 12 }}>
          <button
            onClick={() => switchMode("local")}
            style={{ flex: 1, background: mode === "local" ? "var(--primary)" : "var(--secondary)", color: mode === "local" ? "#fff" : "inherit" }}
          >
            本地相册库
          </button>
          <button
            onClick={() => switchMode("remote")}
            style={{ flex: 1, background: mode === "remote" ? "var(--primary)" : "var(--secondary)", color: mode === "remote" ? "#fff" : "inherit" }}
          >
            连接后台
          </button>
        </div>

        {mode === "remote" && (
          <input
            placeholder="后台地址（http://…:8420）"
            value={serverUrl}
            onChange={(e) => setServerUrl(e.target.value)}
            style={{ marginBottom: 8 }}
          />
        )}
        {mode === "remote" && (
          <input
            placeholder="用户名"
            value={username}
            autoFocus={mode === "remote"}
            onChange={(e) => setUsername(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && submit()}
            style={{ marginBottom: 8 }}
          />
        )}
        <input
          type="password"
          placeholder={mode === "remote" ? "口令" : "相册口令"}
          value={password}
          autoFocus={mode === "local"}
          onChange={(e) => setPassword(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && submit()}
          disabled={busy}
        />
        <button onClick={submit} disabled={busy || !password}>
          {mode === "remote" ? (busy ? "登录中…" : "登录") : busy ? "派生密钥中（Argon2id）…" : "解锁"}
        </button>
        {error && <p className="error">{error}</p>}
        <p className="hint">
          {mode === "remote"
            ? "登录后台账号；照片经后台加密管线存取。"
            : "口令不落盘；主密钥仅存在于本次会话内存。"}
        </p>
      </div>
    </div>
  );
}
