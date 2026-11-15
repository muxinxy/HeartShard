const fs = require("fs");
const p = "D:/Documents/Code/HeartShard/server/src/webapp/app.js";
let s = fs.readFileSync(p, "utf8");
let n = 0;
const rep = (from, to, tag) => {
  if (!s.includes(from)) { console.log("MISS:", tag); return; }
  s = s.split(from).join(to);
  n++;
  console.log("ok:", tag);
};

// 1) 探索卡片：有内容显示首张缩略图，无内容显示图标
rep(`  const card = (dataAttr, iconId, label, cnt) => {
    const iconSvg = iconId
      ? `<svg class="ic" style="width:36px;height:36px"><use href="#i-${iconId}"/></svg>`
      : "";
    return `
    <div class="album-card" ${dataAttr}>
      <div class="cov" style="display:grid;place-items:center">${iconSvg}</div>
      <div class="nm"><span>${label}</span><span class="cnt">${cnt}</span></div>
    </div>`;
  };`,
`  const card = (dataAttr, iconId, label, cnt, cover) => {
    const cov = cover
      ? `<img src="/api/blobs/${cover}/thumbnail?token=${encodeURIComponent(token)}" style="width:100%;height:100%;object-fit:cover">`
      : `<svg class="ic" style="width:36px;height:36px"><use href="#i-${iconId}"/></svg>`;
    return `
    <div class="album-card" ${dataAttr}>
      <div class="cov" style="display:grid;place-items:center;overflow:hidden">${cov}</div>
      <div class="nm"><span>${label}</span><span class="cnt">${cnt}</span></div>
    </div>`;
  };`, "card cover param");
rep('${card(\'data-kind="recent"', "recent", "recent", recent.length)}',
    '${card(\'data-kind="recent"', "recent", "最近添加", recent.length, recent[0]?.blob_hash)}', "recent cover");
rep('${card(\'data-kind="photo"', "image", "照片", photos.length)}',
    '${card(\'data-kind="photo"', "image", "照片", photos.length, photos[0]?.blob_hash)}', "photo cover");
rep('${card(\'data-kind="video"', "video", "视频", videos.length)}',
    '${card(\'data-kind="video"', "video", "视频", videos.length, videos[0]?.blob_hash)}', "video cover");
rep('${card(\'data-kind="motion"', "motion", "动态照片", motions.length)}',
    '${card(\'data-kind="motion"', "motion", "动态照片", motions.length, motions[0]?.blob_hash)}', "motion cover");
rep('${card(\'data-kind="orphan"', "box", "未入相册", orphans.length)}',
    '${card(\'data-kind="orphan"', "box", "未入相册", orphans.length, orphans[0]?.blob_hash)}', "orphan cover");
rep('${placeList.map(([k, items]) => card(`data-place="${k}"`, "explore", k, items.length)).join("")}',
    '${placeList.map(([k, items]) => card(`data-place="${k}"`, "explore", k, items.length, items[0]?.blob_hash)).join("")}', "place covers");

// 2) openFiltered 返回目标可指定
rep(`async function openFiltered(title, predOrItems) {`,
    `async function openFiltered(title, predOrItems, backHash = "#/explore") {`, "openFiltered sig");
rep(`<button style="margin-left:auto" id="back-explore">← 返回探索</button></div>`,
    `<button class="back-btn" id="back-explore">← 返回</button></div>`, "back btn style");
rep(`  $("back-explore").onclick = () => { location.hash = "#/explore"; };`,
    `  $("back-explore").onclick = () => { location.hash = backHash; };`, "back bind");
// 地图页两个入口指定返回地图
rep(`if ($("mp-list")) $("mp-list").onclick = () => openFiltered(place ? \`\${place}\` : "全部地点", gps);`,
    `if ($("mp-list")) $("mp-list").onclick = () => openFiltered(place ? \`\${place}\` : "全部地点", gps, "#/map");`, "map list back");
rep(`else { openFiltered("此处照片", [...g.items].sort((a, b) => b.taken_at - a.taken_at)); }`,
    `else { openFiltered("此处照片", [...g.items].sort((a, b) => b.taken_at - a.taken_at), "#/map"); }`, "cluster back");

// 3) 复制链接 ✓ 反馈（1 秒后恢复）
rep(`onclick="copyShare('${s.id}')"`, `onclick="copyShare('${s.id}', this)"`, "copy btn pass this");
rep(`window.copyShare = async (id) => {`,
    `window.copyShare = async (id, btn) => {`, "copyShare sig");
// 复制完成后按钮反馈
rep(`window.copyShare = async (id, btn) => {
  try {`, `window.copyShare = async (id, btn) => {
  const flash = () => {
    if (!btn) return;
    const orig = btn.innerHTML;
    btn.classList.add("primary");
    btn.textContent = "✓ 已复制";
    btn.disabled = true;
    setTimeout(() => { btn.classList.remove("primary"); btn.innerHTML = orig; btn.disabled = false; }, 1000);
  };
  try {`, "copyShare flash");
// 在剪贴板写入成功后调用 flash
rep(`window.copyShare = async (id, btn) => {
  const flash = () => {
    if (!btn) return;
    const orig = btn.innerHTML;
    btn.classList.add("primary");
    btn.textContent = "✓ 已复制";
    btn.disabled = true;
    setTimeout(() => { btn.classList.remove("primary"); btn.innerHTML = orig; btn.disabled = false; }, 1000);
  };
  try {
    await navigator.clipboard.writeText(location.origin + "/s/" + id);
    toast("链接已复制");
  } catch (e) { toast(e.message, true); }
};`,
`window.copyShare = async (id, btn) => {
  const flash = () => {
    if (!btn) return;
    const orig = btn.innerHTML;
    btn.classList.add("primary");
    btn.textContent = "✓ 已复制";
    btn.disabled = true;
    setTimeout(() => { btn.classList.remove("primary"); btn.innerHTML = orig; btn.disabled = false; }, 1000);
  };
  try {
    await navigator.clipboard.writeText(location.origin + "/s/" + id);
    flash();
    toast("链接已复制");
  } catch (e) { toast(e.message, true); }
};`, "copyShare flash wired");

// 4) 分享内容弹窗：预览/日期不换行 + 弹窗顶部间距
rep(`<tr><td>${f.name}</td>
        <td>${new Date((f.taken_at || 0) * 1000).toLocaleDateString()}</td>`,
    `<tr><td>${f.name}</td>
        <td style="white-space:nowrap">${new Date((f.taken_at || 0) * 1000).toLocaleDateString()}</td>`, "share date nowrap");
rep(`<td>${f.mime.startsWith("image/") || f.mime.startsWith("video/") ? `<button onclick='previewShared(${JSON.stringify(JSON.stringify(f))})'>预览</button>` : ""}</td></tr>`).join("")}`,
    `<td style="white-space:nowrap">${f.mime.startsWith("image/") || f.mime.startsWith("video/") ? `<button onclick='previewShared(${JSON.stringify(JSON.stringify(f))})'>预览</button>` : ""}</td></tr>`).join("")}`, "share preview nowrap");

// 5) 分享页全选联动
rep(`  const updateBar = () => {
    const n = document.querySelectorAll(".sh-check:checked").length;
    $("sh-copy").style.display = n > 0 ? "inline-block" : "none";
    $("sh-del").style.display = n > 0 ? "inline-block" : "none";
  };`,
`  const updateBar = () => {
    const checks = [...document.querySelectorAll(".sh-check")];
    const n = checks.filter((c) => c.checked).length;
    $("sh-copy").style.display = n > 0 ? "inline-block" : "none";
    $("sh-del").style.display = n > 0 ? "inline-block" : "none";
    // 全选框联动：手动逐条勾选/取消也要同步表头
    if ($("sh-all")) $("sh-all").checked = checks.length > 0 && n === checks.length;
  };`, "select-all sync");

fs.writeFileSync(p, s);
console.log("total:", n);
