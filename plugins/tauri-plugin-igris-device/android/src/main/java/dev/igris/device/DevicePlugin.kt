package dev.igris.device

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.os.BatteryManager
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

@InvokeArg
class LaunchArgs {
    var packageName: String? = null
}

@InvokeArg
class SealArgs {
    var data: String? = null
}

/** Alias of the Android Keystore key that seals IGRIS's device identity keys. */
private const val KEY_ALIAS = "igris-device-identity"
private const val GCM_IV_BYTES = 12

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

    /**
     * The AES key in the Android Keystore that seals IGRIS's device keys. It is
     * created on first use, never leaves the Keystore (hardware-backed where the
     * phone has it), and can't be exported.
     */
    private fun sealingKey(): SecretKey {
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (ks.getEntry(KEY_ALIAS, null) as? KeyStore.SecretKeyEntry)?.let { return it.secretKey }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        gen.init(
            KeyGenParameterSpec.Builder(KEY_ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build()
        )
        return gen.generateKey()
    }

    /** Seal bytes (base64) with the Keystore key: returns base64(iv ‖ AES-GCM ciphertext). */
    @Command
    fun sealKeys(invoke: Invoke) {
        try {
            val plain = Base64.decode(invoke.parseArgs(SealArgs::class.java).data ?: "", Base64.NO_WRAP)
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.ENCRYPT_MODE, sealingKey())
            val sealed = cipher.iv + cipher.doFinal(plain)
            plain.fill(0)
            val ret = JSObject()
            ret.put("data", Base64.encodeToString(sealed, Base64.NO_WRAP))
            invoke.resolve(ret)
        } catch (e: Exception) {
            invoke.reject("Couldn't protect the device keys: ${e.javaClass.simpleName}")
        }
    }

    /** Open bytes sealed by [sealKeys]. */
    @Command
    fun unsealKeys(invoke: Invoke) {
        try {
            val sealed = Base64.decode(invoke.parseArgs(SealArgs::class.java).data ?: "", Base64.NO_WRAP)
            if (sealed.size <= GCM_IV_BYTES) {
                invoke.reject("The stored device keys are corrupt.")
                return
            }
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, sealingKey(), GCMParameterSpec(128, sealed, 0, GCM_IV_BYTES))
            val plain = cipher.doFinal(sealed, GCM_IV_BYTES, sealed.size - GCM_IV_BYTES)
            val ret = JSObject()
            ret.put("data", Base64.encodeToString(plain, Base64.NO_WRAP))
            plain.fill(0)
            invoke.resolve(ret)
        } catch (e: Exception) {
            invoke.reject("Couldn't unlock the device keys: ${e.javaClass.simpleName}")
        }
    }
}
