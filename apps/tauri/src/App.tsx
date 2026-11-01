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
