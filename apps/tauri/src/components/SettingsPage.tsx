// §10 设置页：修改口令（重加密 MK 封装，不重传 shard）、深色模式、
// n/m 参数、默认分片大小、下载并发数。
import { useEffect, useState } from "react";
import { api, SettingsData } from "../lib/api";

const CHUNK_SIZES = [
  { value: 256 * 1024, label: "256 KiB" },
  { value: 512 * 1024, label: "512 KiB" },
  { value: 1024 * 1024, label: "1 MiB" },
  { value: 2 * 1024 * 1024, label: "2 MiB" },
  { value: 4 * 1024 * 1024, label: "4 MiB" },
];

export function SettingsPage({ onDone }: { onDone: () => void }) {
  const [settings, setSettings] = useState<SettingsData | null>(null);
  const [oldPw, setOldPw] = useState("");
  const [newPw, setNewPw] = useState("");
  const [newPw2, setNewPw2] = useState("");
  const [pwMsg, setPwMsg] = useState<string | null>(null);
  const [pwError, setPwError] = useState<string | null>(null);
  const [saveMsg, setSaveMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api.getSettings().then(setSettings).catch(() => {});
  }, []);

  const save = async (s: SettingsData) => {
    setBusy(true);
    try {
      await api.updateSettings(s);
      setSettings(s);
      // §10 主题切换：settings.theme 驱动 body class
      document.body.className = s.theme === "light" ? "theme-light" : "";
      setSaveMsg("已保存。修改分片参数只影响新上传。");
      setTimeout(() => setSaveMsg(null), 3000);
    } catch (e) {
      setSaveMsg(`保存失败：${String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  const doChangePw = async () => {
    if (!oldPw || !newPw || newPw !== newPw2) {
      setPwError("两次输入的新口令不一致，或字段为空。");
      return;
    }
    setBusy(true);
    setPwError(null);
    setPwMsg(null);
    try {
      await api.changePassword(oldPw, newPw);
      setPwMsg("口令已修改（MK 封装已重写，shard 不需要重传）。");
      setOldPw("");
      setNewPw("");
      setNewPw2("");
    } catch (e) {
      setPwError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const update = (patch: Partial<SettingsData>) => {
    if (!settings) return;
    setSettings({ ...settings, ...patch });
  };

  return (
    <div className="wizard">
      <header className="toolbar">
        <span className="brand">设置</span>
        <span className="spacer" />
        <button onClick={onDone}>返回</button>
      </header>

      {saveMsg && <div className="notice">{saveMsg}</div>}

      <h2>修改口令</h2>
      <div className="settings-card">
        <p className="hint">
          重加密 MK 封装，不重传 shard。新口令下次解锁生效。
        </p>
        <input type="password" placeholder="当前口令" value={oldPw}
          onChange={(e) => setOldPw(e.target.value)} disabled={busy} />
        <input type="password" placeholder="新口令" value={newPw}
          onChange={(e) => setNewPw(e.target.value)} disabled={busy} />
        <input type="password" placeholder="确认新口令" value={newPw2}
          onChange={(e) => setNewPw2(e.target.value)} disabled={busy} />
        <button onClick={doChangePw} disabled={busy || !oldPw || !newPw || newPw !== newPw2}>
          修改口令
        </button>
        {pwMsg && <p className="hint" style={{ color: "#7ed6a0" }}>{pwMsg}</p>}
        {pwError && <p className="error">{pwError}</p>}
      </div>

      <h2>存储参数</h2>
      {settings && (
        <div className="settings-card">
          <label className="setting-row">
            <span>默认分片大小</span>
            <select
              value={settings.chunk_size}
              onChange={(e) => update({ chunk_size: Number(e.target.value) })}
            >
              {CHUNK_SIZES.map((c) => (
                <option key={c.value} value={c.value}>{c.label}</option>
              ))}
            </select>
          </label>
          <label className="setting-row">
            <span>n（总 shard 数）</span>
            <select value={settings.n}
              onChange={(e) => update({ n: Number(e.target.value), m: Math.min(settings.m, Number(e.target.value)) })}>
              {[2, 4, 6, 8].map((v) => <option key={v} value={v}>{v}</option>)}
            </select>
          </label>
          <label className="setting-row">
            <span>m（恢复阈值）</span>
            <select value={settings.m}
              onChange={(e) => update({ m: Number(e.target.value) })}>
              {Array.from({ length: settings.n }, (_, i) => i + 1).map((v) => (
                <option key={v} value={v}>{v}</option>
              ))}
            </select>
          </label>
          <label className="setting-row">
            <span>下载并发数</span>
            <input type="number" min={1} max={10} value={settings.download_concurrency}
              onChange={(e) => update({ download_concurrency: Math.max(1, Number(e.target.value) || 1) })} />
          </label>
          <p className="hint">
            存储放大 = n/m = {(settings.n / settings.m).toFixed(1)}×。
            修改分片参数只影响新上传的 blob。
          </p>
          <button className="primary" onClick={() => save(settings)} disabled={busy}>
            保存设置
          </button>
        </div>
      )}

      <h2>外观</h2>
      <div className="settings-card">
        <label className="setting-row">
          <span>主题（也可在右上角头像菜单切换）</span>
          <select
            value={localStorage.getItem("hs_theme") || "dark"}
            onChange={(e) => {
              localStorage.setItem("hs_theme", e.target.value);
              document.body.className = e.target.value === "light" ? "theme-light" : "";
              // 重渲染本组件以刷新 select 值
              setSettings((s) => (s ? { ...s } : s));
            }}
          >
            <option value="dark">深色</option>
            <option value="light">浅色</option>
          </select>
        </label>
        <p className="hint">主题保存在本机；本地模式下不写入后台配置。</p>
      </div>
    </div>
  );
}
