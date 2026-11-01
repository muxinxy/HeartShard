//! HeartShard 前台服务插件（M3 后台传输，简报 §9.1）。
//!
//! Android 端：`HeartshardServicePlugin`（Kotlin，见 gen/android）经
//! `register_android_plugin` 注册；`start_service`/`stop_service` 控制一个
//! Foreground Service——持续通知使系统在长时间上传/同步期间不回收进程。
//! 桌面端：空实现（无系统回收问题），命令静默成功。

#[cfg(mobile)]
use serde::de::DeserializeOwned;
#[cfg(mobile)]
use tauri::{AppHandle, Runtime};

#[cfg(target_os = "android")]
const PLUGIN_IDENTIFIER: &str = "app.heartshard.desktop";

#[cfg(mobile)]
pub struct HeartshardService<R: Runtime>(tauri::plugin::PluginHandle<R>);

#[cfg(mobile)]
impl<R: Runtime> HeartshardService<R> {
    pub fn start(&self) -> Result<(), String> {
        self.0
            .run_mobile_plugin::<serde_json::Value>("startService", serde_json::Value::Null)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn stop(&self) -> Result<(), String> {
        self.0
            .run_mobile_plugin::<serde_json::Value>("stopService", serde_json::Value::Null)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

/// 初始化移动端插件（gen/android 中的 Kotlin 类）。
#[cfg(mobile)]
pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    api: tauri::plugin::PluginApi<R, C>,
) -> tauri::Result<HeartshardService<R>> {
    let handle = api.register_android_plugin(PLUGIN_IDENTIFIER, "HeartshardServicePlugin")?;
    Ok(HeartshardService(handle))
}

/// 桌面端占位（保持命令签名统一）。
#[cfg(desktop)]
pub struct HeartshardService;

/// 会话持有物：移动端为插件句柄，桌面端为占位。
pub struct MobileService {
    pub(crate) inner: Option<MobileServiceInner>,
}

#[cfg(mobile)]
pub(crate) struct MobileServiceInner(pub(crate) HeartshardService<tauri::Wry>);

#[cfg(desktop)]
pub(crate) struct MobileServiceInner;

impl MobileService {
    pub fn start(&self) -> Result<(), String> {
        #[cfg(mobile)]
        {
            if let Some(inner) = &self.inner {
                return inner.0.start();
            }
        }
        Ok(())
    }

    pub fn stop(&self) -> Result<(), String> {
        #[cfg(mobile)]
        {
            if let Some(inner) = &self.inner {
                return inner.0.stop();
            }
        }
        Ok(())
    }
}
