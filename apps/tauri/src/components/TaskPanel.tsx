// §10 任务中心：显示进行中/已完成的后台任务（导入/同步/修复/再平衡/体检）
// 及其实时进度。任务由 Gallery 通过 TaskCtx 推入。
import { createContext, useContext, useState } from "react";

export type TaskKind = "import" | "sync" | "repair" | "rebalance" | "health" | "dir-import";

export interface TaskItem {
  id: number;
  kind: TaskKind;
  label: string;
  progress?: string; // 实时进度文案
  state: "running" | "done" | "failed";
  detail?: string;
}

interface TaskCtxValue {
  tasks: TaskItem[];
  push: (kind: TaskKind, label: string) => number;
  update: (id: number, patch: Partial<TaskItem>) => void;
  finish: (id: number, state: "done" | "failed", detail?: string) => void;
}

const TaskCtx = createContext<TaskCtxValue | null>(null);

let nextId = 1;

export function TaskProvider({ children }: { children: React.ReactNode }) {
  // React.ReactNode 引用（避免 TSX 下 React 导入缺失报错）

  const [tasks, setTasks] = useState<TaskItem[]>([]);
  const push = (kind: TaskKind, label: string) => {
    const id = nextId++;
    setTasks((t) => [...t, { id, kind, label, state: "running" }]);
    return id;
  };
  const update = (id: number, patch: Partial<TaskItem>) => {
    setTasks((t) => t.map((x) => (x.id === id ? { ...x, ...patch } : x)));
  };
  const finish = (id: number, state: "done" | "failed", detail?: string) => {
    setTasks((t) => t.map((x) => (x.id === id ? { ...x, state, detail } : x)));
    // 完成任务 8s 后自动移出面板，避免堆积
    setTimeout(() => setTasks((t) => t.filter((x) => x.id !== id)), 8000);
  };
  return <TaskCtx.Provider value={{ tasks, push, update, finish }}>{children}</TaskCtx.Provider>;
}

export function useTasks(): TaskCtxValue {
  const ctx = useContext(TaskCtx);
  if (!ctx) throw new Error("useTasks outside TaskProvider");
  return ctx;
}

/// 面板本体：挂右下角，仅在有任务时显示。
export function TaskPanel() {
  const { tasks } = useTasks();
  if (tasks.length === 0) return null;
  return (
    <div className="task-panel">
      <div className="task-panel-title">任务中心</div>
      {tasks.map((t) => (
        <div key={t.id} className={`task-item ${t.state}`}>
          <span className="task-label">{t.label}</span>
          <span className="task-state">
            {t.state === "running" && `⏳ ${t.progress ?? "进行中…"}`}
            {t.state === "done" && "✅ 完成"}
            {t.state === "failed" && `❌ ${t.detail ?? "失败"}`}
          </span>
        </div>
      ))}
    </div>
  );
}
