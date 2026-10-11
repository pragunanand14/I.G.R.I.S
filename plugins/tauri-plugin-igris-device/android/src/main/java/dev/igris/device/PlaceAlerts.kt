package dev.igris.device

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.location.LocationManager
import android.os.Build
import androidx.core.app.NotificationCompat

/**
 * Location reminders: Android watches for the user arriving at or leaving a
 * saved place (a proximity alert — no polling, works with IGRIS closed) and
 * this receiver shows the reminder, once.
 */
object PlaceAlerts {
    private const val ACTION = "dev.igris.PLACE_ALERT"
    private const val CHANNEL = "reminders"

    private fun intent(ctx: Context, id: Int, enter: Boolean, title: String, text: String): PendingIntent {
        val i = Intent(ctx, PlaceAlertReceiver::class.java)
            .setAction("$ACTION.$id")
            .putExtra("id", id)
            .putExtra("enter", enter)
            .putExtra("title", title)
            .putExtra("text", text)
        // Mutable: Android adds whether the user is entering or leaving.
        return PendingIntent.getBroadcast(ctx, 9000 + id, i, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_MUTABLE)
    }

    @Suppress("MissingPermission")
    fun add(ctx: Context, id: Int, lat: Double, lon: Double, radius: Float, enter: Boolean, title: String, text: String) {
        if (ctx.checkSelfPermission(Manifest.permission.ACCESS_FINE_LOCATION) != PackageManager.PERMISSION_GRANTED) {
            throw SecurityException("IGRIS isn't allowed to use location.")
        }
        val lm = ctx.getSystemService(Context.LOCATION_SERVICE) as LocationManager
        lm.addProximityAlert(lat, lon, radius, -1, intent(ctx, id, enter, title, text))
    }

    fun remove(ctx: Context, id: Int) {
        val lm = ctx.getSystemService(Context.LOCATION_SERVICE) as LocationManager
        try {
            lm.removeProximityAlert(intent(ctx, id, true, "", ""))
        } catch (e: SecurityException) {
            // Permission was withdrawn: nothing is being watched anyway.
        }
    }

    fun notify(ctx: Context, id: Int, title: String, text: String) {
        val nm = ctx.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (Build.VERSION.SDK_INT >= 26 && nm.getNotificationChannel(CHANNEL) == null) {
            nm.createNotificationChannel(NotificationChannel(CHANNEL, "Reminders and timers", NotificationManager.IMPORTANCE_HIGH))
        }
        val open = ctx.packageManager.getLaunchIntentForPackage(ctx.packageName)?.let {
            PendingIntent.getActivity(ctx, 9500 + id, it, PendingIntent.FLAG_IMMUTABLE)
        }
        val n = NotificationCompat.Builder(ctx, CHANNEL)
            .setSmallIcon(ctx.applicationInfo.icon)
            .setContentTitle(title)
            .setContentText(text)
            .setStyle(NotificationCompat.BigTextStyle().bigText(text))
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setAutoCancel(true)
            .apply { if (open != null) setContentIntent(open) }
            .build()
        nm.notify(20000 + id, n)
    }
}

class PlaceAlertReceiver : BroadcastReceiver() {
    override fun onReceive(ctx: Context, intent: Intent) {
        val entering = intent.getBooleanExtra(LocationManager.KEY_PROXIMITY_ENTERING, false)
        val wantEnter = intent.getBooleanExtra("enter", true)
        if (entering != wantEnter) return
        val id = intent.getIntExtra("id", 0)
        PlaceAlerts.notify(ctx, id, intent.getStringExtra("title") ?: "IGRIS · Reminder", intent.getStringExtra("text") ?: "")
        // One-shot: the reminder is done.
        PlaceAlerts.remove(ctx, id)
        ctx.getSharedPreferences("igris_place_alerts", Context.MODE_PRIVATE).edit().putBoolean("fired_$id", true).apply()
    }
}
