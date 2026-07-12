import 'dart:io';
import 'package:flutter/services.dart';

/// Registers a received file with Android's MediaStore.
///
/// The Rust engine writes files with a plain filesystem path. On Android 10+
/// such files are not indexed by MediaStore automatically, so stock Files /
/// Downloads / Gallery apps do not show them even though they exist on disk.
/// A `MediaScannerConnection.scanFile` call over the platform channel makes the
/// file appear.
class MediaScanner {
  static const MethodChannel _channel = MethodChannel('plenum/media');

  /// Best-effort MediaStore registration. No-op off Android; swallows failures
  /// so a scan hiccup never breaks the transfer flow.
  static Future<void> scan(String path) async {
    if (!Platform.isAndroid) return;
    try {
      await _channel.invokeMethod('scanFile', {'path': path});
    } catch (_) {
      // Best-effort only.
    }
  }
}
