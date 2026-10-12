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

    /**
     * Android forgets proximity alerts when the phone restarts. Re-add every active location reminder
     * from IGRIS's database (at boot, and when IGRIS starts). Without location permission nothing is added.
     */
    fun restoreAll(ctx: Context) {
        val db = java.io.File(ctx.dataDir, "igris.db")
        if (!db.exists()) return
        val fired = ctx.getSharedPreferences("igris_place_alerts", Context.MODE_PRIVATE)
        var n = 0
        try {
            android.database.sqlite.SQLiteDatabase.openDatabase(db.path, null, android.database.sqlite.SQLiteDatabase.OPEN_READONLY).use { d ->
                d.rawQuery(
                    "SELECT r.id, r.on_arrive, r.text, p.latitude, p.longitude FROM place_reminders r JOIN places p ON p.name = r.place WHERE r.status = 'active'",
                    null,
                ).use { c ->
                    while (c.moveToNext()) {
                        val id = c.getInt(0)
                        if (fired.getBoolean("fired_$id", false)) continue
                        try {
                            add(ctx, id, c.getDouble(3), c.getDouble(4), 150f, c.getInt(1) != 0, "IGRIS · Reminder", c.getString(2) ?: "")
                            n++
                        } catch (e: SecurityException) {
                            return
                        }
                    }
                }
            }
            android.util.Log.i("IGRIS", "IGRIS_PLACES restored $n")
        } catch (e: Exception) {
            android.util.Log.w("IGRIS", "IGRIS_PLACES restore failed: ${e.javaClass.simpleName}")
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

/** After the phone restarts: put the location reminders back. */
class PlacesBootReceiver : BroadcastReceiver() {
    override fun onReceive(ctx: Context, intent: Intent) {
        if (intent.action == Intent.ACTION_BOOT_COMPLETED || intent.action == "android.intent.action.QUICKBOOT_POWERON") {
            PlaceAlerts.restoreAll(ctx)
        }
    }
}
