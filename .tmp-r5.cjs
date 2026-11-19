const fs = require("fs");
let n = 0;
const rep = (p, from, to, tag) => {
  let s = fs.readFileSync(p, "utf8");
  if (!s.includes(from)) { console.log("MISS:", tag); return; }
  s = s.split(from).join(to);
  fs.writeFileSync(p, s);
  n++;
  console.log("ok:", tag);
};

const web = "D:/Documents/Code/HeartShard/server/src/webapp/app.js";
// 1) webapp 上传附带原拍摄时间
rep(web, `      const fd = new FormData();
      fd.append("file", f, f.name);
      if (albumId) fd.append("album_id", albumId);`,
`      const fd = new FormData();
      fd.append("file", f, f.name);
      if (albumId) fd.append("album_id", albumId);
      fd.append("last_modified", String(f.lastModified || 0));`, "webapp last_modified");
// 2) webapp 分享延期
rep(web, `window.showShareList = async (id) => {`,
`window.shareExtend = (id, name) => {
  modal(\`<h3>延期分享\${name ? "：" + name : ""}</h3>
    <p class="hint">从当前时刻（或现有过期时间，取较晚者）延长：</p>
    <select id="ext-days" style="width:100%;margin:8px 0">
      <option value="1">1 天</option>
      <option value="7" selected>7 天</option>
      <option value="30">30 天</option>
      <option value="365">365 天</option>
    </select>
    <div class="row" style="justify-content:flex-end;margin-top:12px">
      <button onclick="closeModal()">取消</button>
      <button class="primary" id="ext-go">延期</button>
    </div>\`);
  $("ext-go").onclick = async () => {
    try {
      await api(\`/api/shares/\${id}/extend\`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ days: Number($("ext-days").value) }) });
      closeModal();
      toast("已延期");
      route();
    } catch (e) { toast(e.message, true); }
  };
};
window.showShareList = async (id) => {`, "webapp shareExtend");

// 3) 分享行加延期按钮（复制链接后）
rep(web, `        <td class="row" style="white-space:nowrap">
          <button onclick="copyShare('${s.id}', this)">复制链接</button>
          ${s.expired ? "" : `<button class="danger" onclick="cancelShare('${s.id}')">取消分享</button>`}
        </td></tr>`).join("")}`,
`        <td class="row" style="white-space:nowrap">
          <button onclick="copyShare('${s.id}', this)">复制链接</button>
          <button onclick="shareExtend('${s.id}', '${s.album_name || s.id}')">延期</button>
          ${s.expired ? "" : `<button class="danger" onclick="cancelShare('${s.id}')">取消分享</button>`}
        </td></tr>`).join("")}`, "share extend btn");

// 4) 头像菜单笔图标（webapp menu-avatar）
rep(web, `          <div style="position:relative;display:inline-block;margin-top:6px">
            <div class="avatar" id="menu-avatar" style="width:72px;height:72px;font-size:28px;cursor:default">我</div>
          </div>`,
`          <div style="position:relative;display:inline-block;margin-top:6px">
            <div class="avatar" id="menu-avatar" style="width:72px;height:72px;font-size:28px;cursor:pointer" title="点击更换头像" onclick="document.getElementById('avatar-input').click()">我</div>
            <span style="position:absolute;right:-2px;bottom:-2px;width:26px;height:26px;border-radius:50%;background:var(--primary);color:#fff;display:grid;place-items:center;cursor:pointer" onclick="document.getElementById('avatar-input').click()"><svg class="ic" style="width:13px;height:13px"><use href="#i-edit"/></svg></span>
          </div>`, "webapp avatar pencil");

fs.writeFileSync(web, s);

// ---- 桌面端 ----
const desk = "D:/Documents/Code/HeartShard/apps/tauri/src/components/Shell.tsx";
let d = fs.readFileSync(desk, "utf8");
// 5) 头像笔图标（桌面）
rep(desk, `          <div class="avatar" style={{ width: 72, height: 72, fontSize: 28, margin: "6px auto 0", cursor: platform.isRemote ? "pointer" : "default" }} onClick={platform.isRemote ? onPickAvatar : undefined} title={platform.isRemote ? "点击更换头像" : undefined}>
            {me?.avatar ? <img src={me.avatar} alt="" /> : avatarChar}
          </div>`,
`          <div style={{ position: "relative", display: "inline-block", margin: "6px auto 0" }}>
            <div className="avatar" style={{ width: 72, height: 72, fontSize: 28, cursor: platform.isRemote ? "pointer" : "default" }} onClick={platform.isRemote ? onPickAvatar : undefined} title={platform.isRemote ? "点击更换头像" : undefined}>
              {me?.avatar ? <img src={me.avatar} alt="" /> : avatarChar}
            </div>
            {platform.isRemote && (
              <span style={{ position: "absolute", right: -2, bottom: -2, width: 26, height: 26, borderRadius: "50%", background: "var(--primary)", color: "#fff", display: "grid", placeItems: "center", cursor: "pointer" }} onClick={onPickAvatar}>
                <Ic name="i-edit" />
              </span>
            )}
          </div>`, "desktop avatar pencil");
fs.writeFileSync(desk, d);

// ---- 桌面 api.ts：shareExtend + upload last_modified ----
const api = "D:/Documents/Code/HeartShard/apps/tauri/src/lib/api.ts";
let a = fs.readFileSync(api, "utf8");
rep(api, `  cancelShare: (id: string) => remoteFetch<void>(\`/api/shares/\${id}\`, { method: "DELETE" }),`,
`  cancelShare: (id: string) => remoteFetch<void>(\`/api/shares/\${id}\`, { method: "DELETE" }),
  shareExtend: (id: string, days: number) =>
    remoteFetch<{ expires_at: number | null }>(\`/api/shares/\${id}/extend\`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ days }),
    }),`, "desktop shareExtend");
rep(api, `      const fd = new FormData();
      fd.append("file", f, f.name);
      if (albumId) fd.append("album_id", albumId);`,
`      const fd = new FormData();
      fd.append("file", f, f.name);
      if (albumId) fd.append("album_id", albumId);
      fd.append("last_modified", String(f.lastModified || 0));`, "desktop last_modified");
rep(api, `  cancelShare(id: string): Promise<void>;`,
`  cancelShare(id: string): Promise<void>;
  shareExtend(id: string, days: number): Promise<{ expires_at: number | null }>;`, "interface shareExtend");
rep(api, `  cancelShare: async () => {
    throw new Error("分享链接需连接后台使用");
  },`,
`  cancelShare: async () => {
    throw new Error("分享链接需连接后台使用");
  },
  shareExtend: async () => {
    throw new Error("分享链接需连接后台使用");
  },`, "local stub shareExtend");
fs.writeFileSync(api, a);

// ---- Android 数据目录修复 ----
const cmd = "D:/Documents/Code/HeartShard/apps/tauri/src-tauri/src/commands.rs";
let c = fs.readFileSync(cmd, "utf8");
const oldFn = `/// 构造数据目录：便携模式 → exe 同目录 data/（无 app_data_dir 依赖）。
pub fn resolve_data_dir(_app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("无法定位程序路径: {e}"))?;
    let dir = exe
        .parent()
        .ok_or("无法确定程序所在目录")?
        .join("data");
    std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建数据目录: {e}"))?;
    Ok(dir)
}`;
const newFn = `/// 构造数据目录：
/// - Android：应用私有目录（exe 同目录只读，用 portable 逻辑会启动即 panic = 闪退）
/// - 桌面：便携模式 → exe 同目录 data/
pub fn resolve_data_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    #[cfg(target_os = "android")]
    {
        use tauri::Manager;
        let dir = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("无法确定数据目录: {e}"))?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建数据目录: {e}"))?;
        return Ok(dir.join("data"));
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        let exe = std::env::current_exe().map_err(|e| format!("无法定位程序路径: {e}"))?;
        let dir = exe
            .parent()
            .ok_or("无法确定程序所在目录")?
            .join("data");
        std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建数据目录: {e}"))?;
        Ok(dir)
    }
}`;
rep(cmd, oldFn, newFn, "resolve_data_dir android");
fs.writeFileSync(cmd, c);

console.log("total:", n);
