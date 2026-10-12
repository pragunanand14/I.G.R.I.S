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
 * while, or at the length cap — the same rules as the computer's recorder.
 */
object VoiceRecorder {
    private const val RATE = 16000
    private const val THRESHOLD = 0.02
    private const val SILENCE_MS = 1200
    private const val NO_SPEECH_MS = 8000
    private const val MAX_MS = 30000

    /** idle, recording, done, failed */
    @Volatile
    var state = "idle"
        private set

    @Volatile
    var level = 0f
        private set

    @Volatile
    var heardSpeech = false
        private set

    @Volatile
    var error: String? = null
        private set

    @Volatile
    private var stopping = false

    @Volatile
    private var cancelled = false

    private var pcm = ByteArrayOutputStream()
    private var thread: Thread? = null

    @SuppressLint("MissingPermission")
    @Synchronized
    fun start() {
        if (state == "recording") return
        stopping = false
        cancelled = false
        heardSpeech = false
        level = 0f
        error = null
        pcm = ByteArrayOutputStream()
        state = "recording"
        // The wake-word listener lets go of the microphone meanwhile.
        WakeWordService.hold("record", true)
        thread = Thread({ run() }, "igris-record").apply { start() }
    }

    /** Finish now (keep what was said), or `cancel` to drop it. */
    fun stop(cancel: Boolean) {
        cancelled = cancel
        stopping = true
    }

    @SuppressLint("MissingPermission")
    private fun run() {
        val minBuf = AudioRecord.getMinBufferSize(RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
        var rec: AudioRecord? = null
        try {
            rec = AudioRecord(MediaRecorder.AudioSource.VOICE_RECOGNITION, RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT, maxOf(minBuf, 4096) * 2)
            if (rec.state != AudioRecord.STATE_INITIALIZED) throw IllegalStateException("The microphone isn't available.")
            rec.startRecording()
            val frame = ShortArray(RATE / 50) // 20 ms
            val bytes = ByteBuffer.allocate(frame.size * 2).order(ByteOrder.LITTLE_ENDIAN)
            val started = System.currentTimeMillis()
            var lastLoud = 0L
            while (!stopping) {
                val n = rec.read(frame, 0, frame.size)
                if (n <= 0) continue
                var sum = 0.0
                bytes.clear()
                for (i in 0 until n) {
                    val v = frame[i] / 32768.0
                    sum += v * v
                    bytes.putShort(frame[i])
                }
                pcm.write(bytes.array(), 0, n * 2)
                val rms = sqrt(sum / n)
                level = rms.toFloat()
                val now = System.currentTimeMillis()
                if (rms >= THRESHOLD) {
                    heardSpeech = true
                    lastLoud = now
                }
                val elapsed = now - started
                if (elapsed >= MAX_MS) break
                if (!heardSpeech && elapsed >= NO_SPEECH_MS) break
                if (heardSpeech && now - lastLoud >= SILENCE_MS) break
            }
            state = if (cancelled) "idle" else "done"
        } catch (e: Exception) {
            error = e.message ?: e.javaClass.simpleName
            state = "failed"
            Log.w("IGRIS", "IGRIS_RECORD failed: ${e.javaClass.simpleName}")
        } finally {
            try {
                rec?.stop()
            } catch (e: Exception) {
                // Already stopped.
            }
            rec?.release()
            level = 0f
            WakeWordService.hold("record", false)
        }
    }

    /** The finished recording as a 16 kHz mono WAV, once (then idle again). */
    @Synchronized
    fun take(): ByteArray? {
        if (state != "done") return null
        state = "idle"
        val data = pcm.toByteArray()
        pcm = ByteArrayOutputStream()
        return wav(data)
    }

    private fun wav(data: ByteArray): ByteArray {
        val h = ByteBuffer.allocate(44).order(ByteOrder.LITTLE_ENDIAN)
        h.put("RIFF".toByteArray()).putInt(36 + data.size).put("WAVE".toByteArray())
        h.put("fmt ".toByteArray()).putInt(16).putShort(1).putShort(1).putInt(RATE).putInt(RATE * 2).putShort(2).putShort(16)
        h.put("data".toByteArray()).putInt(data.size)
        return h.array() + data
    }
}
