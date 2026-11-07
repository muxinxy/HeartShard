//! 相册 ACL：用户/组 × 相册节点，沿树继承、显式覆盖。
//!
//! 权限级别 none < view < upload < manage。
//! 有效权限 = 沿相册树向根找最近的显式设置；同节点用户显式 > 组显式（取组中
//! 最高权限）；无任何设置 → none（admin 恒为 manage）。

use crate::auth::User;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Perm {
    None,
    View,
    Upload,
    Manage,
}

impl Perm {
    pub fn parse(s: &str) -> Option<Perm> {
        match s {
            "none" => Some(Perm::None),
            "view" => Some(Perm::View),
            "upload" => Some(Perm::Upload),
            "manage" => Some(Perm::Manage),
            _ => None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Principal {
    /// user | group
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AclEntry {
    pub album_id: String,
    pub principal: Principal,
    pub perm: Perm,
}

pub struct AclStore {
    pub entries: Vec<AclEntry>,
    path: std::path::PathBuf,
}

impl AclStore {
    pub fn load(root: &Path) -> Self {
        let path = root.join("acls.json");
        let entries: Vec<AclEntry> =
            std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Self { entries, path }
    }

    pub fn save(&self) -> Result<(), String> {
        let json = serde_json::to_vec_pretty(&self.entries).map_err(|e| e.to_string())?;
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, &json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }

    pub fn set(&mut self, album_id: &str, principal: Principal, perm: Perm) -> Result<(), String> {
        self.entries.retain(|e| {
            !(e.album_id == album_id
                && e.principal.kind == principal.kind
                && e.principal.id == principal.id)
        });
        self.entries.push(AclEntry {
            album_id: album_id.to_string(),
            principal,
            perm,
        });
        self.save()
    }

    pub fn unset(&mut self, album_id: &str, principal: &Principal) -> Result<(), String> {
        self.entries.retain(|e| {
            !(e.album_id == album_id
                && e.principal.kind == principal.kind
                && e.principal.id == principal.id)
        });
        self.save()
    }

    pub fn for_album(&self, album_id: &str) -> Vec<&AclEntry> {
        self.entries.iter().filter(|e| e.album_id == album_id).collect()
    }

    /// 用户在某相册节点的显式权限：**用户显式优先**；否则组取最高（组用于放权）。
    /// `group_gids`: 用户所属组 id 集合。
    fn explicit_at(&self, album_id: &str, user: &User, group_gids: &[String]) -> Option<Perm> {
        let entries = self.for_album(album_id);
        // 用户显式条目存在 → 直接生效（可用于收权，如 eve 设 none）
        if let Some(e) = entries
            .iter()
            .find(|e| e.principal.kind == "user" && e.principal.id == user.uid)
        {
            return Some(e.perm);
        }
        // 组条目取最高
        let mut best: Option<Perm> = None;
        for e in entries {
            if e.principal.kind == "group" && group_gids.iter().any(|g| g == &e.principal.id) {
                if best.map_or(true, |b| e.perm > b) {
                    best = Some(e.perm);
                }
            }
        }
        best
    }

    /// 有效权限：沿相册树向上取最近的显式设置。
    ///
    /// `parent_of`: album_id → 父相册 id（来自 HeartCore 相册树）。
    pub fn effective(
        &self,
        user: &User,
        group_gids: &[String],
        album_id: &str,
        parent_of: &HashMap<String, Option<String>>,
    ) -> Perm {
        if user.is_admin {
            return Perm::Manage;
        }
        let mut cur = Some(album_id.to_string());
        while let Some(id) = cur {
            if let Some(p) = self.explicit_at(&id, user, group_gids) {
                return p;
            }
            cur = parent_of.get(&id).cloned().flatten();
        }
        Perm::None
    }
}
