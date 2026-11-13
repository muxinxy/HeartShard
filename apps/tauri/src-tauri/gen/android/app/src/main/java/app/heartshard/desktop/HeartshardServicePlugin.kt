// HeartShard M3 后台传输：Tauri 移动插件（Kotlin 侧）。
// 由 Rust 侧 service_plugin::init 经 register_android_plugin 注册。
package app.heartshard.desktop

import android.app.Activity
import android.content.Intent
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

@TauriPlugin
class HeartshardServicePlugin(private val activity: Activity) : Plugin(activity) {

  @Command
  fun startService(invoke: Invoke) {
    val intent = Intent(activity, UploadForegroundService::class.java)
    activity.startForegroundService(intent)
    invoke.resolve()
  }

  @Command
  fun stopService(invoke: Invoke) {
    activity.stopService(Intent(activity, UploadForegroundService::class.java))
    invoke.resolve()
  }
}
