package com.plenum.mobile

import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.media.MediaScannerConnection
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.os.PowerManager
import android.provider.MediaStore
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import java.io.File

class MainActivity : FlutterActivity() {
    private val channelName = "plenum/media"
    private val shareChannelName = "plenum/share"

    // Wi-Fi + CPU locks held only for the duration of a transfer.
    private var wifiLock: WifiManager.WifiLock? = null
    private var wakeLock: PowerManager.WakeLock? = null

    private var pendingSharedFiles: List<Map<String, Any>>? = null
    private var shareChannel: MethodChannel? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        handleShareIntent(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleShareIntent(intent)
    }

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)

        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, channelName)
            .setMethodCallHandler { call, result ->
                when (call.method) {
                    "acquireTransferLock" -> {
                        try {
                            acquireTransferLock()
                            result.success(true)
                        } catch (e: Exception) {
                            result.error("LOCK_FAILED", e.message, null)
                        }
                    }
                    "releaseTransferLock" -> {
                        releaseTransferLock()
                        result.success(true)
                    }
                    "scanFile" -> {
                        val path = call.argument<String>("path")
                        if (path.isNullOrEmpty()) {
                            result.error("NO_PATH", "path argument missing", null)
                        } else {
                            MediaScannerConnection.scanFile(
                                applicationContext,
                                arrayOf(path),
                                null,
                                null
                            )
                            result.success(true)
                        }
                    }
                    "saveToDownloads" -> {
                        val path = call.argument<String>("path")
                        if (path.isNullOrEmpty()) {
                            result.error("NO_PATH", "path argument missing", null)
                            return@setMethodCallHandler
                        }
                        val source = File(path)
                        if (!source.exists()) {
                            result.error("NOT_FOUND", "source file does not exist: $path", null)
                            return@setMethodCallHandler
                        }
                        Thread {
                            try {
                                val saved = exportToDownloads(source)
                                runOnUiThread { result.success(saved) }
                            } catch (e: Exception) {
                                runOnUiThread {
                                    result.error("SAVE_FAILED", e.message, null)
                                }
                            }
                        }.start()
                    }
                    else -> result.notImplemented()
                }
            }

        shareChannel = MethodChannel(flutterEngine.dartExecutor.binaryMessenger, shareChannelName).apply {
            setMethodCallHandler { call, result ->
                when (call.method) {
                    "getInitialSharing" -> {
                        val files = pendingSharedFiles
                        pendingSharedFiles = null
                        result.success(files ?: emptyList<Map<String, Any>>())
                    }
                    "clearSharedCache" -> {
                        Thread {
                            try {
                                File(applicationContext.cacheDir, "shared_transfers").deleteRecursively()
                                runOnUiThread { result.success(true) }
                            } catch (_: Exception) {
                                runOnUiThread { result.success(false) }
                            }
                        }.start()
                    }
                    else -> result.notImplemented()
                }
            }
        }
    }

    private fun handleShareIntent(intent: Intent?) {
        if (intent == null) return
        val action = intent.action
        if (action != Intent.ACTION_SEND && action != Intent.ACTION_SEND_MULTIPLE && action != Intent.ACTION_VIEW) {
            return
        }

        Thread {
            val uris = extractUris(intent)
            val files = uris.mapNotNull { resolveAndCopyUri(applicationContext, it) }
            if (files.isNotEmpty()) {
                pendingSharedFiles = files
                runOnUiThread {
                    shareChannel?.invokeMethod("onSharedFiles", files)
                }
            }
        }.start()
    }

    private fun extractUris(intent: Intent): List<Uri> {
        val uris = mutableListOf<Uri>()
        when (intent.action) {
            Intent.ACTION_SEND -> {
                val uri = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
                } else {
                    @Suppress("DEPRECATION")
                    intent.getParcelableExtra<Uri>(Intent.EXTRA_STREAM)
                }
                if (uri != null) {
                    uris.add(uri)
                } else if (intent.data != null) {
                    uris.add(intent.data!!)
                } else if (intent.clipData != null) {
                    val clip = intent.clipData!!
                    for (i in 0 until clip.itemCount) {
                        val itemUri = clip.getItemAt(i).uri
                        if (itemUri != null) uris.add(itemUri)
                    }
                }
            }
            Intent.ACTION_SEND_MULTIPLE -> {
                val list = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM, Uri::class.java)
                } else {
                    @Suppress("DEPRECATION")
                    intent.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM)
                }
                if (!list.isNullOrEmpty()) {
                    uris.addAll(list.filterNotNull())
                } else if (intent.clipData != null) {
                    val clip = intent.clipData!!
                    for (i in 0 until clip.itemCount) {
                        val itemUri = clip.getItemAt(i).uri
                        if (itemUri != null) uris.add(itemUri)
                    }
                }
            }
            Intent.ACTION_VIEW -> {
                intent.data?.let { uris.add(it) }
            }
        }
        return uris.distinct()
    }

    private fun resolveAndCopyUri(context: Context, uri: Uri): Map<String, Any>? {
        var fileName: String? = null
        var fileSize: Long = -1

        if (uri.scheme == "content") {
            try {
                context.contentResolver.query(uri, null, null, null, null)?.use { cursor ->
                    val nameIndex = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                    val sizeIndex = cursor.getColumnIndex(OpenableColumns.SIZE)
                    if (cursor.moveToFirst()) {
                        if (nameIndex != -1) fileName = cursor.getString(nameIndex)
                        if (sizeIndex != -1) fileSize = cursor.getLong(sizeIndex)
                    }
                }
            } catch (_: Exception) {}
        } else if (uri.scheme == "file") {
            fileName = uri.lastPathSegment
            fileSize = File(uri.path ?: "").length()
        }

        if (fileName.isNullOrEmpty()) {
            val type = context.contentResolver.getType(uri)
            val ext = MimeTypeMap.getSingleton().getExtensionFromMimeType(type) ?: "bin"
            fileName = "shared_${System.currentTimeMillis()}.$ext"
        }

        val cacheDir = File(context.cacheDir, "shared_transfers").apply { mkdirs() }
        var targetFile = File(cacheDir, fileName!!)
        var counter = 1
        val dot = fileName!!.lastIndexOf('.')
        while (targetFile.exists()) {
            val base = if (dot > 0) fileName!!.substring(0, dot) else fileName!!
            val ext = if (dot > 0) fileName!!.substring(dot) else ""
            targetFile = File(cacheDir, "$base ($counter)$ext")
            counter++
        }

        try {
            context.contentResolver.openInputStream(uri)?.use { input ->
                targetFile.outputStream().use { output ->
                    input.copyTo(output)
                }
            } ?: return null
        } catch (_: Exception) {
            return null
        }

        if (fileSize <= 0) {
            fileSize = targetFile.length()
        }

        val mimeType = context.contentResolver.getType(uri) ?: "*/*"

        return mapOf(
            "path" to targetFile.absolutePath,
            "name" to targetFile.name,
            "size" to fileSize,
            "mimeType" to mimeType
        )
    }

    private fun acquireTransferLock() {
        TransferForegroundService.startService(applicationContext)

        if (wifiLock == null) {
            val wifiManager =
                applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
            val mode = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                WifiManager.WIFI_MODE_FULL_LOW_LATENCY
            } else {
                @Suppress("DEPRECATION")
                WifiManager.WIFI_MODE_FULL_HIGH_PERF
            }
            wifiLock = wifiManager.createWifiLock(mode, "plenum:transfer")
        }
        wifiLock?.let { if (!it.isHeld) it.acquire() }

        if (wakeLock == null) {
            val powerManager =
                applicationContext.getSystemService(Context.POWER_SERVICE) as PowerManager
            wakeLock = powerManager.newWakeLock(
                PowerManager.PARTIAL_WAKE_LOCK,
                "plenum:transfer"
            )
        }
        wakeLock?.let { if (!it.isHeld) it.acquire() }
    }

    private fun releaseTransferLock() {
        TransferForegroundService.stopService(applicationContext)
        wifiLock?.let { if (it.isHeld) it.release() }
        wakeLock?.let { if (it.isHeld) it.release() }
    }

    private fun exportToDownloads(source: File): String {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            val values = ContentValues().apply {
                put(MediaStore.Downloads.DISPLAY_NAME, source.name)
                put(MediaStore.Downloads.IS_PENDING, 1)
            }
            val resolver = applicationContext.contentResolver
            val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
                ?: throw IllegalStateException("MediaStore insert failed")
            resolver.openOutputStream(uri).use { out ->
                if (out == null) throw IllegalStateException("cannot open MediaStore stream")
                source.inputStream().use { it.copyTo(out) }
            }
            values.clear()
            values.put(MediaStore.Downloads.IS_PENDING, 0)
            resolver.update(uri, values, null, null)
            var name = source.name
            resolver.query(uri, arrayOf(MediaStore.Downloads.DISPLAY_NAME), null, null, null)
                ?.use { cursor ->
                    if (cursor.moveToFirst()) name = cursor.getString(0)
                }
            return "Download/$name"
        }

        @Suppress("DEPRECATION")
        val downloads = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS)
        downloads.mkdirs()
        var target = File(downloads, source.name)
        var counter = 1
        while (target.exists()) {
            val dot = source.name.lastIndexOf('.')
            target = if (dot > 0) {
                File(downloads, "${source.name.substring(0, dot)} ($counter)${source.name.substring(dot)}")
            } else {
                File(downloads, "${source.name} ($counter)")
            }
            counter++
        }
        source.copyTo(target)
        MediaScannerConnection.scanFile(applicationContext, arrayOf(target.path), null, null)
        return "Download/${target.name}"
    }
}
