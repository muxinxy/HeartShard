// HeartShard M3 后台传输：前台服务。持续通知使系统在长时间上传/同步期间
// 不回收进程（简报 §9.1 "移动端上传不被系统回收"）。
package app.heartshard.desktop

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.IBinder

class UploadForegroundService : Service() {

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    startForeground(NOTIFICATION_ID, buildNotification())
    return START_STICKY
  }

  private fun buildNotification(): Notification {
    val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      manager.createNotificationChannel(
        NotificationChannel(CHANNEL_ID, "HeartShard 后台传输", NotificationManager.IMPORTANCE_LOW)
      )
    }
    val builder =
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        Notification.Builder(this, CHANNEL_ID)
      } else {
        @Suppress("DEPRECATION")
        Notification.Builder(this)
      }
    return builder
      .setContentTitle("HeartShard · 牧心相册")
      .setContentText("后台传输进行中，系统不会回收本进程")
      .setSmallIcon(android.R.drawable.stat_sys_upload)
      .setOngoing(true)
      .build()
  }

  companion object {
    private const val CHANNEL_ID = "heartshard_transfer"
    private const val NOTIFICATION_ID = 0x4853 // "HS"
  }
}
