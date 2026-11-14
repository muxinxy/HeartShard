import { useEffect, useState } from "react";
import "./App.css";
import { ShellWithTasks } from "./components/Shell";
import { LockScreen } from "./components/LockScreen";
import { ToastProvider } from "./components/Toast";
import { api } from "./lib/api";

function App() {
  const [unlocked, setUnlocked] = useState<boolean | null>(null);

  useEffect(() => {
    // 主题在锁屏阶段也生效（localStorage，双模式）
    document.body.className = (localStorage.getItem("hs_theme") || "dark") === "light" ? "theme-light" : "";
    api.sessionStatus().then((s) => setUnlocked(s.unlocked));
  }, []);

  if (unlocked === null) {
    return <div className="loading">…</div>;
  }
  return (
    <ToastProvider>
      {unlocked ? (
        <ShellWithTasks onLock={() => setUnlocked(false)} />
      ) : (
        <LockScreen onUnlocked={() => setUnlocked(true)} />
      )}
    </ToastProvider>
  );
}

export default App;
