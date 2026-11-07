// 牧心相册 PWA（M5a）：hash 路由多视图 + 侧边栏 + 管理界面 + 多选分享 + 年份导航。
const $ = (id) => document.getElementById(id);
const TOKEN_KEY = "hs_token";
let token = localStorage.getItem(TOKEN_KEY) || "";
let me = null;
let albums = [];
let blobs = [];
let selectMode = false;
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

function modal(html) {
  const root = $("modal-root");
  root.innerHTML = `<div class="modal-backdrop"><div class="modal"><button class="close" onclick="document.getElementById('modal-root').innerHTML=''">✕</button>${html}</div></div>`;
}
function closeModal() { $("modal-root").innerHTML = ""; }

// ---------------- 登录 ----------------
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
$("logout-btn") && null;
$("avatar").onclick = async () => {
  if (!confirm("退出登录？")) return;
  fetch("/api/logout", { method: "POST", headers: { authorization: `Bearer ${token}` } }).catch(() => {});
  showLogin();
};
$("theme-toggle").onclick = () => {
  document.body.classList.toggle("theme-light");
  localStorage.setItem("hs_theme", document.body.classList.contains("theme-light") ? "light" : "dark");
};
if (localStorage.getItem("hs_theme") === "light") document.body.classList.add("theme-light");

if (token) {
  api("/api/me").then((u) => { me = u; showMain(); }).catch(() => showLogin());
} else showLogin();

// ---------------- 路由 ----------------
const VIEWS = ["photos", "albums", "shares", "trash", "admin-users", "admin-groups", "admin-sources", "admin-status"];
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
  $("select-btn").style.display = view === "photos" || view.startsWith("album-") ? "inline-block" : "none";
  if (!(view === "photos" || view.startsWith("album-"))) exitSelect();
  currentAlbum = view.startsWith("album-") ? view.replace("album-", "") : "";
  const map = {
    photos: viewPhotos,
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
// 多选模式开关（照片/相册详情视图）
$("select-btn").onclick = () => {
  if (selectMode) { exitSelect(); renderPhotoViewContent(); return; }
  selectMode = true;
  selected = new Set();
  $("select-btn").textContent = "✕ 退出选择";
  renderSelbar();
  rerenderCurrentGrid();
};
function rerenderCurrentGrid() {
  const view = (location.hash || "#/photos").replace(/^#\//, "");
  if (view === "photos") renderPhotoGrid();
  else if (view.startsWith("album-")) renderAlbumDetail();
}
function renderPhotoViewContent() {
  $("select-btn").textContent = "☑ 选择";
  rerenderCurrentGrid();
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
  // 按日分组
  const groups = new Map();
  for (const b of list) {
    const d = b.taken_at ? new Date(b.taken_at * 1000).toLocaleDateString() : "未知日期";
    (groups.get(d) || groups.set(d, []).get(d)).push(b);
  }
  // 年份锚点（右侧年份栏：点击滚动到该年首个分组）
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
    for (const b of items) {
      grid.appendChild(photoCell(b));
    }
    v.appendChild(grid);
  }
}
function photoCell(b) {
  const cell = document.createElement("div");
  cell.className = "cell" + (selectMode && selected.has(b.blob_hash) ? " sel" : "");
  cell.innerHTML = `<span class="check">✓</span><span class="name">${b.favorite ? "♥ " : ""}${b.file_name}</span>`;
  if (b.mime.startsWith("image/")) {
    const img = new Image();
    img.loading = "lazy";
    img.src = `/api/blobs/${b.blob_hash}/thumbnail?token=${encodeURIComponent(token)}`;
    img.onerror = () => (cell.style.visibility = "hidden");
    cell.prepend(img);
  } else {
    cell.insertAdjacentHTML("afterbegin", `<div style="position:absolute;inset:0;display:grid;place-items:center;font-size:32px">🎬</div>`);
  }
  cell.onclick = () => {
    if (selectMode) { toggleSel(b.blob_hash); cell.classList.toggle("sel", selected.has(b.blob_hash)); }
    else openViewer(b);
  };
  cell.oncontextmenu = (e) => { e.preventDefault(); openPhotoMenu(b); };
  return cell;
}
const viewer = $("viewer");
function openViewer(b) {
  if (!b.mime.startsWith("image/")) { toast("视频请用桌面/安卓客户端在线播放", true); return; }
  $("full-img").src = `/api/blobs/${b.blob_hash}/raw?token=${encodeURIComponent(token)}`;
  viewer.style.display = "block";
}
viewer.onclick = (e) => { if (e.target !== $("full-img")) viewer.style.display = "none"; };

// 照片右键菜单
function openPhotoMenu(b) {
  modal(`
    <h3>${b.file_name}</h3>
    <div style="display:flex;flex-direction:column;gap:8px">
      <button onclick="closeModal();openViewerById('${b.blob_hash}','${b.mime}')">🔍 预览</button>
      <button onclick="closeModal();fav('${b.blob_hash}',${!b.favorite})">${b.favorite ? "♥ 取消收藏" : "♡ 收藏"}</button>
      <button onclick="closeModal();downloadBlob('${b.blob_hash}','${b.file_name}')">⬇ 下载原图</button>
      <button class="danger" onclick="closeModal();delBlob('${b.blob_hash}','${b.file_name}')">🗑 移入回收站</button>
    </div>`);
}
window.openViewerById = (hash, mime) => openViewer({ blob_hash: hash, mime });
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
window.delBlob = async (hash, name) => {
  if (!confirm(`把「${name}」移入回收站？`)) return;
  await api(`/api/blobs/${hash}/delete`, { method: "POST" });
  toast("已移入回收站");
  route();
};

// ---------------- 多选 + 分享 ----------------
function exitSelect() { selectMode = false; selected = new Set(); const sb = document.querySelector(".selbar"); if (sb) sb.remove(); }
function toggleSel(hash) {
  selected.has(hash) ? selected.delete(hash) : selected.add(hash);
  renderSelbar();
}
function renderSelbar() {
  let bar = document.querySelector(".selbar");
  if (!selectMode) { if (bar) bar.remove(); return; }
  if (!bar) {
    bar = document.createElement("div");
    bar.className = "selbar";
    bar.innerHTML = `
      <span style="font-weight:600">已选 <span id="sel-count">0</span> 张</span>
      <button id="sel-share">🔗 分享</button>
      <button id="sel-cancel">取消</button>`;
    document.body.appendChild(bar);
    $("sel-share").onclick = shareSelected;
    $("sel-cancel").onclick = () => { exitSelect(); route(); };
  }
  $("sel-count").textContent = selected.size;
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
      exitSelect();
      route();
      toast("分享链接已创建并复制：\n" + url);
    } catch (e) { toast(e.message, true); }
  };
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
  $("album-new").onclick = async () => {
    const name = prompt("相册名称：");
    if (!name) return;
    try { await api("/api/albums", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ name }) }); route(); }
    catch (e) { toast(e.message, true); }
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
  $("share-album-btn").onclick = async () => {
    if (!confirm(`为「${a.name}」创建分享链接？`)) return;
    try {
      const data = await api(`/api/albums/${currentAlbum}/share`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({}) });
      const url = location.origin + data.url;
      await navigator.clipboard.writeText(url).catch(() => {});
      toast("分享链接已创建并复制：\n" + url);
    } catch (e) { toast(e.message, true); }
  };
  $("acl-btn").onclick = () => openAclEditor(a);
}
async function openAclEditor(a) {
  const acl = await api(`/api/albums/${a.id}/acl`).catch(() => []);
  const users = me.is_admin ? await api("/api/admin/users").catch(() => []) : [];
  const groups = me.is_admin ? await api("/api/admin/groups").catch(() => []) : [];
  const rowFor = (kind, id, label, cur) => `
    <tr><td>${kind === "user" ? "👤" : "🏷"} ${label}</td><td>${cur}</td><td>
      <select onchange="setAcl('${a.id}','${kind}','${id}',this.value)">
        <option value="">继承</option>
        ${["none", "view", "upload", "manage"].map((p) => `<option value="${p}" ${cur === p ? "selected" : ""}>${{ none: "隐藏", view: "只看", upload: "投稿", manage: "管理" }[p]}</option>`).join("")}
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
    ${shares.length === 0 ? `<p class="hint">还没有分享。在照片（多选）或相册页创建。</p>` : `
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
window.delShare = async (id) => {
  if (!confirm("删除该分享？链接立即失效。")) return;
  await api(`/api/shares/${id}`, { method: "DELETE" });
  route();
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
    </table>`;
}
window.restoreBlob = async (h) => { await api(`/api/blobs/${h}/restore`, { method: "POST" }); toast("已恢复"); route(); };
window.purgeBlob = async (h, n) => {
  if (!confirm(`彻底删除「${n}」？碎片将从存储清除，无法恢复。`)) return;
  await api(`/api/blobs/${h}/purge`, { method: "POST" });
  route();
};

// ---------------- 上传 ----------------
$("upload-btn").onclick = () => {
  if (!currentAlbum) { toast("先进入一个相册再上传（顶部下拉或相册页）", true); return; }
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

// ---------------- 管理：用户 ----------------
async function viewAdminUsers() {
  const users = await api("/api/admin/users");
  $("view").innerHTML = `
    <h2 style="margin:6px 0">用户管理</h2>
    <div class="form-card">
      <input id="nu-name" placeholder="用户名">
      <input id="nu-pass" placeholder="口令" type="password">
      <label><input type="checkbox" id="nu-admin"> 管理员</label>
      <button class="primary" id="nu-add">添加用户</button>
    </div>
    <table class="admin"><tr><th>用户名</th><th>ID</th><th>角色</th><th></th></tr>
      ${users.map((u) => `
        <tr><td>${u.name}</td><td><span class="badge">${u.uid}</span></td>
        <td>${u.is_admin ? "管理员" : "成员"}</td>
        <td class="row">
          <button onclick="resetPw('${u.uid}','${u.name}')">改口令</button>
          <button onclick="toggleAdmin('${u.uid}',${!u.is_admin})">${u.is_admin ? "降为成员" : "升为管理员"}</button>
          <button class="danger" onclick="delUser('${u.uid}','${u.name}')">删除</button>
        </td></tr>`).join("")}
    </table>`;
  $("nu-add").onclick = async () => {
    try {
      await api("/api/admin/users", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ name: $("nu-name").value, password: $("nu-pass").value, admin: $("nu-admin").checked }),
      });
      toast("用户已创建");
      route();
    } catch (e) { toast(e.message, true); }
  };
}
window.resetPw = async (uid, name) => {
  const pw = prompt(`为「${name}」设置新口令：`);
  if (!pw) return;
  try { await api(`/api/admin/users/${uid}`, { method: "PUT", headers: { "content-type": "application/json" }, body: JSON.stringify({ password: pw }) }); toast("口令已更新"); }
  catch (e) { toast(e.message, true); }
};
window.toggleAdmin = async (uid, admin) => {
  try { await api(`/api/admin/users/${uid}`, { method: "PUT", headers: { "content-type": "application/json" }, body: JSON.stringify({ admin }) }); route(); }
  catch (e) { toast(e.message, true); }
};
window.delUser = async (uid, name) => {
  if (!confirm(`删除用户「${name}」？其会话将立即失效。`)) return;
  try { await api(`/api/admin/users/${uid}`, { method: "DELETE" }); route(); }
  catch (e) { toast(e.message, true); }
};

// ---------------- 管理：用户组 ----------------
async function viewAdminGroups() {
  const groups = await api("/api/admin/groups");
  const users = await api("/api/admin/users");
  const nameOf = (uid) => users.find((u) => u.uid === uid)?.name || uid;
  $("view").innerHTML = `
    <h2 style="margin:6px 0">用户组</h2>
    <p class="hint">family 组为全员组（自动维护），不能删除；自定义组用于批量授权。</p>
    <div class="form-card">
      <input id="ng-name" placeholder="组名">
      <button class="primary" id="ng-add">创建组</button>
    </div>
    ${groups.map((g) => `
      <div class="form-card" data-gid="${g.gid}">
        <strong>${g.name}</strong>
        <span class="hint">${g.members.length} 名成员：${g.members.map(nameOf).join("、") || "无"}</span>
        <select id="gm-${g.gid}">${users.map((u) => `<option value="${u.uid}">${u.name}</option>`).join("")}</select>
        <button onclick="addMember('${g.gid}')">添加成员</button>
        ${g.gid !== "family" ? `<button class="danger" onclick="delGroup('${g.gid}','${g.name}')">删除组</button>` : ""}
      </div>`).join("")}`;
  $("ng-add").onclick = async () => {
    const name = $("ng-name").value.trim();
    if (!name) return;
    const cur = await api("/api/admin/groups");
    cur.push({ gid: `g-${name.toLowerCase()}`, name, members: [] });
    await api("/api/admin/groups", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ groups: cur }) });
    route();
  };
}
window.addMember = async (gid) => {
  const uid = $(`gm-${gid}`).value;
  const cur = await api("/api/admin/groups");
  const g = cur.find((x) => x.gid === gid);
  if (g && !g.members.includes(uid)) g.members.push(uid);
  await api("/api/admin/groups", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ groups: cur }) });
  route();
};
window.delGroup = async (gid, name) => {
  if (!confirm(`删除组「${name}」？`)) return;
  const cur = (await api("/api/admin/groups")).filter((g) => g.gid !== gid);
  await api("/api/admin/groups", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ groups: cur }) });
  route();
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
