import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:mobile/src/rust/api/plenum_api.dart';
import '../config.dart';
import '../services/internet_settings.dart';
import '../services/media_scanner.dart';
import '../services/receive_storage.dart';
import '../theme.dart';
import '../widgets/animated_radar.dart';

enum _TransferMode { local, internet }

String _friendlyState(String state) {
  switch (state) {
    case 'Discovering':
      return 'Searching...';
    case 'Listening':
      return 'Ready to receive files';
    case 'Connecting':
    case 'SignalingConnected':
      return 'Connecting to device...';
    case 'NegotiatingIce':
      return 'Establishing connection...';
    case 'Connected':
      return 'Connected to device...';
    default:
      return 'Connecting to device...';
  }
}

class ReceiveScreen extends StatefulWidget {
  const ReceiveScreen({super.key});

  @override
  State<ReceiveScreen> createState() => _ReceiveScreenState();
}

class _ReceiveScreenState extends State<ReceiveScreen> {
  _TransferMode _mode = _TransferMode.local;

  bool _isListening = false;
  String _statusMessage = 'Tap radar to start receiving';
  String? _pin;
  double? _progress;
  bool _copied = false;

  String? _roomCode;
  bool _roomCodeCopied = false;
  bool _remoteStarted = false;

  void _startReceiving() async {
    final granted = await ReceiveStorage.ensurePermission();
    if (!granted) {
      if (mounted) {
        setState(() => _statusMessage = 'Storage permission needed to save files');
      }
      return;
    }
    final outputDir = await ReceiveStorage.outputDir();
    setState(() {
      _isListening = true;
      _statusMessage = 'Listening for incoming files...';
    });

    startReceive(outputDir: outputDir, port: 0, announce: true).listen((eventJson) {
      final event = jsonDecode(eventJson);

      if (event['Log'] != null) {
        // TEMP DIAG
        // ignore: avoid_print
        print('[PLENUM] ${event['Log']['message']}');
        return;
      }

      if (event['Discovery'] != null) {
        final discEvent = event['Discovery'];
        if (discEvent['BroadcastStarted'] != null) {
          setState(() {
            _pin = discEvent['BroadcastStarted']['token'];
            _statusMessage = 'Ready to receive files';
          });
        }
      } else if (event['Transfer'] != null) {
        final transEvent = event['Transfer'];
        if (transEvent['Started'] != null) {
          setState(() => _statusMessage = 'Receiving ${transEvent['Started']['file_name']}...');
        } else if (transEvent['Progress'] != null) {
          final p = transEvent['Progress'];
          setState(() {
            _progress = p['transferred_bytes'] / p['total_bytes'];
          });
        } else if (transEvent['Completed'] != null) {
          final fileName = transEvent['Completed']['file_name'];
          if (fileName != null) MediaScanner.scan('$outputDir/$fileName');
          setState(() {
            _statusMessage = 'Transfer complete!';
            _progress = 1.0;
            _isListening = false;
          });
        }
      }
    }, onError: (e) {
      setState(() {
        _statusMessage = 'Error: $e';
        _isListening = false;
      });
    });
  }

  Future<void> _setupRemoteReceiver() async {
    if (_remoteStarted) return;
    _remoteStarted = true;

    final granted = await ReceiveStorage.ensurePermission();
    if (!granted) {
      if (mounted) {
        setState(() => _statusMessage = 'Storage permission needed to save files');
      }
      _remoteStarted = false;
      return;
    }

    setState(() {
      _statusMessage = 'Generating room code...';
      _roomCode = null;
      _progress = null;
    });

    final code = generateRoomCodeSync();
    final myPeerId = generatePeerIdSync();

    if (!mounted) return;
    setState(() => _roomCode = code);

    const relayServerUrl = PlenumConfig.relayServerUrl;
    final iceServers = PlenumConfig.defaultIceServers();

    if (mounted) setState(() => _statusMessage = 'Waiting for sender...');

    final outputDir = await ReceiveStorage.outputDir();
    final iceServersJson = await InternetSettings.buildIceServersJsonWithTurn(
      relayServerUrl,
      myPeerId,
      iceServers,
    );

    startReceiveRemote(
      outputDir: outputDir,
      relayServerUrl: relayServerUrl,
      sessionId: code,
      myPeerId: myPeerId,
      iceServersJson: iceServersJson,
      connectTimeoutSecs: BigInt.from(30),
    ).listen((eventJson) {
      final event = jsonDecode(eventJson);
      if (event['Log'] != null) {
        // TEMP DIAG
        // ignore: avoid_print
        print('[PLENUM] ${event['Log']['message']}');
        return;
      }
      if (event['Transfer'] != null) {
        final trans = event['Transfer'];
        if (trans['StateChanged'] != null) {
          if (trans['StateChanged']['state'] != 'Closed') {
            setState(() {
              _statusMessage = trans['StateChanged']['state'] == 'Connected'
                  ? 'Connected to device...'
                  : _friendlyState(trans['StateChanged']['state']);
            });
          }
        } else if (trans['Started'] != null) {
          setState(() {
            _statusMessage = 'Receiving ${trans['Started']['file_name']}...';
            _progress = 0.0;
          });
        } else if (trans['Progress'] != null) {
          setState(() {
            _progress = trans['Progress']['transferred_bytes'] / trans['Progress']['total_bytes'];
          });
        } else if (trans['Completed'] != null) {
          final fileName = trans['Completed']['file_name'];
          if (fileName != null) MediaScanner.scan('$outputDir/$fileName');
          setState(() {
            _statusMessage = 'Received successfully!';
            _progress = 1.0;
          });
        }
      }
    }, onError: (e) {
      if (mounted) setState(() => _statusMessage = 'Error: $e');
    });
  }

  void _copyPin() {
    if (_pin != null) {
      Clipboard.setData(ClipboardData(text: _pin!));
      setState(() => _copied = true);
      Future.delayed(const Duration(seconds: 2), () {
        if (mounted) setState(() => _copied = false);
      });
    }
  }

  void _copyRoomCode() {
    if (_roomCode != null) {
      Clipboard.setData(ClipboardData(text: _roomCode!));
      setState(() => _roomCodeCopied = true);
      Future.delayed(const Duration(seconds: 2), () {
        if (mounted) setState(() => _roomCodeCopied = false);
      });
    }
  }

  void _switchMode(_TransferMode mode) {
    setState(() {
      _mode = mode;
      _statusMessage = mode == _TransferMode.local
          ? 'Tap radar to start receiving'
          : 'Preparing...';
    });
    if (mode == _TransferMode.internet) {
      _remoteStarted = false;
      _setupRemoteReceiver();
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Receive')),
      body: Center(
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Row(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                _ModeChip(
                  icon: Icons.wifi,
                  label: 'Local Network',
                  selected: _mode == _TransferMode.local,
                  onTap: () => _switchMode(_TransferMode.local),
                ),
                const SizedBox(width: 12),
                _ModeChip(
                  icon: Icons.public,
                  label: 'Internet',
                  selected: _mode == _TransferMode.internet,
                  onTap: () => _switchMode(_TransferMode.internet),
                ),
              ],
            ),
            const SizedBox(height: 32),

            if (_mode == _TransferMode.local)
              GestureDetector(
                onTap: _isListening ? null : _startReceiving,
                child: AnimatedRadar(isListening: _isListening),
              )
            else
              const Icon(Icons.public, size: 96, color: AppTheme.accentPrimary),

            const SizedBox(height: 40),
            Text(
              _statusMessage,
              style: const TextStyle(fontSize: 16, color: AppTheme.textSecondary),
              textAlign: TextAlign.center,
            ),

            if (_mode == _TransferMode.local && _pin != null)
              Container(
                margin: const EdgeInsets.only(top: 24),
                padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 16),
                decoration: BoxDecoration(
                  color: AppTheme.bgCard,
                  borderRadius: BorderRadius.circular(12),
                  border: Border.all(color: AppTheme.accentPrimary, width: 1, style: BorderStyle.solid),
                ),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    const Text('PIN Required', style: TextStyle(fontSize: 12, color: AppTheme.textSecondary)),
                    const SizedBox(height: 8),
                    Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Text(
                          _pin!,
                          style: const TextStyle(
                            fontSize: 28,
                            fontWeight: FontWeight.bold,
                            letterSpacing: 6,
                            color: AppTheme.accentPrimary
                          )
                        ),
                        const SizedBox(width: 16),
                        GestureDetector(
                          onTap: _copyPin,
                          child: Container(
                            padding: const EdgeInsets.all(8),
                            decoration: BoxDecoration(
                              color: AppTheme.bgSidebar,
                              borderRadius: BorderRadius.circular(8),
                            ),
                            child: Icon(
                              _copied ? Icons.check : Icons.copy,
                              size: 20,
                              color: _copied ? AppTheme.accentPrimary : AppTheme.textSecondary,
                            ),
                          ),
                        ),
                      ],
                    ),
                  ],
                ),
              ),

            if (_mode == _TransferMode.internet && _roomCode != null)
              Container(
                margin: const EdgeInsets.only(top: 24),
                padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 16),
                decoration: BoxDecoration(
                  color: AppTheme.bgCard,
                  borderRadius: BorderRadius.circular(12),
                  border: Border.all(color: AppTheme.accentPrimary, width: 1),
                ),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    const Text('Room Code', style: TextStyle(fontSize: 12, color: AppTheme.textSecondary)),
                    const SizedBox(height: 8),
                    Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Text(
                          _roomCode!,
                          style: const TextStyle(
                            fontSize: 28,
                            fontWeight: FontWeight.bold,
                            letterSpacing: 6,
                            color: AppTheme.accentPrimary
                          )
                        ),
                        const SizedBox(width: 16),
                        GestureDetector(
                          onTap: _copyRoomCode,
                          child: Container(
                            padding: const EdgeInsets.all(8),
                            decoration: BoxDecoration(
                              color: AppTheme.bgSidebar,
                              borderRadius: BorderRadius.circular(8),
                            ),
                            child: Icon(
                              _roomCodeCopied ? Icons.check : Icons.copy,
                              size: 20,
                              color: _roomCodeCopied ? AppTheme.accentPrimary : AppTheme.textSecondary,
                            ),
                          ),
                        ),
                      ],
                    ),
                  ],
                ),
              ),

            if (_progress != null)
              Padding(
                padding: const EdgeInsets.all(32.0),
                child: ClipRRect(
                  borderRadius: BorderRadius.circular(8),
                  child: LinearProgressIndicator(
                    value: _progress,
                    minHeight: 8,
                    backgroundColor: AppTheme.bgSidebar,
                    valueColor: const AlwaysStoppedAnimation<Color>(AppTheme.accentPrimary),
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }
}

class _ModeChip extends StatelessWidget {
  final IconData icon;
  final String label;
  final bool selected;
  final VoidCallback onTap;

  const _ModeChip({required this.icon, required this.label, required this.selected, required this.onTap});

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
        decoration: BoxDecoration(
          color: AppTheme.bgCard,
          borderRadius: BorderRadius.circular(20),
          border: Border.all(color: selected ? AppTheme.accentPrimary : AppTheme.borderColor, width: selected ? 2 : 1),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(icon, size: 16, color: selected ? AppTheme.accentPrimary : AppTheme.textSecondary),
            const SizedBox(width: 6),
            Text(label, style: TextStyle(color: selected ? AppTheme.accentPrimary : AppTheme.textSecondary, fontWeight: FontWeight.w600, fontSize: 12)),
          ],
        ),
      ),
    );
  }
}
