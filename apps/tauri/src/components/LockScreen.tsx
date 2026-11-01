import { useState } from "react";
import { api } from "../lib/api";

export function LockScreen({ onUnlocked }: { onUnlocked: () => void }) {
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    if (!password || busy) return;
    setBusy(true);
    setError(null);
    try {
      // Argon2id m=64MiB 首次派生需数秒
      await api.unlock(password);
      setPassword("");
      onUnlocked();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="lock-screen">
      <div className="lock-card">
        <h1>HeartShard</h1>
        <p className="tagline">把记忆碎成片，散养在你信任的每一朵云上</p>
        <input
          type="password"
          placeholder="相册口令"
          value={password}
          autoFocus
          onChange={(e) => setPassword(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && submit()}
          disabled={busy}
        />
        <button onClick={submit} disabled={busy || !password}>
          {busy ? "派生密钥中（Argon2id）…" : "解锁"}
        </button>
        {error && <p className="error">{error}</p>}
        <p className="hint">口令不落盘；主密钥仅存在于本次会话内存。</p>
      </div>
    </div>
  );
}
