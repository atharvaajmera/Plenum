import 'dart:io';
import 'package:device_info_plus/device_info_plus.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';
import 'package:permission_handler/permission_handler.dart';

/// Storage strategy for received files under Android scoped storage.
///
/// The Rust engine writes files with a plain `std::fs` path, which on modern
/// Android can only reach app-specific directories. So we receive into the
/// app's external files dir (no permission needed), and once a transfer
/// completes, [exportToDownloads] copies the file into the user's public
/// Download collection via MediaStore (also no permission on Android 10+).
///
/// The app-dir copy is kept afterwards: "Open" and "Share" need a real
/// filesystem path, which MediaStore URIs don't provide.
class ReceiveStorage {
  static const MethodChannel _channel = MethodChannel('plenum/media');

  /// The receive dir is app-specific and the Downloads export goes through
  /// MediaStore, so Android 10+ needs no permission. Android 9 and below
  /// still needs the legacy WRITE permission for the direct-copy export.
  static Future<bool> ensurePermission() async {
    if (!Platform.isAndroid) return true;
    final info = await DeviceInfoPlugin().androidInfo;
    if (info.version.sdkInt >= 29) return true;
    final status = await Permission.storage.request();
    return status.isGranted;
  }

  /// Returns the directory received files should be written to: the
  /// app-specific external dir on Android (user-browsable under
  /// Android/data, survives without any permission), else app documents.
  static Future<String> outputDir() async {
    if (Platform.isAndroid) {
      final external = await getExternalStorageDirectory();
      if (external != null) {
        final dir = Directory('${external.path}/received');
        if (!await dir.exists()) {
          await dir.create(recursive: true);
        }
        return dir.path;
      }
    }
    final docs = await getApplicationDocumentsDirectory();
    return docs.path;
  }

  /// Copies a completed received file into the public Downloads collection.
  /// Returns a display string of where it landed (e.g. "Download/photo.jpg"),
  /// or null if the export failed / isn't applicable on this platform.
  static Future<String?> exportToDownloads(String path) async {
    if (!Platform.isAndroid) return null;
    try {
      final saved = await _channel.invokeMethod<String>('saveToDownloads', {'path': path});
      return saved;
    } catch (_) {
      return null;
    }
  }
}
