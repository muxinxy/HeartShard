// 牧心相册 PWA（M5a+ 交互打磨）：
// - 勾选圈常显（点圆圈多选，无需选择模式按钮）
// - 右键菜单在鼠标位置弹出（照片/相册）
// - 灯箱：操作栏（收藏/分享/详情/重命名/回收站）+ 滚轮缩放/旋转/翻转 + 图片适配屏幕
// - 时间线：按天分组 + 稀疏月份隔断 + 当天全选 + 动态加载 + 上传（可选相册）
// - 探索：类别（最近添加/时间线/照片/视频/动态照片/未入相册）+ 地点（GPS 聚类）
// - 管理：用户/组卡片编辑弹窗（含相册权限与头像）；全部弹窗自绘
const $ = (id) => document.getElementById(id);
const TOKEN_KEY = "hs_token";
let token = localStorage.getItem(TOKEN_KEY) || "";
let me = null;
let albums = [];
let blobs = [];
let selected = new Set();          // 照片勾选
let selectedAlbums = new Set();    // 相册勾选
let currentAlbum = "";
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

// 自绘弹窗（无浏览器原生框）
function modal(html) {
  $("modal-root").innerHTML = `<div class="modal-backdrop"><div class="modal"><button class="close" onclick="closeModal()">✕</button>${html}</div></div>`;
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
// 鼠标位置右键菜单（按索引分发，避免闭包源码字符串化丢变量）
let ctxItems = [];
function ctxMenu(x, y, items) {
  const root = $("ctx-root");
  ctxItems = items;
  const w = 190;
  const px = Math.min(x, window.innerWidth - w - 8);
  const py = Math.min(y, window.innerHeight - items.length * 42 - 12);
  root.innerHTML = `<div class="menu-overlay" onclick="closeCtx()"></div>
    <div class="ctx-menu" style="left:${px}px;top:${py}px">
      ${items.map((it, i) => `<button class="${it.danger ? "danger" : ""}" data-i="${i}">${it.label}</button>`).join("")}
    </div>`;
  root.querySelectorAll(".ctx-menu button").forEach((btn) => {
    btn.onclick = () => {
      const fn = ctxItems[Number(btn.dataset.i)].fn;
      closeCtx();
      fn();
    };
  });
}
function closeCtx() { $("ctx-root").innerHTML = ""; }
window.closeCtx = closeCtx;

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
  setAvatarUi($("avatar"), u);
  setAvatarUi($("menu-avatar"), u);
  $("menu-name").textContent = u.name || "我";
  const isAdmin = !!u.is_admin;
  ["admin-users", "admin-groups", "admin-sources", "admin-status"].forEach((v) => {
    $(`nav-${v}`).style.display = isAdmin ? "flex" : "none";
  });
  $("admin-sec").style.display = isAdmin ? "block" : "none";
  route();
}
function setAvatarUi(el, u) {
  if (u && u.avatar) {
    el.textContent = "";
    el.style.background = `center/cover url(${u.avatar})`;
  } else {
    el.textContent = (u && u.name ? u.name : "我").slice(0, 1);
    el.style.background = "var(--primary)";
  }
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
function toggleTheme() {
  document.body.classList.toggle("theme-light");
  const light = document.body.classList.contains("theme-light");
  localStorage.setItem("hs_theme", light ? "light" : "dark");
  $("menu-theme").textContent = light ? "☀️ 浅色模式" : "🌙 深色模式";
}
$("menu-theme").onclick = toggleTheme;
// 头像上传（点菜单大头像 → 选图 → 缩到 256px → data URL）
$("menu-avatar").onclick = () => $("avatar-input").click();
$("avatar-input").onchange = async () => {
  const file = $("avatar-input").files[0];
  if (!file) return;
  const img = new Image();
  img.onload = async () => {
    const c = document.createElement("canvas");
    const side = Math.min(img.width, img.height);
    c.width = c.height = 256;
    c.getContext("2d").drawImage(img, (img.width - side) / 2, (img.height - side) / 2, side, side, 0, 0, 256, 256);
    const dataUrl = c.toDataURL("image/jpeg", 0.85);
    try {
      await api("/api/me/avatar", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ data_url: dataUrl }) });
      me.avatar = dataUrl;
      setAvatarUi($("avatar"), me);
      setAvatarUi($("menu-avatar"), me);
      toast("头像已更新");
    } catch (e) { toast(e.message, true); }
  };
  img.src = URL.createObjectURL(file);
  $("avatar-input").value = "";
};
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
// 深浅色菜单项初始文案
$("menu-theme").textContent = document.body.classList.contains("theme-light") ? "☀️ 浅色模式" : "🌙 深色模式";

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
  $("upload-btn").style.display = ["photos", "albums"].includes(view) || view.startsWith("album-") ? "inline-block" : "none";
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

// ---------------- 多选（勾选圈常显） ----------------
function clearSelection() {
  selected = new Set();
  selectedAlbums = new Set();
  const bar = document.querySelector(".selbar");
  if (bar) bar.remove();
}
let currentGroups = []; // 当前时间线分组（同步日期复选框状态用）
function toggleSel(hash) {
  selected.has(hash) ? selected.delete(hash) : selected.add(hash);
  renderSelbar();
  updateDayChecks();
}
function updateDayChecks() {
  document.querySelectorAll("input[data-day]").forEach((c) => {
    const g = currentGroups.find((x) => x.key === c.dataset.day);
    if (g) c.checked = g.items.every((b) => selected.has(b.blob_hash));
  });
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
      <button id="sel-rename">✏️ 重命名</button>
      <button id="sel-share">🔗 分享</button>
      <button id="sel-detail">ℹ️ 详情</button>
      <button id="sel-download">⬇️ 下载</button>
      <button id="sel-move">↪️ 移动到</button>
      <button id="sel-copy">⧉️ 复制到</button>
      <button class="danger" id="sel-trash">🗑️ 回收站</button>
      <button id="sel-cancel">取消</button>`;
    document.body.appendChild(bar);
    $("sel-fav").onclick = () => batch((h) => api(`/api/blobs/${h}/favorite`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ favorite: true }) }), "已收藏");
    $("sel-rename").onclick = () => {
      if (selected.size === 1) {
        const b = blobs.find((x) => selected.has(x.blob_hash));
        if (b) renameDialog(b);
      } else batchRenameDialog();
    };
    $("sel-share").onclick = () => shareDialog([...selected]);
    $("sel-detail").onclick = () => {
      if (selected.size === 1) {
        const first = blobs.find((b) => selected.has(b.blob_hash));
        if (first) showBlobDetail(first);
        return;
      }
      // 多选详情：数量 + 总大小
      let total = 0, n = 0, vids = 0;
      for (const h of selected) {
        const b = blobs.find((x) => x.blob_hash === h);
        if (b) { n++; total += b.total_len; if (b.mime.startsWith("video/")) vids++; }
      }
      modal(`<h3>详情（${n} 项）</h3>
        <table class="admin">
          <tr><td>数量</td><td>${n} 项（视频 ${vids} 个）</td></tr>
          <tr><td>总大小</td><td>${(total / 1048576).toFixed(2)} MB</td></tr>
        </table>
        <div class="row" style="justify-content:flex-end"><button onclick="closeModal()">关闭</button></div>`);
    };
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
    $("sel-move").onclick = () => pickAlbumThen("move", [...selected]);
    $("sel-copy").onclick = () => pickAlbumThen("copy", [...selected]);
    $("sel-cancel").onclick = () => { clearSelection(); route(); };
  }
  $("sel-count").textContent = selected.size;
}
// 选择目标相册（移动/复制）
function pickAlbumThen(mode, hashes) {
  api("/api/albums").then((list) => {
    modal(`
      <h3>${mode === "move" ? "移动到相册" : "复制到相册"}</h3>
      <p class="hint">${mode === "move" ? "照片将加入目标相册，并从当前位置移除。" : "照片将加入目标相册，原位置保留。"}</p>
      <div style="max-height:260px;overflow-y:auto">
        ${list.map((a) => `<div class="album-row" style="cursor:pointer" data-aid="${a.id}">📁 ${a.name}</div>`).join("") || `<p class="hint">还没有相册。</p>`}
      </div>`);
    document.querySelectorAll("#modal-root .album-row").forEach((row) => {
      row.onclick = async () => {
        const aid = row.dataset.aid;
        const op = mode === "move"
          ? api(`/api/albums/${aid}/move-blobs`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ blob_hashes: hashes }) })
          : api(`/api/albums/${aid}/add-blobs`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ blob_hashes: hashes }) });
        try {
          await op;
          closeModal();
          clearSelection();
          toast(mode === "move" ? "已移动" : "已复制");
          route();
        } catch (e) { toast(e.message, true); }
      };
    });
  });
}
async function batch(fn, okMsg) {
  let ok = 0;
  for (const h of selected) { await fn(h).then(() => ok++).catch(() => {}); }
  toast(`${okMsg}（${ok}/${selected.size}）`);
  clearSelection();
  route();
}
// 分享弹窗（聚合/逐个 + 可选分享码）
function shareDialog(hashes) {
  modal(`
    <h3>分享 ${hashes.length} 张照片</h3>
    <p class="hint">链接分享默认去掉照片元数据（EXIF/GPS），接收者无需登录即可在浏览器解密查看。</p>
    <div class="row" style="margin:8px 0">
      <label><input type="radio" name="share-mode" value="agg" checked> 聚合分享（一个链接）</label>
      <label style="margin-left:12px"><input type="radio" name="share-mode" value="each"> 逐个分享（每张一个链接）</label>
    </div>
    <div class="row">
      <label>有效期 <select id="share-exp"><option value="">永久</option><option value="1">1 天</option><option value="7">7 天</option><option value="30">30 天</option></select></label>
      <input id="share-code" placeholder="分享码（可选）">
    </div>
    <div class="row" style="justify-content:flex-end;margin-top:12px"><button class="primary" id="share-go">创建链接</button></div>`);
  $("share-go").onclick = async () => {
    const mode = document.querySelector('input[name="share-mode"]:checked').value;
    const exp = $("share-exp").value ? Number($("share-exp").value) : null;
    const code = $("share-code").value.trim() || null;
    try {
      const urls = [];
      if (mode === "agg") {
        const data = await api("/api/shares", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ blob_hashes: hashes, expires_days: exp, access_code: code }) });
        urls.push(location.origin + data.url + (code ? `（分享码：${code}）` : ""));
      } else {
        for (const h of hashes) {
          const data = await api("/api/shares", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ blob_hashes: [h], expires_days: exp, access_code: code }) });
          urls.push(location.origin + data.url);
        }
      }
      closeModal();
      clearSelection();
      modal(`<h3>分享链接（${urls.length} 个）</h3>
        <textarea style="width:100%;height:140px" readonly>${urls.join("\n")}</textarea>
        <div class="row" style="justify-content:flex-end"><button class="primary" onclick="copyText(${JSON.stringify(urls.join("\n")).replace(/"/g, "&quot;")})">复制全部</button></div>`);
    } catch (e) { toast(e.message, true); }
  };
}
window.copyText = async (text) => {
  await navigator.clipboard.writeText(text).catch(() => {});
  toast("已复制到剪贴板");
};

// ---------------- 照片时间线 ----------------
async function viewPhotos() {
  $("view").innerHTML = "";
  blobs = await api("/api/blobs");
  renderPhotoGrid();
}
function filteredBlobs() {
  return searchQ ? blobs.filter((b) => b.file_name.toLowerCase().includes(searchQ)) : blobs;
}
// 按天分组（时间线细分到天；间隔 > 45 天插入月份隔断）
function groupByDay(list) {
  const out = []; // {type:sep|day, key, label, items?}
  let lastDate = null;
  for (const b of list) {
    const d = b.taken_at ? new Date(b.taken_at * 1000) : null;
    const key = d ? d.toDateString() : "unknown";
    const label = d ? d.toLocaleDateString() : "未知日期";
    // 稀疏隔断：与上一张相隔 > 45 天 → 插入年月隔断
    if (d && lastDate && (d - lastDate) / 86400000 > 45) {
      out.push({ type: "sep", label: `${d.getFullYear()} 年 ${d.getMonth() + 1} 月` });
    }
    let g = out.find((x) => x.type === "day" && x.key === key);
    if (!g) {
      g = { type: "day", key, label, date: d, items: [] };
      out.push(g);
    }
    g.items.push(b);
    lastDate = d || lastDate;
  }
  return out;
}
function renderPhotoGrid() {
  const list = filteredBlobs();
  const v = $("view");
  v.innerHTML = "";
  if (list.length === 0) { v.innerHTML = `<p class="hint">没有照片。点右上角「⬆ 上传」导入。</p>`; return; }
  const groups = groupByDay(list);
  currentGroups = groups;
  // 动态加载：先渲染 40 组，滚动到底加载更多
  const CHUNK = 40;
  let rendered = 0;
  const renderChunk = () => {
    const slice = groups.slice(rendered, rendered + CHUNK);
    for (const g of slice) {
      if (g.type === "sep") {
        const sep = document.createElement("div");
        sep.className = "day-header";
        sep.style.color = "var(--primary)";
        sep.textContent = `—— ${g.label} ——`;
        v.appendChild(sep);
        continue;
      }
      const header = document.createElement("div");
      header.className = "day-header";
      if (g.date && !document.querySelector(`[data-anchor="${g.date.getFullYear()}"]`)) header.setAttribute("data-anchor", g.date.getFullYear());
      // 当天全选/取消全选
      const allIn = g.items.every((b) => selected.has(b.blob_hash));
      header.innerHTML = `<label style="cursor:pointer;display:inline-flex;align-items:center;gap:6px">
          <input type="checkbox" data-day="${g.key}" ${allIn ? "checked" : ""}> ${g.label}</label>
        <span class="hint" style="margin-left:8px">${g.items.length} 张</span>`;
      header.querySelector("input").onchange = (e) => toggleDay(g, e.target.checked);
      v.appendChild(header);
      const grid = document.createElement("div");
      grid.className = "grid";
      for (const b of g.items) grid.appendChild(photoCell(b));
      v.appendChild(grid);
    }
    rendered += slice.length;
  };
  renderChunk();
  // 滚动哨兵（动态加载）
  const sentinel = document.createElement("div");
  sentinel.id = "tl-sentinel";
  sentinel.style.height = "1px";
  v.appendChild(sentinel);
  const io = new IntersectionObserver((entries) => {
    if (entries.some((e) => e.isIntersecting) && rendered < groups.length) {
      renderChunk();
      if (rendered < groups.length) v.appendChild(sentinel);
      else io.disconnect();
    }
  });
  io.observe(sentinel);
  // 年份锚点
  const years = [...new Set(list.filter((b) => b.taken_at).map((b) => new Date(b.taken_at * 1000).getFullYear()))].sort((a, b) => b - a);
  $("years").innerHTML = years.map((y) => `<div data-year="${y}">${y}</div>`).join("");
  document.querySelectorAll("#years div").forEach((el) => {
    el.onclick = () => document.querySelector(`[data-anchor="${el.dataset.year}"]`)?.scrollIntoView({ behavior: "smooth" });
  });
}
function toggleDay(group, checked) {
  for (const b of group.items) {
    checked ? selected.add(b.blob_hash) : selected.delete(b.blob_hash);
  }
  renderSelbar();
  // 更新该天勾选状态（不整页重渲染，保留滚动位置）
  renderPhotoGrid();
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
  cell.querySelector(".check").onclick = (e) => {
    e.stopPropagation();
    toggleSel(b.blob_hash);
    cell.classList.toggle("sel", selected.has(b.blob_hash));
  };
  cell.onclick = () => openViewer(b);
  cell.oncontextmenu = (e) => {
    e.preventDefault();
    ctxMenu(e.clientX, e.clientY, photoMenuItems(b));
  };
  return cell;
}
function photoMenuItems(b) {
  return [
    { label: "🔍 预览", fn: () => openViewer(b) },
    { label: b.favorite ? "♥ 取消收藏" : "♡ 收藏", fn: () => fav(b.blob_hash, !b.favorite) },
    { label: "✏️ 重命名", fn: () => renameDialog(b) },
    { label: "⬇ 下载原图", fn: () => downloadBlob(b.blob_hash, b.file_name) },
    { label: "🔗 分享", fn: () => shareDialog([b.blob_hash]) },
    { label: "ℹ 详情", fn: () => showBlobDetail(b) },
    { label: "↪ 移动到相册", fn: () => pickAlbumThen("move", [b.blob_hash]) },
    { label: "⧉ 复制到相册", fn: () => pickAlbumThen("copy", [b.blob_hash]) },
    { label: "🗑 移入回收站", fn: () => delBlob(b.blob_hash, b.file_name), danger: true },
  ];
}
function showBlobDetail(b) {
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
}
function renameDialog(b) {
  uiForm("重命名", [{ id: "rn-name", label: "新名称", value: b.file_name }], async (v) => {
    if (!v["rn-name"].trim()) { toast("名称不能为空", true); return; }
    await api(`/api/blobs/${b.blob_hash}/rename`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ new_name: v["rn-name"].trim() }) });
    closeModal();
    toast("已重命名");
    route();
  });
}
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

// ---------------- 灯箱（缩放以鼠标为中心 / 旋转以屏幕中心 / 拖拽平移 / 视频支持） ----------------
const viewer = $("viewer");
const lbImg = $("full-img");
let lbState = { zoom: 1, rotate: 0, flipH: 1, flipV: 1, ox: 0, oy: 0 };
let lbCurrent = null;
function lbTransform() {
  lbImg.style.transform = `translate(${lbState.ox}px, ${lbState.oy}px) rotate(${lbState.rotate}deg) scale(${lbState.zoom * lbState.flipH}, ${lbState.zoom * lbState.flipV})`;
}
function openViewer(b) {
  lbCurrent = b;
  lbState = { zoom: 1, rotate: 0, flipH: 1, flipV: 1, ox: 0, oy: 0 };
  lbTransform();
  $("lb-title").textContent = b.file_name;
  $("lb-fav").textContent = b.favorite ? "♥ 已收藏" : "♡ 收藏";
  // 操作按钮与多选条一致：收藏/重命名/分享/详情/下载/回收站
  $("lb-rename").style.display = "inline-block";
  $("lb-download").style.display = "inline-block";
  const url = `/api/blobs/${b.blob_hash}/raw?token=${encodeURIComponent(token)}`;
  if (b.mime.startsWith("video/")) {
    // 视频用 <video> 在线播放（解密字节流直接喂给 video 元素）
    lbImg.style.display = "none";
    let vid = $("lb-video");
    if (!vid) {
      vid = document.createElement("video");
      vid.id = "lb-video";
      vid.controls = true;
      vid.style.maxWidth = "96%";
      vid.style.maxHeight = "100%";
      $("lb-stage").appendChild(vid);
    }
    vid.style.display = "block";
    vid.src = url;
  } else {
    lbImg.style.display = "block";
    const vid = $("lb-video");
    if (vid) { vid.pause?.(); vid.style.display = "none"; }
    lbImg.src = url;
  }
  viewer.style.display = "flex";
}
function closeViewer() {
  viewer.style.display = "none";
  const vid = $("lb-video");
  if (vid) { vid.pause?.(); }
  lbImg.src = "";
  lbCurrent = null;
}
// 缩放以鼠标指针为中心：换算指针在图片坐标系里的位置，缩放后平移补偿
$("lb-stage").addEventListener("wheel", (e) => {
  e.preventDefault();
  if (!lbCurrent || lbCurrent.mime.startsWith("video/")) return;
  const rect = lbImg.getBoundingClientRect();
  const cx = e.clientX - (rect.left + rect.width / 2);
  const cy = e.clientY - (rect.top + rect.height / 2);
  const old = lbState.zoom;
  const zoom = Math.min(8, Math.max(0.2, e.deltaY < 0 ? old * 1.2 : old / 1.2));
  const ratio = zoom / old;
  lbState.ox = cx - (cx - lbState.ox) * ratio;
  lbState.oy = cy - (cy - lbState.oy) * ratio;
  lbState.zoom = zoom;
  if (lbState.zoom === 1) { lbState.ox = 0; lbState.oy = 0; }
  lbTransform();
}, { passive: false });
// 拖拽平移（旋转后的位移在屏幕坐标系近似处理）
let lbDrag = null;
$("lb-stage").addEventListener("mousedown", (e) => {
  if (e.target !== lbImg && e.target.id !== "lb-stage") return;
  lbDrag = { x: e.clientX, y: e.clientY, ox: lbState.ox, oy: lbState.oy };
  lbImg.style.cursor = "grabbing";
});
window.addEventListener("mousemove", (e) => {
  if (!lbDrag) return;
  lbState.ox = lbDrag.ox + (e.clientX - lbDrag.x);
  lbState.oy = lbDrag.oy + (e.clientY - lbDrag.y);
  lbTransform();
});
window.addEventListener("mouseup", () => { lbDrag = null; lbImg.style.cursor = "grab"; });
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
$("lb-share").onclick = () => { if (lbCurrent) shareDialog([lbCurrent.blob_hash]); };
$("lb-detail").onclick = () => { if (lbCurrent) showBlobDetail(lbCurrent); };
  $("lb-rename").onclick = () => { if (lbCurrent) renameDialog(lbCurrent); };
  $("lb-download").onclick = () => { if (lbCurrent) downloadBlob(lbCurrent.blob_hash, lbCurrent.file_name); };
  $("lb-move").onclick = () => { if (lbCurrent) pickAlbumThen("move", [lbCurrent.blob_hash]); };
  $("lb-copy").onclick = () => { if (lbCurrent) pickAlbumThen("copy", [lbCurrent.blob_hash]); };
// 重命名入口放详情弹窗里（灯箱工具条空间有限）
$("lb-zoom-in").onclick = () => { lbState.zoom = Math.min(8, lbState.zoom * 1.25); lbTransform(); };
$("lb-zoom-out").onclick = () => { lbState.zoom = Math.max(0.2, lbState.zoom / 1.25); lbTransform(); };
// 旋转/翻转以屏幕中心为原点：清空平移后仅变换
$("lb-rotate").onclick = () => { lbState.rotate = (lbState.rotate + 90) % 360; lbState.ox = 0; lbState.oy = 0; lbTransform(); };
$("lb-flip-h").onclick = () => { lbState.flipH *= -1; lbState.ox = 0; lbState.oy = 0; lbTransform(); };
$("lb-flip-v").onclick = () => { lbState.flipV *= -1; lbState.ox = 0; lbState.oy = 0; lbTransform(); };
$("lb-reset").onclick = () => { lbState = { zoom: 1, rotate: 0, flipH: 1, flipV: 1, ox: 0, oy: 0 }; lbTransform(); };
viewer.onclick = (e) => { if (e.target === $("lb-stage")) closeViewer(); };

// 批量重命名（模式中 {n} 为序号，自动保留扩展名）
function batchRenameDialog() {
  const hashes = [...selected];
  if (hashes.length === 0) { toast("先勾选照片", true); return; }
  uiForm("批量重命名", [
    { id: "rn-pattern", label: "命名模式（{n} = 序号）", value: "照片-{n}" },
  ], async (v) => {
    const pattern = v["rn-pattern"].trim();
    if (!pattern.includes("{n}")) { toast("模式中需包含 {n}", true); return; }
    let ok = 0;
    for (let i = 0; i < hashes.length; i++) {
      const b = blobs.find((x) => x.blob_hash === hashes[i]);
      if (!b) continue;
      const dot = b.file_name.lastIndexOf(".");
      const ext = dot >= 0 ? b.file_name.slice(dot) : "";
      const newName = pattern.replace("{n}", String(i + 1).padStart(3, "0")) + ext;
      try {
        await api(`/api/blobs/${b.blob_hash}/rename`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ new_name: newName }) });
        ok++;
      } catch {}
    }
    toast(`已重命名 ${ok}/${hashes.length}`);
    clearSelection();
    route();
  });
}
window.showBlobDetail = showBlobDetail;
window.openViewer = openViewer;

// ---------------- 探索（类别 + 地点） ----------------
async function viewExplore() {
  const all = await api("/api/blobs");
  blobs = all;
  const photos = all.filter((b) => b.mime.startsWith("image/"));
  const videos = all.filter((b) => b.mime.startsWith("video/"));
  const recent = [...all].sort((a, b) => (b.taken_at || 0) - (a.taken_at || 0)).slice(0, 12);
  let orphans = [];
  try { orphans = await api("/api/blobs/orphans"); } catch {}
  const places = new Map();
  for (const b of all) {
    if (b.gps_lat == null || b.gps_lon == null) continue;
    const key = `${b.gps_lat.toFixed(1)}, ${b.gps_lon.toFixed(1)}`;
    (places.get(key) || places.set(key, []).get(key)).push(b);
  }
  const placeList = [...places.entries()].sort((a, b) => b[1].length - a[1].length);
  const v = $("view");
  const card = (dataAttr, icon, label, cnt) => `
    <div class="album-card" ${dataAttr}>
      <div class="cov" style="display:grid;place-items:center;font-size:34px">${icon}</div>
      <div class="nm"><span>${label}</span><span class="cnt">${cnt}</span></div>
    </div>`;
  v.innerHTML = `
    <h2 style="margin:6px 0">探索</h2>
    <h3 class="day-header">类别</h3>
    <div class="album-wall" id="exp-kinds">
      ${card('data-kind="recent"', "🆕", "最近添加", recent.length)}
      ${card('data-kind="photo"', "🖼", "照片", photos.length)}
      ${card('data-kind="video"', "🎬", "视频", videos.length)}
      ${card('data-kind="motion"', "🎞", "动态照片", "即将支持")}
      ${card('data-kind="orphan"', "📦", "未入相册", orphans.length)}
    </div>
    <h3 class="day-header" style="margin-top:22px">地点（GPS 聚类）</h3>
    ${placeList.length === 0
      ? `<p class="hint">还没有带 GPS 位置的照片。导入的照片若含 EXIF GPS 会自动归入地点。</p>`
      : `<div class="album-wall" id="exp-places">
          ${placeList.map(([k, items]) => card(`data-place="${k}"`, "📍", k, items.length)).join("")}
        </div>`}`;
  v.querySelector('[data-kind="recent"]').onclick = () => openFiltered("🆕 最近添加", recent);
  v.querySelector('[data-kind="photo"]').onclick = () => openFiltered("🖼 照片", (b) => b.mime.startsWith("image/"));
  v.querySelector('[data-kind="video"]').onclick = () => openFiltered("🎬 视频", (b) => b.mime.startsWith("video/"));
  v.querySelector('[data-kind="motion"]').onclick = () => toast("动态照片检测将在后续版本上线（M5c）");
  v.querySelector('[data-kind="orphan"]').onclick = () => openFiltered("📦 未入相册", orphans);
  v.querySelectorAll("[data-place]").forEach((c) => {
    c.onclick = () => openFiltered(`📍 ${c.dataset.place}`, places.get(c.dataset.place) || []);
  });
}
// 过滤结果按时间线排列（按天分组 + 日期头）
function openFiltered(title, predOrItems) {
  const list = Array.isArray(predOrItems) ? predOrItems : blobs.filter(predOrItems);
  const v = $("view");
  v.innerHTML = `
    <div class="row"><h2 style="margin:6px 0">${title}</h2><span class="badge">${list.length} 张</span>
      <button style="margin-left:auto" id="back-explore">← 返回探索</button></div>
    <div id="filtered-body"></div>`;
  const body = $("filtered-body");
  const groups = groupByDay(list);
  currentGroups = groups;
  if (list.length === 0) { body.innerHTML = `<p class="hint">没有内容。</p>`; return; }
  for (const g of groups) {
    if (g.type === "sep") {
      body.insertAdjacentHTML("beforeend", `<div class="day-header" style="color:var(--primary)">—— ${g.label} ——</div>`);
      continue;
    }
    body.insertAdjacentHTML("beforeend", `<div class="day-header">${g.label} · ${g.items.length} 张</div>`);
    const grid = document.createElement("div");
    grid.className = "grid";
    for (const b of g.items) grid.appendChild(photoCell(b));
    body.appendChild(grid);
  }
  $("back-explore").onclick = () => { location.hash = "#/explore"; route(); };
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
  if (favs.length === 0) { v.innerHTML = `<p class="hint">还没有收藏。右键照片 → 收藏。</p>`; return; }
  for (const b of favs) grid.appendChild(photoCell(b));
}

// ---------------- 相册（右键/多选/删除） ----------------
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
  const list = albums.filter((a) => !searchQ || a.name.toLowerCase().includes(searchQ));
  v.innerHTML = `
    <div class="row"><h2 style="margin:6px 0">相册</h2><span style="flex:1"></span>
      <button class="primary" id="album-new">＋ 新建相册</button></div>
    <div class="album-wall" id="wall">
      ${list.map((a) => `
        <div class="album-card" data-id="${a.id}" style="position:relative">
          <span class="check" data-aid="${a.id}">✓</span>
          <div class="cov">${covers[a.id] ? `<img src="/api/blobs/${covers[a.id]}/thumbnail?token=${encodeURIComponent(token)}">` : `<div style="display:grid;place-items:center;height:100%;font-size:34px">📁</div>`}</div>
          <div class="nm"><span>${a.name}</span><span class="cnt">${a.blob_count} 张</span></div>
        </div>`).join("")}
    </div>
    <div class="selbar" id="album-selbar" style="display:none">
      <span style="font-weight:600">已选 <span id="asel-count">0</span> 个相册</span>
      <button id="asel-share">🔗 分享</button>
      <button class="danger" id="asel-delete">🗑 删除相册</button>
      <button id="asel-cancel">取消</button>
    </div>`;
  v.querySelectorAll(".album-card").forEach((el) => {
    const aid = el.dataset.id;
    const a = albums.find((x) => x.id === aid);
    el.onclick = (e) => {
      if (e.target.classList.contains("check")) return;
      location.hash = `#/album-${aid}`;
    };
    el.querySelector(".check").onclick = (e) => {
      e.stopPropagation();
      selectedAlbums.has(aid) ? selectedAlbums.delete(aid) : selectedAlbums.add(aid);
      el.classList.toggle("sel", selectedAlbums.has(aid));
      const bar = $("album-selbar");
      bar.style.display = selectedAlbums.size > 0 ? "flex" : "none";
      $("asel-count").textContent = selectedAlbums.size;
    };
    el.oncontextmenu = (e) => {
      e.preventDefault();
      ctxMenu(e.clientX, e.clientY, [
        { label: "📂 打开相册", fn: () => { location.hash = `#/album-${aid}`; } },
        { label: "🔗 分享相册", fn: () => shareAlbum(a) },
        { label: "🛂 权限", fn: () => openAclEditor(a) },
        { label: "🗑 删除相册", fn: () => deleteAlbumDialog(a), danger: true },
      ]);
    };
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
  $("asel-share").onclick = async () => {
    uiConfirm("批量分享", `为选中的 ${selectedAlbums.size} 个相册分别创建分享链接（逐个分享）？`, async () => {
      const urls = [];
      for (const aid of selectedAlbums) {
        try {
          const data = await api(`/api/albums/${aid}/share`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({}) });
          urls.push(location.origin + data.url);
        } catch {}
      }
      clearSelection();
      route();
      modal(`<h3>分享链接（${urls.length} 个）</h3><textarea style="width:100%;height:140px" readonly>${urls.join("\n")}</textarea>`);
    });
  };
  $("asel-delete").onclick = () => {
    uiConfirm("删除相册", `删除选中的 ${selectedAlbums.size} 个相册？\n\n相册一经删除便无法恢复；相册中的照片和视频仍会保留在「全部照片」中。`, async () => {
      for (const aid of selectedAlbums) {
        await api(`/api/albums/${aid}`, { method: "DELETE" }).catch(() => {});
      }
      clearSelection();
      toast("相册已删除（照片保留）");
      route();
    }, true);
  };
  $("asel-cancel").onclick = () => { clearSelection(); route(); };
}
function shareAlbum(a) {
  uiConfirm("分享相册", `为「${a.name}」创建分享链接？接收者无需登录即可查看（图片自动去元数据）。`, async () => {
    const data = await api(`/api/albums/${a.id}/share`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({}) });
    const url = location.origin + data.url;
    await navigator.clipboard.writeText(url).catch(() => {});
    toast("分享链接已创建并复制：\n" + url);
  });
}
window.deleteAlbumDialog = (a) => {
  uiConfirm("删除相册", `删除相册「${a.name}」？\n\n相册一经删除便无法恢复；相册中的照片和视频仍会保留在「全部照片」中。`, async () => {
    await api(`/api/albums/${a.id}`, { method: "DELETE" });
    toast("相册已删除（照片保留）");
    route();
  }, true);
};
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
      <button class="danger" id="del-album-btn">删除相册</button>
      <button id="back-albums">返回</button></div>
    <div id="album-grid" class="grid"></div>`;
  const grid = $("album-grid");
  for (const b of blobs) grid.appendChild(photoCell(b));
  $("back-albums").onclick = () => { location.hash = "#/albums"; };
  $("share-album-btn").onclick = () => shareAlbum(a);
  $("del-album-btn").onclick = () => window.deleteAlbumDialog(a);
  $("acl-btn").onclick = () => openAclEditor(a);
}
async function openAclEditor(a) {
  const acl = await api(`/api/albums/${a.id}/acl`).catch(() => []);
  const users = me.is_admin ? await api("/api/admin/users").catch(() => []) : [];
  const groups = me.is_admin ? await api("/api/admin/groups").catch(() => []) : [];
  const PERM_LABEL = { none: "隐藏", view: "只看", upload: "上传", manage: "管理" };
  const isRoot = !a.parent_id;
  const rowFor = (kind, id, label, cur) => `
    <tr><td>${kind === "user" ? "👤" : "🏷"} ${label}</td><td>${cur ? PERM_LABEL[cur] || cur : "继承"}</td><td>
      <select onchange="setAcl('${a.id}','${kind}','${id}',this.value)">
        <option value="">继承</option>
        ${["none", "view", "upload", "manage"].map((p) => `<option value="${p}" ${cur === p ? "selected" : ""}>${PERM_LABEL[p]}</option>`).join("")}
      </select></td></tr>`;
  const userRows = users.map((u) => {
    const hit = acl.find((e) => e.principal.kind === "user" && e.principal.id === u.uid);
    return rowFor("user", u.uid, u.name, hit ? hit.perm : "");
  }).join("");
  const groupRows = groups.map((g) => {
    const hit = acl.find((e) => e.principal.kind === "group" && e.principal.id === g.gid);
    return rowFor("group", g.gid, g.name, hit ? hit.perm : "");
  }).join("");
  modal(`
    <h3>权限：${a.name}</h3>
    ${isRoot ? `<p class="hint">根相册没有父级可继承——未显式设置权限的用户/组将看不到此相册（管理员始终可见）。创建根相册时已默认授予 family 组「管理」。</p>` : `<p class="hint">子相册默认继承上级权限；显式设置覆盖继承；同一层级用户设置优先于组。</p>`}
    <div class="row" style="margin:8px 0">
      <button class="primary" id="acl-tab-user">👤 用户</button>
      <button id="acl-tab-group">🏷 用户组</button>
    </div>
    <table class="admin" id="acl-users"><tr><th>对象</th><th>当前</th><th>设置</th></tr>${userRows}</table>
    <table class="admin" id="acl-groups" style="display:none"><tr><th>对象</th><th>当前</th><th>设置</th></tr>${groupRows}</table>`);
  $("acl-tab-user").onclick = () => {
    $("acl-users").style.display = "table";
    $("acl-groups").style.display = "none";
    $("acl-tab-user").classList.add("primary");
    $("acl-tab-group").classList.remove("primary");
  };
  $("acl-tab-group").onclick = () => {
    $("acl-users").style.display = "none";
    $("acl-groups").style.display = "table";
    $("acl-tab-group").classList.add("primary");
    $("acl-tab-user").classList.remove("primary");
  };
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

// ---------------- 分享列表（分页 + 页内全选 + 勾选后显示操作 + 清单可预览） ----------------
let sharePage = 0;
const SHARE_PAGE_SIZE = 10;
async function viewShares() {
  const shares = await api("/api/shares");
  const pages = Math.max(1, Math.ceil(shares.length / SHARE_PAGE_SIZE));
  if (sharePage >= pages) sharePage = pages - 1;
  const slice = shares.slice(sharePage * SHARE_PAGE_SIZE, (sharePage + 1) * SHARE_PAGE_SIZE);
  $("view").innerHTML = `
    <div class="row"><h2 style="margin:6px 0">分享</h2><span style="flex:1"></span>
      <button id="sh-copy" style="display:none">复制选中链接</button>
      <button class="danger" id="sh-del" style="display:none">取消分享选中</button></div>
    ${shares.length === 0 ? `<p class="hint">还没有分享。在照片（勾选圆圈多选）或相册页创建。</p>` : `
    <table class="admin"><tr>
        <th><input type="checkbox" id="sh-all" title="全选/取消全选本页"></th>
        <th>名称</th><th>类型</th><th>数量</th><th>分享码</th><th>过期</th><th>内容</th><th></th></tr>
      ${slice.map((s) => `
        <tr><td><input type="checkbox" class="sh-check" data-sid="${s.id}"></td>
        <td>${s.album_name || s.id}</td>
        <td>${s.kind === "album" ? "相册" : "照片"}</td>
        <td>${(s.blob_hashes || []).length}</td>
        <td>${s.access_code_hash ? "有" : "—"}</td>
        <td>${s.expires_at ? new Date(s.expires_at * 1000).toLocaleString() : "永久"}</td>
        <td><button onclick="showShareList('${s.id}')">查看内容</button></td>
        <td class="row">
          <button onclick="copyShare('${s.id}')">复制链接</button>
          <button class="danger" onclick="cancelShare('${s.id}')">取消分享</button>
        </td></tr>`).join("")}
    </table>
    ${pages > 1 ? `<div class="row" style="justify-content:center;margin-top:12px">
      <button ${sharePage === 0 ? "disabled" : ""} id="sh-prev">‹ 上一页</button>
      <span class="hint" style="align-self:center">${sharePage + 1} / ${pages}</span>
      <button ${sharePage === pages - 1 ? "disabled" : ""} id="sh-next">下一页 ›</button>
    </div>` : ""}`}`;
  const updateBar = () => {
    const n = document.querySelectorAll(".sh-check:checked").length;
    $("sh-copy").style.display = n > 0 ? "inline-block" : "none";
    $("sh-del").style.display = n > 0 ? "inline-block" : "none";
  };
  document.querySelectorAll(".sh-check").forEach((c) => { c.onchange = updateBar; });
  const all = $("sh-all");
  if (all) {
    all.onchange = (e) => {
      document.querySelectorAll(".sh-check").forEach((c) => {
        c.checked = e.target.checked;
      });
      updateBar();
    };
  }
  $("sh-del").onclick = () => {
    const ids = [...document.querySelectorAll(".sh-check:checked")].map((c) => c.dataset.sid);
    uiConfirm("取消分享", `取消选中的 ${ids.length} 个分享？链接立即失效。`, async () => {
      for (const id of ids) await api(`/api/shares/${id}`, { method: "DELETE" }).catch(() => {});
      route();
    }, true);
  };
  $("sh-copy").onclick = () => {
    const ids = [...document.querySelectorAll(".sh-check:checked")].map((c) => c.dataset.sid);
    copyText(ids.map((id) => location.origin + `/s/${id}`).join("\n"));
  };
  const prev = $("sh-prev");
  if (prev) prev.onclick = () => { sharePage--; route(); };
  const next = $("sh-next");
  if (next) next.onclick = () => { sharePage++; route(); };
}
window.copyShare = async (id) => {
  await navigator.clipboard.writeText(location.origin + `/s/${id}`).catch(() => {});
  toast("链接已复制");
};
window.cancelShare = (id) => {
  uiConfirm("取消分享", "取消后链接立即失效，确定？", async () => {
    await api(`/api/shares/${id}`, { method: "DELETE" });
    route();
  }, true);
};
window.showShareList = async (id) => {
  try {
    const meta = await fetch(`/s/${id}/meta`).then((r) => r.json());
    modal(`<h3>分享内容：${meta.album}</h3>
      <table class="admin">${(meta.files || []).map((f) => `
        <tr><td>${f.name}</td>
        <td>${new Date((f.taken_at || 0) * 1000).toLocaleDateString()}</td>
        <td style="white-space:nowrap"><span class="badge">${f.sanitized ? "已去元数据" : "含元数据"}</span></td>
        <td>${f.mime.startsWith("image/") || f.mime.startsWith("video/") ? `<button onclick='previewShared(${JSON.stringify(JSON.stringify(f))})'>预览</button>` : ""}</td></tr>`).join("")}</table>
      <div class="row" style="justify-content:flex-end"><button onclick="closeModal()">关闭</button></div>`);
  } catch (e) { toast(e.message || "加载失败", true); }
};
// 分享清单预览（所有者侧走 /api/blobs/{hash}/raw）
window.previewShared = (fJson) => {
  const f = typeof fJson === "string" ? JSON.parse(fJson) : fJson;
  closeModal(); // 避免弹窗压在预览层上
  const b = blobs.find((x) => x.blob_hash === f.hash);
  if (b) { openViewer(b); return; }
  openViewer({ blob_hash: f.hash, mime: f.mime, file_name: f.name, favorite: false, total_len: 0, taken_at: f.taken_at || 0, gps_lat: null, gps_lon: null });
};

// ---------------- 回收站（可预览） ----------------
async function viewTrash() {
  const list = await api("/api/trash").catch(() => []);
  $("view").innerHTML = `
    <h2 style="margin:6px 0">回收站</h2>
    ${list.length === 0 ? `<p class="hint">回收站是空的。</p>` : `
    <table class="admin"><tr><th></th><th>名称</th><th>大小</th><th>拍摄时间</th><th></th></tr>
      ${list.map((b) => `
        <tr><td>${b.mime.startsWith("image/") ? `<img src="/api/blobs/${b.blob_hash}/thumbnail?token=${encodeURIComponent(token)}" style="width:44px;height:44px;object-fit:cover;border-radius:6px" onclick="openViewer({blob_hash:'${b.blob_hash}',mime:'${b.mime}',file_name:'${b.file_name}',favorite:false,total_len:${b.total_len},taken_at:${b.taken_at || 0},gps_lat:null,gps_lon:null})" style="cursor:pointer">` : "🎬"}</td>
        <td>${b.file_name}</td><td>${(b.total_len / 1048576).toFixed(1)} MB</td>
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

// ---------------- 上传（时间线/相册页都可用；可选相册） ----------------
$("upload-btn").onclick = () => {
  api("/api/albums").then((list) => {
    modal(`
      <h3>上传照片 / 视频</h3>
      <label class="hint">上传到相册（可选「不入相册」）</label>
      <select id="up-album" style="width:100%;margin:6px 0">
        <option value="">不入相册</option>
        ${list.map((a) => `<option value="${a.id}" ${a.perm === "none" ? "disabled" : ""}>📁 ${a.name}</option>`).join("")}
      </select>
      <div class="row" style="justify-content:flex-end;margin-top:10px">
        <button id="up-pick" class="primary">选择文件</button>
      </div>`);
    $("up-pick").onclick = () => $("file-input").click();
  });
};
$("file-input").onchange = async () => {
  const files = [...$("file-input").files];
  if (files.length === 0) return;
  const albumId = $("up-album") ? $("up-album").value : "";
  closeModal();
  let ok = 0, fail = 0;
  for (const f of files) {
    const fd = new FormData();
    fd.append("file", f, f.name);
    if (albumId) fd.append("album_id", albumId);
    try {
      const res = await api("/api/blobs/upload", { method: "POST", body: fd });
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
    <div class="album-card">
      <div class="cov" style="background:var(--primary);display:grid;place-items:center;position:relative">
        ${u.avatar ? `<img src="${u.avatar}" style="width:84px;height:84px;border-radius:50%;object-fit:cover">` : `<div class="avatar" style="width:84px;height:84px;font-size:32px;background:rgba(255,255,255,.25)">${u.name.slice(0, 1)}</div>`}
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
// 添加/编辑用户共用弹窗（创建时可设权限；头像上传）
async function userDialog(existing) {
  const albums = await api("/api/albums");
  const PERM_LABEL = { none: "隐藏", view: "只看", upload: "上传", manage: "管理" };
  const uid = existing ? existing.uid : null;
  let avatarData = existing ? existing.avatar || "" : "";
  const aclOf = {};
  if (uid) {
    for (const a of albums) {
      const entries = await api(`/api/albums/${a.id}/acl`).catch(() => []);
      const hit = entries.find((e) => e.principal.kind === "user" && e.principal.id === uid);
      aclOf[a.id] = hit ? hit.perm : "";
    }
  }
  modal(`
    <h3>${existing ? "编辑用户：" + existing.name : "添加用户"}</h3>
    <div style="text-align:center">
      <div class="avatar" id="ud-avatar" style="width:72px;height:72px;font-size:26px;margin:4px auto;cursor:pointer" title="点击上传头像" onclick="document.getElementById('ud-avatar-file').click()">
        ${avatarData ? "" : (existing ? existing.name.slice(0, 1) : "?")}
      </div>
      <input type="file" id="ud-avatar-file" accept="image/*" style="display:none">
    </div>
    <label class="hint">显示名</label>
    <input id="ud-name" value="${existing ? existing.name : ""}" style="width:100%;margin:4px 0 10px">
    ${existing ? `<label class="hint">新口令（留空不改）</label><input id="ud-pass" type="password" style="width:100%;margin:4px 0 10px">` : `<label class="hint">口令</label><input id="ud-pass" type="password" style="width:100%;margin:4px 0 10px">`}
    <div class="row" style="margin:8px 0">
      ${existing ? `<label><input type="checkbox" id="ud-admin" ${existing.is_admin ? "checked" : ""}> 管理员</label>
      <label style="margin-left:16px"><input type="checkbox" id="ud-enabled" ${existing.enabled === false ? "" : "checked"}> 启用</label>` : ""}
    </div>
    ${albums.length > 0 ? `<label class="hint">相册权限（默认 = 继承）</label>
    <table class="admin">${albums.map((a) => `
      <tr><td>📁 ${a.name}</td><td>
        <select id="ud-acl-${a.id}">
          <option value="">继承</option>
          ${["none", "view", "upload", "manage"].map((p) => `<option value="${p}" ${aclOf[a.id] === p ? "selected" : ""}>${PERM_LABEL[p]}</option>`).join("")}
        </select></td></tr>`).join("")}
    </table>` : ""}
    <div class="row" style="justify-content:flex-end;margin-top:14px">
      <button id="ud-cancel">取消</button>
      <button class="primary" id="ud-ok">保存</button>
    </div>`);
  const avatarPreview = $("ud-avatar");
  if (avatarData) avatarPreview.style.background = `center/cover url(${avatarData})`;
  $("ud-avatar-file").onchange = () => {
    const file = $("ud-avatar-file").files[0];
    if (!file) return;
    const img = new Image();
    img.onload = () => {
      const c = document.createElement("canvas");
      const side = Math.min(img.width, img.height);
      c.width = c.height = 256;
      c.getContext("2d").drawImage(img, (img.width - side) / 2, (img.height - side) / 2, side, side, 0, 0, 256, 256);
      avatarData = c.toDataURL("image/jpeg", 0.85);
      avatarPreview.textContent = "";
      avatarPreview.style.background = `center/cover url(${avatarData})`;
    };
    img.src = URL.createObjectURL(file);
  };
  $("ud-cancel").onclick = closeModal;
  $("ud-ok").onclick = async () => {
    try {
      let newUid = uid;
      if (existing) {
        await api(`/api/admin/users/${uid}`, {
          method: "PUT",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            name: $("ud-name").value,
            password: $("ud-pass").value || null,
            admin: $("ud-admin").checked,
            enabled: $("ud-enabled").checked,
            avatar: avatarData || "",
          }),
        });
      } else {
        const created = await api("/api/admin/users", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ name: $("ud-name").value, password: $("ud-pass").value, admin: false }),
        });
        newUid = created.uid;
        if (avatarData) {
          await api(`/api/admin/users/${newUid}`, { method: "PUT", headers: { "content-type": "application/json" }, body: JSON.stringify({ avatar: avatarData }) });
        }
      }
      for (const a of albums) {
        const sel = $(`ud-acl-${a.id}`);
        if (!sel) continue;
        await api(`/api/albums/${a.id}/acl`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ principal: { type: "user", id: newUid }, perm: sel.value || null }),
        });
      }
      closeModal();
      toast("已保存");
      route();
    } catch (e) { toast(e.message, true); }
  };
}
window.addUserDialog = () => userDialog(null);
window.editUser = (uid) => api("/api/admin/users").then((users) => {
  const u = users.find((x) => x.uid === uid);
  if (u) userDialog(u);
});
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

// ---------------- 管理：用户组（编辑弹窗含成员+相册权限+头像） ----------------
async function viewAdminGroups() {
  const groups = await api("/api/admin/groups");
  $("view").innerHTML = `
    <h2 style="margin:6px 0">用户组</h2>
    <p class="hint">family 组为全员组（自动维护）；自定义组用于批量授权。编辑可添加/删除成员并设置组对各相册的权限。</p>
    <div class="album-wall" id="group-wall">
      ${groups.map((g) => `
        <div class="album-card">
          <div class="cov" style="background:var(--success);display:grid;place-items:center;position:relative">
            ${g.avatar ? `<img src="${g.avatar}" style="width:84px;height:84px;border-radius:50%;object-fit:cover">` : `<div class="avatar" style="width:84px;height:84px;font-size:32px;background:rgba(255,255,255,.25)">${g.name.slice(0, 1)}</div>`}
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
// 组的运行时头像（avatar 字段存 groups.json）
window.addGroupDialog = () => {
  uiForm("创建用户组", [{ id: "ng-name", label: "组名" }], async (v) => {
    if (!v["ng-name"].trim()) { toast("组名不能为空", true); return; }
    const cur = await api("/api/admin/groups");
    cur.push({ gid: `g-${v["ng-name"].trim().toLowerCase()}`, name: v["ng-name"].trim(), members: [], avatar: "" });
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
  const PERM_LABEL = { none: "隐藏", view: "只看", upload: "上传", manage: "管理" };
  const aclOf = {};
  for (const a of albums) {
    const entries = await api(`/api/albums/${a.id}/acl`).catch(() => []);
    const hit = entries.find((e) => e.principal.kind === "group" && e.principal.id === gid);
    aclOf[a.id] = hit ? hit.perm : "";
  }
  let avatarData = g.avatar || "";
  modal(`
    <h3>编辑组：${g.name}</h3>
    <div style="text-align:center">
      <div class="avatar" id="eg-avatar" style="width:72px;height:72px;font-size:26px;margin:4px auto 10px;cursor:pointer;background:var(--success)" title="点击上传组头像" onclick="document.getElementById('eg-avatar-file').click()">
        ${avatarData ? "" : g.name.slice(0, 1)}
      </div>
      <input type="file" id="eg-avatar-file" accept="image/*" style="display:none">
    </div>
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
  const avatarPreview = $("eg-avatar");
  if (avatarData) avatarPreview.style.background = `center/cover url(${avatarData})`;
  $("eg-avatar-file").onchange = () => {
    const file = $("eg-avatar-file").files[0];
    if (!file) return;
    const img = new Image();
    img.onload = () => {
      const c = document.createElement("canvas");
      const side = Math.min(img.width, img.height);
      c.width = c.height = 256;
      c.getContext("2d").drawImage(img, (img.width - side) / 2, (img.height - side) / 2, side, side, 0, 0, 256, 256);
      avatarData = c.toDataURL("image/jpeg", 0.85);
      avatarPreview.textContent = "";
      avatarPreview.style.background = `center/cover url(${avatarData})`;
    };
    img.src = URL.createObjectURL(file);
  };
  $("eg-cancel").onclick = closeModal;
  $("eg-ok").onclick = async () => {
    try {
      const members = [...document.querySelectorAll(".eg-member:checked")].map((c) => c.dataset.uid);
      const idx = groups.findIndex((x) => x.gid === gid);
      groups[idx].members = members;
      groups[idx].avatar = avatarData;
      await api("/api/admin/groups", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ groups }) });
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
