// 自定义通知组件（替换系统 alert）。
import { createContext, useContext, useState, useCallback, useMemo } from "react";

interface ToastItem {
  id: number;
  kind: "success" | "error" | "info";
  message: string;
}

interface ToastCtxValue {
  show: (kind: ToastItem["kind"], message: string) => void;
  success: (message: string) => void;
  error: (message: string) => void;
  info: (message: string) => void;
}

const ToastCtx = createContext<ToastCtxValue | null>(null);
let nextId = 1;

export function ToastProvider({ children }: { children: React.ReactNode }) {
  const [toasts, setToasts] = useState<ToastItem[]>([]);

  const remove = (id: number) => {
    setToasts((t) => t.filter((x) => x.id !== id));
  };

  const show = useCallback((kind: ToastItem["kind"], message: string) => {
    const id = nextId++;
    setToasts((t) => [...t, { id, kind, message }]);
    setTimeout(() => remove(id), 5000);
  }, []);

  const success = useCallback((msg: string) => show("success", msg), [show]);
  const error = useCallback((msg: string) => show("error", msg), [show]);
  const info = useCallback((msg: string) => show("info", msg), [show]);

  // 稳定 value：避免消费方 effect 因 identity 变化反复触发
  const value = useMemo(() => ({ show, success, error, info }), [show, success, error, info]);

  return (
    <ToastCtx.Provider value={value}>
      {children}
      <div className="toast-container">
        {toasts.map((t) => (
          <div key={t.id} className={`toast toast-${t.kind}`} onClick={() => remove(t.id)}>
            <span className="toast-icon">
              {t.kind === "success" ? "✅" : t.kind === "error" ? "❌" : "ℹ️"}
            </span>
            <span className="toast-msg">{t.message}</span>
          </div>
        ))}
      </div>
    </ToastCtx.Provider>
  );
}

export function useToast(): ToastCtxValue {
  const ctx = useContext(ToastCtx);
  if (!ctx) throw new Error("useToast outside ToastProvider");
  return ctx;
}
