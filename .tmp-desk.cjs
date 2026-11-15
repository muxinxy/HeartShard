const fs = require("fs");
const base = "D:/Documents/Code/HeartShard/apps/tauri/src/components/";
const rep = (f, from, to) => {
  let s = fs.readFileSync(base + f, "utf8");
  if (!s.includes(from)) { console.log("MISS:", f, "::", from.slice(0, 50)); return; }
  s = s.split(from).join(to);
  fs.writeFileSync(base + f, s);
  console.log("ok:", f);
};

// Thumb 占位 → SVG
rep("Thumb.tsx",
  `<div className="thumb-placeholder">{isImage ? "🖼" : "📄"}</div>`,
  `<div className="thumb-placeholder"><svg className="ic" style={{ width: 22, height: 22 }}><use href={isImage ? "#i-image" : "#i-box"} /></svg></div>`);

// AlbumsView 占位 + ACL 行图标
rep("views/AlbumsView.tsx",
  `<div className="thumb-placeholder">📁</div>`,
  `<div className="thumb-placeholder"><svg className="ic" style={{ width: 26, height: 26 }}><use href="#i-albums" /></svg></div>`);
rep("views/AlbumsView.tsx",
  `<td>{r.kind === "user" ? "👤" : "🏷"} {r.name}</td>`,
  `<td><svg className="ic"><use href={r.kind === "user" ? "#i-users" : "#i-groups"} /></svg> {r.name}</td>`);
rep("views/AlbumsView.tsx",
  `return src ? <img src={src} alt="" /> : <div className="thumb-placeholder">🖼</div>;`,
  `return src ? <img src={src} alt="" /> : <div className="thumb-placeholder"><svg className="ic" style={{ width: 20, height: 20 }}><use href="#i-image" /></svg></div>;`);

// TrashView 占位
rep("views/TrashView.tsx",
  `{mime.startsWith("video/") ? "🎬" : "🖼"}`,
  `<svg className="ic" style={{ width: 18, height: 18 }}><use href={mime.startsWith("video/") ? "#i-video" : "#i-image"} /></svg>`);

// AlbumTree 封面标记
rep("AlbumTree.tsx",
  `{a.cover_hash ? "📸 " : ""}{a.name}`,
  `{a.cover_hash ? <svg className="ic" style={{ color: "#ffd479" }}><use href="#i-image" /></svg> : null}{a.name}`);

// BlobDetailModal：状态字形 → badge 文本；♥ → svg
rep("BlobDetailModal.tsx",
  `? \`✔ 全部 ${r.shard_count} 个分片健康\``,
  `? \`全部 ${r.shard_count} 个分片健康\``);
rep("BlobDetailModal.tsx",
  `? \`✘ 不可恢复（可用 ${r.available}/${r.shard_count}）\``,
  `? \`不可恢复（可用 ${r.available}/${r.shard_count}）\``);
rep("BlobDetailModal.tsx",
  `: \`⚠ 降级（可用 ${r.available}/${r.shard_count}），可点「修复」\``,
  `: \`降级（可用 ${r.available}/${r.shard_count}），可点「修复」\``);
rep("BlobDetailModal.tsx",
  `? \`✘ ${r.unrecoverable.length} 个分片组损坏过重，无法自动修复\``,
  `? \`${r.unrecoverable.length} 个分片组损坏过重，无法自动修复\``);
rep("BlobDetailModal.tsx",
  `{favorite && <span style={{ color: "#f0b45c" }}>♥ </span>}`,
  `{favorite && <svg className="ic" style={{ color: "#ffd479" }}><use href="#i-heart-filled" /></svg>}`);
rep("BlobDetailModal.tsx",
  `<button onClick={doFavorite}>{favorite ? "♥ 取消收藏" : "♡ 收藏"}</button>`,
  `<button onClick={doFavorite}>{favorite ? "取消收藏" : "收藏"}</button>`);

// Lightbox 播放按钮 ▶ → svg play
rep("Lightbox.tsx",
  `▶ 播放动态`,
  `<svg className="ic" style={{ width: 14, height: 14, marginRight: 4 }}><use href="#i-play" /></svg>播放动态`);

// PhotoGrid 名称前缀 ♥ → svg
rep("PhotoGrid.tsx",
  `${b.favorite && "♥ "}`,
  ``);

// ExploreView 📍 → svg / 纯文本
rep("views/ExploreView.tsx",
  "const label = (key: string) => (labels[key] ? `📍 ${labels[key]}` : `📍 ${key}`);",
  `const label = (key: string) => (labels[key] ?? key);`);

console.log("done");
