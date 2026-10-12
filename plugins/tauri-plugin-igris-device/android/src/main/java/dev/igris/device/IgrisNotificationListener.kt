package dev.igris.device

import android.app.Notification
import android.app.RemoteInput
import android.content.Intent
import android.os.Bundle
import android.os.Parcelable
import android.service.notification.NotificationListenerService
import android.util.Log
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

        /** The running listener, to read what's already in the notification bar. */
        @Volatile
        private var instance: IgrisNotificationListener? = null

        /** Ask Android to reconnect the listener (it sometimes stays unbound after an update or restart). */
        fun rebind(context: android.content.Context) {
            if (connected || android.os.Build.VERSION.SDK_INT < 24) return
            try {
                requestRebind(android.content.ComponentName(context, IgrisNotificationListener::class.java))
            } catch (e: Exception) {
                Log.w("IGRIS", "notification listener rebind failed: ${e.javaClass.simpleName}")
            }
        }

        /** Pick up notifications already showing (e.g. unread chats from before IGRIS started). */
        fun refresh() {
            val l = instance ?: return
            try {
                l.activeNotifications?.forEach { l.add(it) }
            } catch (e: Exception) {
                Log.w("IGRIS", "couldn't read active notifications: ${e.javaClass.simpleName}")
            }
        }

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
        instance = this
        refresh()
        Log.i("IGRIS", "IGRIS_MESSAGES_LISTENER connected, ${synchronized(items) { items.size }} notifications")
    }

    override fun onListenerDisconnected() {
        connected = false
        instance = null
        Log.i("IGRIS", "IGRIS_MESSAGES_LISTENER disconnected")
    }

    override fun onNotificationPosted(sbn: StatusBarNotification?) {
        add(sbn ?: return)
    }

    /** Chat apps (WhatsApp, Messages, Telegram…) put each message in MessagingStyle; others only have a text. */
    private fun messages(e: Bundle): String? {
        val arr = e.getParcelableArray(Notification.EXTRA_MESSAGES) ?: return null
        val lines = arr.mapNotNull { p: Parcelable? ->
            val b = p as? Bundle ?: return@mapNotNull null
            val text = b.getCharSequence("text")?.toString()?.trim().orEmpty()
            if (text.isEmpty()) return@mapNotNull null
            val sender = b.getCharSequence("sender")?.toString()?.trim().orEmpty()
            if (sender.isEmpty()) text else "$sender: $text"
        }
        return if (lines.isEmpty()) null else lines.takeLast(8).joinToString("\n")
    }

    private fun add(sbn: StatusBarNotification) {
        val n = sbn.notification ?: return
        if (sbn.packageName == packageName || sbn.isOngoing || (n.flags and Notification.FLAG_GROUP_SUMMARY) != 0) return
        val e = n.extras ?: return
        val title = e.getCharSequence(Notification.EXTRA_TITLE)?.toString()?.trim().orEmpty()
        val title0 = e.getCharSequence(Notification.EXTRA_CONVERSATION_TITLE)?.toString()?.trim()
        val chatText = messages(e)
        val text = (chatText ?: (e.getCharSequence(Notification.EXTRA_BIG_TEXT) ?: e.getCharSequence(Notification.EXTRA_TEXT))?.toString())?.trim().orEmpty()
        if (title.isEmpty() && text.isEmpty()) return
        val app = try {
            packageManager.getApplicationLabel(packageManager.getApplicationInfo(sbn.packageName, 0)).toString()
        } catch (ex: Exception) {
            sbn.packageName
        }
        val reply = n.actions?.firstOrNull { a -> a.remoteInputs?.any { it.allowFreeFormInput } == true }
        val chat = if (!title0.isNullOrEmpty()) title0 else title
        val item = Item(sbn.key, sbn.packageName, app, hideCodes(chat).take(120), hideCodes(text).take(1200), sbn.postTime, reply)
        synchronized(items) {
            // A chat's notification (its recent messages) is updated as messages arrive: keep its newest state.
            // Apps that reuse one notification for separate alerts (bank SMS, OTPs): keep each distinct text.
            if (chatText != null) items.removeAll { it.key == item.key } else items.removeAll { it.key == item.key && it.text == item.text }
            items.add(item)
            val cutoff = System.currentTimeMillis() - KEEP_MS
            items.removeAll { it.time < cutoff }
            while (items.size > MAX) items.removeAt(0)
        }
    }
}
