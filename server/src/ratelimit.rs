//! 登录限流：同一 IP 连续失败达阈值后强制等待（内存态，重启清零）。
//! 家庭规模够用；不做持久化黑名单。

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 客户端 IP 提取器：`into_make_service_with_connect_info` 注入的 extensions。
/// 直接读 extensions 而不用 `Option<ConnectInfo>`（axum 0.8 已不支持）。
pub struct ClientIp(pub Option<IpAddr>);

impl<S> axum::extract::FromRequestParts<S> for ClientIp
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let ip = parts
            .extensions
            .get::<axum::extract::ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip());
        Ok(ClientIp(ip))
    }
}

/// 连续失败次数阈值
const MAX_FAILS: u32 = 5;
/// 达到阈值后的锁定时长
const LOCKOUT: Duration = Duration::from_secs(600);
/// 失败计数窗口（窗口内失败次数累计）
const FAIL_WINDOW: Duration = Duration::from_secs(600);

#[derive(Default, Clone)]
struct Entry {
    fails: u32,
    first_fail: Option<Instant>,
    blocked_until: Option<Instant>,
}

static MAP: Mutex<Option<HashMap<IpAddr, Entry>>> = Mutex::new(None);

fn key(ip: Option<IpAddr>) -> Option<IpAddr> {
    ip
}

/// 登录前检查：被锁则返回剩余等待秒数。
pub fn check(ip: Option<IpAddr>) -> Result<(), u64> {
    let Some(ip) = key(ip) else { return Ok(()) };
    let map = MAP.lock().unwrap();
    if let Some(e) = map.as_ref().and_then(|m| m.get(&ip)) {
        if let Some(until) = e.blocked_until {
            let now = Instant::now();
            if until > now {
                return Err(until.duration_since(now).as_secs().max(1));
            }
        }
    }
    Ok(())
}

/// 登录失败记录；达到阈值进入锁定。
pub fn record_fail(ip: Option<IpAddr>) {
    let Some(ip) = key(ip) else { return };
    let now = Instant::now();
    let mut map = MAP.lock().unwrap();
    let m = map.get_or_insert_with(HashMap::new);
    let e = m.entry(ip).or_default();
    // 窗口过期则重新计数
    if let Some(t) = e.first_fail {
        if now.duration_since(t) > FAIL_WINDOW {
            e.fails = 0;
            e.first_fail = None;
        }
    }
    e.fails += 1;
    if e.first_fail.is_none() {
        e.first_fail = Some(now);
    }
    if e.fails >= MAX_FAILS {
        e.blocked_until = Some(now + LOCKOUT);
        e.fails = 0;
        e.first_fail = None;
    }
}

/// 登录成功清零。
pub fn record_ok(ip: Option<IpAddr>) {
    let Some(ip) = key(ip) else { return };
    let mut map = MAP.lock().unwrap();
    if let Some(m) = map.as_mut() {
        m.remove(&ip);
    }
}
