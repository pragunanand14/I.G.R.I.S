package dev.igris.device

import android.app.Activity
import android.app.PendingIntent
import android.content.Intent
import android.os.Build
import android.os.Bundle

/**
 * Shows Google's own sign-in / consent sheet (a PendingIntent from Google Play services) and hands its
 * result back to the plugin. Invisible itself; it only exists because the sheet has to be started "for a
 * result" from an activity.
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
        if (pending == null) {
            setResult(RESULT_CANCELED)
            finish()
            return
        }
        try {
            startIntentSenderForResult(pending.intentSender, REQUEST, null, 0, 0, 0)
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
        private const val REQUEST = 7301
    }
}
