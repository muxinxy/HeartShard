import { useEffect, useState } from "react";
import "./App.css";
import { GalleryWithTasks } from "./components/Gallery";
import { LockScreen } from "./components/LockScreen";
import { api } from "./lib/api";

function App() {
  const [unlocked, setUnlocked] = useState<boolean | null>(null);

  useEffect(() => {
    api.sessionStatus().then((s) => setUnlocked(s.unlocked));
  }, []);

  // §10 浅色/深色主题：settings.theme 驱动 body class
  useEffect(() => {
    api.getSettings().then((s) => {
      document.body.className = s.theme === "light" ? "theme-light" : "";
    }).catch(() => {});
  }, []);

  if (unlocked === null) {
    return <div className="loading">…</div>;
  }
  return unlocked ? (
    <GalleryWithTasks onLock={() => setUnlocked(false)} />
  ) : (
    <LockScreen onUnlocked={() => setUnlocked(true)} />
  );
}

export default App;
