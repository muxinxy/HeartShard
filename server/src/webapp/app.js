// 牧心相册 PWA（M5a+）：hash 路由多视图 + MT Photos 风交互。
// 照片勾选圈常显（点击圆圈多选）、灯箱操作栏（收藏/分享/详情/回收站 + 缩放/旋转/翻转）、
// 探索页（地点/类别）、管理界面（用户卡片/组编辑弹窗）、全部弹窗自绘（无浏览器原生框）。
const $ = (id) => document.getElementById(id);
const TOKEN_KEY = "hs_token";
let token = localStorage.getItem(TOKEN_KEY) || "";
let me = null;
let albums = [];
let blobs = [];
let selected = new Set();
let currentAlbum = ""; // 相册详情视图的相册 id
let searchQ = "";

// ---------------- 基础设施 ----------------
function toast(msg, isErr = false) {
  const t = $("toast");
  t.textContent = msg;
  t.style.borderLeftColor = isErr ? "var(--danger)" : "var(--success)";
  t.style.display = "block";
  clearTimeout(t._h);
  t._h = setTimeout(() => (t.style.display = "none"), 4000);
}

async function api(path, opts = {}) {
  const headers = opts.headers || {};
  if (token) headers["authorization"] = `Bearer ${token}`;
  const res = await fetch(path, { ...opts, headers });
  if (res.status === 401) { showLogin(); throw new Error("登录已过期"); }
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new Error(body.error || res.statusText);
  }
  const ct = res.headers.get("content-type") || "";
  return ct.includes("json") ? res.json() : res;
}

// 自绘弹窗（替代浏览器原生 confirm/prompt/alert）
function modal(html) {
  const root = $("modal-root");
  root.innerHTML = `<div class="modal-backdrop"><div class="modal"><button class="close" onclick="closeModal()">✕</button>${html}</div></div>`;
}
function closeModal() { $("modal-root").innerHTML = ""; }
function uiConfirm(title, msg, onOk, danger = false) {
  modal(`<h3>${title}</h3><p style="color:var(--muted)">${msg}</p>
    <div class="row" style="justify-content:flex-end;margin-top:16px">
      <button id="uc-cancel">取消</button>
      <button class="${danger ? "danger" : "primary"}" id="uc-ok">确定</button>
    </div>`);
  $("uc-cancel").onclick = closeModal;
  $("uc-ok").onclick = async () => { closeModal(); await onOk(); };
}
// fields: [{id,label,type:"text"|"password",value,placeholder}]
function uiForm(title, fields, onOk) {
  modal(`<h3>${title}</h3>
    ${fields.map((f) => `<input id="${f.id}" type="${f.type || "text"}" placeholder="${f.placeholder || f.label}" value="${f.value || ""}" style="width:100%;margin:6px 0">`).join("")}
    <div class="row" style="justify-content:flex-end;margin-top:14px">
      <button id="uf-cancel">取消</button>
      <button class="primary" id="uf-ok">确定</button>
    </div>`);
  $("uf-cancel").onclick = closeModal;
  $("uf-ok").onclick = async () => {
    const vals = {};
    for (const f of fields) vals[f.id] = $(f.id).value;
    try { await onOk(vals); } catch (e) { toast(e.message, true); }
  };
}

// ---------------- 登录/会话 ----------------
function showLogin() {
  $("login").style.display = "block";
  $("app").classList.remove("layout");
  token = "";
  localStorage.removeItem(TOKEN_KEY);
}
function showMain() {
  $("login").style.display = "none";
  $("app").classList.add("layout");
  const u = me || {};
  $("avatar").textContent = (u.name || "我").slice(0, 1);
  $("menu-avatar").textContent = (u.name || "我").slice(0, 1);
  $("menu-name").textContent = u.name || "我";
  const isAdmin = !!u.is_admin;
  ["admin-users", "admin-groups", "admin-sources", "admin-status"].forEach((v) => {
    $(`nav-${v}`).style.display = isAdmin ? "flex" : "none";
  });
  $("admin-sec").style.display = isAdmin ? "block" : "none";
  route();
}

$("login-btn").onclick = async () => {
  try {
    const data = await fetch("/api/login", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name: $("user").value, password: $("pass").value }),
    }).then(async (r) => {
      if (!r.ok) throw new Error((await r.json().catch(() => ({}))).error || "登录失败");
      return r.json();
    });
    token = data.token;
    me = data;
    localStorage.setItem(TOKEN_KEY, token);
    $("pass").value = "";
    showMain();
  } catch (e) { toast(e.message, true); }
};
$("pass").addEventListener("keydown", (e) => e.key === "Enter" && $("login-btn").click());

// 头像下拉菜单（MT Photos 式）
$("avatar").onclick = () => {
  const menu = $("avatar-menu");
  menu.style.display = menu.style.display === "none" ? "block" : "none";
};
document.addEventListener("click", (e) => {
  if (!$("avatar-menu").contains(e.target) && e.target.id !== "avatar") {
    $("avatar-menu").style.display = "none";
  }
});
$("menu-logout").onclick = () => {
  fetch("/api/logout", { method: "POST", headers: { authorization: `Bearer ${token}` } }).catch(() => {});
  showLogin();
};
$("menu-theme").onclick = toggleTheme;
$("theme-toggle").onclick = toggleTheme;
function toggleTheme() {
  document.body.classList.toggle("theme-light");
  localStorage.setItem("hs_theme", document.body.classList.contains("theme-light") ? "light" : "dark");
}
$("menu-password").onclick = () => {
  uiForm("修改密码", [
    { id: "pw-old", type: "password", label: "旧口令" },
    { id: "pw-new", type: "password", label: "新口令" },
    { id: "pw-new2", type: "password", label: "确认新口令" },
  ], async (v) => {
    if (v["pw-new"] !== v["pw-new2"]) { toast("两次新口令不一致", true); return; }
    await api("/api/me/password", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ old: v["pw-old"], new: v["pw-new"] }),
    });
    closeModal();
    toast("密码已修改");
  });
};
if (localStorage.getItem("hs_theme") === "light") document.body.classList.add("theme-light");

if (token) {
  api("/api/me").then((u) => { me = u; showMain(); }).catch(() => showLogin());
} else showLogin();

// ---------------- 路由 ----------------
document.querySelectorAll(".sidebar a[data-view]").forEach((a) => {
  a.onclick = () => { location.hash = `#/${a.dataset.view}`; };
});
window.addEventListener("hashchange", route);

function route() {
  const view = (location.hash || "#/photos").replace(/^#\//, "");
  if (!token) return;
  document.querySelectorAll(".sidebar a[data-view]").forEach((a) => {
    a.classList.toggle("active", a.dataset.view === view);
  });
  $("years").innerHTML = "";
  $("search").style.display = ["photos", "albums"].includes(view) ? "block" : "none";
  $("upload-btn").style.display = view === "albums" || view.startsWith("album-") ? "inline-block" : "none";
  if (!["photos", "favorites"].includes(view) && !view.startsWith("album-")) clearSelection();
  currentAlbum = view.startsWith("album-") ? view.replace("album-", "") : "";
  const map = {
    photos: viewPhotos,
    explore: viewExplore,
    favorites: viewFavorites,
    albums: viewAlbums,
    shares: viewShares,
    trash: viewTrash,
    "admin-users": viewAdminUsers,
    "admin-groups": viewAdminGroups,
    "admin-sources": viewAdminSources,
    "admin-status": viewAdminStatus,
  };
  $("view").innerHTML = `<p class="hint">加载中…</p>`;
  (map[view] || viewPhotos)().catch((e) => { $("view").innerHTML = `<p class="hint">${e.message}</p>`; });
}

$("search").addEventListener("input", () => {
  searchQ = $("search").value.trim().toLowerCase();
  const view = (location.hash || "#/photos").replace(/^#\//, "");
  if (view === "photos") renderPhotoGrid();
  if (view === "albums") viewAlbums();
});

// ---------------- 多选（勾选圈常显，点击圆圈勾选） ----------------
function clearSelection() {
  selected = new Set();
  const bar = document.querySelector(".selbar");
  if (bar) bar.remove();
}
function toggleSel(hash) {
  selected.has(hash) ? selected.delete(hash) : selected.add(hash);
  renderSelbar();
}
function renderSelbar() {
  let bar = document.querySelector(".selbar");
  if (selected.size === 0) { if (bar) bar.remove(); return; }
  if (!bar) {
    bar = document.createElement("div");
    bar.className = "selbar";
    bar.innerHTML = `
      <span style="font-weight:600">已选 <span id="sel-count">0</span> 张</span>
      <button id="sel-fav">⭐ 收藏</button>
      <button id="sel-share">🔗 分享</button>
      <button id="sel-download">⬇ 下载</button>
      <button class="danger" id="sel-trash">🗑 回收站</button>
      <button id="sel-cancel">取消</button>`;
    document.body.appendChild(bar);
    $("sel-fav").onclick = () => batch((h) => api(`/api/blobs/${h}/favorite`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ favorite: true }) }), `已收藏`);
    $("sel-trash").onclick = () => uiConfirm("移入回收站", `把选中的 ${selected.size} 张照片移入回收站？碎片保留，可恢复。`, async () => {
      await batch((h) => api(`/api/blobs/${h}/delete`, { method: "POST" }), "已移入回收站");
      route();
    }, true);
    $("sel-download").onclick = async () => {
      for (const h of selected) {
        const b = blobs.find((x) => x.blob_hash === h);
        if (b) await downloadBlob(h, b.file_name);
      }
    };
    $("sel-share").onclick = shareSelected;
    $("sel-cancel").onclick = () => { clearSelection(); route(); };
  }
  $("sel-count").textContent = selected.size;
}
async function batch(fn, okMsg) {
  let ok = 0;
  for (const h of selected) { await fn(h).then(() => ok++).catch(() => {}); }
  toast(`${okMsg}（${ok}/${selected.size}）`);
  clearSelection();
  route();
}
async function shareSelected() {
  if (selected.size === 0) { toast("先勾选照片", true); return; }
  modal(`
    <h3>分享 ${selected.size} 张照片</h3>
    <p class="hint">链接分享默认去掉照片元数据（EXIF/GPS），接收者无需登录即可在浏览器解密查看。</p>
    <div class="row" style="margin-top:10px">
      <label>有效期 <select id="share-exp"><option value="">永久</option><option value="1">1 天</option><option value="7">7 天</option><option value="30">30 天</option></select></label>
      <button class="primary" id="share-go">创建链接</button>
    </div>`);
  $("share-go").onclick = async () => {
    const exp = $("share-exp").value;
    try {
      const data = await api("/api/shares", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ blob_hashes: [...selected], expires_days: exp ? Number(exp) : null }),
      });
      const url = location.origin + data.url;
      await navigator.clipboard.writeText(url).catch(() => {});
      closeModal();
      clearSelection();
      route();
      toast("分享链接已创建并复制：\n" + url);
    } catch (e) { toast(e.message, true); }
  };
}

// ---------------- 照片 ----------------
async function viewPhotos() {
  $("view").innerHTML = "";
  blobs = await api("/api/blobs");
  renderPhotoGrid();
}
function filteredBlobs() {
  return searchQ ? blobs.filter((b) => b.file_name.toLowerCase().includes(searchQ)) : blobs;
}
function renderPhotoGrid() {
  const list = filteredBlobs();
  const v = $("view");
  v.innerHTML = "";
  if (list.length === 0) { v.innerHTML = `<p class="hint">没有照片。上传入口在「相册」页选择相册后使用。</p>`; return; }
  const groups = new Map();
  for (const b of list) {
    const d = b.taken_at ? new Date(b.taken_at * 1000).toLocaleDateString() : "未知日期";
    (groups.get(d) || groups.set(d, []).get(d)).push(b);
  }
  const years = [...new Set(list.filter((b) => b.taken_at).map((b) => new Date(b.taken_at * 1000).getFullYear()))].sort((a, b) => b - a);
  $("years").innerHTML = years.map((y) => `<div data-year="${y}">${y}</div>`).join("");
  document.querySelectorAll("#years div").forEach((el) => {
    el.onclick = () => document.querySelector(`[data-anchor="${el.dataset.year}"]`)?.scrollIntoView({ behavior: "smooth" });
  });
  for (const [day, items] of groups) {
    const y = items[0].taken_at ? new Date(items[0].taken_at * 1000).getFullYear() : "";
    const header = document.createElement("div");
    header.className = "day-header";
    if (y && !document.querySelector(`[data-anchor="${y}"]`)) header.setAttribute("data-anchor", y);
    header.textContent = day;
    v.appendChild(header);
    const grid = document.createElement("div");
    grid.className = "grid";
    for (const b of items) grid.appendChild(photoCell(b));
    v.appendChild(grid);
  }
}
function photoCell(b) {
  const cell = document.createElement("div");
  cell.className = "cell" + (selected.has(b.blob_hash) ? " sel" : "");
  cell.innerHTML = `
    <span class="check" data-hash="${b.blob_hash}">✓</span>
    <span class="name">${b.favorite ? "♥ " : ""}${b.file_name}</span>`;
  if (b.mime.startsWith("image/")) {
    const img = new Image();
    img.loading = "lazy";
    img.src = `/api/blobs/${b.blob_hash}/thumbnail?token=${encodeURIComponent(token)}`;
    img.onerror = () => (cell.style.visibility = "hidden");
    cell.prepend(img);
  } else {
    cell.insertAdjacentHTML("afterbegin", `<div style="position:absolute;inset:0;display:grid;place-items:center;font-size:32px">🎬</div>`);
  }
  // 点击圆圈 = 勾选；点击图片本体 = 预览
  cell.querySelector(".check").onclick = (e) => {
    e.stopPropagation();
    toggleSel(b.blob_hash);
    cell.classList.toggle("sel", selected.has(b.blob_hash));
  };
  cell.onclick = () => openViewer(b);
  cell.oncontextmenu = (e) => { e.preventDefault(); openPhotoMenu(b); };
  return cell;
}

// ---------------- 灯箱（操作栏 + 缩放/旋转/翻转） ----------------
const viewer = $("viewer");
const lbImg = $("full-img");
let lbState = { zoom: 1, rotate: 0, flipH: 1, flipV: 1 };
let lbCurrent = null;
function lbTransform() {
  lbImg.style.transform = `scale(${lbState.zoom}) rotate(${lbState.rotate}deg) scale(${lbState.flipH}, ${lbState.flipV})`;
}
function openViewer(b) {
  if (!b.mime.startsWith("image/")) { toast("视频请用桌面/安卓客户端在线播放", true); return; }
  lbCurrent = b;
  lbState = { zoom: 1, rotate: 0, flipH: 1, flipV: 1 };
  lbTransform();
  $("lb-title").textContent = b.file_name;
  $("lb-fav").textContent = b.favorite ? "♥ 已收藏" : "♡ 收藏";
  $("full-img").src = `/api/blobs/${b.blob_hash}/raw?token=${encodeURIComponent(token)}`;
  viewer.style.display = "flex";
}
function closeViewer() { viewer.style.display = "none"; lbCurrent = null; }
$("lb-close").onclick = closeViewer;
$("lb-fav").onclick = async () => {
  if (!lbCurrent) return;
  await api(`/api/blobs/${lbCurrent.blob_hash}/favorite`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ favorite: !lbCurrent.favorite }) });
  lbCurrent.favorite = !lbCurrent.favorite;
  $("lb-fav").textContent = lbCurrent.favorite ? "♥ 已收藏" : "♡ 收藏";
  toast(lbCurrent.favorite ? "已收藏" : "已取消收藏");
};
$("lb-trash").onclick = () => {
  if (!lbCurrent) return;
  const b = lbCurrent;
  uiConfirm("移入回收站", `把「${b.file_name}」移入回收站？碎片保留，可恢复。`, async () => {
    await api(`/api/blobs/${b.blob_hash}/delete`, { method: "POST" });
    closeViewer();
    toast("已移入回收站");
    route();
  }, true);
};
$("lb-share").onclick = async () => {
  if (!lbCurrent) return;
  const b = lbCurrent;
  uiForm("分享照片", [{ id: "share-exp", label: "有效期（天数，留空永久）" }], async (v) => {
    const data = await api("/api/shares", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ blob_hashes: [b.blob_hash], expires_days: v["share-exp"] ? Number(v["share-exp"]) : null }),
    });
    const url = location.origin + data.url;
    await navigator.clipboard.writeText(url).catch(() => {});
    closeModal();
    toast("分享链接已创建并复制：\n" + url);
  });
};
$("lb-detail").onclick = () => {
  if (!lbCurrent) return;
  const b = lbCurrent;
  modal(`
    <h3>照片详情</h3>
    <table class="admin">
      <tr><td>名称</td><td>${b.file_name}</td></tr>
      <tr><td>类型</td><td>${b.mime}</td></tr>
      <tr><td>大小</td><td>${(b.total_len / 1048576).toFixed(2)} MB</td></tr>
      <tr><td>拍摄时间</td><td>${b.taken_at ? new Date(b.taken_at * 1000).toLocaleString() : "—"}</td></tr>
      <tr><td>位置</td><td>${b.gps_lat != null ? `${b.gps_lat.toFixed(4)}, ${b.gps_lon.toFixed(4)}` : "—"}</td></tr>
    </table>
    <div class="row" style="justify-content:flex-end"><button onclick="closeModal()">关闭</button></div>`);
};
$("lb-zoom-in").onclick = () => { lbState.zoom = Math.min(6, lbState.zoom * 1.25); lbTransform(); };
$("lb-zoom-out").onclick = () => { lbState.zoom = Math.max(0.3, lbState.zoom / 1.25); lbTransform(); };
$("lb-rotate").onclick = () => { lbState.rotate = (lbState.rotate + 90) % 360; lbTransform(); };
$("lb-flip-h").onclick = () => { lbState.flipH *= -1; lbTransform(); };
$("lb-flip-v").onclick = () => { lbState.flipV *= -1; lbTransform(); };
$("lb-reset").onclick = () => { lbState = { zoom: 1, rotate: 0, flipH: 1, flipV: 1 }; lbTransform(); };
$("lb-stage").addEventListener("wheel", (e) => {
  e.preventDefault();
  lbState.zoom = Math.min(6, Math.max(0.3, e.deltaY < 0 ? lbState.zoom * 1.15 : lbState.zoom / 1.15));
  lbTransform();
}, { passive: false });
viewer.onclick = (e) => { if (e.target === $("lb-stage")) closeViewer(); };

// 照片右键菜单（自绘）
function openPhotoMenu(b) {
  modal(`
    <h3>${b.file_name}</h3>
    <div style="display:flex;flex-direction:column;gap:8px">
      <button onclick="closeModal();openViewer({blob_hash:'${b.blob_hash}',mime:'${b.mime}',file_name:'${b.file_name}',favorite:${b.favorite},total_len:${b.total_len},taken_at:${b.taken_at || 0},gps_lat:${b.gps_lat ?? "null"},gps_lon:${b.gps_lon ?? "null"}})">🔍 预览</button>
      <button onclick="closeModal();fav('${b.blob_hash}',${!b.favorite})">${b.favorite ? "♥ 取消收藏" : "♡ 收藏"}</button>
      <button onclick="closeModal();downloadBlob('${b.blob_hash}','${b.file_name}')">⬇ 下载原图</button>
      <button class="danger" onclick="closeModal();delBlob('${b.blob_hash}','${b.file_name}')">🗑 移入回收站</button>
    </div>`);
}
window.closeModal = closeModal;
window.fav = async (hash, fav) => {
  await api(`/api/blobs/${hash}/favorite`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ favorite: fav }) });
  toast(fav ? "已收藏" : "已取消收藏");
  route();
};
window.downloadBlob = async (hash, name) => {
  const res = await fetch(`/api/blobs/${hash}/raw?token=${encodeURIComponent(token)}`);
  const buf = await res.arrayBuffer();
  const a = document.createElement("a");
  a.href = URL.createObjectURL(new Blob([buf]));
  a.download = name;
  a.click();
};
window.delBlob = (hash, name) => {
  uiConfirm("移入回收站", `把「${name}」移入回收站？碎片保留，可恢复。`, async () => {
    await api(`/api/blobs/${hash}/delete`, { method: "POST" });
    toast("已移入回收站");
    route();
  }, true);
};

// ---------------- 探索（地点 / 类别） ----------------
async function viewExplore() {
  const all = await api("/api/blobs");
  blobs = all;
  const photos = all.filter((b) => b.mime.startsWith("image/"));
  const videos = all.filter((b) => b.mime.startsWith("video/"));
  const recent = [...all].sort((a, b) => (b.taken_at || 0) - (a.taken_at || 0)).slice(0, 12);
  // GPS 聚类：坐标保留 1 位小数（约 11km 网格）
  const places = new Map();
  for (const b of all) {
    if (b.gps_lat == null || b.gps_lon == null) continue;
    const key = `${b.gps_lat.toFixed(1)}, ${b.gps_lon.toFixed(1)}`;
    (places.get(key) || places.set(key, []).get(key)).push(b);
  }
  const placeList = [...places.entries()].sort((a, b) => b[1].length - a[1].length);
  const v = $("view");
  v.innerHTML = `
    <h2 style="margin:6px 0">探索</h2>
    <h3 class="day-header">类别</h3>
    <div class="album-wall" id="exp-kinds">
      <div class="album-card" data-kind="recent"><div class="cov" style="display:grid;place-items:center;font-size:34px">🆕</div><div class="nm">最近添加<span class="cnt">${recent.length}</span></div></div>
      <div class="album-card" data-kind="photo"><div class="cov" style="display:grid;place-items:center;font-size:34px">🖼</div><div class="nm">照片<span class="cnt">${photos.length}</span></div></div>
      <div class="album-card" data-kind="video"><div class="cov" style="display:grid;place-items:center;font-size:34px">🎬</div><div class="nm">视频<span class="cnt">${videos.length}</span></div></div>
      <div class="album-card" data-kind="motion" title="动态照片检测将在后续版本上线"><div class="cov" style="display:grid;place-items:center;font-size:34px">🎞</div><div class="nm">动态照片<span class="cnt">即将支持</span></div></div>
    </div>
    <h3 class="day-header" style="margin-top:22px">地点（GPS 聚类）</h3>
    ${placeList.length === 0
      ? `<p class="hint">还没有带 GPS 位置的照片。导入的照片若含 EXIF GPS 会自动归入地点。</p>`
      : `<div class="album-wall" id="exp-places">
          ${placeList.map(([k, items]) => `
            <div class="album-card" data-place="${k}">
              <div class="cov" style="display:grid;place-items:center;font-size:30px">📍</div>
              <div class="nm"><span>${k}</span><span class="cnt">${items.length}</span></div>
            </div>`).join("")}
        </div>`}`;
  v.querySelector('[data-kind="recent"]').onclick = () => openFiltered("🆕 最近添加", recent);
  v.querySelector('[data-kind="photo"]').onclick = () => openFiltered("🖼 照片", (b) => b.mime.startsWith("image/"));
  v.querySelector('[data-kind="video"]').onclick = () => openFiltered("🎬 视频", (b) => b.mime.startsWith("video/"));
  v.querySelector('[data-kind="motion"]').onclick = () => toast("动态照片检测将在后续版本上线（M5c）");
  v.querySelectorAll("[data-place]").forEach((card) => {
    card.onclick = () => {
      const items = places.get(card.dataset.place) || [];
      openFiltered(`📍 ${card.dataset.place}`, items);
    };
  });
}

// 类型/地点命中后的网格视图
function openFiltered(title, predOrItems) {
  const list = Array.isArray(predOrItems) ? predOrItems : blobs.filter(predOrItems);
  const v = $("view");
  v.innerHTML = `
    <div class="row"><h2 style="margin:6px 0">${title}</h2><span class="badge">${list.length} 张</span>
      <span style="flex:1"></span><button id="back-explore">返回探索</button></div>
    <div id="filtered-grid" class="grid"></div>`;
  const grid = $("filtered-grid");
  for (const b of list) grid.appendChild(photoCell(b));
  if (list.length === 0) grid.innerHTML = `<p class="hint">没有照片。</p>`;
  $("back-explore").onclick = () => { location.hash = "#/explore"; };
  $("years").innerHTML = "";
}

// ---------------- 收藏夹 ----------------
async function viewFavorites() {
  blobs = await api("/api/blobs");
  const favs = blobs.filter((b) => b.favorite);
  const v = $("view");
  v.innerHTML = `
    <div class="row"><h2 style="margin:6px 0">⭐ 收藏夹</h2><span class="badge">${favs.length} 张</span></div>
    <div class="grid" id="fav-grid"></div>`;
  const grid = $("fav-grid");
  if (favs.length === 0) {
    v.innerHTML = `<p class="hint">还没有收藏。把鼠标移到照片上点左上角圆圈旁的右键菜单 → 收藏。</p>`;
    return;
  }
  for (const b of favs) grid.appendChild(photoCell(b));
}

// ---------------- 相册 ----------------
async function viewAlbums() {
  albums = await api("/api/albums");
  if (currentAlbum) return renderAlbumDetail();
  const v = $("view");
  const covers = {};
  for (const a of albums.slice(0, 200)) {
    try {
      const list = await api(`/api/albums/${a.id}/blobs`);
      const imgs = list.filter((b) => b.mime.startsWith("image/"));
      if (imgs.length > 0) covers[a.id] = imgs[0].blob_hash;
    } catch {}
  }
  const q = searchQ;
  const list = albums.filter((a) => !q || a.name.toLowerCase().includes(q));
  v.innerHTML = `
    <div class="row"><h2 style="margin:6px 0">相册</h2><span style="flex:1"></span>
      <button class="primary" id="album-new">＋ 新建相册</button></div>
    <div class="album-wall" id="wall">
      ${list.map((a) => `
        <div class="album-card" data-id="${a.id}">
          <div class="cov">${covers[a.id] ? `<img src="/api/blobs/${covers[a.id]}/thumbnail?token=${encodeURIComponent(token)}">` : `<div style="display:grid;place-items:center;height:100%;font-size:34px">📁</div>`}</div>
          <div class="nm"><span>${a.name}</span><span class="cnt">${a.blob_count} 张</span></div>
        </div>`).join("")}
    </div>`;
  v.querySelectorAll(".album-card").forEach((card) => {
    card.onclick = () => { location.hash = `#/album-${card.dataset.id}`; };
  });
  $("album-new").onclick = () => {
    uiForm("新建相册", [{ id: "al-name", label: "相册名称" }], async (v2) => {
      if (!v2["al-name"].trim()) { toast("名称不能为空", true); return; }
      await api("/api/albums", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ name: v2["al-name"] }) });
      closeModal();
      toast("相册已创建");
      route();
    });
  };
}
async function renderAlbumDetail() {
  const a = albums.find((x) => x.id === currentAlbum) || { id: currentAlbum, name: currentAlbum };
  blobs = await api(`/api/albums/${currentAlbum}/blobs`);
  const v = $("view");
  v.innerHTML = `
    <div class="row"><h2 style="margin:6px 0">📁 ${a.name}</h2>
      <span class="badge">${blobs.length} 张</span>
      <span style="flex:1"></span>
      <button id="acl-btn">权限</button>
      <button id="share-album-btn">🔗 分享相册</button>
      <button id="back-albums">返回</button></div>
    <div id="album-grid" class="grid"></div>`;
  const grid = $("album-grid");
  for (const b of blobs) grid.appendChild(photoCell(b));
  $("back-albums").onclick = () => { location.hash = "#/albums"; };
  $("share-album-btn").onclick = () => {
    uiConfirm("分享相册", `为「${a.name}」创建分享链接？接收者无需登录即可查看（图片自动去元数据）。`, async () => {
      const data = await api(`/api/albums/${currentAlbum}/share`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({}) });
      const url = location.origin + data.url;
      await navigator.clipboard.writeText(url).catch(() => {});
      toast("分享链接已创建并复制：\n" + url);
    });
  };
  $("acl-btn").onclick = () => openAclEditor(a);
}
async function openAclEditor(a) {
  const acl = await api(`/api/albums/${a.id}/acl`).catch(() => []);
  const users = me.is_admin ? await api("/api/admin/users").catch(() => []) : [];
  const groups = me.is_admin ? await api("/api/admin/groups").catch(() => []) : [];
  const PERM_LABEL = { none: "隐藏", view: "只看", upload: "投稿", manage: "管理" };
  const rowFor = (kind, id, label, cur) => `
    <tr><td>${kind === "user" ? "👤" : "🏷"} ${label}</td><td>${cur ? PERM_LABEL[cur] || cur : "继承"}</td><td>
      <select onchange="setAcl('${a.id}','${kind}','${id}',this.value)">
        <option value="">继承</option>
        ${["none", "view", "upload", "manage"].map((p) => `<option value="${p}" ${cur === p ? "selected" : ""}>${PERM_LABEL[p]}</option>`).join("")}
      </select></td></tr>`;
  modal(`
    <h3>权限：${a.name}</h3>
    <p class="hint">子相册默认继承；显式设置覆盖继承；同一层级用户设置优先于组。</p>
    <table class="admin"><tr><th>对象</th><th>当前</th><th>设置</th></tr>
      ${acl.filter((e) => e.principal.kind === "user").map((e) => rowFor("user", e.principal.id, users.find((u) => u.uid === e.principal.id)?.name || e.principal.id, e.perm)).join("")}
      ${acl.filter((e) => e.principal.kind === "group").map((e) => rowFor("group", e.principal.id, groups.find((g) => g.gid === e.principal.id)?.name || e.principal.id, e.perm)).join("")}
      ${users.map((u) => acl.some((e) => e.principal.kind === "user" && e.principal.id === u.uid) ? "" : rowFor("user", u.uid, u.name, "")).join("")}
      ${groups.map((g) => acl.some((e) => e.principal.kind === "group" && e.principal.id === g.gid) ? "" : rowFor("group", g.gid, g.name, "")).join("")}
    </table>`);
}
window.setAcl = async (albumId, kind, id, perm) => {
  try {
    await api(`/api/albums/${albumId}/acl`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ principal: { type: kind, id }, perm: perm || null }),
    });
    toast("权限已保存");
    closeModal();
  } catch (e) { toast(e.message, true); }
};

// ---------------- 分享列表 ----------------
async function viewShares() {
  const shares = await api("/api/shares");
  $("view").innerHTML = `
    <h2 style="margin:6px 0">分享</h2>
    ${shares.length === 0 ? `<p class="hint">还没有分享。在照片（勾选圆圈多选）或相册页创建。</p>` : `
    <table class="admin"><tr><th>名称</th><th>类型</th><th>数量</th><th>过期</th><th></th></tr>
      ${shares.map((s) => `
        <tr><td>${s.album_name || s.id}</td>
        <td>${s.kind === "album" ? "相册" : "照片"}</td>
        <td>${(s.blob_hashes || []).length}</td>
        <td>${s.expires_at ? new Date(s.expires_at * 1000).toLocaleString() : "永久"}</td>
        <td class="row">
          <button onclick="copyShare('${s.id}')">复制链接</button>
          <button class="danger" onclick="delShare('${s.id}')">删除</button>
        </td></tr>`).join("")}
    </table>`}`;
}
window.copyShare = async (id) => {
  await navigator.clipboard.writeText(location.origin + `/s/${id}`).catch(() => {});
  toast("链接已复制");
};
window.delShare = (id) => {
  uiConfirm("删除分享", "删除后链接立即失效，确定？", async () => {
    await api(`/api/shares/${id}`, { method: "DELETE" });
    route();
  }, true);
};

// ---------------- 回收站 ----------------
async function viewTrash() {
  const list = await api("/api/trash").catch(() => []);
  $("view").innerHTML = `
    <h2 style="margin:6px 0">回收站</h2>
    ${list.length === 0 ? `<p class="hint">回收站是空的。</p>` : `
    <table class="admin"><tr><th>名称</th><th>大小</th><th>拍摄时间</th><th></th></tr>
      ${list.map((b) => `
        <tr><td>${b.file_name}</td><td>${(b.total_len / 1048576).toFixed(1)} MB</td>
        <td>${b.taken_at ? new Date(b.taken_at * 1000).toLocaleString() : "—"}</td>
        <td class="row">
          <button onclick="restoreBlob('${b.blob_hash}')">恢复</button>
          <button class="danger" onclick="purgeBlob('${b.blob_hash}','${b.file_name}')">彻底删除</button>
        </td></tr>`).join("")}
    </table>`}`;
}
window.restoreBlob = async (h) => { await api(`/api/blobs/${h}/restore`, { method: "POST" }); toast("已恢复"); route(); };
window.purgeBlob = (h, n) => {
  uiConfirm("彻底删除", `从存储中清除「${n}」的加密碎片？此操作无法恢复。`, async () => {
    await api(`/api/blobs/${h}/purge`, { method: "POST" });
    route();
  }, true);
};

// ---------------- 上传 ----------------
$("upload-btn").onclick = () => {
  if (!currentAlbum) { toast("先进入一个相册再上传（相册页点开相册）", true); return; }
  $("file-input").click();
};
$("file-input").onchange = async () => {
  const files = [...$("file-input").files];
  if (files.length === 0) return;
  let ok = 0, fail = 0;
  for (const f of files) {
    const fd = new FormData();
    fd.append("file", f, f.name);
    try {
      const res = await api(`/api/albums/${currentAlbum}/upload`, { method: "POST", body: fd });
      res.status === "imported" ? ok++ : fail++;
    } catch (e) { fail++; toast(f.name + "：" + e.message, true); }
  }
  $("file-input").value = "";
  toast(`上传完成：${ok} 成功${fail ? `，${fail} 失败` : ""}`, fail > 0);
  route();
};

// ---------------- 管理：用户（MT Photos 卡片式） ----------------
async function viewAdminUsers() {
  const users = await api("/api/admin/users");
  const card = (u) => `
    <div class="album-card user-card" data-uid="${u.uid}">
      <div class="cov" style="background:var(--primary);display:grid;place-items:center">
        <div class="avatar" style="width:84px;height:84px;font-size:32px;background:rgba(255,255,255,.25)">${u.name.slice(0, 1)}</div>
      </div>
      <div class="nm"><span>${u.name}${u.is_admin ? ' <span class="badge">管理员</span>' : ""}${u.enabled === false ? ' <span class="badge" style="color:var(--danger)">已禁用</span>' : ""}</span></div>
      <div class="row" style="padding:0 10px 10px">
        <button onclick="editUser('${u.uid}')">✏️ 编辑</button>
        <button onclick="resetPw('${u.uid}','${u.name}')">🔑 重置密码</button>
        <button class="danger" onclick="delUser('${u.uid}','${u.name}')">🗑 删除</button>
      </div>
    </div>`;
  $("view").innerHTML = `
    <h2 style="margin:6px 0">用户管理</h2>
    <div class="album-wall" id="user-wall">
      ${users.map(card).join("")}
      <div class="album-card" style="border:2px dashed var(--border);display:grid;place-items:center;min-height:220px;cursor:pointer" onclick="addUserDialog()">
        <div style="text-align:center"><div style="font-size:34px">＋👤</div><div style="font-weight:600;margin-top:6px">添加用户</div></div>
      </div>
    </div>`;
}
window.addUserDialog = () => {
  uiForm("添加用户", [
    { id: "nu-name", label: "用户名" },
    { id: "nu-pass", type: "password", label: "口令" },
  ], async (v) => {
    await api("/api/admin/users", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name: v["nu-name"], password: v["nu-pass"], admin: false }),
    });
    closeModal();
    toast("用户已创建");
    route();
  });
};
window.editUser = async (uid) => {
  const users = await api("/api/admin/users");
  const u = users.find((x) => x.uid === uid);
  if (!u) return;
  const albums = await api("/api/albums");
  const PERM_LABEL = { none: "隐藏", view: "只看", upload: "投稿", manage: "管理" };
  // 该用户在各相册的显式权限
  const aclOf = {};
  for (const a of albums) {
    const entries = await api(`/api/albums/${a.id}/acl`).catch(() => []);
    const hit = entries.find((e) => e.principal.kind === "user" && e.principal.id === uid);
    aclOf[a.id] = hit ? hit.perm : "";
  }
  modal(`
    <h3>编辑用户：${u.name}</h3>
    <div class="avatar" style="width:64px;height:64px;font-size:24px;margin:4px auto 10px;background:var(--primary)">${u.name.slice(0, 1)}</div>
    <label class="hint">显示名 / 用户名（uid）</label>
    <input id="eu-name" value="${u.name}" style="width:100%;margin:4px 0 10px">
    <label class="hint">新口令（留空不改）</label>
    <input id="eu-pass" type="password" style="width:100%;margin:4px 0 10px">
    <div class="row" style="margin:8px 0">
      <label><input type="checkbox" id="eu-admin" ${u.is_admin ? "checked" : ""}> 管理员</label>
      <label style="margin-left:16px"><input type="checkbox" id="eu-enabled" ${u.enabled === false ? "" : "checked"}> 启用此用户</label>
    </div>
    ${albums.length > 0 ? `<label class="hint">相册权限（默认 = 继承）</label>
    <table class="admin">${albums.map((a) => `
      <tr><td>📁 ${a.name}</td><td>
        <select id="eu-acl-${a.id}">
          <option value="">继承</option>
          ${["none", "view", "upload", "manage"].map((p) => `<option value="${p}" ${aclOf[a.id] === p ? "selected" : ""}>${PERM_LABEL[p]}</option>`).join("")}
        </select></td></tr>`).join("")}
    </table>` : ""}
    <div class="row" style="justify-content:flex-end;margin-top:14px">
      <button id="eu-cancel">取消</button>
      <button class="primary" id="eu-ok">保存</button>
    </div>`);
  $("eu-cancel").onclick = closeModal;
  $("eu-ok").onclick = async () => {
    try {
      await api(`/api/admin/users/${uid}`, {
        method: "PUT",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          name: $("eu-name").value,
          password: $("eu-pass").value || null,
          admin: $("eu-admin").checked,
          enabled: $("eu-enabled").checked,
        }),
      });
      // 相册权限逐个保存（仅变化的）
      for (const a of albums) {
        const sel = $(`eu-acl-${a.id}`);
        if (!sel) continue;
        await api(`/api/albums/${a.id}/acl`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ principal: { type: "user", id: uid }, perm: sel.value || null }),
        });
      }
      closeModal();
      toast("用户已保存");
      route();
    } catch (e) { toast(e.message, true); }
  };
};
window.resetPw = (uid, name) => {
  uiForm(`重置「${name}」的口令`, [{ id: "rp-pass", type: "password", label: "新口令" }], async (v) => {
    await api(`/api/admin/users/${uid}`, { method: "PUT", headers: { "content-type": "application/json" }, body: JSON.stringify({ password: v["rp-pass"] }) });
    closeModal();
    toast("口令已重置");
  });
};
window.delUser = (uid, name) => {
  uiConfirm("删除用户", `删除「${name}」？其会话将立即失效。`, async () => {
    await api(`/api/admin/users/${uid}`, { method: "DELETE" });
    route();
  }, true);
};

// ---------------- 管理：用户组 ----------------
async function viewAdminGroups() {
  const groups = await api("/api/admin/groups");
  $("view").innerHTML = `
    <h2 style="margin:6px 0">用户组</h2>
    <p class="hint">family 组为全员组（自动维护）；自定义组用于批量授权。编辑可添加/删除成员并设置组对各相册的权限。</p>
    <div class="album-wall" id="group-wall">
      ${groups.map((g) => `
        <div class="album-card">
          <div class="cov" style="background:var(--success);display:grid;place-items:center">
            <div class="avatar" style="width:84px;height:84px;font-size:32px;background:rgba(255,255,255,.25)">🏷</div>
          </div>
          <div class="nm"><span>${g.name}</span><span class="cnt">${g.members.length} 人</span></div>
          <div class="row" style="padding:0 10px 10px">
            <button onclick="editGroup('${g.gid}')">✏️ 编辑</button>
            ${g.gid !== "family" ? `<button class="danger" onclick="delGroup('${g.gid}','${g.name}')">🗑 删除</button>` : ""}
          </div>
        </div>`).join("")}
      <div class="album-card" style="border:2px dashed var(--border);display:grid;place-items:center;min-height:220px;cursor:pointer" onclick="addGroupDialog()">
        <div style="text-align:center"><div style="font-size:34px">＋🏷</div><div style="font-weight:600;margin-top:6px">创建组</div></div>
      </div>
    </div>`;
}
window.addGroupDialog = () => {
  uiForm("创建用户组", [{ id: "ng-name", label: "组名" }], async (v) => {
    if (!v["ng-name"].trim()) { toast("组名不能为空", true); return; }
    const cur = await api("/api/admin/groups");
    cur.push({ gid: `g-${v["ng-name"].trim().toLowerCase()}`, name: v["ng-name"].trim(), members: [] });
    await api("/api/admin/groups", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ groups: cur }) });
    closeModal();
    route();
  });
};
window.editGroup = async (gid) => {
  const groups = await api("/api/admin/groups");
  const g = groups.find((x) => x.gid === gid);
  if (!g) return;
  const users = await api("/api/admin/users");
  const albums = await api("/api/albums");
  const PERM_LABEL = { none: "隐藏", view: "只看", upload: "投稿", manage: "管理" };
  const aclOf = {};
  for (const a of albums) {
    const entries = await api(`/api/albums/${a.id}/acl`).catch(() => []);
    const hit = entries.find((e) => e.principal.kind === "group" && e.principal.id === gid);
    aclOf[a.id] = hit ? hit.perm : "";
  }
  modal(`
    <h3>编辑组：${g.name}</h3>
    <label class="hint">成员（勾选 = 在组内）</label>
    <div style="max-height:160px;overflow-y:auto;background:var(--input);border-radius:10px;padding:8px 12px;margin:6px 0 12px">
      ${users.map((u) => `<label style="display:block;padding:4px 0"><input type="checkbox" class="eg-member" data-uid="${u.uid}" ${g.members.includes(u.uid) ? "checked" : ""}> ${u.name}</label>`).join("") || `<p class="hint">还没有用户。</p>`}
    </div>
    ${albums.length > 0 ? `<label class="hint">组对各相册的权限（默认 = 继承）</label>
    <table class="admin">${albums.map((a) => `
      <tr><td>📁 ${a.name}</td><td>
        <select id="eg-acl-${a.id}">
          <option value="">继承</option>
          ${["none", "view", "upload", "manage"].map((p) => `<option value="${p}" ${aclOf[a.id] === p ? "selected" : ""}>${PERM_LABEL[p]}</option>`).join("")}
        </select></td></tr>`).join("")}
    </table>` : ""}
    <div class="row" style="justify-content:flex-end;margin-top:14px">
      <button id="eg-cancel">取消</button>
      <button class="primary" id="eg-ok">保存</button>
    </div>`);
  $("eg-cancel").onclick = closeModal;
  $("eg-ok").onclick = async () => {
    try {
      // 成员更新
      const members = [...document.querySelectorAll(".eg-member:checked")].map((c) => c.dataset.uid);
      const idx = groups.findIndex((x) => x.gid === gid);
      groups[idx].members = members;
      await api("/api/admin/groups", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ groups }) });
      // 组对各相册的权限
      for (const a of albums) {
        const sel = $(`eg-acl-${a.id}`);
        if (!sel) continue;
        await api(`/api/albums/${a.id}/acl`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ principal: { type: "group", id: gid }, perm: sel.value || null }),
        });
      }
      closeModal();
      toast("组已保存");
      route();
    } catch (e) { toast(e.message, true); }
  };
};
window.delGroup = (gid, name) => {
  uiConfirm("删除用户组", `删除组「${name}」？`, async () => {
    const cur = (await api("/api/admin/groups")).filter((g) => g.gid !== gid);
    await api("/api/admin/groups", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ groups: cur }) });
    route();
  }, true);
};

// ---------------- 管理：同步源 ----------------
async function viewAdminSources() {
  const sources = await api("/api/admin/sources");
  const KINDS = { local: "本地目录", s3: "S3 兼容", webdav: "WebDAV", openlist: "OpenList" };
  const srcCard = (s) => `
    <div class="form-card" data-sid="${s.id}">
      <strong>${s.id}</strong><span class="badge">${KINDS[s.kind] || s.kind}</span>
      <span class="hint">${s.root || (s.s3 && s.s3.endpoint) || (s.webdav && s.webdav.base_url) || (s.openlist && s.openlist.base_url) || ""}</span>
      <span class="hint">策略 ${s.strategy || "append"} · 删除 ${s.delete_mode || "keep"}</span>
      <span style="flex:1"></span>
      <button class="danger" onclick="delSource('${s.id}')">删除</button>
    </div>`;
  $("view").innerHTML = `
    <h2 style="margin:6px 0">同步源</h2>
    <p class="hint">同步源由后台按计划自动执行（append=只新增 / mirror=镜像；删除 keep/trash）。修改保存后下一轮生效，或到系统状态页手动触发。</p>
    <div id="src-list">${sources.map(srcCard).join("") || `<p class="hint">还没有同步源。</p>`}</div>
    <div class="form-card">
      <strong>新增源：</strong>
      <input id="ns-id" placeholder="id（如 phone）">
      <select id="ns-kind"><option value="local">本地目录</option><option value="s3">S3 兼容</option><option value="webdav">WebDAV</option><option value="openlist">OpenList</option></select>
      <input id="ns-root" placeholder="本地路径 / 端点 URL">
      <select id="ns-strategy"><option value="append">append 仅新增</option><option value="mirror">mirror 镜像</option></select>
      <select id="ns-del"><option value="keep">删除保留</option><option value="trash">删除进回收站</option></select>
      <button class="primary" id="ns-add">添加</button>
    </div>
    <p class="hint">S3/WebDAV/OpenList 的凭据暂不支持网页端配置（防泄漏），请写在 server.toml 的 sources 段由启动迁移，或参考文档手动编辑 sources.json。</p>`;
  $("ns-add").onclick = async () => {
    const id = $("ns-id").value.trim();
    const kind = $("ns-kind").value;
    if (!id) { toast("需要 id", true); return; }
    const s = {
      id, kind,
      root: kind === "local" ? $("ns-root").value : null,
      strategy: $("ns-strategy").value,
      delete_mode: $("ns-del").value,
    };
    if (kind !== "local") {
      const url = $("ns-root").value;
      if (!url) { toast("需要端点 URL 或路径", true); return; }
      if (kind === "s3") s.s3 = { endpoint: url, bucket: prompt("bucket：") || "", access_key: prompt("access_key：") || "", secret_key: prompt("secret_key：") || "", region: "us-east-1", path_style: true };
      if (kind === "webdav") s.webdav = { base_url: url, username: prompt("用户名：") || "", password: prompt("密码：") || "" };
      if (kind === "openlist") s.openlist = { base_url: url, username: prompt("用户名：") || "", password: prompt("密码：") || "", base_path: "/heartshard" };
    }
    const cur = await api("/api/admin/sources");
    cur.push(s);
    try {
      await api("/api/admin/sources", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(cur) });
      route();
    } catch (e) { toast(e.message, true); }
  };
}
window.delSource = async (sid) => {
  const cur = (await api("/api/admin/sources")).filter((s) => s.id !== sid);
  await api("/api/admin/sources", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(cur) });
  route();
};

// ---------------- 管理：系统状态 ----------------
async function viewAdminStatus() {
  const st = await api("/api/admin/status");
  $("view").innerHTML = `
    <h2 style="margin:6px 0">系统状态</h2>
    <table class="admin">
      <tr><th>用户</th><td>${st.users}</td></tr>
      <tr><th>用户组</th><td>${st.groups}</td></tr>
      <tr><th>相册</th><td>${st.albums}</td></tr>
      <tr><th>照片</th><td>${st.blobs}</td></tr>
      <tr><th>回收站</th><td>${st.trash}</td></tr>
      <tr><th>分享</th><td>${st.shares}</td></tr>
      <tr><th>同步源</th><td>${st.sources}</td></tr>
      <tr><th>同步间隔</th><td>${st.sync_interval_secs > 0 ? `${st.sync_interval_secs} 秒` : "关闭（仅手动）"}</td></tr>
    </table>
    <div class="form-card"><span class="hint">手动触发一次全源同步：</span><button class="primary" id="sync-run">立即同步</button><span id="sync-out" class="hint"></span></div>`;
  $("sync-run").onclick = async () => {
    $("sync-out").textContent = "同步中…";
    try {
      const report = await api("/api/sync/run", { method: "POST" });
      $("sync-out").textContent = report.map((r) => `${r.source}：新增 ${r.added} · 移动 ${r.moved} · 删除 ${r.removed}`).join("；");
    } catch (e) { $("sync-out").textContent = e.message; }
  };
}

// ---------------- PWA ----------------
if ("serviceWorker" in navigator) navigator.serviceWorker.register("/sw.js").catch(() => {});
