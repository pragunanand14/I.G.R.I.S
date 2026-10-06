package dev.igris.device

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.os.BatteryManager
import android.os.Build
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
class LaunchArgs {
    var packageName: String? = null
}

/**
 * IGRIS's Android device capabilities. Called only from IGRIS's Rust tools
 * (no webview commands), so every use goes through IGRIS's permission checks.
 */
@TauriPlugin
class DevicePlugin(private val activity: Activity) : Plugin(activity) {

    /** Apps the launcher can open: label and package name. */
    @Command
    fun listApps(invoke: Invoke) {
        try {
            val pm = activity.packageManager
            val launcher = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER)
            val seen = HashSet<String>()
            val apps = JSArray()
            for (info in pm.queryIntentActivities(launcher, 0)) {
                val pkg = info.activityInfo?.packageName ?: continue
                if (pkg == activity.packageName || !seen.add(pkg)) continue
                val app = JSObject()
                app.put("label", info.loadLabel(pm)?.toString() ?: pkg)
                app.put("package", pkg)
                apps.put(app)
            }
            val ret = JSObject()
            ret.put("apps", apps)
            invoke.resolve(ret)
        } catch (e: Exception) {
            invoke.reject("Couldn't list the phone's apps: ${e.message}")
        }
    }

    /** Open an app by package name through its launcher entry. */
    @Command
    fun launchApp(invoke: Invoke) {
        val args = invoke.parseArgs(LaunchArgs::class.java)
        val pkg = args.packageName
        if (pkg.isNullOrBlank()) {
            invoke.reject("No app was given.")
            return
        }
        val intent = activity.packageManager.getLaunchIntentForPackage(pkg)
        if (intent == null) {
            invoke.reject("$pkg isn't installed or can't be opened.")
            return
        }
        try {
            intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            activity.startActivity(intent)
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject("Couldn't open $pkg: ${e.message}")
        }
    }

    /** Battery, network and device facts; anything the phone can't report is left out. */
    @Command
    fun status(invoke: Invoke) {
        val ret = JSObject()
        ret.put("manufacturer", Build.MANUFACTURER)
        ret.put("model", Build.MODEL)
        ret.put("androidVersion", Build.VERSION.RELEASE)
        ret.put("sdkInt", Build.VERSION.SDK_INT)

        val battery = activity.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))
        if (battery != null) {
            val level = battery.getIntExtra(BatteryManager.EXTRA_LEVEL, -1)
            val scale = battery.getIntExtra(BatteryManager.EXTRA_SCALE, -1)
            if (level >= 0 && scale > 0) {
                ret.put("batteryPercent", level * 100.0 / scale)
            }
            val state = battery.getIntExtra(BatteryManager.EXTRA_STATUS, -1)
            if (state != -1) {
                ret.put(
                    "charging",
                    state == BatteryManager.BATTERY_STATUS_CHARGING || state == BatteryManager.BATTERY_STATUS_FULL
                )
            }
        }

        val cm = activity.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager
        if (cm != null) {
            val caps = cm.getNetworkCapabilities(cm.activeNetwork)
            val network = when {
                caps == null -> "none"
                caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> "wifi"
                caps.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> "cellular"
                caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> "ethernet"
                else -> "other"
            }
            ret.put("network", network)
        }
        invoke.resolve(ret)
    }
}
