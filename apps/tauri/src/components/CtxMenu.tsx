// 右键菜单（网页端 ctxMenu 同款）：全屏遮罩 + 光标处弹出，索引分发避免闭包字符串化。
import { useEffect, useRef } from "react";

export interface MenuItem {
  label: string;
  icon?: string;
  danger?: boolean;
  onClick: () => void;
}

export function CtxMenu({
  pos,
  items,
  onClose,
}: {
  pos: { x: number; y: number };
  items: MenuItem[];
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);

  // 视口越界钳制
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    if (pos.x + r.width > window.innerWidth - 8) {
      el.style.left = `${Math.max(8, window.innerWidth - r.width - 8)}px`;
    }
    if (pos.y + r.height > window.innerHeight - 8) {
      el.style.top = `${Math.max(8, window.innerHeight - r.height - 8)}px`;
    }
  }, [pos]);

  return (
    <>
      <div
        className="menu-overlay"
        onClick={onClose}
        onContextMenu={(e) => {
          e.preventDefault();
          onClose();
        }}
      />
      <div ref={ref} className="ctx-menu" style={{ left: pos.x, top: pos.y }}>
        {items.map((it, i) => (
          <button
            key={i}
            className={it.danger ? "danger" : ""}
            onClick={() => {
              onClose();
              it.onClick();
            }}
          >
            {it.icon && <svg className="ic"><use href={`#${it.icon}`} /></svg>}
            {it.label}
          </button>
        ))}
      </div>
    </>
  );
}
