package dev.igris.device

import android.Manifest
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
import android.webkit.WebView
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
import app.tauri.PermissionState
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
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
class PlaceArgs {
    var name: String? = null
    var latitude: Double = 0.0
    var longitude: Double = 0.0
}

@InvokeArg
class ScreenArgs {
    var index: Int = -1
    var x: Float = 0f
    var y: Float = 0f
    var long: Boolean = false
    var text: String? = null
    var enter: Boolean = false
    var direction: String? = null
    var action: String? = null
    var show: Boolean = false
}

@InvokeArg
class MessageArgs {
    var id: String? = null
    var text: String? = null
    var sinceMinutes: Int = 180
    var allApps: Boolean = false
    var name: String? = null
    var number: String? = null
    var app: String? = null
}

@InvokeArg
class PlaceAlertArgs {
    var id: Int = 0
    var latitude: Double = 0.0
    var longitude: Double = 0.0
    var radius: Float = 150f
    var enter: Boolean = true
    var title: String? = null
    var text: String? = null
}

@InvokeArg
class WakeArgs {
    var on: Boolean = false
    var chargingOnly: Boolean = false
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
@TauriPlugin(
    permissions = [
        Permission(strings = [Manifest.permission.READ_CONTACTS], alias = "contacts"),
        Permission(strings = [Manifest.permission.CALL_PHONE], alias = "call"),
        Permission(strings = [Manifest.permission.ACCESS_COARSE_LOCATION, Manifest.permission.ACCESS_FINE_LOCATION], alias = "location"),
        Permission(strings = [Manifest.permission.ACCESS_BACKGROUND_LOCATION], alias = "backgroundLocation"),
        Permission(strings = [Manifest.permission.RECORD_AUDIO], alias = "microphone"),
    ]
)
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

    // ── Widgets ──────────────────────────────────────────────────────────

    /** What a widget asked for when it opened IGRIS ("listen"), until IGRIS takes it. */
    @Volatile
    private var launchAction: String? = null

    override fun load(webView: WebView) {
        remember(activity.intent)
        WakeWordService.onWake = { wakeHeard = true }
        // "Hey IGRIS" was on: start it again now that IGRIS is on screen (Android only allows that from the foreground).
        if (WakeWordService.prefs(activity).getBoolean(WakeWordService.KEY_ON, false) &&
            getPermissionState("microphone") == PermissionState.GRANTED && !WakeWordService.running
        ) {
            try {
                WakeWordService.start(activity)
            } catch (e: Exception) {
                android.util.Log.w("IGRIS", "IGRIS_WAKE start failed: ${e.javaClass.simpleName}")
            }
        }
    }

    // While IGRIS is on screen the wake phrase starts listening right here; otherwise it opens IGRIS.
    override fun onResume() {
        WakeWordService.onWake = { wakeHeard = true }
    }

    override fun onPause() {
        WakeWordService.onWake = null
    }

    override fun onNewIntent(intent: Intent) {
        remember(intent)
    }

    private fun remember(intent: Intent?) {
        val action = intent?.getStringExtra(EXTRA_ACTION) ?: return
        intent.removeExtra(EXTRA_ACTION)
        if (action == ACTION_LISTEN || action == ACTION_WIDGETS) launchAction = action
    }

    /** The pending widget action, once. */
    @Command
    fun takeLaunchAction(invoke: Invoke) {
        val ret = JSObject()
        launchAction?.let { ret.put("action", it) }
        launchAction = null
        invoke.resolve(ret)
    }

    /** The weather widget's place (chosen in IGRIS). */
    @Command
    fun setWeatherPlace(invoke: Invoke) {
        val args = invoke.parseArgs(PlaceArgs::class.java)
        val name = args.name?.trim()?.take(80)
        if (name.isNullOrEmpty() || args.latitude !in -90.0..90.0 || args.longitude !in -180.0..180.0) {
            invoke.reject("That place isn't valid.")
            return
        }
        Weather.setPlace(activity, name, args.latitude, args.longitude)
        WeatherWidget.refreshAll(activity)
        invoke.resolve()
    }

    @Command
    fun getWeatherPlace(invoke: Invoke) {
        val ret = JSObject()
        Weather.place(activity)?.let { ret.put("name", it.name) }
        invoke.resolve(ret)
    }

    // ── Screen reading and control (accessibility) ──────────────────────

    private fun screen(invoke: Invoke): IgrisAccessibilityService? {
        val s = IgrisAccessibilityService.instance
        if (s == null) invoke.reject("Screen control is off. Turn it on in IGRIS → Settings → Screen control.")
        return s
    }

    /** Run a screen action, turning failures into a plain message. */
    private fun act(invoke: Invoke, f: (IgrisAccessibilityService) -> Unit) {
        val s = screen(invoke) ?: return
        try {
            f(s)
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject(e.message ?: e.javaClass.simpleName)
        }
    }

    @Command
    fun screenAccess(invoke: Invoke) {
        val ret = JSObject()
        ret.put("enabled", IgrisAccessibilityService.instance != null)
        invoke.resolve(ret)
    }

    /** Android's accessibility settings, where the user turns IGRIS's screen control on or off. */
    @Command
    fun openAccessibilitySettings(invoke: Invoke) {
        try {
            activity.startActivity(Intent(android.provider.Settings.ACTION_ACCESSIBILITY_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject("Couldn't open Android's settings: ${e.javaClass.simpleName}")
        }
    }

    /** What's on screen: the app in front and its visible items (password fields never read). */
    @Command
    fun screenRead(invoke: Invoke) {
        val s = screen(invoke) ?: return
        try {
            val screen = s.read()
            val pm = activity.packageManager
            val app = try { pm.getApplicationLabel(pm.getApplicationInfo(screen.packageName, 0)).toString() } catch (e: Exception) { screen.packageName }
            val items = JSArray()
            for (n in screen.nodes) {
                val o = JSObject()
                o.put("label", n.label)
                o.put("role", n.role)
                o.put("x", n.rect.left)
                o.put("y", n.rect.top)
                o.put("w", n.rect.width())
                o.put("h", n.rect.height())
                o.put("focused", n.info.isFocused)
                items.put(o)
            }
            val ret = JSObject()
            ret.put("package", screen.packageName)
            ret.put("app", app)
            ret.put("width", screen.width)
            ret.put("height", screen.height)
            ret.put("items", items)
            invoke.resolve(ret)
        } catch (e: Exception) {
            invoke.reject(e.message ?: e.javaClass.simpleName)
        }
    }

    @Command
    fun screenTap(invoke: Invoke) {
        val a = invoke.parseArgs(ScreenArgs::class.java)
        act(invoke) { s -> if (a.index >= 0) s.tapNode(a.index, a.long) else s.tap(a.x, a.y, a.long) }
    }

    @Command
    fun screenType(invoke: Invoke) {
        val a = invoke.parseArgs(ScreenArgs::class.java)
        act(invoke) { s -> s.type(a.text ?: "", if (a.index >= 0) a.index else null, a.enter) }
    }

    @Command
    fun screenSwipe(invoke: Invoke) {
        val a = invoke.parseArgs(ScreenArgs::class.java)
        act(invoke) { s -> s.swipe(a.direction ?: "") }
    }

    @Command
    fun screenButton(invoke: Invoke) {
        val a = invoke.parseArgs(ScreenArgs::class.java)
        act(invoke) { s -> s.global(a.action ?: "") }
    }

    /** Show (with a status) or hide the Stop pill. */
    @Command
    fun screenOverlay(invoke: Invoke) {
        val a = invoke.parseArgs(ScreenArgs::class.java)
        val s = IgrisAccessibilityService.instance
        if (a.show) {
            IgrisAccessibilityService.stopRequested = false
            s?.showOverlay(a.text ?: "working")
        } else {
            s?.hideOverlayAsync()
        }
        invoke.resolve()
    }

    /** Whether the user tapped Stop since the last check. */
    @Command
    fun screenTakeStop(invoke: Invoke) {
        val ret = JSObject()
        ret.put("stop", IgrisAccessibilityService.stopRequested)
        IgrisAccessibilityService.stopRequested = false
        invoke.resolve(ret)
    }

    // ── Messages, contacts, calls ───────────────────────────────────────

    @Command
    fun messagesAccess(invoke: Invoke) {
        val enabled = android.provider.Settings.Secure.getString(activity.contentResolver, "enabled_notification_listeners")
            ?.contains(activity.packageName) == true
        // Turned on but not running (common after an update): ask Android to start it.
        if (enabled) IgrisNotificationListener.rebind(activity)
        val ret = JSObject()
        ret.put("enabled", enabled)
        invoke.resolve(ret)
    }

    /** Android's Notification access page, where the user turns IGRIS's message reading on or off. */
    @Command
    fun openNotificationAccessSettings(invoke: Invoke) {
        try {
            activity.startActivity(Intent(android.provider.Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject("Couldn't open Android's settings: ${e.javaClass.simpleName}")
        }
    }

    /** Recent message notifications (one-time codes already hidden). */
    @Command
    fun recentMessages(invoke: Invoke) {
        val a = invoke.parseArgs(MessageArgs::class.java)
        val list = JSArray()
        for (m in IgrisNotificationListener.recent(a.sinceMinutes.coerceIn(1, 1440) * 60_000L, a.allApps)) {
            val o = JSObject()
            o.put("id", m.key)
            o.put("app", m.app)
            o.put("title", m.title)
            o.put("text", m.text)
            o.put("time", m.time)
            o.put("canReply", m.reply != null)
            list.put(o)
        }
        val ret = JSObject()
        ret.put("items", list)
        invoke.resolve(ret)
    }

    @Command
    fun replyMessage(invoke: Invoke) {
        val a = invoke.parseArgs(MessageArgs::class.java)
        try {
            IgrisNotificationListener.reply(activity, a.id ?: "", a.text ?: "")
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject(e.message ?: e.javaClass.simpleName)
        }
    }

    /** Contacts whose name contains `name` (asks for Contacts permission the first time). */
    @Command
    fun findContact(invoke: Invoke) {
        if (getPermissionState("contacts") != PermissionState.GRANTED) {
            requestPermissionForAlias("contacts", invoke, "contactsCallback")
            return
        }
        lookupContacts(invoke)
    }

    @PermissionCallback
    private fun contactsCallback(invoke: Invoke) {
        if (getPermissionState("contacts") == PermissionState.GRANTED) lookupContacts(invoke)
        else invoke.reject("IGRIS isn't allowed to see contacts. Allow it in Android Settings → Apps → IGRIS → Permissions, or give the number.")
    }

    private fun lookupContacts(invoke: Invoke) {
        val name = invoke.parseArgs(MessageArgs::class.java).name?.trim().orEmpty()
        if (name.isEmpty()) {
            invoke.reject("Say whose number to look for.")
            return
        }
        val uri = android.provider.ContactsContract.CommonDataKinds.Phone.CONTENT_URI
        val cols = arrayOf(android.provider.ContactsContract.CommonDataKinds.Phone.DISPLAY_NAME, android.provider.ContactsContract.CommonDataKinds.Phone.NUMBER)
        val found = JSArray()
        val seen = HashSet<String>()
        try {
            activity.contentResolver.query(uri, cols, "${cols[0]} LIKE ?", arrayOf("%$name%"), "${cols[0]} ASC")?.use { c ->
                while (c.moveToNext() && found.length() < 6) {
                    val n = c.getString(0) ?: continue
                    val num = c.getString(1)?.replace(Regex("[^+0-9]"), "") ?: continue
                    if (!seen.add("$n|$num")) continue
                    val o = JSObject()
                    o.put("name", n)
                    o.put("number", num)
                    found.put(o)
                }
            }
            val ret = JSObject()
            ret.put("contacts", found)
            invoke.resolve(ret)
        } catch (e: Exception) {
            invoke.reject("Couldn't read contacts: ${e.javaClass.simpleName}")
        }
    }

    /** Open WhatsApp or the SMS app with the message typed in (the user — or IGRIS with approval — presses send). */
    @Command
    fun startMessage(invoke: Invoke) {
        val a = invoke.parseArgs(MessageArgs::class.java)
        val number = a.number?.replace(Regex("[^+0-9]"), "").orEmpty()
        val text = a.text.orEmpty()
        if (number.length < 5) {
            invoke.reject("That phone number isn't valid.")
            return
        }
        val intent = if (a.app == "whatsapp") {
            Intent(Intent.ACTION_VIEW, android.net.Uri.parse("https://wa.me/${number.trimStart('+')}?text=${android.net.Uri.encode(text)}"))
        } else {
            Intent(Intent.ACTION_SENDTO, android.net.Uri.parse("smsto:$number")).putExtra("sms_body", text)
        }
        try {
            activity.startActivity(intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            invoke.resolve()
        } catch (e: ActivityNotFoundException) {
            invoke.reject(if (a.app == "whatsapp") "WhatsApp isn't installed." else "No messaging app can send this.")
        }
    }

    /** Call a number (asks for the Phone permission the first time; without it, opens the dialer). */
    @Command
    fun callNumber(invoke: Invoke) {
        if (getPermissionState("call") != PermissionState.GRANTED) {
            requestPermissionForAlias("call", invoke, "callCallback")
            return
        }
        placeCall(invoke, true)
    }

    @PermissionCallback
    private fun callCallback(invoke: Invoke) {
        placeCall(invoke, getPermissionState("call") == PermissionState.GRANTED)
    }

    private fun placeCall(invoke: Invoke, direct: Boolean) {
        val number = invoke.parseArgs(MessageArgs::class.java).number?.replace(Regex("[^+0-9]"), "").orEmpty()
        if (number.length < 3) {
            invoke.reject("That phone number isn't valid.")
            return
        }
        val uri = android.net.Uri.parse("tel:$number")
        try {
            activity.startActivity(Intent(if (direct) Intent.ACTION_CALL else Intent.ACTION_DIAL, uri).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            val ret = JSObject()
            ret.put("dialed", !direct)
            invoke.resolve(ret)
        } catch (e: Exception) {
            invoke.reject("Couldn't start the call: ${e.javaClass.simpleName}")
        }
    }

    // ── Location (saved places, location reminders) ─────────────────────

    /** Where the phone is now (asks for location the first time). */
    @Command
    fun currentLocation(invoke: Invoke) {
        if (getPermissionState("location") != PermissionState.GRANTED) {
            requestPermissionForAlias("location", invoke, "locationCallback")
            return
        }
        readLocation(invoke)
    }

    @PermissionCallback
    private fun locationCallback(invoke: Invoke) {
        if (getPermissionState("location") == PermissionState.GRANTED) readLocation(invoke)
        else invoke.reject("IGRIS isn't allowed to use location. Allow it in Android Settings → Apps → IGRIS → Permissions → Location.")
    }

    @Suppress("MissingPermission")
    private fun readLocation(invoke: Invoke) {
        val lm = activity.getSystemService(Context.LOCATION_SERVICE) as android.location.LocationManager
        val done = { loc: android.location.Location? ->
            if (loc == null) {
                invoke.reject("Couldn't get the location right now — is Location turned on?")
            } else {
                val ret = JSObject()
                ret.put("latitude", loc.latitude)
                ret.put("longitude", loc.longitude)
                ret.put("accuracy", loc.accuracy.toDouble())
                invoke.resolve(ret)
            }
        }
        val provider = if (lm.isProviderEnabled(android.location.LocationManager.GPS_PROVIDER)) android.location.LocationManager.GPS_PROVIDER
        else android.location.LocationManager.NETWORK_PROVIDER
        try {
            if (Build.VERSION.SDK_INT >= 30) {
                lm.getCurrentLocation(provider, null, activity.mainExecutor) { done(it) }
            } else {
                done(lm.getLastKnownLocation(provider) ?: lm.getLastKnownLocation(android.location.LocationManager.NETWORK_PROVIDER))
            }
        } catch (e: Exception) {
            invoke.reject("Couldn't get the location: ${e.javaClass.simpleName}")
        }
    }

    /**
     * Watch for arriving at / leaving a place. Needs location "all the time"
     * (Android shows the choice; on Android 11+ in its settings page).
     */
    @Command
    fun addPlaceAlert(invoke: Invoke) {
        when {
            getPermissionState("location") != PermissionState.GRANTED -> requestPermissionForAlias("location", invoke, "placeAlertCallback")
            Build.VERSION.SDK_INT >= 29 && getPermissionState("backgroundLocation") != PermissionState.GRANTED ->
                requestPermissionForAlias("backgroundLocation", invoke, "placeAlertBackgroundCallback")
            else -> placeAlert(invoke)
        }
    }

    @PermissionCallback
    private fun placeAlertCallback(invoke: Invoke) {
        when {
            getPermissionState("location") != PermissionState.GRANTED ->
                invoke.reject("IGRIS isn't allowed to use location, so it can't remind you at a place.")
            // Android asks for "all the time" only after "while using the app".
            Build.VERSION.SDK_INT >= 29 && getPermissionState("backgroundLocation") != PermissionState.GRANTED ->
                requestPermissionForAlias("backgroundLocation", invoke, "placeAlertBackgroundCallback")
            else -> placeAlert(invoke)
        }
    }

    @PermissionCallback
    private fun placeAlertBackgroundCallback(invoke: Invoke) {
        when {
            Build.VERSION.SDK_INT >= 29 && getPermissionState("backgroundLocation") != PermissionState.GRANTED ->
                invoke.reject("Location reminders need location “Allow all the time”: Android Settings → Apps → IGRIS → Permissions → Location → Allow all the time.")
            else -> placeAlert(invoke)
        }
    }

    private fun placeAlert(invoke: Invoke) {
        val a = invoke.parseArgs(PlaceAlertArgs::class.java)
        try {
            PlaceAlerts.add(activity, a.id, a.latitude, a.longitude, a.radius.coerceIn(80f, 2000f), a.enter, a.title ?: "IGRIS · Reminder", a.text ?: "")
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject(e.message ?: e.javaClass.simpleName)
        }
    }

    @Command
    fun removePlaceAlert(invoke: Invoke) {
        PlaceAlerts.remove(activity, invoke.parseArgs(PlaceAlertArgs::class.java).id)
        invoke.resolve()
    }

    /** Whether a place alert has gone off (then IGRIS marks the reminder done). */
    @Command
    fun placeAlertFired(invoke: Invoke) {
        val id = invoke.parseArgs(PlaceAlertArgs::class.java).id
        val prefs = activity.getSharedPreferences("igris_place_alerts", Context.MODE_PRIVATE)
        val ret = JSObject()
        ret.put("fired", prefs.getBoolean("fired_$id", false))
        invoke.resolve(ret)
    }

    // ── "Hey IGRIS" and IGRIS as the phone's assistant ──────────────────

    /** The wake phrase was heard while IGRIS was on screen (IGRIS asks, then it's cleared). */
    @Volatile
    private var wakeHeard = false

    @Command
    fun wakeTakeHeard(invoke: Invoke) {
        val ret = JSObject()
        ret.put("heard", wakeHeard)
        wakeHeard = false
        invoke.resolve(ret)
    }

    private fun assistantHeld(): Boolean {
        if (Build.VERSION.SDK_INT >= 29) {
            val rm = activity.getSystemService(android.app.role.RoleManager::class.java)
            if (rm != null && rm.isRoleAvailable(android.app.role.RoleManager.ROLE_ASSISTANT)) {
                return rm.isRoleHeld(android.app.role.RoleManager.ROLE_ASSISTANT)
            }
        }
        val s = android.provider.Settings.Secure.getString(activity.contentResolver, "assistant") ?: return false
        return s.startsWith(activity.packageName + "/")
    }

    @Command
    fun wakeStatus(invoke: Invoke) {
        val prefs = WakeWordService.prefs(activity)
        val ret = JSObject()
        ret.put("on", prefs.getBoolean(WakeWordService.KEY_ON, false))
        ret.put("chargingOnly", prefs.getBoolean(WakeWordService.KEY_CHARGING_ONLY, false))
        ret.put("running", WakeWordService.running)
        ret.put("listening", WakeWordService.listening)
        WakeWordService.problem?.let { ret.put("problem", it) }
        ret.put("canOpenItself", Build.VERSION.SDK_INT < 23 || android.provider.Settings.canDrawOverlays(activity))
        val pm = activity.getSystemService(Context.POWER_SERVICE) as android.os.PowerManager
        ret.put("batteryUnrestricted", Build.VERSION.SDK_INT < 23 || pm.isIgnoringBatteryOptimizations(activity.packageName))
        ret.put("assistant", assistantHeld())
        invoke.resolve(ret)
    }

    /** Turn "Hey IGRIS" on or off (asks for the microphone the first time). */
    @Command
    fun wakeSet(invoke: Invoke) {
        val a = invoke.parseArgs(WakeArgs::class.java)
        WakeWordService.prefs(activity).edit()
            .putBoolean(WakeWordService.KEY_ON, a.on)
            .putBoolean(WakeWordService.KEY_CHARGING_ONLY, a.chargingOnly)
            .apply()
        if (!a.on) {
            WakeWordService.stop(activity)
            invoke.resolve()
        } else if (getPermissionState("microphone") != PermissionState.GRANTED) {
            requestPermissionForAlias("microphone", invoke, "wakeMicCallback")
        } else {
            startWake(invoke)
        }
    }

    @PermissionCallback
    private fun wakeMicCallback(invoke: Invoke) {
        if (getPermissionState("microphone") == PermissionState.GRANTED) {
            startWake(invoke)
        } else {
            WakeWordService.prefs(activity).edit().putBoolean(WakeWordService.KEY_ON, false).apply()
            invoke.reject("IGRIS needs the microphone to hear “Hey IGRIS”. Allow it in Android Settings → Apps → IGRIS → Permissions.")
        }
    }

    private fun startWake(invoke: Invoke) {
        try {
            if (WakeWordService.running) WakeWordService.hold("settings", false) else WakeWordService.start(activity)
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject("Couldn't start listening: ${e.javaClass.simpleName}")
        }
    }

    /** IGRIS is using the microphone itself (recording or speaking): the wake listener lets go meanwhile. */
    @Command
    fun wakeHold(invoke: Invoke) {
        val a = invoke.parseArgs(WakeArgs::class.java)
        WakeWordService.hold("app", a.on)
        if (!a.on) WakeWordService.hold("wake", false)
        invoke.resolve()
    }

    private fun openSettings(invoke: Invoke, vararg intents: Intent) {
        for (i in intents) {
            try {
                activity.startActivity(i.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
                invoke.resolve()
                return
            } catch (e: Exception) {
                // Not on this phone: try the next page.
            }
        }
        invoke.reject("Couldn't open Android's settings.")
    }

    /** "Display over other apps": lets IGRIS open itself when it hears its name. */
    @Command
    fun openOverlaySettings(invoke: Invoke) {
        val uri = android.net.Uri.parse("package:" + activity.packageName)
        openSettings(invoke, Intent(android.provider.Settings.ACTION_MANAGE_OVERLAY_PERMISSION, uri), Intent(android.provider.Settings.ACTION_APPLICATION_DETAILS_SETTINGS, uri))
    }

    /** Ask Android not to stop IGRIS to save battery (otherwise Samsung phones end "Hey IGRIS" after a while). */
    @Command
    fun openBatterySettings(invoke: Invoke) {
        val uri = android.net.Uri.parse("package:" + activity.packageName)
        openSettings(
            invoke,
            Intent(android.provider.Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, uri),
            Intent(android.provider.Settings.ACTION_APPLICATION_DETAILS_SETTINGS, uri),
        )
    }

    /** Where the user picks the digital assistant app (IGRIS shows there). */
    @Command
    fun openAssistantSettings(invoke: Invoke) {
        openSettings(
            invoke,
            Intent(android.provider.Settings.ACTION_VOICE_INPUT_SETTINGS),
            Intent("android.settings.MANAGE_DEFAULT_APPS_SETTINGS"),
            Intent(android.provider.Settings.ACTION_SETTINGS),
        )
    }
}
