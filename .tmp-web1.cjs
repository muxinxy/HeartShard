const fs = require("fs");
const p = "D:/Documents/Code/HeartShard/server/src/webapp/app.js";
let s = fs.readFileSync(p, "utf8");
let n = 0;
const rep = (from, to, tag) => {
  if (!s.includes(from)) { console.log("MISS:", tag || from.slice(0, 40)); return; }
  s = s.split(from).join(to);
  n++;
  console.log("ok:", tag || from.slice(0, 30));
};

// ---- emoji → svg ----
rep('"☀️ 浅色模式" : "🌙 深色模式"', '"浅色模式" : "深色模式"', "menu-theme text");
rep('${list.map((a) => `<div class="album-row" style="cursor:pointer" data-aid="${a.id}">📁 ${a.name}</di',
    '${list.map((a) => `<div class="album-row" style="cursor:pointer" data-aid="${a.id}"><svg class="ic"><use href="#i-albums"/></svg> ${a.name}</di', "album rows");
rep('「⬆ 上传」', '「上传」', "upload hint");
rep('`<span class="motion-badge" title="动态照片">🎞</span>`',
    '`<span class="motion-badge" title="动态照片"><svg class="ic"><use href="#i-motion"/></svg></span>`', "motion badge");
rep('${b.favorite ? "♥ " : ""}${b.file_name}',
    '${b.favorite ? \'<svg class="ic" style="color:#ffd479"><use href="#i-heart-filled"/></svg> \' : ""}${b.file_name}', "cell fav prefix");
rep('$("lb-title").textContent = (b.is_motion ? "🎞 " : "") + b.file_name;',
    '$("lb-title").innerHTML = (b.is_motion ? \'<svg class="ic"><use href="#i-motion"/></svg> \' : "") + b.file_name;', "lb-title");
rep('$("lb-fav").textContent = lbCurrent.favorite ? "♥ 已收藏" : "♡ 收藏";',
    '$("lb-fav").innerHTML = lbCurrent.favorite ? \'<svg class="ic filled" style="color:#ffd479"><use href="#i-heart-filled"/></svg> 已收藏\' : \'<svg class="ic"><use href="#i-heart"/></svg> 收藏\';', "lb-fav");
rep('${card(\'data-kind="motion"\', "🎞", "动态照片", motions.length)}',
    '${card(\'data-kind="motion"\', "motion", "动态照片", motions.length)}', "explore motion icon");
rep('card(`data-place="${k}"`, "location", k, items.length)',
    'card(`data-place="${k}"`, "explore", k, items.length)', "place card icon");
rep('openFiltered("🆕 最近添加", recent)', 'openFiltered("最近添加", recent)', "title recent");
rep('openFiltered("🖼 照片", (b) => b.mime.startsWith("image/"))', 'openFiltered("照片", (b) => b.mime.startsWith("image/"))', "title photo");
rep('openFiltered("🎬 视频", (b) => b.mime.startsWith("video/"))', 'openFiltered("视频", (b) => b.mime.startsWith("video/"))', "title video");
rep('openFiltered("🎞 动态照片", motions)', 'openFiltered("动态照片", motions)', "title motion");
rep('openFiltered("📦 未入相册", orphans)', 'openFiltered("未入相册", orphans)', "title orphan");
rep('openFiltered(place ? `📍 ${place}` : "🗺 全部地点", gps)', 'openFiltered(place ? `${place}` : "全部地点", gps)', "title map list");
rep('openFiltered("📍 此处照片"', 'openFiltered("此处照片"', "title here");
rep('<h2 style="margin:6px 0">⭐ 收藏夹</h2>', '<h2 style="margin:6px 0"><svg class="ic"><use href="#i-favorites"/></svg> 收藏夹</h2>', "fav header");
rep('<button id="asel-share">🔗 分享</button>', '<button id="asel-share"><svg class="ic"><use href="#i-shares"/></svg>分享</button>', "asel-share");
// 相册勾选圆圈：放大镜 → ✓（与照片多选一致）
rep('<span class="check" data-aid="${a.id}"><svg class="ic" style="width:12px;height:12px"><use href="#i-explore"/></svg></span>',
    '<span class="check" data-aid="${a.id}">✓</span>', "album check ✓");

// admin 卡片文案（如有全角＋）
rep("＋👤 添加用户", '<svg class="ic"><use href="#i-plus-user"/></svg> 添加用户', "admin add user");
rep("＋🏷 创建组", '<svg class="ic"><use href="#i-groups"/></svg> 创建组', "admin add group");

fs.writeFileSync(p, s);
console.log("total replacements:", n);
