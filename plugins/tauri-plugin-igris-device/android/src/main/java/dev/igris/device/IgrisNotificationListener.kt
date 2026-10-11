package dev.igris.device

import android.app.Notification
import android.app.RemoteInput
import android.content.Intent
import android.os.Bundle
import android.service.notification.NotificationListenerService
import android.service.notification.StatusBarNotification

/**
 * Keeps the last day's message notifications in memory (never on disk) so
 * IGRIS can summarise them and reply through the notification's own Reply
 * button. Android only runs it after the user turns on Notification access
 * for IGRIS. One-time codes are hidden before anything leaves this class.
 */
class IgrisNotificationListener : NotificationListenerService() {
    data class Item(
        val key: String,
        val pkg: String,
        val app: String,
        val title: String,
        val text: String,
        val time: Long,
        val reply: Notification.Action?,
    )

    companion object {
        @Volatile
        var connected = false
            private set

        private const val KEEP_MS = 24 * 60 * 60 * 1000L
        private const val MAX = 300
        private val items = ArrayList<Item>()

        /** Messaging apps read by default; others only when the user allows all apps. */
        val MESSAGING = setOf(
            "com.whatsapp", "com.whatsapp.w4b", "org.telegram.messenger", "org.thoughtcrime.securesms",
            "com.google.android.apps.messaging", "com.samsung.android.messaging", "com.android.mms",
            "com.google.android.gm", "com.microsoft.office.outlook", "com.instagram.android", "com.facebook.orca",
            "com.Slack", "com.microsoft.teams", "com.discord", "com.snapchat.android", "com.linkedin.android",
            "com.google.android.apps.dynamite",
        )

        private val CODE_WORDS = Regex("(?i)(otp|one[- ]?time|verification|passcode|pin\\b|security code|login code|sign[- ]?in code|\\bcode\\b)")
        private val DIGITS = Regex("\\b\\d{4,8}\\b")

        /** "Your OTP is 482913" → "Your OTP is ••••" (codes never reach the AI). */
        fun hideCodes(s: String): String = if (CODE_WORDS.containsMatchIn(s)) DIGITS.replace(s, "••••") else s

        fun recent(sinceMs: Long, allApps: Boolean): List<Item> = synchronized(items) {
            val cutoff = System.currentTimeMillis() - sinceMs
            items.filter { it.time >= cutoff && (allApps || it.pkg in MESSAGING) }.sortedByDescending { it.time }
        }

        fun find(key: String): Item? = synchronized(items) { items.lastOrNull { it.key == key } }

        /** Reply through the notification's own Reply button. */
        fun reply(context: android.content.Context, key: String, text: String) {
            val item = find(key) ?: throw IllegalArgumentException("That message isn't available any more (it may have been dismissed).")
            val action = item.reply ?: throw IllegalStateException("${item.app} doesn't allow replying from its notification.")
            val inputs = action.remoteInputs ?: throw IllegalStateException("${item.app} doesn't allow replying from its notification.")
            val intent = Intent()
            val results = Bundle()
            for (ri in inputs) results.putCharSequence(ri.resultKey, text)
            RemoteInput.addResultsToIntent(inputs, intent, results)
            action.actionIntent.send(context, 0, intent)
        }
    }

    override fun onListenerConnected() {
        connected = true
    }

    override fun onListenerDisconnected() {
        connected = false
    }

    override fun onNotificationPosted(sbn: StatusBarNotification?) {
        val n = sbn?.notification ?: return
        if (sbn.packageName == packageName || sbn.isOngoing || (n.flags and Notification.FLAG_GROUP_SUMMARY) != 0) return
        val e = n.extras ?: return
        val title = e.getCharSequence(Notification.EXTRA_TITLE)?.toString()?.trim().orEmpty()
        val text = (e.getCharSequence(Notification.EXTRA_BIG_TEXT) ?: e.getCharSequence(Notification.EXTRA_TEXT))?.toString()?.trim().orEmpty()
        if (title.isEmpty() && text.isEmpty()) return
        val app = try {
            packageManager.getApplicationLabel(packageManager.getApplicationInfo(sbn.packageName, 0)).toString()
        } catch (ex: Exception) {
            sbn.packageName
        }
        val reply = n.actions?.firstOrNull { a -> a.remoteInputs?.any { it.allowFreeFormInput } == true }
        val item = Item(sbn.key, sbn.packageName, app, hideCodes(title).take(120), hideCodes(text).take(600), sbn.postTime, reply)
        synchronized(items) {
            // A chat's notification is updated as messages arrive: keep the newest text per key.
            items.removeAll { it.key == item.key && it.text == item.text }
            items.add(item)
            val cutoff = System.currentTimeMillis() - KEEP_MS
            items.removeAll { it.time < cutoff }
            while (items.size > MAX) items.removeAt(0)
        }
    }
}
