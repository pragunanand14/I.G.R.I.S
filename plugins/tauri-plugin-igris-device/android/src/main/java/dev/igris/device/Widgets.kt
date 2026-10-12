package dev.igris.device

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.widget.RemoteViews
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL
import java.text.DateFormat
import java.util.Date
import java.util.Locale
import kotlin.math.roundToInt

/** What a widget asks IGRIS to do when it opens it (read by [DevicePlugin]). */
internal const val EXTRA_ACTION = "dev.igris.action"
internal const val ACTION_LISTEN = "listen"
internal const val ACTION_WIDGETS = "widgets"
/** "Hey IGRIS" or the assistant gesture: the screen wakes and IGRIS opens as a panel over whatever was on screen. */
internal const val ACTION_ASSISTANT = "assistant"

/** Open IGRIS (its launcher activity), optionally asking it to do something. */
internal fun openIgris(ctx: Context, action: String?, requestCode: Int): PendingIntent? {
    val intent = ctx.packageManager.getLaunchIntentForPackage(ctx.packageName) ?: return null
    intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
    if (action != null) intent.putExtra(EXTRA_ACTION, action)
    return PendingIntent.getActivity(ctx, requestCode, intent, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
}

/** The orb: tap it and IGRIS opens, listening. */
class OrbWidget : AppWidgetProvider() {
    override fun onUpdate(ctx: Context, manager: AppWidgetManager, ids: IntArray) {
        val views = RemoteViews(ctx.packageName, R.layout.igris_widget_orb)
        openIgris(ctx, ACTION_LISTEN, 7101)?.let { views.setOnClickPendingIntent(R.id.igris_orb_root, it) }
        manager.updateAppWidget(ids, views)
    }
}

/** Time and date (TextClock keeps itself current); tap opens IGRIS. */
class ClockWidget : AppWidgetProvider() {
    override fun onUpdate(ctx: Context, manager: AppWidgetManager, ids: IntArray) {
        val views = RemoteViews(ctx.packageName, R.layout.igris_widget_clock)
        openIgris(ctx, null, 7102)?.let { views.setOnClickPendingIntent(R.id.igris_clock_root, it) }
        manager.updateAppWidget(ids, views)
    }
}

/**
 * Weather for the place chosen in IGRIS (Settings → Widgets), from Open-Meteo
 * (free, no account; only the place's coordinates are sent). Refreshes about
 * every 30 minutes; shows the last reading when offline.
 */
class WeatherWidget : AppWidgetProvider() {
    override fun onUpdate(ctx: Context, manager: AppWidgetManager, ids: IntArray) {
        val pending = goAsync()
        Thread {
            try {
                Weather.refresh(ctx.applicationContext)
                render(ctx, manager, ids)
            } finally {
                pending.finish()
            }
        }.start()
    }

    companion object {
        fun render(ctx: Context, manager: AppWidgetManager, ids: IntArray) {
            val views = RemoteViews(ctx.packageName, R.layout.igris_widget_weather)
            val place = Weather.place(ctx)
            val reading = Weather.cached(ctx)
            if (place == null) {
                views.setTextViewText(R.id.igris_weather_icon, "⛅")
                views.setTextViewText(R.id.igris_weather_temp, "—")
                views.setTextViewText(R.id.igris_weather_detail, ctx.getString(R.string.igris_widget_weather_setup))
                views.setTextViewText(R.id.igris_weather_place, "")
            } else if (reading == null) {
                views.setTextViewText(R.id.igris_weather_icon, "⛅")
                views.setTextViewText(R.id.igris_weather_temp, "—")
                views.setTextViewText(R.id.igris_weather_detail, "Couldn't get the weather yet")
                views.setTextViewText(R.id.igris_weather_place, place.name)
            } else {
                views.setTextViewText(R.id.igris_weather_icon, reading.icon)
                views.setTextViewText(R.id.igris_weather_temp, "${reading.temp}°")
                val stale = if (reading.fresh) "" else " · as of ${DateFormat.getTimeInstance(DateFormat.SHORT).format(Date(reading.at))}"
                views.setTextViewText(R.id.igris_weather_detail, "${reading.label} · ${reading.high}° / ${reading.low}°$stale")
                views.setTextViewText(R.id.igris_weather_place, place.name)
            }
            openIgris(ctx, if (place == null) ACTION_WIDGETS else null, 7103)?.let { views.setOnClickPendingIntent(R.id.igris_weather_root, it) }
            manager.updateAppWidget(ids, views)
        }

        /** Fetch and redraw every weather widget (after the place changes). */
        fun refreshAll(ctx: Context) {
            val app = ctx.applicationContext
            Thread {
                Weather.refresh(app)
                val manager = AppWidgetManager.getInstance(app)
                val ids = manager.getAppWidgetIds(ComponentName(app, WeatherWidget::class.java))
                if (ids.isNotEmpty()) render(app, manager, ids)
            }.start()
        }
    }
}

internal object Weather {
    private const val PREFS = "igris_weather"
    /** Readings older than this are shown with their time. */
    private const val FRESH_MS = 90 * 60 * 1000L

    data class Place(val name: String, val lat: Double, val lon: Double)
    data class Reading(val temp: Int, val high: Int, val low: Int, val icon: String, val label: String, val at: Long, val fresh: Boolean)

    fun place(ctx: Context): Place? {
        val p = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val name = p.getString("name", null) ?: return null
        return Place(name, p.getFloat("lat", 0f).toDouble(), p.getFloat("lon", 0f).toDouble())
    }

    fun setPlace(ctx: Context, name: String, lat: Double, lon: Double) {
        ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
            .putString("name", name).putFloat("lat", lat.toFloat()).putFloat("lon", lon.toFloat())
            .remove("reading").apply()
    }

    fun cached(ctx: Context): Reading? {
        val raw = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString("reading", null) ?: return null
        return try {
            val j = JSONObject(raw)
            val at = j.getLong("at")
            Reading(j.getInt("temp"), j.getInt("high"), j.getInt("low"), j.getString("icon"), j.getString("label"), at, System.currentTimeMillis() - at < FRESH_MS)
        } catch (e: Exception) {
            null
        }
    }

    /** Fetch the current weather for the chosen place and keep it. Failures keep the last reading. */
    fun refresh(ctx: Context) {
        val place = place(ctx) ?: return
        val fahrenheit = Locale.getDefault().country in setOf("US", "LR", "MM")
        val url = "https://api.open-meteo.com/v1/forecast?latitude=${place.lat}&longitude=${place.lon}" +
            "&current=temperature_2m,weather_code,is_day&daily=temperature_2m_max,temperature_2m_min&timezone=auto&forecast_days=1" +
            if (fahrenheit) "&temperature_unit=fahrenheit" else ""
        try {
            val conn = URL(url).openConnection() as HttpURLConnection
            conn.connectTimeout = 10000
            conn.readTimeout = 10000
            val body = conn.inputStream.bufferedReader().use { it.readText() }
            conn.disconnect()
            val j = JSONObject(body)
            val current = j.getJSONObject("current")
            val daily = j.getJSONObject("daily")
            val (icon, label) = describe(current.getInt("weather_code"), current.optInt("is_day", 1) == 1)
            val reading = JSONObject()
                .put("temp", current.getDouble("temperature_2m").roundToInt())
                .put("high", daily.getJSONArray("temperature_2m_max").getDouble(0).roundToInt())
                .put("low", daily.getJSONArray("temperature_2m_min").getDouble(0).roundToInt())
                .put("icon", icon).put("label", label).put("at", System.currentTimeMillis())
            ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putString("reading", reading.toString()).apply()
        } catch (e: Exception) {
            // Offline or the service is down: the widget keeps showing the last reading.
        }
    }

    /** WMO weather codes (as Open-Meteo reports them) in plain words. */
    private fun describe(code: Int, day: Boolean): Pair<String, String> = when (code) {
        0 -> (if (day) "☀️" else "🌙") to "Clear"
        1, 2 -> (if (day) "🌤️" else "☁️") to "Partly cloudy"
        3 -> "☁️" to "Cloudy"
        45, 48 -> "🌫️" to "Fog"
        51, 53, 55, 56, 57 -> "🌦️" to "Drizzle"
        61, 63, 65, 66, 67 -> "🌧️" to "Rain"
        71, 73, 75, 77, 85, 86 -> "🌨️" to "Snow"
        80, 81, 82 -> "🌦️" to "Showers"
        95, 96, 99 -> "⛈️" to "Thunderstorm"
        else -> "⛅" to "—"
    }
}
