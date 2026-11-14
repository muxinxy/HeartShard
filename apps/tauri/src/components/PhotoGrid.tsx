// 时间线网格渲染（网页端 groupByDay + photoCell 同款）：
// 月份分隔 → 日分组头（可全选当天）→ 1:1 缩略网格；分批渲染防长列表卡顿。
import { useEffect, useMemo, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import { BlobEntry } from "../lib/api";
import { Ic } from "./icons";
import { Thumb } from "./Thumb";

export interface PhotoGridProps {
  blobs: BlobEntry[];
  selected?: Set<string>;
  onToggleSel?: (hash: string) => void;
  onOpen?: (b: BlobEntry, index: number) => void;
  onCtx?: (e: ReactMouseEvent, b: BlobEntry) => void;
  daySelectable?: boolean;
  selectedDays?: Set<string>;
  onToggleDay?: (dayKey: string, select: boolean) => void;
  /** 每次渲染的分组数增量（默认 30；0 = 全量） */
  chunk?: number;
  emptyHint?: string;
}

interface DayGroup {
  key: string;
  label: string;
  year: number;
  month: number;
  items: { b: BlobEntry; idx: number }[];
}

export function dayKeyOf(b: BlobEntry): string {
  return b.taken_at ? new Date(b.taken_at * 1000).toDateString() : "unknown";
}

export function groupByDay(blobs: BlobEntry[]): DayGroup[] {
  const out: DayGroup[] = [];
  let cur: DayGroup | null = null;
  blobs.forEach((b, idx) => {
    const key = dayKeyOf(b);
    if (!cur || cur.key !== key) {
      const d = b.taken_at ? new Date(b.taken_at * 1000) : null;
      cur = {
        key,
        label: d ? d.toLocaleDateString() : "未知日期",
        year: d ? d.getFullYear() : 0,
        month: d ? d.getMonth() + 1 : 0,
        items: [],
      };
      out.push(cur);
    }
    cur.items.push({ b, idx });
  });
  return out;
}

export function PhotoGrid({
  blobs,
  selected,
  onToggleSel,
  onOpen,
  onCtx,
  daySelectable,
  selectedDays,
  onToggleDay,
  chunk = 30,
  emptyHint = "还没有照片。",
}: PhotoGridProps) {
  const groups = useMemo(() => groupByDay(blobs), [blobs]);
  const [visible, setVisible] = useState(chunk > 0 ? chunk : groups.length);
  const sentinel = useRef<HTMLDivElement>(null);
  const chunkRef = useRef(chunk);
  chunkRef.current = chunk;

  // 数据变化重置分批游标
  useEffect(() => {
    setVisible(chunk > 0 ? chunk : groups.length);
  }, [blobs, chunk, groups.length]);

  // 底部哨兵：进入视口再加载更多组
  useEffect(() => {
    if (chunk <= 0) return;
    const el = sentinel.current;
    if (!el) return;
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          setVisible((v) => (v >= groups.length ? v : v + (chunkRef.current || 30)));
        }
      },
      { rootMargin: "600px" }
    );
    io.observe(el);
    return () => io.disconnect();
  }, [chunk, groups.length]);

  if (blobs.length === 0) {
    return <p className="hint" style={{ marginTop: 60, textAlign: "center" }}>{emptyHint}</p>;
  }

  const shown = groups.slice(0, visible);
  let lastMonth = "";

  return (
    <>
      {shown.map((g) => {
        const showSep = `${g.year}-${g.month}` !== lastMonth;
        lastMonth = `${g.year}-${g.month}`;
        const allSel = selected && selectedDays && onToggleDay
          ? g.items.every((it) => selected.has(it.b.blob_hash))
          : false;
        return (
          <section key={g.key}>
            {showSep && g.year > 0 && <div className="month-sep">—— {g.year} 年 {g.month} 月 ——</div>}
            <div
              className="day-header"
              data-anchor-y={g.year}
              data-anchor-m={`${g.year}-${g.month}`}
            >
              {daySelectable && onToggleDay && (
                <input
                  type="checkbox"
                  className="day-check"
                  checked={allSel}
                  onChange={(e) => onToggleDay(g.key, e.target.checked)}
                />
              )}
              <span>
                {g.label}（{g.items.length} 张）
              </span>
            </div>
            <div className="grid">
              {g.items.map(({ b, idx }) => {
                const sel = selected?.has(b.blob_hash) ?? false;
                return (
                  <div
                    key={b.blob_hash}
                    className={`cell${sel ? " sel" : ""}`}
                    title={`${b.file_name} · ${new Date(b.taken_at * 1000).toLocaleString()}`}
                    onClick={() => onOpen?.(b, idx)}
                    onContextMenu={(e) => onCtx?.(e, b)}
                  >
                    <Thumb blobHash={b.blob_hash} mime={b.mime} />
                    {onToggleSel && (
                      <span
                        className={`check${sel ? " checked" : ""}`}
                        title="选中 / 取消选中"
                        onClick={(e) => {
                          // 网页端同款：点击圆圈切换多选，点击照片其它区域打开预览
                          e.stopPropagation();
                          onToggleSel(b.blob_hash);
                        }}
                      >
                        {sel ? "✓" : ""}
                      </span>
                    )}
                    {b.is_motion && (
                      <span className="motion-badge">
                        <Ic name="i-motion" />
                      </span>
                    )}
                    {b.favorite && !b.is_motion && (
                      <span className="fav-badge">
                        <Ic name="i-heart-filled" />
                      </span>
                    )}
                    <span className="name">
                      {b.favorite && "♥ "}
                      {b.file_name}
                    </span>
                  </div>
                );
              })}
            </div>
          </section>
        );
      })}
      {visible < groups.length && <div ref={sentinel} style={{ height: 1 }} />}
    </>
  );
}
