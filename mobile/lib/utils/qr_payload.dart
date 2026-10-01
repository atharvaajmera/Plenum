/// Helper class for encoding and parsing Plenum QR codes.
/// URI formats:
/// Unified QR scheme: `plenum://v1/connect?v=1&r=<ROOM>&ip=<IPS>&p=<PORT>&pin=<PIN>&id=<PEER_ID>&n=<NAME>`
/// Internet room codes: `plenum://room/<CODE>`
/// LAN PINs: `plenum://pin/<PIN>`
/// Raw code fallback: `<CODE>` or `<PIN>`
enum QrPayloadType {
  room,
  pin,
  unified,
  raw,
}

class UnifiedSharePayload {
  final int version;
  final String roomCode;
  final List<String> lanIps;
  final int port;
  final String? pin;
  final String? peerId;
  final String? deviceName;

  const UnifiedSharePayload({
    this.version = 1,
    required this.roomCode,
    this.lanIps = const [],
    this.port = 0,
    this.pin,
    this.peerId,
    this.deviceName,
  });

  String toUri() {
    final query = <String, String>{
      'v': version.toString(),
      'r': roomCode.trim().toUpperCase(),
    };
    if (lanIps.isNotEmpty) {
      query['ip'] = lanIps.join(',');
    }
    if (port > 0) {
      query['p'] = port.toString();
    }
    if (pin != null && pin!.trim().isNotEmpty) {
      query['pin'] = pin!.trim();
    }
    if (peerId != null && peerId!.trim().isNotEmpty) {
      query['id'] = peerId!.trim();
    }
    if (deviceName != null && deviceName!.trim().isNotEmpty) {
      query['n'] = deviceName!.trim();
    }
    return 'plenum://v1/connect?${Uri(queryParameters: query).query}';
  }
}

class QrPayload {
  final QrPayloadType type;
  final String code;
  final UnifiedSharePayload? unifiedPayload;
  final String? rawUri;

  const QrPayload({
    required this.type,
    required this.code,
    this.unifiedPayload,
    this.rawUri,
  });

  /// Encodes a unified connection payload to a QR URI.
  static String encodeUnified(UnifiedSharePayload payload) {
    return payload.toUri();
  }

  /// Encodes an internet room code to a QR URI.
  static String encodeRoom(String code) {
    final trimmed = code.trim().toUpperCase();
    return 'plenum://room/${Uri.encodeComponent(trimmed)}';
  }

  /// Encodes a LAN PIN to a QR URI.
  static String encodePin(String pin) {
    final trimmed = pin.trim();
    return 'plenum://pin/${Uri.encodeComponent(trimmed)}';
  }

  /// Parses a raw scanned string into a [QrPayload].
  /// Returns `null` if the string is empty or invalid.
  static QrPayload? parse(String raw) {
    final trimmed = raw.trim();
    if (trimmed.isEmpty) return null;

    if (trimmed.startsWith('plenum://')) {
      final withoutPrefix = trimmed.substring('plenum://'.length);

      // Check for query parameter unified format: plenum://v1/connect?... or plenum://connect?...
      final qIndex = withoutPrefix.indexOf('?');
      if (qIndex != -1) {
        try {
          final queryStr = withoutPrefix.substring(qIndex + 1);
          final params = Uri.splitQueryString(queryStr);
          final room = (params['r'] ?? params['room'] ?? '').trim().toUpperCase();
          final ipStr = params['ip'] ?? params['ips'] ?? '';
          final lanIps = ipStr.isNotEmpty
              ? ipStr.split(',').map((s) => s.trim()).where((s) => s.isNotEmpty).toList()
              : <String>[];
          final port = int.tryParse(params['p'] ?? params['port'] ?? '0') ?? 0;
          final pin = params['pin']?.trim();
          final peerId = params['id']?.trim() ?? params['peer']?.trim();
          final deviceName = params['n']?.trim() ?? params['name']?.trim();
          final version = int.tryParse(params['v'] ?? '1') ?? 1;

          if (room.isNotEmpty) {
            final payload = UnifiedSharePayload(
              version: version,
              roomCode: room,
              lanIps: lanIps,
              port: port,
              pin: pin != null && pin.isNotEmpty ? pin : null,
              peerId: peerId != null && peerId.isNotEmpty ? peerId : null,
              deviceName: deviceName != null && deviceName.isNotEmpty ? deviceName : null,
            );
            return QrPayload(
              type: QrPayloadType.unified,
              code: room,
              unifiedPayload: payload,
              rawUri: trimmed,
            );
          }
        } catch (_) {}
      }

      final slashIndex = withoutPrefix.indexOf('/');
      if (slashIndex == -1) {
        return null;
      }

      final typeStr = withoutPrefix.substring(0, slashIndex).toLowerCase();
      final encodedCode = withoutPrefix.substring(slashIndex + 1);
      final decodedCode = Uri.decodeComponent(encodedCode).trim();

      if (decodedCode.isEmpty) return null;

      if (typeStr == 'room') {
        final upper = decodedCode.toUpperCase();
        return QrPayload(
          type: QrPayloadType.room,
          code: upper,
          unifiedPayload: UnifiedSharePayload(roomCode: upper),
          rawUri: trimmed,
        );
      } else if (typeStr == 'pin') {
        return QrPayload(
          type: QrPayloadType.pin,
          code: decodedCode,
          rawUri: trimmed,
        );
      }
      return null;
    }

    // Fallback: treat 9-character alphanumeric strings as room code
    final cleaned = trimmed.replaceAll('-', '').toUpperCase();
    if (RegExp(r'^[A-Z0-9]{9}$').hasMatch(cleaned)) {
      return QrPayload(
        type: QrPayloadType.room,
        code: cleaned,
        unifiedPayload: UnifiedSharePayload(roomCode: cleaned),
        rawUri: trimmed,
      );
    }

    // Fallback for raw code
    return QrPayload(
      type: QrPayloadType.raw,
      code: trimmed,
      rawUri: trimmed,
    );
  }

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is QrPayload &&
          runtimeType == other.runtimeType &&
          type == other.type &&
          code == other.code;

  @override
  int get hashCode => type.hashCode ^ code.hashCode;

  @override
  String toString() => 'QrPayload(type: $type, code: $code)';
}
