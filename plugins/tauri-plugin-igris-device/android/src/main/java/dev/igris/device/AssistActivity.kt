package dev.igris.device

import android.app.Activity
import android.content.Intent
import android.os.Bundle

/**
 * IGRIS as the phone's digital assistant: when the user picks IGRIS in
 * Settings → Default apps → Digital assistant app, the assistant gesture
 * (swipe up from a bottom corner, or hold the Home button) and a headset's
 * voice button open IGRIS's assistant panel, listening. Nothing is shown
 * here; it hands over to IGRIS and closes.
 */
class AssistActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val open = packageManager.getLaunchIntentForPackage(packageName)?.apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT)
            putExtra(EXTRA_ACTION, ACTION_ASSISTANT)
        }
        if (open != null) startActivity(open)
        finish()
    }
}
