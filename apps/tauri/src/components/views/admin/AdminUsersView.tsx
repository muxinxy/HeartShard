// 用户管理（网页端 viewAdminUsers 同款）：卡片墙 + 编辑弹窗（含相册权限表）。
import { useCallback, useEffect, useState } from "react";
import { api, AdminUser, AlbumInfo } from "../../../lib/api";
import { usePhotoActions } from "../../PhotoActions";
import { Ic } from "../../icons";
import { useToast } from "../../Toast";

export function AdminUsersView() {
  const actions = usePhotoActions();
  const toast = useToast();
  const [users, setUsers] = useState<AdminUser[]>([]);
  const [editing, setEditing] = useState<AdminUser | "new" | null>(null);

  const reload = useCallback(() => {
    api
      .adminUsers()
      .then(setUsers)
      .catch((e) => toast.error(String(e)));
  }, [toast]);

  useEffect(() => {
    reload();
  }, [reload, actions.dataVersion]);

  const del = (u: AdminUser) => {
    actions.confirmAsk("删除用户", `删除用户「${u.name}」？其登录将立即失效。`, async () => {
      try {
        await api.deleteAdminUser(u.id);
        toast.success("已删除");
        reload();
      } catch (e) {
        toast.error(String(e));
      }
    }, true);
  };

  const resetPw = (u: AdminUser) => {
    actions.confirmAsk("重置密码", `为「${u.name}」生成新密码？`, async () => {
      try {
        const newPw = Math.random().toString(36).slice(2, 12);
        await api.updateAdminUser(u.id, { password: newPw });
        toast.success(`新密码：${newPw}（请立即转告）`);
      } catch (e) {
        toast.error(String(e));
      }
    });
  };

  return (
    <div>
      <div className="view-head">
        <h2>用户管理</h2>
        <span className="badge">{users.length} 人</span>
      </div>
      <div className="album-wall">
        {users.map((u) => (
          <div key={u.id} className="album-card">
            <div className="album-cover" style={{ background: "var(--primary)", display: "grid", placeItems: "center" }}>
              {u.avatar ? (
                <img src={u.avatar} alt="" style={{ width: 84, height: 84, borderRadius: "50%", objectFit: "cover" }} />
              ) : (
                <div className="avatar" style={{ width: 84, height: 84, fontSize: 32, background: "rgba(255,255,255,.25)" }}>
                  {u.name.slice(0, 1)}
                </div>
              )}
            </div>
            <div className="album-card-name">
              <span>
                {u.name}
                {u.is_admin && " "}
                {u.is_admin && <span className="badge ok">管理员</span>}
                {!u.enabled && <span className="badge bad">已禁用</span>}
              </span>
            </div>
            <div className="row" style={{ padding: "0 12px 12px", justifyContent: "center" }}>
              <button onClick={() => setEditing(u)}>
                <Ic name="i-edit" /> 编辑
              </button>
              <button onClick={() => resetPw(u)}>重置密码</button>
              <button className="danger" onClick={() => del(u)}>
                删除
              </button>
            </div>
          </div>
        ))}
        <div
          className="album-card"
          style={{ borderStyle: "dashed", display: "grid", placeItems: "center", minHeight: 200 }}
          onClick={() => setEditing("new")}
        >
          <span style={{ color: "var(--primary)", fontWeight: 600 }}>
            <Ic name="i-plus-user" /> 添加用户
          </span>
        </div>
      </div>
      {editing && (
        <UserDialog
          existing={editing === "new" ? null : editing}
          onClose={() => setEditing(null)}
          onDone={() => {
            setEditing(null);
            reload();
          }}
        />
      )}
    </div>
  );
}

function UserDialog({ existing, onClose, onDone }: { existing: AdminUser | null; onClose: () => void; onDone: () => void }) {
  const toast = useToast();
  const [name, setName] = useState(existing?.name ?? "");
  const [pw, setPw] = useState("");
  const [isAdmin, setIsAdmin] = useState(existing?.is_admin ?? false);
  const [enabled, setEnabled] = useState(existing?.enabled ?? true);
  const [albums, setAlbums] = useState<AlbumInfo[]>([]);
  const [acl, setAcl] = useState<Record<string, string>>({});
  const busy = useState(false)[0];

  useEffect(() => {
    api.listAlbums().then(setAlbums).catch(() => {});
    if (existing) {
      // 现有相册级覆盖（从 ACL 表反查不可行——服务端未提供用户视角；这里仅展示相册列表供显式设置）
      void existing;
    }
  }, [existing]);

  const submit = async () => {
    if (!name.trim()) return;
    try {
      if (existing) {
        await api.updateAdminUser(existing.id, {
          name: name.trim(),
          ...(pw ? { password: pw } : {}),
          is_admin: isAdmin,
          enabled,
        });
      } else {
        await api.addAdminUser({ name: name.trim(), password: pw, is_admin: isAdmin });
      }
      // 相册级覆盖（仅已有用户；新建用户先保存再编辑权限）
      if (existing) {
        for (const [aid, perm] of Object.entries(acl)) {
          await api.setAcl(aid, "user", existing.id, perm === "" ? null : (perm as "none")).catch(() => {});
        }
      }
      toast.success(existing ? "用户已更新" : "用户已创建");
      onDone();
    } catch (e) {
      toast.error(String(e));
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" style={{ width: 560 }} onClick={(e) => e.stopPropagation()}>
        <h3>{existing ? `编辑用户 · ${existing.name}` : "添加用户"}</h3>
        <div className="row">
          <input placeholder="显示名" value={name} onChange={(e) => setName(e.target.value)} style={{ flex: 1 }} />
          <input
            type="password"
            placeholder={existing ? "新密码（留空不改）" : "登录密码"}
            value={pw}
            onChange={(e) => setPw(e.target.value)}
            style={{ flex: 1 }}
          />
        </div>
        <div className="row" style={{ marginTop: 10 }}>
          <label>
            <input type="checkbox" checked={isAdmin} onChange={(e) => setIsAdmin(e.target.checked)} /> 管理员
          </label>
          {existing && (
            <label>
              <input type="checkbox" checked={enabled} onChange={(e) => setEnabled(e.target.checked)} /> 启用
            </label>
          )}
        </div>
        {albums.length > 0 && (
          <>
            <p className="hint" style={{ marginTop: 14 }}>相册权限（显式设置优先于继承；留空 = 继承）：</p>
            <table className="admin">
              <tbody>
                {albums.map((a) => (
                  <tr key={a.id}>
                    <td>{a.name}</td>
                    <td>
                      <select
                        className="perm-select"
                        value={acl[a.id] ?? ""}
                        onChange={(e) => setAcl((prev) => ({ ...prev, [a.id]: e.target.value }))}
                        disabled={busy}
                      >
                        <option value="">继承</option>
                        <option value="none">隐藏</option>
                        <option value="view">只看</option>
                        <option value="upload">上传</option>
                        <option value="manage">管理</option>
                      </select>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </>
        )}
        <div className="row" style={{ justifyContent: "flex-end", marginTop: 14 }}>
          <button onClick={onClose}>取消</button>
          <button className="primary" onClick={submit} disabled={!name.trim() || (!existing && !pw)}>
            确定
          </button>
        </div>
      </div>
    </div>
  );
}
