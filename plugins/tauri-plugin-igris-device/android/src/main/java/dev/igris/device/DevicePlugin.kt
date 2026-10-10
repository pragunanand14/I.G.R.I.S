package dev.igris.device

import android.app.Activity
import android.app.SearchManager
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.media.AudioManager
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.os.BatteryManager
import android.os.Build
import android.provider.MediaStore
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import android.view.KeyEvent
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
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
class MediaArgs {
    var action: String? = null
}

@InvokeArg
class PlayArgs {
    var query: String? = null
    var packageName: String? = null
}

@InvokeArg
class SealArgs {
    var data: String? = null
}

/** Alias of the Android Keystore key that seals IGRIS's device identity keys. */
private const val KEY_ALIAS = "igris-device-identity"
private const val GCM_IV_BYTES = 12
/** Larger than any settings file; IGRIS refuses files this big. */
private const val MAX_PICKED_BYTES = 64 * 1024 + 1

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

    /**
     * Let the user pick a file with Android's file picker and return its bytes
     * (base64), at most [MAX_PICKED_BYTES]. `{}` when they cancel. Used to
     * import IGRIS's settings file; the bytes go only to IGRIS's Rust side.
     */
    @Command
    fun pickFile(invoke: Invoke) {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("*/*")
        try {
            startActivityForResult(invoke, intent, "pickFileResult")
        } catch (e: Exception) {
            invoke.reject("Couldn't open the file picker: ${e.javaClass.simpleName}")
        }
    }

    @ActivityCallback
    fun pickFileResult(invoke: Invoke, result: ActivityResult) {
        val uri = result.data?.data
        if (result.resultCode != Activity.RESULT_OK || uri == null) {
            invoke.resolve(JSObject())
            return
        }
        try {
            val bytes = activity.contentResolver.openInputStream(uri)?.use { input ->
                val buf = ByteArray(MAX_PICKED_BYTES)
                var n = 0
                while (n < buf.size) {
                    val r = input.read(buf, n, buf.size - n)
                    if (r < 0) break
                    n += r
                }
                buf.copyOf(n).also { buf.fill(0) }
            }
            if (bytes == null) {
                invoke.reject("Couldn't open that file.")
                return
            }
            val ret = JSObject()
            ret.put("data", Base64.encodeToString(bytes, Base64.NO_WRAP))
            bytes.fill(0)
            invoke.resolve(ret)
        } catch (e: Exception) {
            invoke.reject("Couldn't read that file: ${e.javaClass.simpleName}")
        }
    }

    private fun audio(): AudioManager = activity.getSystemService(Context.AUDIO_SERVICE) as AudioManager

    /** Media buttons (whatever app is playing) and the music volume. */
    @Command
    fun mediaControl(invoke: Invoke) {
        val am = audio()
        val volume = when (invoke.parseArgs(MediaArgs::class.java).action) {
            "volume_up" -> AudioManager.ADJUST_RAISE
            "volume_down" -> AudioManager.ADJUST_LOWER
            "mute" -> AudioManager.ADJUST_TOGGLE_MUTE
            else -> null
        }
        try {
            if (volume != null) {
                am.adjustStreamVolume(AudioManager.STREAM_MUSIC, volume, AudioManager.FLAG_SHOW_UI)
                invoke.resolve()
                return
            }
            val code = when (invoke.parseArgs(MediaArgs::class.java).action) {
                "play_pause" -> KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE
                "play" -> KeyEvent.KEYCODE_MEDIA_PLAY
                "pause" -> KeyEvent.KEYCODE_MEDIA_PAUSE
                "next" -> KeyEvent.KEYCODE_MEDIA_NEXT
                "previous" -> KeyEvent.KEYCODE_MEDIA_PREVIOUS
                "stop" -> KeyEvent.KEYCODE_MEDIA_STOP
                else -> {
                    invoke.reject("Unknown media action.")
                    return
                }
            }
            am.dispatchMediaKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, code))
            am.dispatchMediaKeyEvent(KeyEvent(KeyEvent.ACTION_UP, code))
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject("Couldn't control the music: ${e.javaClass.simpleName}")
        }
    }

    /** Whether music is playing (Android doesn't share titles without notification access). */
    @Command
    fun mediaState(invoke: Invoke) {
        val ret = JSObject()
        ret.put("playing", audio().isMusicActive)
        invoke.resolve(ret)
    }

    /** Ask a music app to play a search ("play X"); returns the app's name. */
    @Command
    fun playFromSearch(invoke: Invoke) {
        val args = invoke.parseArgs(PlayArgs::class.java)
        val query = args.query?.trim()
        if (query.isNullOrEmpty()) {
            invoke.reject("Say what to play.")
            return
        }
        val intent = Intent(MediaStore.INTENT_ACTION_MEDIA_PLAY_FROM_SEARCH)
            .putExtra(SearchManager.QUERY, query)
            .putExtra(MediaStore.EXTRA_MEDIA_FOCUS, "vnd.android.cursor.item/*")
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        args.packageName?.takeIf { it.isNotBlank() }?.let { intent.setPackage(it) }
        try {
            activity.startActivity(intent)
            val pm = activity.packageManager
            val name = args.packageName?.let { pkg ->
                try { pm.getApplicationLabel(pm.getApplicationInfo(pkg, 0)).toString() } catch (e: Exception) { null }
            } ?: "the music app"
            val ret = JSObject()
            ret.put("app", name)
            invoke.resolve(ret)
        } catch (e: ActivityNotFoundException) {
            invoke.reject(if (args.packageName.isNullOrBlank()) "No music app on the phone can play a search." else "That music app isn't installed on the phone.")
        } catch (e: Exception) {
            invoke.reject("Couldn't start the music: ${e.javaClass.simpleName}")
        }
    }
}
