import 'dart:async';
import 'package:flutter/services.dart';

/// Represents a file shared into Plenum via Android ACTION_SEND / ACTION_SEND_MULTIPLE.
class SharedFile {
  final String path;
  final String name;
  final int size;
  final String? mimeType;

  const SharedFile({
    required this.path,
    required this.name,
    required this.size,
    this.mimeType,
  });

  factory SharedFile.fromMap(Map<dynamic, dynamic> map) {
    return SharedFile(
      path: map['path'] as String? ?? '',
      name: map['name'] as String? ?? 'shared_file',
      size: (map['size'] as num?)?.toInt() ?? 0,
      mimeType: map['mimeType'] as String?,
    );
  }
}

/// Service handling Android sharing intents (ACTION_SEND, ACTION_SEND_MULTIPLE, ACTION_VIEW).
class ShareIntentHandler {
  static const MethodChannel _channel = MethodChannel('plenum/share');
  static final StreamController<List<SharedFile>> _streamController =
      StreamController<List<SharedFile>>.broadcast();

  static bool _initialized = false;
  static List<SharedFile>? _initialCache;
  static Future<List<SharedFile>>? _initialFuture;

  /// Initializes the method call handler for runtime share intents.
  static void initialize() {
    if (_initialized) return;
    _initialized = true;

    _channel.setMethodCallHandler((call) async {
      if (call.method == 'onSharedFiles') {
        final list = call.arguments as List<dynamic>?;
        if (list != null) {
          final files = list
              .whereType<Map<dynamic, dynamic>>()
              .map((e) => SharedFile.fromMap(e))
              .where((f) => f.path.isNotEmpty)
              .toList();
          if (files.isNotEmpty) {
            _streamController.add(files);
          }
        }
      }
    });
  }

  /// Retrieves initial shared files if the app was launched via ACTION_SEND / ACTION_SEND_MULTIPLE.
  /// Results are cached across callers on startup so multiple widgets can safely inspect them.
  static Future<List<SharedFile>> getInitialSharing() {
    initialize();
    if (_initialCache != null) {
      return Future.value(_initialCache);
    }
    _initialFuture ??= _channel
        .invokeMethod<List<dynamic>>('getInitialSharing')
        .then((list) {
      final files = (list ?? [])
          .whereType<Map<dynamic, dynamic>>()
          .map((e) => SharedFile.fromMap(e))
          .where((f) => f.path.isNotEmpty)
          .toList();
      _initialCache = files;
      return files;
    }).catchError((_) {
      _initialCache = <SharedFile>[];
      return <SharedFile>[];
    });
    return _initialFuture!;
  }

  /// Consumes and clears the initial sharing cache once processed.
  static void consumeInitialSharing() {
    _initialCache = [];
  }

  /// Stream of shared files received while the app is running in foreground or background.
  static Stream<List<SharedFile>> getSharingStream() {
    initialize();
    return _streamController.stream;
  }

  /// Cleans up temporary copies in the shared_transfers cache directory.
  static Future<void> clearSharedCache() async {
    _initialCache = [];
    _initialFuture = null;
    try {
      await _channel.invokeMethod('clearSharedCache');
    } catch (_) {}
  }
}
