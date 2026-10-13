package dev.igris.device

import android.app.Activity
import android.app.PendingIntent
import android.content.Intent
import android.os.Build
import android.os.Bundle

/**
 * Shows a screen from Google Play services — the sign-in / consent sheet (a PendingIntent) or the document
 * scanner (an IntentSender) — and hands its result back to the plugin. Invisible itself; it only exists
 * because those screens have to be started "for a result" from an activity.
 */
class GoogleAuthActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Re-created while Google's sheet is up (e.g. rotation): keep waiting for its result.
        if (savedInstanceState != null) return
        val pending: PendingIntent? = if (Build.VERSION.SDK_INT >= 33) {
            intent.getParcelableExtra(EXTRA_PENDING, PendingIntent::class.java)
        } else {
            @Suppress("DEPRECATION")
            intent.getParcelableExtra(EXTRA_PENDING)
        }
        val sender: android.content.IntentSender? = pending?.intentSender ?: if (Build.VERSION.SDK_INT >= 33) {
            intent.getParcelableExtra(EXTRA_SENDER, android.content.IntentSender::class.java)
        } else {
            @Suppress("DEPRECATION")
            intent.getParcelableExtra(EXTRA_SENDER)
        }
        if (sender == null) {
            setResult(RESULT_CANCELED)
            finish()
            return
        }
        try {
            startIntentSenderForResult(sender, REQUEST, null, 0, 0, 0)
        } catch (e: Exception) {
            setResult(RESULT_CANCELED)
            finish()
        }
    }

    @Deprecated("Activity result API of the platform Activity")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        @Suppress("DEPRECATION")
        super.onActivityResult(requestCode, resultCode, data)
        setResult(resultCode, data)
        finish()
    }

    companion object {
        const val EXTRA_PENDING = "igris.google.pending"
        const val EXTRA_SENDER = "igris.google.sender"
        private const val REQUEST = 7301
    }
}
