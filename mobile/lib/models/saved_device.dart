import 'dart:convert';

/// Represents a paired or saved device that the user trusts.
class SavedDevice {
  final String id;
  final String name;
  final String? platform; // 'desktop', 'mobile', 'tablet', etc.
  final DateTime addedAt;
  final bool autoAccept;

  const SavedDevice({
    required this.id,
    required this.name,
    this.platform,
    required this.addedAt,
    this.autoAccept = true,
  });

  SavedDevice copyWith({
    String? id,
    String? name,
    String? platform,
    DateTime? addedAt,
    bool? autoAccept,
  }) {
    return SavedDevice(
      id: id ?? this.id,
      name: name ?? this.name,
      platform: platform ?? this.platform,
      addedAt: addedAt ?? this.addedAt,
      autoAccept: autoAccept ?? this.autoAccept,
    );
  }

  Map<String, dynamic> toJson() => {
    'id': id,
    'name': name,
    'platform': platform,
    'addedAt': addedAt.toIso8601String(),
    'autoAccept': autoAccept,
  };

  factory SavedDevice.fromJson(Map<String, dynamic> json) => SavedDevice(
    id: json['id'] as String? ?? DateTime.now().millisecondsSinceEpoch.toString(),
    name: json['name'] as String? ?? 'Unknown Device',
    platform: json['platform'] as String?,
    addedAt: json['addedAt'] != null
        ? DateTime.tryParse(json['addedAt'] as String) ?? DateTime.now()
        : DateTime.now(),
    autoAccept: json['autoAccept'] as bool? ?? true,
  );

  static List<SavedDevice> decodeList(List<String> rawList) {
    final result = <SavedDevice>[];
    for (final str in rawList) {
      try {
        final decoded = jsonDecode(str);
        if (decoded is Map<String, dynamic>) {
          result.add(SavedDevice.fromJson(decoded));
        }
      } catch (_) {}
    }
    return result;
  }

  static List<String> encodeList(List<SavedDevice> devices) {
    return devices.map((d) => jsonEncode(d.toJson())).toList();
  }
}
