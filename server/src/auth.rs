//! 认证：users.json（argon2id hash）+ 内存会话 token。

use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, SaltString};
use argon2::{Argon2, PasswordVerifier};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Clone, Serialize, Deserialize)]
pub struct User {
    pub uid: String,
    pub name: String,
    /// PHC 字符串（argon2id + 随机盐）
    pub password_hash: String,
    #[serde(default)]
    pub is_admin: bool,
    /// 是否启用（禁用 = 不能登录 + 会话吊销；默认启用，兼容旧 users.json）
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 头像（data URL，≤256KB；空 = 用用户名首字）
    #[serde(default)]
    pub avatar: String,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Group {
    pub gid: String,
    pub name: String,
    pub members: Vec<String>, // uid
}

pub struct UserStore {
    meta: Arc<dyn crate::meta::MetaStore>,
    sessions: Mutex<HashMap<String, String>>, // token → uid
}

fn hash_password(pw: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(pw.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| e.to_string())
}

fn verify_password(pw: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| Argon2::default().verify_password(pw.as_bytes(), &parsed).is_ok())
        .unwrap_or(false)
}

pub fn uid_for(name: &str) -> String {
    // 用户名即 uid（家庭规模，无需 uuid；只做小写去空白）
    name.trim().to_lowercase()
}

impl UserStore {
    /// 元数据库句柄（sources 等复用）。
    pub fn meta_handle(&self) -> Arc<dyn crate::meta::MetaStore> {
        self.meta.clone()
    }

    /// 列出用户（admin 管理页；每次读库）。
    pub fn list_users(&self) -> Result<Vec<User>, String> {
        self.meta.list_users()
    }

    /// 列出组（admin 管理页；每次读库）。
    pub fn list_groups(&self) -> Result<Vec<Group>, String> {
        self.meta.list_groups()
    }

    /// 新增用户（管理界面；口令在管理页输入，argon2 落库）。
    pub fn add_user(&self, name: &str, password: &str, is_admin: bool) -> Result<User, String> {
        let uid = uid_for(name);
        if uid.is_empty() {
            return Err("用户名不能为空".into());
        }
        if self.meta.list_users()?.iter().any(|u| u.uid == uid) {
            return Err(format!("用户「{name}」已存在"));
        }
        if password.is_empty() {
            return Err("口令不能为空".into());
        }
        let user = User {
            uid: uid.clone(),
            name: name.trim().to_string(),
            password_hash: hash_password(password)?,
            is_admin,
            enabled: true,
            avatar: String::new(),
        };
        self.meta.save_user(&user)?;
        Ok(user)
    }

    /// 修改用户：口令（重哈希）/ 显示名 / admin / enabled / avatar；None 表示不改。
    pub fn update_user(
        &self,
        uid: &str,
        password: Option<&str>,
        name: Option<&str>,
        is_admin: Option<bool>,
        enabled: Option<bool>,
        avatar: Option<&str>,
    ) -> Result<(), String> {
        let mut users = self.meta.list_users()?;
        let user_idx = users
            .iter()
            .position(|u| u.uid == uid)
            .ok_or_else(|| format!("用户 {uid} 不存在"))?;
        if let Some(p) = password {
            if p.is_empty() {
                return Err("口令不能为空".into());
            }
            users[user_idx].password_hash = hash_password(p)?;
        }
        if let Some(n) = name {
            if !n.trim().is_empty() {
                users[user_idx].name = n.trim().to_string();
            }
        }
        if let Some(a) = is_admin {
            // 至少保留一个管理员
            let admins_after = users.iter().filter(|u| if u.uid == uid { a } else { u.is_admin }).count();
            if admins_after == 0 {
                return Err("至少保留一个管理员".into());
            }
            users[user_idx].is_admin = a;
        }
        if let Some(en) = enabled {
            users[user_idx].enabled = en;
            if !en {
                // 禁用即吊销该用户全部会话
                self.sessions.lock().unwrap().retain(|_, v| v != uid);
            }
        }
        if let Some(av) = avatar {
            users[user_idx].avatar = av.to_string();
        }
        self.meta.save_user(&users[user_idx])
    }

    /// 删除用户（同时移出所有组、吊销会话）。
    pub fn remove_user(&self, uid: &str) -> Result<(), String> {
        let users = self.meta.list_users()?;
        if users.iter().filter(|u| u.is_admin).count() <= 1 && users.iter().any(|u| u.uid == uid && u.is_admin) {
            return Err("至少保留一个管理员".into());
        }
        self.meta.delete_user(uid)?;
        let mut groups = self.meta.list_groups()?;
        for g in &mut groups {
            g.members.retain(|m| m != uid);
        }
        self.meta.save_groups(&groups)?;
        self.sessions.lock().unwrap().retain(|_, v| v != uid);
        Ok(())
    }

    /// 整体替换组列表（管理界面）。
    pub fn set_groups(&self, groups: Vec<Group>) -> Result<(), String> {
        self.meta.save_groups(&groups)
    }

    /// 初始化：JSON 文件迁移（meta.rs 完成）+ server.toml 引导用户 upsert + family 组重建。
    pub fn init(meta: Arc<dyn crate::meta::MetaStore>, cfg: &crate::config::ServerConfig) -> Result<Self, String> {
        let store = Self { meta, sessions: Mutex::new(HashMap::new()) };

        let mut users = store.meta.list_users()?;
        for u in &cfg.users {
            let pw = match (&u.password_env, &u.password) {
                (Some(env), _) => std::env::var(env)
                    .map_err(|_| format!("环境变量 {env} 未设置（用户 {}）", u.name))?,
                (None, Some(p)) => p.clone(),
                (None, None) => {
                    return Err(format!("用户 {} 未配置 password/password_env", u.name));
                }
            };
            let uid = uid_for(&u.name);
            let hash = hash_password(&pw)?;
            match users.iter_mut().find(|x| x.uid == uid) {
                Some(existing) => {
                    existing.name = u.name.clone();
                    existing.password_hash = hash;
                    existing.is_admin = u.admin;
                }
                None => users.push(User {
                    uid: uid.clone(),
                    name: u.name.clone(),
                    password_hash: hash,
                    is_admin: u.admin,
                    enabled: true,
                    avatar: String::new(),
                }),
            }
        }
        for u in &users {
            store.meta.save_user(u)?;
        }

        // 自动 family 组 = 全体用户（幂等重建）
        let mut groups = store.meta.list_groups()?;
        groups.retain(|g| g.name != "family");
        groups.push(Group {
            gid: "family".into(),
            name: "family".into(),
            members: users.iter().map(|u| u.uid.clone()).collect(),
        });
        for g in &cfg.groups {
            let gid = format!("g-{}", g.name.trim().to_lowercase());
            groups.retain(|x| x.gid != gid);
            groups.push(Group {
                gid: gid.clone(),
                name: g.name.clone(),
                members: g.members.iter().map(|m| uid_for(m)).collect(),
            });
        }
        store.meta.save_groups(&groups)?;
        Ok(store)
    }

    /// 用户本人修改口令（校验旧口令；管理页改他人口令走 update_user）。
    pub fn change_own_password(&self, uid: &str, old: &str, new: &str) -> Result<(), String> {
        let mut users = self.meta.list_users()?;
        let user = users.iter_mut().find(|u| u.uid == uid).ok_or("用户不存在")?;
        if !verify_password(old, &user.password_hash) {
            return Err("旧口令错误".into());
        }
        if new.is_empty() {
            return Err("新口令不能为空".into());
        }
        user.password_hash = hash_password(new)?;
        self.meta.save_user(user)
    }

    /// 设置用户头像（data URL）。
    pub fn set_avatar(&self, uid: &str, data_url: &str) -> Result<(), String> {
        let mut users = self.meta.list_users()?;
        let user = users.iter_mut().find(|u| u.uid == uid).ok_or("用户不存在")?;
        user.avatar = data_url.to_string();
        self.meta.save_user(user)
    }

    /// 登录：校验口令，签发会话 token（禁用账户拒绝）。
    pub fn login(&self, name: &str, password: &str) -> Result<(String, User), String> {
        let uid = uid_for(name);
        let user = self
            .meta
            .list_users()?
            .into_iter()
            .find(|u| u.uid == uid)
            .ok_or("用户不存在或口令错误")?;
        if !user.enabled {
            return Err("该账户已被禁用".into());
        }
        if !verify_password(password, &user.password_hash) {
            return Err("用户不存在或口令错误".into());
        }
        let token = random_token();
        self.sessions
            .lock()
            .unwrap()
            .insert(token.clone(), user.uid.clone());
        Ok((token, user.clone()))
    }

    pub fn logout(&self, token: &str) {
        self.sessions.lock().unwrap().remove(token);
    }

    /// token → uid（无效返回 None）。
    pub fn verify(&self, token: &str) -> Option<User> {
        let uid = self.sessions.lock().unwrap().get(token)?.clone();
        self.meta.list_users().ok()?.into_iter().find(|u| u.uid == uid)
    }

    pub fn user_by_uid(&self, uid: &str) -> Option<User> {
        self.meta.list_users().ok()?.into_iter().find(|u| u.uid == uid)
    }

    /// 用户所属组的 gid 集合。
    pub fn groups_of(&self, uid: &str) -> Vec<String> {
        self.meta
            .list_groups()
            .unwrap_or_default()
            .into_iter()
            .filter(|g| g.members.iter().any(|m| m == uid))
            .map(|g| g.gid.clone())
            .collect()
    }
}

fn random_token() -> String {
    let mut buf = [0u8; 32];
    getrandom::getrandom(&mut buf).expect("OS RNG failure");
    hex::encode(buf)
}
