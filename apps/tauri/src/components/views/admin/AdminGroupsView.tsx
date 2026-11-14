// 用户组（网页端 viewAdminGroups 同款）：卡片墙 + 编辑弹窗（成员 + 相册组权限）。
import { useCallback, useEffect, useState } from "react";
import { api, AlbumInfo, GroupInfo } from "../../../lib/api";
import { usePhotoActions } from "../../PhotoActions";
import { Ic } from "../../icons";
import { useToast } from "../../Toast";

export function AdminGroupsView() {
  const actions = usePhotoActions();
  const toast = useToast();
  const [groups, setGroups] = useState<GroupInfo[]>([]);
  const [editing, setEditing] = useState<GroupInfo | "new" | null>(null);

  const reload = useCallback(() => {
    api
      .adminGroups()
      .then(setGroups)
      .catch((e) => toast.error(String(e)));
  }, [toast]);

  useEffect(() => {
    reload();
  }, [reload, actions.dataVersion]);

  const del = (g: GroupInfo) => {
    if (g.id === "family" || g.name === "family") {
      toast.info("family 是自动维护的全员组，不可删除");
      return;
    }
    actions.confirmAsk("删除用户组", `删除用户组「${g.name}」？`, async () => {
      const next = groups.filter((x) => x.id !== g.id);
      try {
        await api.saveAdminGroups(next);
        toast.success("已删除");
        reload();
      } catch (e) {
        toast.error(String(e));
      }
    }, true);
  };

  return (
    <div>
      <div className="view-head">
        <h2>用户组</h2>
        <span className="badge">{groups.length} 组</span>
      </div>
      <div className="album-wall">
        {groups.map((g) => (
          <div key={g.id} className="album-card">
            <div className="album-cover" style={{ background: "var(--success)", display: "grid", placeItems: "center", color: "#fff", fontWeight: 700, fontSize: 26 }}>
              {g.name.slice(0, 1)}
            </div>
            <div className="album-card-name">
              {g.name}
              <span className="album-card-count">{g.members.length} 人</span>
            </div>
            <div className="row" style={{ padding: "0 12px 12px", justifyContent: "center" }}>
              <button onClick={() => setEditing(g)}>
                <Ic name="i-edit" /> 编辑
              </button>
              {g.id !== "family" && g.name !== "family" && (
                <button className="danger" onClick={() => del(g)}>
                  删除
                </button>
              )}
            </div>
          </div>
        ))}
        <div
          className="album-card"
          style={{ borderStyle: "dashed", display: "grid", placeItems: "center", minHeight: 200 }}
          onClick={() => setEditing("new")}
        >
          <span style={{ color: "var(--primary)", fontWeight: 600 }}>
            <Ic name="i-groups" /> 创建组
          </span>
        </div>
      </div>
      {editing && (
        <GroupDialog
          existing={editing === "new" ? null : editing}
          allGroups={groups}
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

function GroupDialog({
  existing,
  allGroups,
  onClose,
  onDone,
}: {
  existing: GroupInfo | null;
  allGroups: GroupInfo[];
  onClose: () => void;
  onDone: () => void;
}) {
  const toast = useToast();
  const [name, setName] = useState(existing?.name ?? "");
  const [members, setMembers] = useState<Set<string>>(new Set(existing?.members ?? []));
  const [users, setUsers] = useState<{ id: string; name: string }[]>([]);
  const [albums, setAlbums] = useState<AlbumInfo[]>([]);
  const [acl, setAcl] = useState<Record<string, string>>({});

  useEffect(() => {
    api
      .adminUsers()
      .then((us) => setUsers(us.map((u) => ({ id: u.id, name: u.name }))))
      .catch(() => {});
    api.listAlbums().then(setAlbums).catch(() => {});
  }, []);

  const toggleMember = (id: string) => {
    setMembers((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const submit = async () => {
    if (!name.trim()) return;
    try {
      const gid = existing?.id ?? name.trim().toLowerCase().replace(/\s+/g, "-");
      const next = [
        ...allGroups.filter((g) => g.id !== (existing?.id ?? gid)),
        { id: gid, name: name.trim(), members: [...members] },
      ];
      await api.saveAdminGroups(next);
      // 相册组权限
      if (existing) {
        for (const [aid, perm] of Object.entries(acl)) {
          await api.setAcl(aid, "group", gid, perm === "" ? null : (perm as "none")).catch(() => {});
        }
      }
      toast.success(existing ? "用户组已更新" : "用户组已创建");
      onDone();
    } catch (e) {
      toast.error(String(e));
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" style={{ width: 560 }} onClick={(e) => e.stopPropagation()}>
        <h3>{existing ? `编辑用户组 · ${existing.name}` : "创建用户组"}</h3>
        <input placeholder="组名" value={name} onChange={(e) => setName(e.target.value)} style={{ width: "100%" }} />
        <p className="hint" style={{ marginTop: 12 }}>成员（family 组自动包含全部用户）：</p>
        <div className="row">
          {users.map((u) => (
            <label key={u.id} className="hint">
              <input type="checkbox" checked={members.has(u.id)} onChange={() => toggleMember(u.id)} /> {u.name}
            </label>
          ))}
          {users.length === 0 && <span className="hint">（暂无其他用户）</span>}
        </div>
        {albums.length > 0 && existing && (
          <>
            <p className="hint" style={{ marginTop: 14 }}>相册权限：</p>
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
          <button className="primary" onClick={submit} disabled={!name.trim()}>
            确定
          </button>
        </div>
      </div>
    </div>
  );
}
