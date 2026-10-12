package dev.igris.device

import android.annotation.SuppressLint
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.util.Log
import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlin.math.sqrt

/**
 * Records one spoken request on the phone, natively. The webview's own
 * microphone is a call-style (communication) capture, so Android turns the
 * music down and routes sound like a phone call; this records as voice
 * recognition (like "Hey Google"), which leaves other audio alone.
 *
 * Stops by itself after a pause following speech, if nothing is said for a
 * while, or at the length cap — rules passed in by the UI (the same ones the
 * computer's recorder uses).
 *
 * Each recording is a session: a newer one replaces an older one that is
 * still finishing, and only the current session's results are reported.
 */
object VoiceRecorder {
    private const val RATE = 16000

    class Rules(val threshold: Double, val silenceMs: Long, val noSpeechMs: Long, val maxMs: Long)

    private class Session(val id: Int, val rules: Rules) {
        @Volatile var stopping = false
        @Volatile var cancelled = false
        @Volatile var state = "recording"
        @Volatile var level = 0f
        @Volatile var heardSpeech = false
        @Volatile var error: String? = null
        val pcm = ByteArrayOutputStream()
        var thread: Thread? = null
    }

    private var nextId = 1
    private val lock = Any()

    @Volatile
    private var current: Session? = null

    /** idle, recording, done, failed — of the current session. */
    val state: String get() = current?.state ?: "idle"
    val level: Float get() = current?.level ?: 0f
    val heardSpeech: Boolean get() = current?.heardSpeech ?: false
    val error: String? get() = current?.error

    /** Start a new recording (an earlier one still finishing is dropped). */
    fun start(rules: Rules) {
        val s = synchronized(lock) {
            current?.let { old ->
                old.cancelled = true
                old.stopping = true
            }
            Session(nextId++, rules).also { current = it }
        }
        // The wake-word listener lets go of the microphone meanwhile.
        WakeWordService.hold("record", true)
        s.thread = Thread({ run(s) }, "igris-record").apply { start() }
    }

    /** Finish now (keep what was said), or `cancel` to drop it. */
    fun stop(cancel: Boolean) {
        val s = current ?: return
        if (cancel) s.cancelled = true
        s.stopping = true
    }

    @SuppressLint("MissingPermission")
    private fun run(s: Session) {
        val minBuf = AudioRecord.getMinBufferSize(RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
        var rec: AudioRecord? = null
        try {
            // Give the wake-word listener a moment to release the microphone.
            val waitUntil = System.currentTimeMillis() + 400
            while (WakeWordService.listening && System.currentTimeMillis() < waitUntil && !s.stopping) Thread.sleep(20)
            rec = AudioRecord(MediaRecorder.AudioSource.VOICE_RECOGNITION, RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT, maxOf(minBuf, 4096) * 2)
            if (rec.state != AudioRecord.STATE_INITIALIZED) throw IllegalStateException("The microphone isn't available.")
            rec.startRecording()
            val frame = ShortArray(RATE / 50) // 20 ms
            val bytes = ByteBuffer.allocate(frame.size * 2).order(ByteOrder.LITTLE_ENDIAN)
            val started = System.currentTimeMillis()
            var lastLoud = 0L
            val r = s.rules
            while (!s.stopping) {
                val n = rec.read(frame, 0, frame.size)
                // A read error (another app or a call took the microphone): stop instead of spinning.
                if (n < 0) throw IllegalStateException("The microphone stopped working (it may be in use by a call or another app).")
                val now = System.currentTimeMillis()
                if (n > 0) {
                    var sum = 0.0
                    bytes.clear()
                    for (i in 0 until n) {
                        val v = frame[i] / 32768.0
                        sum += v * v
                        bytes.putShort(frame[i])
                    }
                    s.pcm.write(bytes.array(), 0, n * 2)
                    val rms = sqrt(sum / n)
                    s.level = rms.toFloat()
                    if (rms >= r.threshold) {
                        s.heardSpeech = true
                        lastLoud = now
                    }
                }
                val elapsed = now - started
                if (elapsed >= r.maxMs) break
                if (!s.heardSpeech && elapsed >= r.noSpeechMs) break
                if (s.heardSpeech && now - lastLoud >= r.silenceMs) break
            }
            s.state = if (s.cancelled) "idle" else "done"
        } catch (e: Exception) {
            s.error = e.message ?: e.javaClass.simpleName
            s.state = if (s.cancelled) "idle" else "failed"
            Log.w("IGRIS", "IGRIS_RECORD failed: ${e.javaClass.simpleName}")
        } finally {
            try {
                rec?.stop()
            } catch (e: Exception) {
                // Already stopped.
            }
            rec?.release()
            s.level = 0f
            // Only the current session hands the microphone back to the wake-word listener.
            synchronized(lock) {
                if (current === s) WakeWordService.hold("record", false)
            }
        }
    }

    /** The finished recording as a 16 kHz mono WAV, once (then the session ends). */
    fun take(): ByteArray? {
        val s = synchronized(lock) {
            val s = current ?: return null
            if (s.state != "done") return null
            current = null
            s
        }
        return wav(s.pcm.toByteArray())
    }

    private fun wav(data: ByteArray): ByteArray {
        val h = ByteBuffer.allocate(44).order(ByteOrder.LITTLE_ENDIAN)
        h.put("RIFF".toByteArray()).putInt(36 + data.size).put("WAVE".toByteArray())
        h.put("fmt ".toByteArray()).putInt(16).putShort(1).putShort(1).putInt(RATE).putInt(RATE * 2).putShort(2).putShort(16)
        h.put("data".toByteArray()).putInt(data.size)
        return h.array() + data
    }
}
