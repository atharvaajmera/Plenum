import 'dart:convert';
import 'package:shared_preferences/shared_preferences.dart';

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

const _relayServerUrlKey = 'internet.relay_server_url';
const _iceServersKey = 'internet.ice_servers';

/// Persists internet-transfer settings (relay server URL + ICE servers) via
/// shared_preferences, mirroring the desktop app's localStorage-backed
/// `SettingsContext`.
class InternetSettings {
  static const List<Map<String, String>> _defaultIceServers = [
    {'urls': 'stun:stun.l.google.com:19302'},
  ];

  static Future<String> loadRelayServerUrl() async {
    final prefs = await SharedPreferences.getInstance();
    return prefs.getString(_relayServerUrlKey) ?? '';
  }

  static Future<void> saveRelayServerUrl(String url) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(_relayServerUrlKey, url);
  }

  static Future<List<IceServerSetting>> loadIceServers() async {
    final prefs = await SharedPreferences.getInstance();
    final raw = prefs.getString(_iceServersKey);
    if (raw == null) {
      return _defaultIceServers.map((s) => IceServerSetting.fromJson(s)).toList();
    }
    try {
      final decoded = jsonDecode(raw) as List<dynamic>;
      return decoded
          .map((e) => IceServerSetting.fromJson(e as Map<String, dynamic>))
          .toList();
    } catch (_) {
      return _defaultIceServers.map((s) => IceServerSetting.fromJson(s)).toList();
    }
  }

  static Future<void> saveIceServers(List<IceServerSetting> servers) async {
    final prefs = await SharedPreferences.getInstance();
    final encoded = jsonEncode(servers.map((s) => s.toJson()).toList());
    await prefs.setString(_iceServersKey, encoded);
  }

  /// Encodes the given servers into the JSON string the Rust FFI's
  /// `ice_servers_json` parameter expects.
  static String encodeIceServersForFfi(List<IceServerSetting> servers) {
    return jsonEncode(servers.map((s) => s.toIceServerJson()).toList());
  }
}
