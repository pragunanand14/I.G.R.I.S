package dev.igris.device

import android.content.ContentValues
import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Matrix
import android.media.ExifInterface
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.util.Base64
import java.io.ByteArrayOutputStream
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/** Photos and scans for IGRIS: made small enough to send to the AI, the right way up. */
object Camera {
    /** Where the camera app writes the photo (the app's cache; shared through its FileProvider). */
    fun photoFile(ctx: Context): File = File(ctx.cacheDir, "igris-camera.jpg").also { it.delete() }

    fun photoUri(ctx: Context, f: File): Uri =
        androidx.core.content.FileProvider.getUriForFile(ctx, ctx.packageName + ".fileprovider", f)

    /** A JPEG (base64), longest side at most [maxSide], rotated as the camera meant it. */
    fun encode(ctx: Context, uri: Uri, maxSide: Int): String? {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        ctx.contentResolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, bounds) } ?: return null
        if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return null
        var sample = 1
        while (maxOf(bounds.outWidth, bounds.outHeight) / (sample * 2) >= maxSide) sample *= 2
        val raw = ctx.contentResolver.openInputStream(uri)?.use {
            BitmapFactory.decodeStream(it, null, BitmapFactory.Options().apply { inSampleSize = sample })
        } ?: return null
        val degrees = try {
            ctx.contentResolver.openInputStream(uri)?.use {
                when (ExifInterface(it).getAttributeInt(ExifInterface.TAG_ORIENTATION, ExifInterface.ORIENTATION_NORMAL)) {
                    ExifInterface.ORIENTATION_ROTATE_90 -> 90f
                    ExifInterface.ORIENTATION_ROTATE_180 -> 180f
                    ExifInterface.ORIENTATION_ROTATE_270 -> 270f
                    else -> 0f
                }
            } ?: 0f
        } catch (e: Exception) {
            0f
        }
        val scale = minOf(1f, maxSide / maxOf(raw.width, raw.height).toFloat())
        val m = Matrix().apply {
            postScale(scale, scale)
            postRotate(degrees)
        }
        val fixed = Bitmap.createBitmap(raw, 0, 0, raw.width, raw.height, m, true)
        val out = ByteArrayOutputStream()
        fixed.compress(Bitmap.CompressFormat.JPEG, 82, out)
        if (fixed !== raw) fixed.recycle()
        raw.recycle()
        return Base64.encodeToString(out.toByteArray(), Base64.NO_WRAP)
    }

    /**
     * Keep a scanned PDF where the user can find it: Documents/IGRIS (Android 10+), else IGRIS's own
     * documents folder. Returns where it went.
     */
    fun savePdf(ctx: Context, pdf: Uri): String {
        val name = "Scan " + SimpleDateFormat("yyyy-MM-dd HH.mm", Locale.US).format(Date()) + ".pdf"
        if (Build.VERSION.SDK_INT >= 29) {
            val values = ContentValues().apply {
                put(MediaStore.MediaColumns.DISPLAY_NAME, name)
                put(MediaStore.MediaColumns.MIME_TYPE, "application/pdf")
                put(MediaStore.MediaColumns.RELATIVE_PATH, Environment.DIRECTORY_DOCUMENTS + "/IGRIS")
            }
            val target = ctx.contentResolver.insert(MediaStore.Files.getContentUri("external"), values)
                ?: throw IllegalStateException("Couldn't save the PDF.")
            ctx.contentResolver.openOutputStream(target)?.use { o -> ctx.contentResolver.openInputStream(pdf)?.use { it.copyTo(o) } }
            return "Documents/IGRIS/$name"
        }
        val dir = ctx.getExternalFilesDir(Environment.DIRECTORY_DOCUMENTS) ?: ctx.filesDir
        val f = File(dir, name)
        f.outputStream().use { o -> ctx.contentResolver.openInputStream(pdf)?.use { it.copyTo(o) } }
        return f.absolutePath
    }
}
