package dev.igris.device

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.GestureDescription
import android.content.ClipData
import android.content.ClipboardManager
import android.graphics.Bitmap
import android.graphics.Color
import android.graphics.Path
import android.graphics.PixelFormat
import android.graphics.Rect
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.util.TypedValue
import android.view.Gravity
import android.view.WindowManager
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityNodeInfo
import android.view.accessibility.AccessibilityWindowInfo
import java.io.ByteArrayOutputStream
import java.util.concurrent.Executors
import android.widget.TextView
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * IGRIS's screen reading and control. Android only runs it after the user
 * turns it on in Settings → Accessibility; IGRIS only uses it when asked
 * (reading the screen) or during a task the user approved (tapping, typing,
 * swiping), with a Stop pill on screen the whole time. Password fields are
 * never read or typed into. Every use comes from IGRIS's Rust tools.
 */
class IgrisAccessibilityService : AccessibilityService() {
    companion object {
        @Volatile
        var instance: IgrisAccessibilityService? = null
            private set

        /** The user tapped Stop on the pill; IGRIS reads and clears it. */
        @Volatile
        var stopRequested = false

        const val MAX_NODES = 220
    }

    data class Node(val info: AccessibilityNodeInfo, val label: String, val role: String, val rect: Rect)

    /** The last screen read; tap/type by index refer to it. */
    @Volatile
    private var nodes: List<Node> = emptyList()
    private val callbacks by lazy { HandlerThread("igris-gestures").apply { start() } }
    private val main = Handler(Looper.getMainLooper())
    private var pill: TextView? = null

    override fun onServiceConnected() {
        instance = this
    }

    override fun onUnbind(intent: android.content.Intent?): Boolean {
        instance = null
        main.post { hideOverlay() }
        return super.onUnbind(intent)
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {}

    override fun onInterrupt() {}

    // ── Reading ──────────────────────────────────────────────────────────

    data class Screen(val packageName: String, val nodes: List<Node>, val width: Int, val height: Int)

    /**
     * Everything on screen, top window first: pop-ups and Android's own dialogs ("Allow location?",
     * "Turn on device location") come before the app under them. The keyboard and IGRIS's own Stop pill
     * are left out.
     */
    fun read(): Screen {
        val found = ArrayList<Node>()
        var pkg = ""
        val wins = try { windows } catch (e: Exception) { emptyList<AccessibilityWindowInfo>() }
        val shown = wins
            .filter { it.type != AccessibilityWindowInfo.TYPE_INPUT_METHOD && it.type != AccessibilityWindowInfo.TYPE_ACCESSIBILITY_OVERLAY }
            .sortedByDescending { it.layer }
        for (w in shown) {
            val root = w.root ?: continue
            if (w.type == AccessibilityWindowInfo.TYPE_APPLICATION && pkg.isEmpty()) pkg = root.packageName?.toString().orEmpty()
            walk(root, found, 0)
        }
        if (found.isEmpty()) {
            val root = rootInActiveWindow ?: throw IllegalStateException("Can't see the screen right now (the phone may be locked).")
            pkg = root.packageName?.toString().orEmpty()
            walk(root, found, 0)
        }
        if (pkg.isEmpty()) pkg = rootInActiveWindow?.packageName?.toString().orEmpty()
        nodes = found
        val metrics = resources.displayMetrics
        return Screen(pkg, found, metrics.widthPixels, metrics.heightPixels)
    }

    /**
     * A JPEG of the screen, its longest side at most [maxSide] (Android 11+). Null when Android won't give
     * one (secure screens such as banking apps show black, so they're skipped too).
     */
    fun screenshot(maxSide: Int): Shot? {
        if (Build.VERSION.SDK_INT < 30) return null
        for (attempt in 0 until 2) {
            val done = CountDownLatch(1)
            var bitmap: Bitmap? = null
            var tooSoon = false
            takeScreenshot(android.view.Display.DEFAULT_DISPLAY, shotExecutor, object : TakeScreenshotCallback {
                override fun onSuccess(result: ScreenshotResult) {
                    try {
                        val hw = Bitmap.wrapHardwareBuffer(result.hardwareBuffer, result.colorSpace)
                        bitmap = hw?.copy(Bitmap.Config.ARGB_8888, false)
                        hw?.recycle()
                    } finally {
                        result.hardwareBuffer.close()
                        done.countDown()
                    }
                }

                override fun onFailure(errorCode: Int) {
                    tooSoon = errorCode == ERROR_TAKE_SCREENSHOT_INTERVAL_TIME_SHORT
                    done.countDown()
                }
            })
            if (!done.await(3, TimeUnit.SECONDS)) return null
            val full = bitmap
            if (full == null) {
                if (tooSoon && attempt == 0) { Thread.sleep(400); continue }
                return null
            }
            val scale = maxOf(1f, maxOf(full.width, full.height) / maxSide.toFloat())
            val small = if (scale > 1f) Bitmap.createScaledBitmap(full, (full.width / scale).toInt(), (full.height / scale).toInt(), true) else full
            val out = ByteArrayOutputStream()
            small.compress(Bitmap.CompressFormat.JPEG, 70, out)
            val shot = Shot(out.toByteArray(), small.width, small.height, scale)
            if (small !== full) small.recycle()
            full.recycle()
            return shot
        }
        return null
    }

    data class Shot(val jpeg: ByteArray, val width: Int, val height: Int, val scale: Float)

    private val shotExecutor by lazy { Executors.newSingleThreadExecutor() }

    private fun walk(n: AccessibilityNodeInfo?, out: MutableList<Node>, depth: Int) {
        if (n == null || out.size >= MAX_NODES || depth > 40) return
        if (!n.isVisibleToUser) return
        val text = n.text?.toString()?.trim().orEmpty()
        val desc = n.contentDescription?.toString()?.trim().orEmpty()
        val hint = if (Build.VERSION.SDK_INT >= 26) n.hintText?.toString()?.trim().orEmpty() else ""
        val interactive = n.isClickable || n.isEditable || n.isCheckable || n.isScrollable || n.isLongClickable
        val label = when {
            n.isPassword -> if (hint.isNotEmpty()) "$hint (password field — not read)" else "(password field — not read)"
            Build.VERSION.SDK_INT >= 26 && n.isShowingHintText -> hint
            text.isNotEmpty() -> text
            desc.isNotEmpty() -> desc
            else -> hint
        }.take(200)
        if ((interactive || label.isNotEmpty())) {
            val r = Rect()
            n.getBoundsInScreen(r)
            if (r.width() > 0 && r.height() > 0) out.add(Node(n, label, role(n), r))
        }
        for (i in 0 until n.childCount) walk(n.getChild(i), out, depth + 1)
    }

    private fun role(n: AccessibilityNodeInfo): String {
        val cls = n.className?.toString().orEmpty()
        return when {
            n.isEditable -> "field"
            n.isCheckable -> if (n.isChecked) "checked" else "unchecked"
            cls.endsWith("Button") || cls.endsWith("ImageButton") -> "button"
            n.isScrollable -> "list"
            n.isClickable -> "button"
            else -> "text"
        }
    }

    fun node(index: Int): Node = nodes.getOrNull(index) ?: throw IllegalArgumentException("There's no item $index on the last screen read. Read the screen again.")

    // ── Acting ───────────────────────────────────────────────────────────

    private fun gesture(path: Path, durationMs: Long) {
        val g = GestureDescription.Builder().addStroke(GestureDescription.StrokeDescription(path, 0, durationMs)).build()
        val done = CountDownLatch(1)
        var ok = false
        val cb = object : AccessibilityService.GestureResultCallback() {
            override fun onCompleted(d: GestureDescription?) { ok = true; done.countDown() }
            override fun onCancelled(d: GestureDescription?) { done.countDown() }
        }
        if (!dispatchGesture(g, cb, Handler(callbacks.looper))) throw IllegalStateException("Android didn't accept the gesture.")
        if (!done.await(durationMs + 3000, TimeUnit.MILLISECONDS) || !ok) throw IllegalStateException("The gesture was interrupted.")
    }

    fun tap(x: Float, y: Float, long: Boolean) {
        gesture(Path().apply { moveTo(x, y) }, if (long) 650 else 60)
    }

    /** Tap an item from the last read: its click action, or a tap at its centre. */
    fun tapNode(index: Int, long: Boolean) {
        val n = node(index)
        var target: AccessibilityNodeInfo? = n.info
        val action = if (long) AccessibilityNodeInfo.ACTION_LONG_CLICK else AccessibilityNodeInfo.ACTION_CLICK
        while (target != null && !(if (long) target.isLongClickable else target.isClickable)) target = target.parent
        if (target != null && target.performAction(action)) return
        tap(n.rect.exactCenterX(), n.rect.exactCenterY(), long)
    }

    fun swipe(direction: String) {
        val m = resources.displayMetrics
        val w = m.widthPixels.toFloat()
        val h = m.heightPixels.toFloat()
        val path = Path()
        when (direction) {
            // "up" moves the content up (scrolls down the page), like a finger swiping up.
            "up" -> { path.moveTo(w / 2, h * 0.72f); path.lineTo(w / 2, h * 0.28f) }
            "down" -> { path.moveTo(w / 2, h * 0.28f); path.lineTo(w / 2, h * 0.72f) }
            "left" -> { path.moveTo(w * 0.85f, h / 2); path.lineTo(w * 0.15f, h / 2) }
            "right" -> { path.moveTo(w * 0.15f, h / 2); path.lineTo(w * 0.85f, h / 2) }
            else -> throw IllegalArgumentException("Swipe up, down, left or right.")
        }
        gesture(path, 320)
    }

    /** Type into the focused field (or item `index`), adding to what's there. */
    fun type(text: String, index: Int?, enter: Boolean) {
        val n = if (index != null) node(index).info else findFocus(AccessibilityNodeInfo.FOCUS_INPUT)
            ?: throw IllegalStateException("No text field is selected. Tap one first.")
        if (n.isPassword) throw SecurityException("That's a password field — IGRIS doesn't type passwords. Please type it yourself.")
        if (!n.isEditable) throw IllegalStateException("That isn't a text field.")
        if (index != null) n.performAction(AccessibilityNodeInfo.ACTION_FOCUS)
        val existing = if (Build.VERSION.SDK_INT >= 26 && n.isShowingHintText) "" else n.text?.toString().orEmpty()
        val args = Bundle().apply { putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, existing + text) }
        if (!n.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)) {
            // Some apps (web pages, custom text boxes) refuse "set text": paste it instead, like a person would.
            val clip = getSystemService(android.content.Context.CLIPBOARD_SERVICE) as ClipboardManager
            clip.setPrimaryClip(ClipData.newPlainText("IGRIS", text))
            n.performAction(AccessibilityNodeInfo.ACTION_FOCUS)
            if (!n.performAction(AccessibilityNodeInfo.ACTION_PASTE)) throw IllegalStateException("That field didn't accept the text.")
        }
        if (enter) {
            if (Build.VERSION.SDK_INT < 30) throw IllegalStateException("Pressing Enter needs Android 11 or newer.")
            n.performAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_IME_ENTER.id)
        }
    }

    fun global(action: String) {
        val a = when (action) {
            "back" -> AccessibilityService.GLOBAL_ACTION_BACK
            "home" -> AccessibilityService.GLOBAL_ACTION_HOME
            "recents" -> AccessibilityService.GLOBAL_ACTION_RECENTS
            "notifications" -> AccessibilityService.GLOBAL_ACTION_NOTIFICATIONS
            else -> throw IllegalArgumentException("Unknown button: $action")
        }
        if (!performGlobalAction(a)) throw IllegalStateException("Android didn't do that.")
    }

    // ── The Stop pill ────────────────────────────────────────────────────

    fun showOverlay(text: String) = main.post {
        val wm = getSystemService(android.content.Context.WINDOW_SERVICE) as WindowManager
        val view = pill ?: TextView(this).also { v ->
            v.setTextColor(Color.WHITE)
            v.setTextSize(TypedValue.COMPLEX_UNIT_SP, 14f)
            val pad = (12 * resources.displayMetrics.density).toInt()
            v.setPadding(pad * 2, pad, pad * 2, pad)
            v.background = GradientDrawable().apply { cornerRadius = 100f; setColor(Color.argb(235, 20, 24, 32)); setStroke(3, Color.rgb(94, 184, 255)) }
            v.setOnClickListener { stopRequested = true; v.text = "Stopping…" }
            val lp = WindowManager.LayoutParams(
                WindowManager.LayoutParams.WRAP_CONTENT, WindowManager.LayoutParams.WRAP_CONTENT,
                WindowManager.LayoutParams.TYPE_ACCESSIBILITY_OVERLAY,
                WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN,
                PixelFormat.TRANSLUCENT,
            )
            lp.gravity = Gravity.TOP or Gravity.CENTER_HORIZONTAL
            lp.y = (48 * resources.displayMetrics.density).toInt()
            wm.addView(v, lp)
            pill = v
        }
        if (!stopRequested) view.text = "● IGRIS: $text   ·   Tap to stop"
    }

    fun hideOverlay() {
        pill?.let { (getSystemService(android.content.Context.WINDOW_SERVICE) as WindowManager).removeView(it) }
        pill = null
    }

    fun hideOverlayAsync() = main.post { hideOverlay() }
}
