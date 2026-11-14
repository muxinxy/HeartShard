// 应用壳（网页端同款 IA）：侧边栏 + hash 路由 + 顶栏（搜索/上传/头像菜单）。
// 分享与「管理」区仅连接后台模式显示；存储管理/设置仅本地模式显示。
import { useCallback, useEffect, useRef, useState } from "react";
import QRCode from "qrcode";
import { api, MeInfo, platform, AlbumInfo } from "../lib/api";
import { Ic } from "./icons";
import { PhotoActionsProvider, usePhotoActions } from "./PhotoActions";
import { TaskPanel, TaskProvider } from "./TaskPanel";
import { useToast } from "./Toast";
import { TimelineView } from "./views/TimelineView";
import { ExploreView } from "./views/ExploreView";
import { AlbumsView } from "./views/AlbumsView";
import { FavoritesView } from "./views/FavoritesView";
import { TrashView } from "./views/TrashView";
import { SharesView } from "./views/SharesView";
import { AdminUsersView } from "./views/admin/AdminUsersView";
import { AdminGroupsView } from "./views/admin/AdminGroupsView";
import { AdminSourcesView } from "./views/admin/AdminSourcesView";
import { AdminStorageView } from "./views/admin/AdminStorageView";
import { AdminStatusView } from "./views/admin/AdminStatusView";
import { SettingsPage } from "./SettingsPage";
import { StoragePage } from "./StoragePage";
import { MapView } from "./MapView";
import { ImportWizard } from "./ImportWizard";

function useHashRoute(): [string, (v: string) => void] {
  const [hash, setHash] = useState(window.location.hash || "#/photos");
  useEffect(() => {
    const f = () => setHash(window.location.hash || "#/photos");
    window.addEventListener("hashchange", f);
    return () => window.removeEventListener("hashchange", f);
  }, []);
  const nav = useCallback((v: string) => {
    window.location.hash = `#/${v}`;
  }, []);
  return [hash, nav];
}

export function ShellWithTasks({ onLock }: { onLock: () => void }) {
  return (
    <TaskProvider>
      <PhotoActionsProvider>
        <ShellInner onLock={onLock} />
        <TaskPanel />
      </PhotoActionsProvider>
    </TaskProvider>
  );
}

function ShellInner({ onLock }: { onLock: () => void }) {
  const actions = usePhotoActions();
  const toast = useToast();
  const [hash, nav] = useHashRoute();
  const [searchQ, setSearchQ] = useState("");
  const [menuOpen, setMenuOpen] = useState(false);
  const [uploadOpen, setUploadOpen] = useState(false);
  const [theme, setTheme] = useState(localStorage.getItem("hs_theme") || "dark");
  const [me, setMe] = useState<MeInfo | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const avatarInputRef = useRef<HTMLInputElement>(null);
  const mainRef = useRef<HTMLDivElement>(null);

  const view = hash.replace(/^#\//, "").split("?")[0];
  const albumId = view.startsWith("album-") ? view.slice("album-".length) : null;

  // 主题（localStorage，双模式生效）
  useEffect(() => {
    document.body.className = theme === "light" ? "theme-light" : "";
  }, [theme]);

  const toggleTheme = () => {
    const next = theme === "light" ? "dark" : "light";
    setTheme(next);
    localStorage.setItem("hs_theme", next);
  };

  // 远端模式拉取个人信息（头像菜单）
  useEffect(() => {
    if (platform.isRemote) {
      api.me().then(setMe).catch(() => setMe(null));
    }
  }, [hash]);

  // 路由切换回顶部 + 收起菜单 + 清空搜索（非搜索视图）
  useEffect(() => {
    mainRef.current?.scrollTo({ top: 0 });
    setMenuOpen(false);
    if (!["photos", "albums"].includes(albumId ? "albums" : view)) setSearchQ("");
  }, [hash]); // eslint-disable-line react-hooks/exhaustive-deps

  const doLock = async () => {
    await api.lock();
    onLock();
  };

  const avatarChar = (me?.name || "我").slice(0, 1);

  // 头像上传（远端）
  const onAvatarFile = (file: File) => {
    const img = new Image();
    img.onload = async () => {
      const c = document.createElement("canvas");
      const side = Math.min(img.width, img.height);
      c.width = c.height = 256;
      c.getContext("2d")!.drawImage(img, (img.width - side) / 2, (img.height - side) / 2, side, side, 0, 0, 256, 256);
      const dataUrl = c.toDataURL("image/jpeg", 0.85);
      try {
        await api.setAvatar(dataUrl);
        setMe((m) => (m ? { ...m, avatar: dataUrl } : m));
        toast.success("头像已更新");
      } catch (e) {
        toast.error(String(e));
      }
    };
    img.src = URL.createObjectURL(file);
  };

  const navItems: { view: string; label: string; icon: string; show: boolean }[] = [
    { view: "photos", label: "时间线", icon: "i-timeline", show: true },
    { view: "explore", label: "探索", icon: "i-explore", show: true },
    { view: "albums", label: "相册", icon: "i-albums", show: true },
    { view: "shares", label: "分享", icon: "i-shares", show: platform.isRemote },
    { view: "favorites", label: "收藏夹", icon: "i-favorites", show: true },
    { view: "trash", label: "回收站", icon: "i-trash", show: true },
  ];
  const adminItems: { view: string; label: string; icon: string }[] = [
    { view: "admin-users", label: "用户管理", icon: "i-users" },
    { view: "admin-groups", label: "用户组", icon: "i-groups" },
    { view: "admin-sources", label: "同步源", icon: "i-sync" },
    { view: "admin-storage", label: "存储管理", icon: "i-storage" },
    { view: "admin-status", label: "系统状态", icon: "i-status" },
  ];

  const searchVisible = view === "photos" || view === "albums" || (albumId !== null);
  const uploadVisible = view === "photos" || view === "albums" || albumId !== null;

  const pickUploadFiles = () => {
    if (platform.isRemote) fileRef.current?.click();
    else actions.importDialog();
  };

  return (
    <div className="app-shell">
      <nav className="sidebar">
        <div className="logo">牧心相册</div>
        {navItems.map((it) => (
          <a key={it.view} className={view === it.view || (it.view === "albums" && albumId) ? "active" : ""} onClick={() => nav(it.view)}>
            <Ic name={it.icon} />
            {it.label}
          </a>
        ))}
        {platform.isRemote && me?.is_admin && (
          <>
            <div className="sec">管理</div>
            {adminItems.map((it) => (
              <a key={it.view} className={view === it.view ? "active" : ""} onClick={() => nav(it.view)}>
                <Ic name={it.icon} />
                {it.label}
              </a>
            ))}
          </>
        )}
        {!platform.isRemote && (
          <>
            <div className="sec">本地库</div>
            <a className={view === "storage" ? "active" : ""} onClick={() => nav("storage")}>
              <Ic name="i-storage" />
              存储管理
            </a>
          </>
        )}
        <div style={{ flex: 1 }} />
        <a className={view === "settings" ? "active" : ""} onClick={() => nav("settings")}>
          <Ic name="i-settings" />
          设置
        </a>
      </nav>

      <div className="main" ref={mainRef}>
        <div className="topbar">
          {searchVisible && (
            <input
              type="search"
              placeholder="搜索照片 / 相册…"
              value={searchQ}
              onChange={(e) => setSearchQ(e.target.value)}
            />
          )}
          {searchVisible && <span style={{ flex: 1 }} />}
          {uploadVisible && (
            <button className="primary" onClick={pickUploadFiles}>
              <Ic name="i-upload" />
              {platform.isRemote ? "上传" : "导入照片 / 视频"}
            </button>
          )}
          <div style={{ position: "relative" }}>
            <div className="avatar" onClick={() => setMenuOpen(!menuOpen)} title={me?.name}>
              {me?.avatar ? <img src={me.avatar} alt="" /> : avatarChar}
            </div>
            {menuOpen && (
              <AvatarMenu
                me={me}
                onClose={() => setMenuOpen(false)}
                onChangePassword={() => setMenuOpen(false)}
                onTotp={() => setMenuOpen(false)}
                onTheme={toggleTheme}
                theme={theme}
                onLock={doLock}
                onPickAvatar={() => avatarInputRef.current?.click()}
              />
            )}
          </div>
        </div>

        <div className="content">
          {view === "photos" && <TimelineView searchQ={searchQ} />}
          {view === "explore" && <ExploreView />}
          {(view === "albums" || albumId) && <AlbumsView albumId={albumId} searchQ={searchQ} />}
          {view === "favorites" && <FavoritesView />}
          {view === "trash" && <TrashView />}
          {view === "shares" && platform.isRemote && <SharesView />}
          {view === "admin-users" && <AdminUsersView />}
          {view === "admin-groups" && <AdminGroupsView />}
          {view === "admin-sources" && <AdminSourcesView />}
          {view === "admin-storage" && <AdminStorageView />}
          {view === "admin-status" && <AdminStatusView />}
          {view === "storage" && !platform.isRemote && <StoragePage onDone={() => nav("photos")} />}
          {view === "settings" && <SettingsPage onDone={() => nav("photos")} />}
          {view === "import" && !platform.isRemote && <ImportWizard onDone={() => nav("photos")} />}
          {view === "map" && <MapViewView />}
          {view === "shares" && !platform.isRemote && (
            <p className="hint" style={{ marginTop: 60, textAlign: "center" }}>
              分享链接需连接后台使用。
            </p>
          )}
        </div>
      </div>

      {/* 远端上传文件选择 */}
      {platform.isRemote && (
        <input
          ref={fileRef}
          type="file"
          multiple
          accept="image/*,video/*"
          style={{ display: "none" }}
          onChange={(e) => {
            const files = Array.from(e.target.files || []);
            e.target.value = "";
            if (files.length === 0) return;
            if (albumId) {
              api.uploadFiles(files, albumId).then((rs) => {
                const ok = rs.filter((r) => r.status === "imported").length;
                toast.success(`上传完成：${ok} 成功${rs.length - ok ? `，${rs.length - ok} 失败` : ""}`);
                actions.onChanged();
              });
            } else {
              setUploadOpen(true);
              (window as unknown as { __pendingUpload?: File[] }).__pendingUpload = files;
            }
          }}
        />
      )}
      <input
        ref={avatarInputRef}
        type="file"
        accept="image/*"
        style={{ display: "none" }}
        onChange={(e) => {
          const f = e.target.files?.[0];
          e.target.value = "";
          if (f) onAvatarFile(f);
        }}
      />
      {uploadOpen && (
        <UploadModal
          onClose={() => setUploadOpen(false)}
          onDone={() => {
            setUploadOpen(false);
            actions.onChanged();
          }}
        />
      )}
    </div>
  );
}

function MapViewView() {
  const [blobs, setBlobs] = useState<import("../lib/api").BlobEntry[]>([]);
  useEffect(() => {
    api.listBlobs().then((b) => setBlobs(b.filter((x) => x.gps_lat != null && x.gps_lon != null)));
  }, []);
  return (
    <div>
      <div className="view-head">
        <a className="hint" onClick={() => (window.location.hash = "#/explore")} style={{ cursor: "pointer" }}>
          ← 返回探索
        </a>
        <h2>地图</h2>
      </div>
      <MapView blobs={blobs} onDone={() => (window.location.hash = "#/explore")} />
    </div>
  );
}

// ---------------- 头像菜单 ----------------

function AvatarMenu({
  me,
  onClose,
  onTheme,
  theme,
  onLock,
  onPickAvatar,
}: {
  me: MeInfo | null;
  onClose: () => void;
  onChangePassword: () => void;
  onTotp: () => void;
  onTheme: () => void;
  theme: string;
  onLock: () => void;
  onPickAvatar: () => void;
}) {
  const [pwOpen, setPwOpen] = useState(false);
  const [totpOpen, setTotpOpen] = useState(false);
  const avatarChar = (me?.name || "我").slice(0, 1);

  return (
    <>
      <div style={{ position: "fixed", inset: 0, zIndex: 140 }} onClick={onClose} />
      <div className="avatar-menu" style={{ zIndex: 150 }}>
        <div
          className="avatar"
          style={{ width: 72, height: 72, fontSize: 28, margin: "6px auto 0", cursor: platform.isRemote ? "pointer" : "default" }}
          onClick={platform.isRemote ? onPickAvatar : undefined}
          title={platform.isRemote ? "点击更换头像" : undefined}
        >
          {me?.avatar ? <img src={me.avatar} alt="" /> : avatarChar}
        </div>
        <div style={{ fontWeight: 600, fontSize: 17, margin: "8px 0 12px" }}>{me?.name || "本地相册库"}</div>
        <button className="menu-item" onClick={() => setPwOpen(true)}>
          <Ic name="i-lock" /> 修改密码
        </button>
        {platform.isRemote && (
          <button className="menu-item" onClick={() => setTotpOpen(true)}>
            <Ic name="i-shares" /> 两步验证 (2FA)
          </button>
        )}
        <button className="menu-item" onClick={onTheme}>
          <Ic name={theme === "light" ? "i-moon" : "i-sun"} />
          {theme === "light" ? "深色模式" : "浅色模式"}
        </button>
        <button className="menu-item danger" onClick={onLock}>
          <Ic name="i-logout" />
          {platform.isRemote ? "退出登录" : "锁定"}
        </button>
      </div>
      {pwOpen && <PasswordModal onClose={() => setPwOpen(false)} />}
      {totpOpen && <TotpModal onClose={() => setTotpOpen(false)} />}
    </>
  );
}

// ---------------- 修改密码（双模式） ----------------

function PasswordModal({ onClose }: { onClose: () => void }) {
  const toast = useToast();
  const [old, setOld] = useState("");
  const [nw, setNw] = useState("");
  const [nw2, setNw2] = useState("");
  const [busy, setBusy] = useState(false);
  const mismatch = nw.length > 0 && nw2.length > 0 && nw !== nw2;
  const submit = async () => {
    if (!old || !nw || nw !== nw2) return;
    setBusy(true);
    try {
      await api.changePassword(old, nw);
      toast.success("密码已修改");
      onClose();
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" style={{ width: 420 }} onClick={(e) => e.stopPropagation()}>
        <h3>修改密码</h3>
        <input type="password" placeholder="当前密码" value={old} onChange={(e) => setOld(e.target.value)} style={{ width: "100%", marginBottom: 8 }} />
        <input type="password" placeholder="新密码" value={nw} onChange={(e) => setNw(e.target.value)} style={{ width: "100%" }} />
        <input type="password" placeholder="确认新密码" value={nw2} onChange={(e) => setNw2(e.target.value)} style={{ width: "100%", marginTop: 8 }} />
        {mismatch && <p className="error">两次输入的新密码不一致</p>}
        <div className="row" style={{ justifyContent: "flex-end", marginTop: 14 }}>
          <button onClick={onClose}>取消</button>
          <button className="primary" onClick={submit} disabled={busy || !old || !nw || mismatch}>
            确定
          </button>
        </div>
      </div>
    </div>
  );
}

// ---------------- TOTP 两步验证（远端） ----------------

function TotpModal({ onClose }: { onClose: () => void }) {
  const toast = useToast();
  const [setup, setSetup] = useState<{ secret: string; uri: string } | null>(null);
  const [qr, setQr] = useState<string | null>(null);
  const [code, setCode] = useState("");
  const [totpHint, setTotpHint] = useState("");

  useEffect(() => {
    api
      .totpSetup()
      .then((s) => {
        setSetup(s);
        // otpauth:// uri → 二维码（验证器 App 扫码绑定）
        QRCode.toDataURL(s.uri, { width: 200, margin: 1 })
          .then(setQr)
          .catch(() => setQr(null));
      })
      .catch((e) => toast.error(String(e)));
  }, [toast]);

  useEffect(() => {
    if (setup && /^\d{6}$/.test(code)) {
      totpVerifyLocal(setup.secret, code).then((ok) => setTotpHint(ok ? "✓ 验证码正确" : ""));
    } else {
      setTotpHint("");
    }
  }, [setup, code]);

  const disable = async () => {
    try {
      await api.totpDisable();
      toast.success("两步验证已关闭");
      onClose();
    } catch (e) {
      toast.error(String(e));
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" style={{ width: 460 }} onClick={(e) => e.stopPropagation()}>
        <h3>两步验证 (TOTP 2FA)</h3>
        <p className="hint">用验证器 App（Google Authenticator / 微软 Authenticator 等）扫码或手动输入密钥：</p>
        {setup && (
          <>
            {qr && (
              <div style={{ textAlign: "center", margin: "8px 0" }}>
                <img src={qr} alt="TOTP 二维码" style={{ background: "#fff", padding: 6, borderRadius: 10 }} />
              </div>
            )}
            <div style={{ background: "var(--input)", borderRadius: 10, padding: "10px 12px", fontFamily: "monospace", fontSize: 16, letterSpacing: 2, textAlign: "center", wordBreak: "break-all" }}>
              {setup.secret}
            </div>
            <p className="hint" style={{ wordBreak: "break-all" }}>{setup.uri}</p>
            <label className="hint">输入 App 显示的 6 位验证码确认启用：</label>
            <input
              inputMode="numeric"
              maxLength={6}
              value={code}
              onChange={(e) => setCode(e.target.value.replace(/\D/g, ""))}
              style={{ width: "100%", margin: "6px 0" }}
            />
            <p className="hint">{totpHint}</p>
          </>
        )}
        <div className="row" style={{ justifyContent: "space-between", marginTop: 10 }}>
          <button className="danger" onClick={disable}>关闭两步验证</button>
          <button
            className="primary"
            onClick={async () => {
              if (!setup) return;
              if (await totpVerifyLocal(setup.secret, code)) {
                toast.success("两步验证已启用");
                onClose();
              } else {
                toast.error("验证码错误，未启用");
              }
            }}
          >
            确认启用
          </button>
        </div>
      </div>
    </div>
  );
}

/** 前端本地 TOTP 校验（RFC 6238 / SHA1 / 30s / 6 位，±1 窗口；网页端同款） */
export async function totpVerifyLocal(secretB32: string, code: string): Promise<boolean> {
  if (!/^\d{6}$/.test(code)) return false;
  const alpha = "abcdefghijklmnopqrstuvwxyz234567";
  let bits = "";
  for (const ch of secretB32.toLowerCase()) {
    const v = alpha.indexOf(ch);
    if (v < 0) return false;
    bits += v.toString(2).padStart(5, "0");
  }
  const bytes: number[] = [];
  for (let i = 0; i + 8 <= bits.length; i += 8) bytes.push(parseInt(bits.slice(i, i + 8), 2));
  const key = await crypto.subtle.importKey("raw", new Uint8Array(bytes), { name: "HMAC", hash: "SHA-1" }, false, ["sign"]);
  const counter = Math.floor(Date.now() / 1000 / 30);
  for (const drift of [-1, 0, 1]) {
    const msg = new Uint8Array(8);
    new DataView(msg.buffer).setUint32(4, counter + drift);
    const mac = new Uint8Array(await crypto.subtle.sign("HMAC", key, msg));
    const off = mac[mac.length - 1] & 0xf;
    const v = ((mac[off] & 0x7f) << 24) | (mac[off + 1] << 16) | (mac[off + 2] << 8) | mac[off + 3];
    if (String(v % 1000000).padStart(6, "0") === code) return true;
  }
  return false;
}

// ---------------- 上传入相册弹窗（远端） ----------------

function UploadModal({ onClose, onDone }: { onClose: () => void; onDone: () => void }) {
  const toast = useToast();
  const [albums, setAlbums] = useState<AlbumInfo[]>([]);
  const [albumId, setAlbumId] = useState("");
  const [busy, setBusy] = useState(false);
  const pending = (window as unknown as { __pendingUpload?: File[] }).__pendingUpload || [];

  useEffect(() => {
    api.listAlbums().then(setAlbums).catch(() => {});
  }, []);

  const submit = async () => {
    setBusy(true);
    let ok = 0;
    let fail = 0;
    for (const f of pending) {
      try {
        const r = await api.uploadFiles([f], albumId || null);
        if (r.some((x) => x.status === "imported")) ok++;
        else fail++;
      } catch {
        fail++;
      }
    }
    (window as unknown as { __pendingUpload?: File[] }).__pendingUpload = undefined;
    toast.success(`上传完成：${ok} 成功${fail ? `，${fail} 失败` : ""}`);
    onDone();
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" style={{ width: 420 }} onClick={(e) => e.stopPropagation()}>
        <h3>上传 {pending.length} 张到相册</h3>
        <select value={albumId} onChange={(e) => setAlbumId(e.target.value)} style={{ width: "100%" }}>
          <option value="">不入相册（仅时间线）</option>
          {albums.map((a) => (
            <option key={a.id} value={a.id}>
              {a.name}
            </option>
          ))}
        </select>
        <div className="row" style={{ justifyContent: "flex-end", marginTop: 14 }}>
          <button onClick={onClose}>取消</button>
          <button className="primary" onClick={submit} disabled={busy}>
            {busy ? "上传中…" : "开始上传"}
          </button>
        </div>
      </div>
    </div>
  );
}
