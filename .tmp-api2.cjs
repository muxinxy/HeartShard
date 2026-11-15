const fs = require("fs");
const p = "D:/Documents/Code/HeartShard/apps/tauri/src/lib/api.ts";
let s = fs.readFileSync(p, "utf8");
let n = 0;
const rep = (from, to, tag) => {
  if (!s.includes(from)) { console.log("MISS:", tag); return; }
  s = s.split(from).join(to);
  n++;
  console.log("ok:", tag);
};
rep(
  `export interface AdminUser {
  id: string;
  name: string;
  avatar: string | null;
  is_admin: boolean;
  enabled: boolean;
}`,
  `export interface AdminUser {
  id: string;
  name: string;
  avatar: string | null;
  is_admin: boolean;
  enabled: boolean;
  /** 该成员是否已启用两步验证（仅管理端可见状态，不外泄密钥） */
  totp_enabled?: boolean;
}`,
  "AdminUser totp_enabled"
);
rep(
  `  adminUsers: () => remoteFetch<AdminUser[]>("/api/admin/users"),`,
  `  adminUsers: () => remoteFetch<AdminUser[]>("/api/admin/users"),
  adminResetTotp: (uid: string) =>
    remoteFetch<void>(\`/api/admin/users/\${uid}/reset-totp\`, { method: "POST" }),`,
  "remote adminResetTotp"
);
rep(
  `  adminUsers: async (): Promise<AdminUser[]> => {
    throw new Error("管理区需连接后台使用");
  },`,
  `  adminUsers: async (): Promise<AdminUser[]> => {
    throw new Error("管理区需连接后台使用");
  },
  adminResetTotp: async () => {
    throw new Error("管理区需连接后台使用");
  },`,
  "local stub adminResetTotp"
);
rep(
  `  adminUsers(): Promise<AdminUser[]>;`,
  `  adminUsers(): Promise<AdminUser[]>;
  adminResetTotp(uid: string): Promise<void>;`,
  "interface adminResetTotp"
);
fs.writeFileSync(p, s);
console.log("total:", n);
