// Relay/NAT-traversal configuration for internet transfers.
//
// These point at the deployed Plenum relay server and are intentionally NOT
// user-configurable: end users only deal with room codes. TURN credentials are
// fetched automatically at transfer time (see
// InternetSettings.buildIceServersJsonWithTurn), so only a STUN server needs to
// be listed here for direct hole-punching. Mirrors desktop's `src/config.ts`.
import 'services/internet_settings.dart';

class PlenumConfig {
  static const String relayServerUrl = 'wss://relay.plenumonline.me/ws';

  static List<IceServerSetting> defaultIceServers() => [
        IceServerSetting(urls: 'stun:relay.plenumonline.me:3478'),
      ];
}
