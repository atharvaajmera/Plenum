package com.example.mobile

import android.media.MediaScannerConnection
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

class MainActivity : FlutterActivity() {
    private val channelName = "plenum/media"

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, channelName)
            .setMethodCallHandler { call, result ->
                when (call.method) {
                    "scanFile" -> {
                        val path = call.argument<String>("path")
                        if (path.isNullOrEmpty()) {
                            result.error("NO_PATH", "path argument missing", null)
                        } else {
                            // Register the raw-written file with MediaStore so it
                            // becomes visible in Files / Downloads / Gallery. Files
                            // created via a plain filesystem path are not indexed
                            // automatically on Android 10+.
                            MediaScannerConnection.scanFile(
                                applicationContext,
                                arrayOf(path),
                                null,
                                null
                            )
                            result.success(true)
                        }
                    }
                    else -> result.notImplemented()
                }
            }
    }
}
