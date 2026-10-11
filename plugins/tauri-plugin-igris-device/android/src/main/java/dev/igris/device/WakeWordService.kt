package dev.igris.device

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.ServiceInfo
import android.os.BatteryManager
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.util.Log
import androidx.core.app.NotificationCompat
import org.json.JSONObject
import org.vosk.LibVosk
import org.vosk.LogLevel
import org.vosk.Model
import org.vosk.Recognizer
import org.vosk.android.RecognitionListener
import org.vosk.android.SpeechService
import org.vosk.android.StorageService

/**
 * "Hey IGRIS" / "Wake up IGRIS" without the screen on IGRIS.
 *
 * Phones keep the low-power hotword chip for the phone maker's own assistant
 * ("Hey Google", Bixby), so an app can only listen with the normal
 * microphone. This service does that, and recognises the phrase **on the
 * phone** (Vosk, an offline recogniser limited to a handful of words): no
 * sound leaves the phone until IGRIS hears its name and opens. It records
 * as a voice-recognition source, so music keeps playing at full volume.
 *
 * It runs as a foreground service (Android shows its notification) that the
 * user turns on in IGRIS; optionally only while charging.
 */
class WakeWordService : Service(), RecognitionListener {
    companion object {
        private const val TAG = "IGRIS"
        const val PREFS = "igris_wake"
        const val KEY_ON = "on"
        const val KEY_CHARGING_ONLY = "charging_only"
        private const val CHANNEL = "wake"
        private const val NOTE_ID = 7301
        private const val HEARD_ID = 7302

        /** The words the recogniser may hear: "igris" isn't an English word, so its nearest sounds stand in for it. */
        private const val GRAMMAR =
            "[\"hey egress\", \"hey agrees\", \"hey degrees\", \"hey he agrees\", \"hey a grease\", " +
                "\"wake up egress\", \"wake up agrees\", \"wake up degrees\", \"wake up he agrees\", \"wake up a grease\", \"[unk]\"]"
        private val WAKE = Regex("\\b(hey|wake up)\\s+(he\\s+|a\\s+)?(egress|agrees|degrees|grease)\\b")

        /** Whether a recognised phrase is the wake phrase ("hey igris" often comes out as "he agrees"). */
        fun matches(text: String): Boolean {
            val t = text.lowercase().trim()
            return WAKE.containsMatchIn(t) || t == "he agrees"
        }

        @Volatile
        var running = false
            private set

        @Volatile
        var listening = false
            private set

        @Volatile
        var problem: String? = null
            private set

        /** Set while IGRIS is open: the wake phrase starts listening there instead of opening the app. */
        @Volatile
        var onWake: (() -> Unit)? = null

        private val holds = HashSet<String>()

        @Volatile
        private var self: WakeWordService? = null

        fun prefs(ctx: Context) = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

        fun start(ctx: Context) {
            val i = Intent(ctx, WakeWordService::class.java)
            if (Build.VERSION.SDK_INT >= 26) ctx.startForegroundService(i) else ctx.startService(i)
        }

        fun stop(ctx: Context) {
            ctx.stopService(Intent(ctx, WakeWordService::class.java))
        }

        /**
         * Stop listening for a while: "app" while IGRIS itself records or speaks
         * (one app can't share the microphone with itself), "wake" right after
         * the phrase was heard. Listening resumes when no hold is left.
         */
        fun hold(reason: String, on: Boolean) {
            synchronized(holds) { if (on) holds.add(reason) else holds.remove(reason) }
            self?.let { s -> s.main.post { s.apply() } }
        }

        private fun held(): Boolean = synchronized(holds) { holds.isNotEmpty() }
    }

    private val main = Handler(Looper.getMainLooper())
    private var model: Model? = null
    private var speech: SpeechService? = null
    private var power: BroadcastReceiver? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        self = this
        running = true
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        try {
            if (Build.VERSION.SDK_INT >= 29) {
                startForeground(NOTE_ID, note(), ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE)
            } else {
                startForeground(NOTE_ID, note())
            }
        } catch (e: Exception) {
            // Android refuses a microphone service started while IGRIS isn't on screen (e.g. after a restart).
            problem = "Open IGRIS once to turn “Hey IGRIS” back on."
            Log.w(TAG, "IGRIS_WAKE couldn't start: ${e.javaClass.simpleName}")
            stopSelf()
            return START_NOT_STICKY
        }
        if (power == null) {
            power = object : BroadcastReceiver() {
                override fun onReceive(c: Context, i: Intent) = apply()
            }
            val f = IntentFilter().apply {
                addAction(Intent.ACTION_POWER_CONNECTED)
                addAction(Intent.ACTION_POWER_DISCONNECTED)
            }
            if (Build.VERSION.SDK_INT >= 33) registerReceiver(power, f, Context.RECEIVER_NOT_EXPORTED) else registerReceiver(power, f)
        }
        if (model == null) load()
        return START_STICKY
    }

    private fun load() {
        LibVosk.setLogLevel(LogLevel.WARNINGS)
        // The model ships inside the app; Vosk copies it to app storage on first use.
        StorageService.unpack(
            this, "model-en-us", "wake-model",
            { m ->
                model = m
                problem = null
                Log.i(TAG, "IGRIS_WAKE ready")
                apply()
            },
            { e ->
                problem = "The wake-word model couldn't load: ${e.message}"
                Log.w(TAG, "IGRIS_WAKE model failed: ${e.message}")
            },
        )
    }

    private fun charging(): Boolean {
        val b = registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED)) ?: return false
        return b.getIntExtra(BatteryManager.EXTRA_PLUGGED, 0) != 0
    }

    /** Listen exactly when it should: model loaded, nothing holding the mic, and (if chosen) charging. */
    private fun apply() {
        val chargingOnly = prefs(this).getBoolean(KEY_CHARGING_ONLY, false)
        val m = model
        val want = m != null && !held() && (!chargingOnly || charging())
        if (want && speech == null && m != null) {
            try {
                val s = SpeechService(Recognizer(m, 16000f, GRAMMAR), 16000f)
                s.startListening(this)
                speech = s
                listening = true
                Log.i(TAG, "IGRIS_WAKE listening")
            } catch (e: Exception) {
                problem = "Couldn't use the microphone: ${e.message}"
                Log.w(TAG, "IGRIS_WAKE mic failed: ${e.message}")
            }
        } else if (!want && speech != null) {
            speech?.stop()
            speech?.shutdown()
            speech = null
            listening = false
            Log.i(TAG, "IGRIS_WAKE paused")
        }
        getSystemService(NotificationManager::class.java)?.notify(NOTE_ID, note())
    }

    private fun heard(json: String?) {
        val text = try {
            JSONObject(json ?: return).optString("text")
        } catch (e: Exception) {
            return
        }
        if (!matches(text)) return
        Log.i(TAG, "IGRIS_WAKE heard")
        // Let go of the microphone so IGRIS can listen to the request; back after a short while if nothing does.
        hold("wake", true)
        main.postDelayed({ hold("wake", false) }, 20_000)
        val inApp = onWake
        if (inApp != null) {
            inApp()
            return
        }
        val open = packageManager.getLaunchIntentForPackage(packageName)?.apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT)
            putExtra(EXTRA_ACTION, ACTION_LISTEN)
        } ?: return
        try {
            startActivity(open)
        } catch (e: Exception) {
            Log.w(TAG, "IGRIS_WAKE couldn't open IGRIS: ${e.javaClass.simpleName}")
        }
        // Android may block opening an app from the background (until "Display over other apps" is allowed);
        // the notification is a one-tap way in either way.
        val tap = PendingIntent.getActivity(this, 7303, open, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        val n = NotificationCompat.Builder(this, channel())
            .setSmallIcon(applicationInfo.icon)
            .setContentTitle("IGRIS is listening")
            .setContentText("Tap if IGRIS didn't open.")
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setAutoCancel(true)
            .setTimeoutAfter(15_000)
            .setContentIntent(tap)
            .build()
        getSystemService(NotificationManager::class.java)?.notify(HEARD_ID, n)
    }

    override fun onPartialResult(hypothesis: String?) {}

    override fun onResult(hypothesis: String?) = heard(hypothesis)

    override fun onFinalResult(hypothesis: String?) = heard(hypothesis)

    override fun onError(exception: Exception?) {
        Log.w(TAG, "IGRIS_WAKE error: ${exception?.message}")
        speech?.shutdown()
        speech = null
        listening = false
        main.postDelayed({ apply() }, 3_000)
    }

    override fun onTimeout() {
        speech = null
        listening = false
        apply()
    }

    private fun channel(): String {
        if (Build.VERSION.SDK_INT >= 26) {
            val nm = getSystemService(NotificationManager::class.java)
            if (nm?.getNotificationChannel(CHANNEL) == null) {
                nm?.createNotificationChannel(NotificationChannel(CHANNEL, "Hey IGRIS", NotificationManager.IMPORTANCE_LOW))
            }
        }
        return CHANNEL
    }

    private fun note(): Notification {
        val open = packageManager.getLaunchIntentForPackage(packageName)?.let {
            PendingIntent.getActivity(this, 7304, it, PendingIntent.FLAG_IMMUTABLE)
        }
        val text = when {
            model == null -> "Getting ready…"
            listening -> "Say “Hey IGRIS” or “Wake up IGRIS”. Heard only on this phone."
            prefs(this).getBoolean(KEY_CHARGING_ONLY, false) && !charging() -> "Waiting for the charger (you chose “only while charging”)."
            else -> "Paused while IGRIS uses the microphone."
        }
        return NotificationCompat.Builder(this, channel())
            .setSmallIcon(applicationInfo.icon)
            .setContentTitle("Hey IGRIS is on")
            .setContentText(text)
            .setOngoing(true)
            .setSilent(true)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .apply { if (open != null) setContentIntent(open) }
            .build()
    }

    override fun onDestroy() {
        speech?.stop()
        speech?.shutdown()
        speech = null
        model?.close()
        model = null
        power?.let { unregisterReceiver(it) }
        power = null
        listening = false
        running = false
        self = null
        main.removeCallbacksAndMessages(null)
        Log.i(TAG, "IGRIS_WAKE stopped")
        super.onDestroy()
    }
}
