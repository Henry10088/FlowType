package app.flowtype.image

import android.content.ContentResolver
import android.graphics.Bitmap
import android.graphics.ImageDecoder
import android.net.Uri
import java.io.ByteArrayOutputStream

object ImagePreparer {
    private const val MAX_EDGE = 4096
    private const val MAX_PIXELS = 40_000_000L
    private const val MAX_OPTIMIZED_BYTES = 15 * 1024 * 1024
    private const val MAX_ORIGINAL_BYTES = 32 * 1024 * 1024

    fun prepare(resolver: ContentResolver, uri: Uri, original: Boolean): PreparedImage {
        val source = ImageDecoder.createSource(resolver, uri)
        var sourceWidth = 0
        var sourceHeight = 0
        val bitmap = ImageDecoder.decodeBitmap(source) { decoder, info, _ ->
            sourceWidth = info.size.width
            sourceHeight = info.size.height
            require(sourceWidth > 0 && sourceHeight > 0) { "invalid image" }
            require(sourceWidth.toLong() * sourceHeight <= MAX_PIXELS) { "image has too many pixels" }
            decoder.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
            if (!original) {
                val scale = minOf(1.0, MAX_EDGE.toDouble() / maxOf(sourceWidth, sourceHeight))
                decoder.setTargetSize(
                    maxOf(1, (sourceWidth * scale).toInt()),
                    maxOf(1, (sourceHeight * scale).toInt()),
                )
            }
        }

        if (original) {
            val preserveAlpha = bitmap.hasAlpha()
            val format = if (preserveAlpha) Bitmap.CompressFormat.PNG else Bitmap.CompressFormat.JPEG
            val mime = if (preserveAlpha) "image/png" else "image/jpeg"
            val bytes = ByteArrayOutputStream().use { output ->
                check(bitmap.compress(format, 100, output)) { "cannot encode image" }
                output.toByteArray()
            }
            require(bytes.size <= MAX_ORIGINAL_BYTES) { "original image is too large" }
            return PreparedImage(
                bytes = bytes,
                mimeType = mime,
                width = sourceWidth,
                height = sourceHeight,
                original = true,
            )
        }

        val preserveAlpha = bitmap.hasAlpha()
        val format = if (preserveAlpha) Bitmap.CompressFormat.PNG else Bitmap.CompressFormat.JPEG
        val mime = if (preserveAlpha) "image/png" else "image/jpeg"
        val bytes = ByteArrayOutputStream().use { output ->
            check(bitmap.compress(format, 92, output)) { "cannot encode image" }
            output.toByteArray()
        }
        require(bytes.size <= MAX_OPTIMIZED_BYTES) { "optimized image is too large" }
        return PreparedImage(
            bytes = bytes,
            mimeType = mime,
            width = bitmap.width,
            height = bitmap.height,
            original = false,
        )
    }

    fun preview(resolver: ContentResolver, uri: Uri): Bitmap =
        ImageDecoder.decodeBitmap(ImageDecoder.createSource(resolver, uri)) { decoder, info, _ ->
            val scale = minOf(1.0, 1200.0 / maxOf(info.size.width, info.size.height))
            decoder.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
            decoder.setTargetSize(
                maxOf(1, (info.size.width * scale).toInt()),
                maxOf(1, (info.size.height * scale).toInt()),
            )
        }

}
