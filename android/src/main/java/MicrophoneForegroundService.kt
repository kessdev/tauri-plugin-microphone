package app.tauri.microphone

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.IBinder

/**
 * Foreground service of type `microphone` that keeps the recording alive while
 * the app is in the background (Android 10+). It shows a persistent notification
 * as required by the platform.
 */
class MicrophoneForegroundService : Service() {

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    startForeground(NOTIFICATION_ID, buildNotification())
    return START_STICKY
  }

  private fun buildNotification(): Notification {
    val channelId = CHANNEL_ID
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      val channel = NotificationChannel(
        channelId,
        "Microphone recording",
        NotificationManager.IMPORTANCE_LOW
      )
      (getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager)
        .createNotificationChannel(channel)
    }
    val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      Notification.Builder(this, channelId)
    } else {
      @Suppress("DEPRECATION")
      Notification.Builder(this)
    }
    return builder
      .setContentTitle("Recording audio")
      .setContentText("The microphone is recording in the background")
      .setOngoing(true)
      .setSmallIcon(android.R.drawable.ic_btn_speak_now)
      .build()
  }

  companion object {
    private const val CHANNEL_ID = "tauri-microphone"
    private const val NOTIFICATION_ID = 4242

    fun start(context: Context) {
      val intent = Intent(context, MicrophoneForegroundService::class.java)
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        context.startForegroundService(intent)
      } else {
        context.startService(intent)
      }
    }

    fun stop(context: Context) {
      context.stopService(Intent(context, MicrophoneForegroundService::class.java))
    }
  }
}
