import 'dart:convert';
import 'package:http/http.dart' as http;

/// One STUN/TURN server entry, mirroring `plenum::signaling::IceServer`.
///
/// `urls` is kept as a single string in the UI/storage layer (like the
/// desktop app's settings), and wrapped into a one-element list when built
/// into the JSON payload the Rust FFI expects.
class IceServerSetting {
  String urls;
  String? username;
  String? credential;

  IceServerSetting({required this.urls, this.username, this.credential});

  factory IceServerSetting.fromJson(Map<String, dynamic> json) {
    return IceServerSetting(
      urls: json['urls'] as String? ?? '',
      username: json['username'] as String?,
      credential: json['credential'] as String?,
    );
  }

  Map<String, dynamic> toJson() => {
    'urls': urls,
    if (username != null && username!.isNotEmpty) 'username': username,
    if (credential != null && credential!.isNotEmpty) 'credential': credential,
  };

  /// Shape expected by the Rust side's `ice_servers_json` parameter:
  /// `{ urls: string[], username?: string, credential?: string }`.
  Map<String, dynamic> toIceServerJson() => {
    'urls': [urls],
    if (username != null && username!.isNotEmpty) 'username': username,
    if (credential != null && credential!.isNotEmpty) 'credential': credential,
  };
}

/// Internet transfer utilities (TURN credentials negotiation).
class InternetSettings {
  /// Derives the `/turn-credentials` HTTPS endpoint from a `wss://.../ws`
  /// (or `ws://.../ws`) relay signaling URL.
  static Uri? _turnCredentialsUri(String relayServerUrl, String peerId) {
    final trimmed = relayServerUrl.trim();
    if (trimmed.isEmpty) return null;
    Uri parsed;
    try {
      parsed = Uri.parse(trimmed);
    } catch (_) {
      return null;
    }
    final httpsScheme = switch (parsed.scheme) {
      'wss' || 'https' => 'https',
      'ws' || 'http' => 'http',
      _ => 'https',
    };
    return parsed.replace(
      scheme: httpsScheme,
      path: '/turn-credentials',
      queryParameters: {'peer_id': peerId},
    );
  }

  /// Builds the full `ice_servers_json` FFI payload for a transfer: the
  /// user-configured STUN/TURN servers plus freshly minted, short-lived TURN
  /// credentials fetched from the relay's `/turn-credentials` endpoint.
  ///
  /// The fetch is best-effort: if the relay has no TURN configured, is
  /// unreachable, or returns an error, we silently fall back to just the
  /// configured servers (STUN-only still works for most NATs).
  static Future<String> buildIceServersJsonWithTurn(
    String relayServerUrl,
    String peerId,
    List<IceServerSetting> configured,
  ) async {
    final maps = configured.map((s) => s.toIceServerJson()).toList();

    final uri = _turnCredentialsUri(relayServerUrl, peerId);
    if (uri != null) {
      try {
        final resp = await http
            .get(uri)
            .timeout(const Duration(seconds: 8));
        if (resp.statusCode == 200) {
          final body = jsonDecode(resp.body) as Map<String, dynamic>;
          final urls = (body['urls'] as List<dynamic>?)?.cast<String>() ?? [];
          if (urls.isNotEmpty) {
            maps.add({
              'urls': urls,
              if (body['username'] != null) 'username': body['username'],
              if (body['credential'] != null) 'credential': body['credential'],
            });
          }
        }
      } catch (_) {
        // Best-effort: fall back to configured servers only.
      }
    }

    return jsonEncode(maps);
  }
}
