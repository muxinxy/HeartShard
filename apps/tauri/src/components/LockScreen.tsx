// 锁屏 / 登录（网页端同款）：双模式切换；远端登录带滑动验证码 + TOTP 两步验证。
import { useCallback, useEffect, useRef, useState } from "react";
import { api, currentMode, remoteUrl, setMode, TotpRequiredError, type ApiMode } from "../lib/api";

export function LockScreen({ onUnlocked }: { onUnlocked: () => void }) {
  const [mode, setUiMode] = useState<ApiMode>(currentMode());
  const [password, setPassword] = useState("");
  const [serverUrl, setServerUrl] = useState(remoteUrl() || "http://localhost:8420");
  const [username, setUsername] = useState("");
  const [totpCode, setTotpCode] = useState("");
  const [needTotp, setNeedTotp] = useState(false);
  const [sliderDone, setSliderDone] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      if (mode === "remote") {
        if (!username || !password) {
          setError("请输入用户名和密码");
          setBusy(false);
          return;
        }
        await api.remoteLogin(serverUrl, username, password, needTotp ? totpCode : undefined);
        setTotpCode("");
        setNeedTotp(false);
      } else {
        // 本地库：Argon2id m=64MiB 首次派生需数秒
        await api.unlock(password);
      }
      setPassword("");
      onUnlocked();
    } catch (e) {
      if (e instanceof TotpRequiredError) {
        setNeedTotp(true);
        setError("该账号已启用两步验证，请输入验证码");
      } else {
        setError(String(e));
      }
    } finally {
      setBusy(false);
    }
  };

  const switchMode = (m: ApiMode) => {
    setMode(m);
    setUiMode(m);
    setError(null);
    setNeedTotp(false);
    setSliderDone(false);
  };

  const enterSubmit = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && (mode === "local" || sliderDone)) submit();
  };

  return (
    <div className="lock-screen">
      <div className="lock-card">
        <h1>牧心相册</h1>
        <p className="tagline">把记忆碎成片，散养在你信任的每一朵云上</p>

        <div style={{ display: "flex", gap: 8, marginBottom: 12 }}>
          <button
            onClick={() => switchMode("local")}
            style={{ flex: 1, background: mode === "local" ? "var(--primary)" : "var(--input)", color: mode === "local" ? "#fff" : "inherit", border: mode === "local" ? "none" : "1px solid var(--border)" }}
          >
            本地相册库
          </button>
          <button
            onClick={() => switchMode("remote")}
            style={{ flex: 1, background: mode === "remote" ? "var(--primary)" : "var(--input)", color: mode === "remote" ? "#fff" : "inherit", border: mode === "remote" ? "none" : "1px solid var(--border)" }}
          >
            连接后台
          </button>
        </div>

        {mode === "remote" && (
          <>
            <input
              placeholder="后台地址（http://…:8420）"
              value={serverUrl}
              onChange={(e) => setServerUrl(e.target.value)}
              style={{ marginBottom: 8 }}
            />
            <input
              placeholder="用户名"
              value={username}
              autoFocus={mode === "remote"}
              onChange={(e) => setUsername(e.target.value)}
              style={{ marginBottom: 8 }}
            />
          </>
        )}
        <input
          type="password"
          placeholder={mode === "remote" ? "密码" : "相册口令"}
          value={password}
          autoFocus={mode === "local"}
          onChange={(e) => setPassword(e.target.value)}
          onKeyDown={enterSubmit}
          disabled={busy}
        />
        {needTotp && (
          <input
            placeholder="两步验证码（6 位数字）"
            inputMode="numeric"
            maxLength={6}
            value={totpCode}
            onChange={(e) => setTotpCode(e.target.value.replace(/\D/g, ""))}
            onKeyDown={enterSubmit}
            style={{ marginBottom: 0, marginTop: 12 }}
          />
        )}
        {mode === "remote" && <Slider onDone={setSliderDone} />}
        <button onClick={submit} disabled={busy || !password || (mode === "remote" && !sliderDone)}>
          {mode === "remote" ? (busy ? "登录中…" : "登录") : busy ? "派生密钥中（Argon2id）…" : "解锁"}
        </button>
        {error && <p className="error">{error}</p>}
        <p className="hint">
          {mode === "remote"
            ? "登录后台账号；照片经后台加密管线存取，服务器零知识。"
            : "口令不落盘；主密钥仅存在于本次会话内存。"}
        </p>
      </div>
    </div>
  );
}

// ---------------- 滑动验证码（网页端 initSlider 同款，Pointer Events 兼容鼠标/触摸） ----------------

function Slider({ onDone }: { onDone: (ok: boolean) => void }) {
  const [done, setDone] = useState(false);
  const [fillPx, setFillPx] = useState(0);
  const track = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);

  const finish = useCallback(() => {
    setDone(true);
    onDone(true);
  }, [onDone]);

  useEffect(() => {
    const move = (e: PointerEvent) => {
      if (!dragging.current || !track.current || done) return;
      const rect = track.current.getBoundingClientRect();
      const x = Math.max(0, Math.min(e.clientX - rect.left, rect.width - 57));
      setFillPx(x);
      if (x >= rect.width - 60) finish();
    };
    const up = () => {
      dragging.current = false;
      if (!done) setFillPx(0);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
  }, [done, finish]);

  return (
    <div className={`slider${done ? " done" : ""}`} ref={track}>
      <div className="slider-fill" style={{ width: done ? undefined : fillPx + 57 }} />
      <div
        className="slider-thumb"
        style={done ? undefined : { left: Math.max(3, fillPx) }}
        onPointerDown={(e) => {
          if (!done) {
            dragging.current = true;
            e.currentTarget.setPointerCapture?.(e.pointerId);
          }
        }}
      >
        {done ? "✓" : "»"}
      </div>
      <span className="slider-text">{done ? "验证通过" : "按住滑块拖到最右侧"}</span>
    </div>
  );
}
