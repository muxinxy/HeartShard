// 牧心相册 PWA 前端（M4）：登录 / 相册切换 / 网格 / 灯箱 / 上传 / 分享链接。
const $ = (id) => document.getElementById(id);
const TOKEN_KEY = "hs_token";

let token = localStorage.getItem(TOKEN_KEY) || "";
let albums = [];
let currentAlbum = ""; // ""=全部

function toast(msg, isErr = false) {
  const t = $("toast");
  t.textContent = msg;
  t.style.borderLeftColor = isErr ? "#ef6a6a" : "#7ed6a0";
  t.style.display = "block";
  setTimeout(() => (t.style.display = "none"), 3500);
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
  return res;
}

function showLogin() { $("login").style.display = "block"; $("main").style.display = "none"; }
function showMain() { $("login").style.display = "none"; $("main").style.display = "block"; refresh(); }

// ---------- 登录 ----------
$("login-btn").onclick = async () => {
  try {
    const res = await fetch("/api/login", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name: $("user").value, password: $("pass").value }),
    });
    if (!res.ok) { toast("用户名或口令错误", true); return; }
    const data = await res.json();
    token = data.token;
    localStorage.setItem(TOKEN_KEY, token);
    $("pass").value = "";
    showMain();
  } catch (e) { toast(e.message, true); }
};
$("pass").addEventListener("keydown", (e) => e.key === "Enter" && $("login-btn").click());
if (token) { api("/api/me").then(showMain).catch(() => {}); } else showLogin();

$("logout-btn").onclick = () => {
  fetch("/api/logout", { method: "POST", headers: { authorization: `Bearer ${token}` } }).catch(() => {});
  localStorage.removeItem(TOKEN_KEY);
  token = "";
  showLogin();
};

// ---------- 相册 ----------
async function loadAlbums() {
  const res = await api("/api/albums");
  albums = await res.json();
  const sel = $("album-sel");
  sel.innerHTML = `<option value="">全部照片</option>` +
    albums.map((a) => `<option value="${a.id}">📁 ${a.name}（${a.blob_count}）</option>`).join("");
  sel.value = currentAlbum;
}
$("album-sel").onchange = () => { currentAlbum = $("album-sel").value; refresh(); };

// ---------- 内容网格 ----------
const MONTH_FMT = new Intl.DateTimeFormat("zh-CN", { year: "numeric", month: "long" });
let blobs = [];

async function refresh() {
  await loadAlbums();
  const url = currentAlbum ? `/api/albums/${currentAlbum}/blobs` : "/api/blobs";
  const res = await api(url);
  blobs = await res.json();
  $("count").textContent = `${blobs.length} 张`;
  const grid = $("content");
  grid.className = "grid";
  grid.innerHTML = "";
  for (const b of blobs) {
    const cell = document.createElement("div");
    cell.className = "cell";
    cell.innerHTML = `<div class="name">${b.favorite ? "♥ " : ""}${b.file_name}</div>`;
    cell.onclick = () => openViewer(b);
    grid.appendChild(cell);
    if (b.mime.startsWith("image/")) {
      const img = new Image();
      img.loading = "lazy";
      img.src = `/api/blobs/${b.blob_hash}/thumbnail?token=${encodeURIComponent(token)}`;
      img.onerror = () => (cell.style.visibility = "hidden");
      cell.prepend(img);
    } else {
      cell.style.display = "grid";
      cell.style.placeItems = "center";
      cell.insertAdjacentHTML("afterbegin", `<div style="font-size:34px">🎬</div>`);
    }
  }
}

// ---------- 灯箱 ----------
const viewer = $("viewer");
function openViewer(b) {
  if (b.mime.startsWith("image/")) {
    $("full-img").src = `/api/blobs/${b.blob_hash}/raw?token=${encodeURIComponent(token)}`;
    viewer.style.display = "block";
  } else {
    toast("视频请用桌面版/安卓客户端在线播放", true);
  }
}
viewer.onclick = (e) => { if (e.target !== $("full-img")) viewer.style.display = "none"; };

// ---------- 上传 ----------
$("upload-btn").onclick = () => {
  if (!currentAlbum) { toast("先选择一个相册（全部照片无法上传）", true); return; }
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
      const data = await res.json();
      data.status === "imported" ? ok++ : (fail++, console.log(data));
    } catch (e) { fail++; toast(f.name + "：" + e.message, true); }
  }
  $("file-input").value = "";
  toast(`上传完成：${ok} 成功${fail ? `，${fail} 失败` : ""}`, fail > 0);
  refresh();
};

// ---------- 分享 ----------
$("share-btn").onclick = async () => {
  if (!currentAlbum) { toast("先选择一个相册", true); return; }
  if (!confirm(`为当前相册创建分享链接？\n\n收到链接的人无需登录即可浏览（服务器只见加密数据，浏览器端解密）。`)) return;
  try {
    const res = await api(`/api/albums/${currentAlbum}/share`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({}),
    });
    const data = await res.json();
    const full = location.origin + data.url;
    await navigator.clipboard.writeText(full).catch(() => {});
    toast("分享链接已创建并复制到剪贴板：\n" + full);
  } catch (e) { toast(e.message, true); }
};

// ---------- PWA ----------
if ("serviceWorker" in navigator) navigator.serviceWorker.register("/sw.js").catch(() => {});
