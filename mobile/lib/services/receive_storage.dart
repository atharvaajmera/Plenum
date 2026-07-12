import 'dart:io';
import 'package:path_provider/path_provider.dart';
import 'package:permission_handler/permission_handler.dart';

/// Resolves the directory that received files are written into, and ensures the
/// app can actually write there.
///
/// We want files to land somewhere the user can find them (the phone's public
/// Download folder), not the app's private sandbox
/// (`getApplicationDocumentsDirectory`, invisible to the Files app / Gallery
/// and wiped on uninstall).
///
/// The Rust engine writes files with a plain `std::fs` path, so on Android it
/// needs broad filesystem access to reach `/storage/emulated/0/Download`. That
/// is `MANAGE_EXTERNAL_STORAGE` ("All files access"), which is a settings-page
/// grant rather than a normal runtime prompt.
class ReceiveStorage {
  static const String _publicDownloadPath = '/storage/emulated/0/Download';

  /// Best-effort request for the storage permission needed to write into the
  /// public Download folder. Returns true if we believe we can write there.
  static Future<bool> ensurePermission() async {
    if (!Platform.isAndroid) return true;

    // Android 11+: All-files access is the only way a raw filesystem path can
    // write into public Download. `manageExternalStorage` opens the dedicated
    // system settings toggle.
    if (await Permission.manageExternalStorage.isGranted) return true;

    final status = await Permission.manageExternalStorage.request();
    if (status.isGranted) return true;

    // Fallback for older devices (Android 10 and below) that still honor the
    // legacy WRITE_EXTERNAL_STORAGE runtime permission.
    final legacy = await Permission.storage.request();
    return legacy.isGranted;
  }

  /// Returns the directory received files should be written to. Prefers the
  /// public Download folder; falls back to the app's external files dir, then
  /// the private documents dir, so a transfer never hard-fails on storage.
  static Future<String> outputDir() async {
    if (Platform.isAndroid) {
      final publicDownload = Directory(_publicDownloadPath);
      if (await publicDownload.exists()) {
        return publicDownload.path;
      }
      final external = await getExternalStorageDirectory();
      if (external != null) {
        return external.path;
      }
    }
    final docs = await getApplicationDocumentsDirectory();
    return docs.path;
  }
}
