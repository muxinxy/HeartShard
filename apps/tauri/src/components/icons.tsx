// 极简线性 SVG 图标（与网页端 sprite 同源）：<Ic name="i-heart" filled />。
export function Ic({ name, filled = false }: { name: string; filled?: boolean }) {
  return (
    <svg className={`ic${filled ? " filled" : ""}`} aria-hidden="true">
      <use href={`#${name}`} />
    </svg>
  );
}
