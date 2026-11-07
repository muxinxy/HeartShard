//! 认证：users.json（argon2id hash）+ 内存会话 token。

use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, SaltString};
use argon2::{Argon2, PasswordVerifier};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

#[derive(Clone, Serialize, Deserialize)]
pub struct User {
    pub uid: String,
    pub name: String,
    /// PHC 字符串（argon2id + 随机盐）
    pub password_hash: String,
    #[serde(default)]
    pub is_admin: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Group {
    pub gid: String,
    pub name: String,
    pub members: Vec<String>, // uid
}

pub struct UserStore {
    pub users: Vec<User>,
    pub groups: Vec<Group>,
    users_path: std::path::PathBuf,
    groups_path: std::path::PathBuf,
    sessions: Mutex<HashMap<String, String>>, // token → uid
}

fn read_json<T: for<'de> Deserialize<'de>>(p: &Path) -> Option<T> {
    std::fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

fn write_json<T: Serialize>(p: &Path, v: &T) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?;
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, &json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, p).map_err(|e| e.to_string())?;
    Ok(())
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
    /// 新增用户（管理界面；口令在管理页输入，argon2 落盘）。
    pub fn add_user(&mut self, name: &str, password: &str, is_admin: bool) -> Result<User, String> {
        let uid = uid_for(name);
        if uid.is_empty() {
            return Err("用户名不能为空".into());
        }
        if self.users.iter().any(|u| u.uid == uid) {
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
        };
        self.users.push(user.clone());
        self.save_users()?;
        Ok(user)
    }

    /// 修改用户：口令（重哈希）/ 显示名 / admin 标记；None 表示不改。
    pub fn update_user(
        &mut self,
        uid: &str,
        password: Option<&str>,
        name: Option<&str>,
        is_admin: Option<bool>,
    ) -> Result<(), String> {
        let user_idx = self
            .users
            .iter()
            .position(|u| u.uid == uid)
            .ok_or_else(|| format!("用户 {uid} 不存在"))?;
        if let Some(p) = password {
            if p.is_empty() {
                return Err("口令不能为空".into());
            }
            self.users[user_idx].password_hash = hash_password(p)?;
        }
        if let Some(n) = name {
            if !n.trim().is_empty() {
                self.users[user_idx].name = n.trim().to_string();
            }
        }
        if let Some(a) = is_admin {
            // 至少保留一个管理员
            let admins_after = self
                .users
                .iter()
                .filter(|u| if u.uid == uid { a } else { u.is_admin })
                .count();
            if admins_after == 0 {
                return Err("至少保留一个管理员".into());
            }
            self.users[user_idx].is_admin = a;
        }
        self.save_users()
    }

    /// 删除用户（同时移出所有组、吊销会话）。
    pub fn remove_user(&mut self, uid: &str) -> Result<(), String> {
        if self.users.iter().filter(|u| u.is_admin).count() <= 1
            && self.users.iter().any(|u| u.uid == uid && u.is_admin)
        {
            return Err("至少保留一个管理员".into());
        }
        self.users.retain(|u| u.uid != uid);
        for g in &mut self.groups {
            g.members.retain(|m| m != uid);
        }
        self.sessions
            .lock()
            .unwrap()
            .retain(|_, v| v != uid);
        self.save_users()?;
        self.save_groups()
    }

    pub fn save_users(&self) -> Result<(), String> {
        let path = self.users_path.clone();
        write_json(&path, &self.users)
    }

    pub fn save_groups(&self) -> Result<(), String> {
        let path = self.groups_path.clone();
        write_json(&path, &self.groups)
    }

    /// 整体替换组列表（管理界面）。
    pub fn set_groups(&mut self, groups: Vec<Group>) -> Result<(), String> {
        self.groups = groups;
        self.save_groups()
    }

    /// 从 server.toml 的 users/groups 初始化（upsert 到 users.json / groups.json）。
    /// 口令以 server.toml/环境变量为准，每次启动同步 hash（便于改密）。
    pub fn init(root: &Path, cfg: &crate::config::ServerConfig) -> Result<Self, String> {
        let users_path = root.join("users.json");
        let groups_path = root.join("groups.json");
        std::fs::create_dir_all(root).map_err(|e| e.to_string())?;

        let mut users: Vec<User> = read_json(&users_path).unwrap_or_default();
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
                }),
            }
        }
        write_json(&users_path, &users)?;

        // 自动 family 组 = 全体用户（幂等重建）
        let mut groups: Vec<Group> = read_json(&groups_path).unwrap_or_default();
        groups.retain(|g| g.name != "family");
        groups.push(Group {
            gid: "family".into(),
            name: "family".into(),
            members: users.iter().map(|u| u.uid.clone()).collect(),
        });
        // 自定义组按名字 upsert
        for g in &cfg.groups {
            let gid = format!("g-{}", g.name.trim().to_lowercase());
            groups.retain(|x| x.gid != gid);
            groups.push(Group {
                gid: gid.clone(),
                name: g.name.clone(),
                members: g.members.iter().map(|m| uid_for(m)).collect(),
            });
        }
        write_json(&groups_path, &groups)?;

        Ok(Self {
            users,
            groups,
            users_path,
            groups_path,
            sessions: Mutex::new(HashMap::new()),
        })
    }

    /// 用户本人修改口令（校验旧口令；管理页改他人口令走 update_user）。
    pub fn change_own_password(&mut self, uid: &str, old: &str, new: &str) -> Result<(), String> {
        let user = self
            .users
            .iter()
            .find(|u| u.uid == uid)
            .ok_or("用户不存在")?;
        if !verify_password(old, &user.password_hash) {
            return Err("旧口令错误".into());
        }
        if new.is_empty() {
            return Err("新口令不能为空".into());
        }
        let hash = hash_password(new)?;
        let user = self
            .users
            .iter_mut()
            .find(|u| u.uid == uid)
            .ok_or("用户不存在")?;
        user.password_hash = hash;
        self.save_users()
    }

    /// 登录：校验口令，签发会话 token。
    pub fn login(&self, name: &str, password: &str) -> Result<(String, User), String> {
        let uid = uid_for(name);
        let user = self
            .users
            .iter()
            .find(|u| u.uid == uid)
            .ok_or("用户不存在或口令错误")?;
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
        self.users.iter().find(|u| u.uid == uid).cloned()
    }

    pub fn user_by_uid(&self, uid: &str) -> Option<User> {
        self.users.iter().find(|u| u.uid == uid).cloned()
    }

    /// 用户所属组的 gid 集合。
    pub fn groups_of(&self, uid: &str) -> Vec<String> {
        self.groups
            .iter()
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
