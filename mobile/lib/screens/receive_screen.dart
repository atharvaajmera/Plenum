import 'dart:async';
import 'dart:convert';
import 'package:network_info_plus/network_info_plus.dart';
import 'package:open_filex/open_filex.dart';
import 'package:share_plus/share_plus.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:mobile/src/rust/api/plenum_api.dart';
import 'package:permission_handler/permission_handler.dart';
import '../services/receive_storage.dart';
import '../services/internet_settings.dart';
import '../theme.dart';
import '../widgets/animated_radar.dart';
import 'package:provider/provider.dart';
import '../services/settings_service.dart';

import '../utils/transfer_status.dart';
import '../utils/formatters.dart';
import 'settings_screen.dart';

class ReceiveScreen extends StatefulWidget {
  const ReceiveScreen({super.key});

  @override
  State<ReceiveScreen> createState() => _ReceiveScreenState();
}

class _ReceiveScreenState extends State<ReceiveScreen> {
  TransferMode _mode = TransferMode.local;
  bool _initializedMode = false;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (!_initializedMode) {
      final settings = context.read<SettingsService>();
      setState(() {
        _mode = settings.defaultTransferMode == 0 ? TransferMode.local : TransferMode.internet;
      });
      _initializedMode = true;
    }
  }

  bool _isListening = false;
  String _statusMessage = 'Tap radar to start receiving';
  String? _pin;
  double? _progress;
  bool _copied = false;

  String? _roomCode;
  bool _roomCodeCopied = false;
  bool _remoteStarted = false;

  StreamSubscription<String>? _localSub;
  StreamSubscription<String>? _remoteSub;
  String? _localAddress;
  String? _sessionToken;
  bool _requirePinActive = false;
  bool _autoAcceptActive = true;

  int? _totalBytes;
  int? _transferredBytes;
  DateTime? _transferStartTime;
  String? _speedText;
  String? _etaText;
  String? _savedFilePath;
  String? _savedLocation;

  void _fetchAndSetLocalAddress(int port) async {
    try {
      final ip = await NetworkInfo().getWifiIP();
      if (ip != null && mounted) {
        setState(() {
          _localAddress = '$ip:$port';
        });
      }
    } catch (_) {}
  }

  void _handleLogEvent(dynamic log) {
    final level = log['level'];
    if (level == 'Warn' || level == 'Error') {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(
        content: Text(log['message'] ?? 'Error occurred'),
        backgroundColor: level == 'Error' ? Colors.redAccent : Colors.orange,
      ));
    }
  }

  @override
  void dispose() {
    final token = _sessionToken;
    if (token != null) {
      try {
        cancelSession(sessionToken: token);
      } catch (_) {}
    }
    _localSub?.cancel();
    _remoteSub?.cancel();
    super.dispose();
  }

  void _stopReceiving() {
    // Cancel Rust-side transfer loop first, then drop the Dart subscriptions.
    final token = _sessionToken;
    if (token != null) {
      try {
        cancelSession(sessionToken: token);
      } catch (_) {}
    }
    _sessionToken = null;
    _localSub?.cancel();
    _remoteSub?.cancel();
    setState(() {
      _isListening = false;
      _remoteStarted = false;
      _statusMessage = 'Tap radar to start receiving';
      _pin = null;
      _requirePinActive = false;
      _roomCode = null;
      _progress = null;
      _localAddress = null;
      _totalBytes = null;
      _transferredBytes = null;
      _transferStartTime = null;
      _speedText = null;
      _etaText = null;
      _savedFilePath = null;
      _savedLocation = null;
    });
  }

  Future<void> _startLocalReceiver() async {
    if (_isListening) return;

    final grantedStorage = await ReceiveStorage.ensurePermission();
    if (!grantedStorage) {
      if (mounted) {
        setState(() => _statusMessage = 'Storage permission needed to save files');
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content: const Text('Storage permission is required to save files to Download'),
          action: SnackBarAction(label: 'Settings', onPressed: () => openAppSettings()),
        ));
      }
      return;
    }

    await Permission.nearbyWifiDevices.request();
    if (!mounted) return;

    final outputDir = await ReceiveStorage.outputDir();
    final settings = context.read<SettingsService>();
    final deviceName = settings.deviceName;
    final requirePin = settings.requirePin;
    final autoAccept = settings.autoAccept;

    setState(() {
      _isListening = true;
      _requirePinActive = requirePin;
      _autoAcceptActive = autoAccept;
      _statusMessage = 'Listening for incoming files...';
    });

    final sessionToken = DateTime.now().millisecondsSinceEpoch.toString();
    _sessionToken = sessionToken;
    _localSub = startReceive(
      outputDir: outputDir,
      port: 0,
      announce: true,
      deviceName: deviceName,
      sessionToken: sessionToken,
      requirePin: requirePin,
      autoAccept: autoAccept,
    ).listen((eventJson) {
      if (!mounted) return;
      final event = jsonDecode(eventJson);

      if (event['Log'] != null) {
        _handleLogEvent(event['Log']);
        return;
      }

      if (event['Discovery'] != null) {
        final discEvent = event['Discovery'];
        if (discEvent['BroadcastStarted'] != null) {
          final port = discEvent['BroadcastStarted']['port'];
          setState(() {
            _pin = discEvent['BroadcastStarted']['token'];
            _statusMessage = 'Ready to receive files';
          });
          _fetchAndSetLocalAddress(port);
        }
      } else if (event['Transfer'] != null) {
        _handleTransferEvent(event['Transfer'], outputDir);
      }
    }, onError: (e) {
      setState(() {
        _statusMessage = 'Error: $e';
        _isListening = false;
      });
    });
  }

  void _handleTransferEvent(dynamic transEvent, String outputDir) {
    if (transEvent['StateChanged'] != null) {
      final state = transEvent['StateChanged']['state'];
      if (state == 'Closed') {
        _stopReceiving();
        setState(() => _statusMessage = 'Connection closed');
      } else {
        setState(() {
          _statusMessage = state == 'Connected' ? 'Connected to device...' : friendlyState(state, isReceive: true);
        });
      }
    } else if (transEvent['IncomingRequest'] != null) {
      final req = transEvent['IncomingRequest'];
      _showIncomingRequestDialog(
        fileName: req['file_name'] ?? 'Unknown file',
        totalBytes: req['total_bytes'] ?? 0,
        peer: req['peer'],
      );
    } else if (transEvent['Cancelled'] != null) {
      setState(() {
        _statusMessage = 'Transfer cancelled';
        _progress = null;
      });
    } else if (transEvent['Declined'] != null) {
      final reason = transEvent['Declined']['reason'];
      setState(() {
        _statusMessage = reason == 'cancelled'
            ? 'Sender cancelled the transfer'
            : 'Transfer declined';
        _progress = null;
      });
    } else if (transEvent['Started'] != null) {
      setState(() {
        _statusMessage = 'Receiving ${transEvent['Started']['file_name']}...';
        _progress = 0.0;
        _totalBytes = transEvent['Started']['total_bytes'];
        _transferStartTime = DateTime.now();
        _speedText = null;
        _etaText = null;
      });
    } else if (transEvent['Resumed'] != null) {
      setState(() {
        final resumedBytes = transEvent['Resumed']['resumed_bytes'] ?? 0;
        final percent = _totalBytes != null && _totalBytes! > 0 ? (resumedBytes / _totalBytes! * 100).toStringAsFixed(1) : '0';
        _statusMessage = 'Resuming from $percent%...';
      });
    } else if (transEvent['Progress'] != null) {
      final p = transEvent['Progress'];
      setState(() {
        _transferredBytes = p['transferred_bytes'];
        _totalBytes = p['total_bytes'];
        if (_totalBytes != null && _totalBytes! > 0) {
          _progress = _transferredBytes! / _totalBytes!;
        }

        if (_transferStartTime != null && _transferredBytes != null && _totalBytes != null) {
          final elapsed = DateTime.now().difference(_transferStartTime!);
          if (elapsed.inSeconds > 0) {
            final speedBps = _transferredBytes! / elapsed.inSeconds;
            _speedText = '${formatBytes(speedBps.round())}/s';
            final remainingBytes = _totalBytes! - _transferredBytes!;
            final etaSeconds = speedBps > 0 ? remainingBytes / speedBps : 0;
            _etaText = '${etaSeconds.round()}s left';
          }
        }
      });
    } else if (transEvent['Completed'] != null) {
      final summary = transEvent['Completed'];
      final fileName = summary['file_name'];
      final localPath = fileName != null ? '$outputDir/$fileName' : null;
      final settings = context.read<SettingsService>();
      settings.addTransferHistory({
        'direction': 'receive',
        'fileName': fileName ?? 'Unknown file',
        'size': summary['total_bytes'] ?? _totalBytes,
        'peerName': summary['peer'] ?? 'Unknown sender',
        'path': localPath,
        'timestamp': DateTime.now().toIso8601String(),
      });
      setState(() {
        _statusMessage = 'Transfer complete!';
        _progress = 1.0;
        // Keep the app-dir path for Open/Share: they need a real file path,
        // which the MediaStore copy in Downloads doesn't expose.
        _savedFilePath = localPath;
        _savedLocation = null;
      });
      // Copy into public Downloads via MediaStore (no permission needed on
      // Android 10+). Best-effort: the file is safe in app storage either way.
      if (localPath != null) {
        ReceiveStorage.exportToDownloads(localPath).then((saved) {
          if (mounted && saved != null) {
            setState(() => _savedLocation = saved);
          }
        });
      }
    }
  }

  /// Accept gate: shown when the engine emits `IncomingRequest` and
  /// auto-accept is off. The Rust loop blocks until we answer (or its
  /// 120s approval timeout declines for us).
  Future<void> _showIncomingRequestDialog({
    required String fileName,
    required int totalBytes,
    String? peer,
  }) async {
    if (_autoAcceptActive) return; // engine already proceeding
    final token = _sessionToken;
    if (token == null) return;
    setState(() => _statusMessage = 'Incoming file — waiting for your decision');
    final accepted = await showDialog<bool>(
      context: context,
      barrierDismissible: false,
      builder: (context) => AlertDialog(
        title: const Text('Incoming file'),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(fileName, style: const TextStyle(fontWeight: FontWeight.bold)),
            const SizedBox(height: 4),
            Text(formatBytes(totalBytes), style: const TextStyle(color: AppTheme.textSecondary)),
            if (peer != null) ...[
              const SizedBox(height: 4),
              Text('From: $peer', style: const TextStyle(color: AppTheme.textSecondary, fontSize: 12)),
            ],
          ],
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: const Text('Decline'),
          ),
          ElevatedButton(
            onPressed: () => Navigator.of(context).pop(true),
            child: const Text('Accept'),
          ),
        ],
      ),
    );
    if (!mounted) return;
    try {
      respondToIncoming(sessionToken: token, accept: accepted == true);
    } catch (_) {}
    if (accepted != true) {
      setState(() => _statusMessage = 'Transfer declined');
    }
  }

  Future<void> _setupRemoteReceiver() async {
    _stopReceiving();
    if (_remoteStarted) return;
    _remoteStarted = true;

    final granted = await ReceiveStorage.ensurePermission();
    if (!granted) {
      if (mounted) {
        setState(() => _statusMessage = 'Storage permission needed to save files');
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content: const Text('Storage permission is required to save files to Download'),
          action: SnackBarAction(label: 'Settings', onPressed: () => openAppSettings()),
        ));
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

    final settings = context.read<SettingsService>();
    final relayServerUrl = settings.relayServerUrl;
    final autoAccept = settings.autoAccept;
    _autoAcceptActive = autoAccept;
    final iceServers = settings.iceServers
        .split('\n')
        .map((e) => e.trim())
        .where((e) => e.isNotEmpty)
        .map((e) => IceServerSetting(urls: e))
        .toList();

    if (mounted) setState(() => _statusMessage = 'Waiting for sender...');

    final outputDir = await ReceiveStorage.outputDir();
    final iceServersJson = await InternetSettings.buildIceServersJsonWithTurn(
      relayServerUrl,
      myPeerId,
      iceServers,
    );

    final sessionToken = DateTime.now().millisecondsSinceEpoch.toString();
    _sessionToken = sessionToken;
    _remoteSub = startReceiveRemote(
      outputDir: outputDir,
      relayServerUrl: relayServerUrl,
      sessionId: code,
      myPeerId: myPeerId,
      iceServersJson: iceServersJson,
      connectTimeoutSecs: BigInt.from(600),
      sessionToken: sessionToken,
      autoAccept: autoAccept,
    ).listen((eventJson) {
      if (!mounted) return;
      final event = jsonDecode(eventJson);
      if (event['Log'] != null) {
        _handleLogEvent(event['Log']);
        return;
      }
      if (event['Transfer'] != null) {
        _handleTransferEvent(event['Transfer'], outputDir);
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

  void _switchMode(TransferMode mode) {
    setState(() {
      _mode = mode;
      _statusMessage = mode == TransferMode.local
          ? 'Tap radar to start receiving'
          : 'Preparing...';
    });
    if (mode == TransferMode.internet) {
      _remoteStarted = false;
      _setupRemoteReceiver();
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Plenum', style: TextStyle(fontWeight: FontWeight.w900, color: AppTheme.accentPrimary, letterSpacing: -0.5)),
        actions: [
          IconButton(
            icon: const Icon(Icons.settings),
            onPressed: () {
              Navigator.push(
                context,
                MaterialPageRoute(builder: (context) => const SettingsScreen()),
              );
            },
          )
        ],
      ),
      body: Center(
        child: SingleChildScrollView(
          padding: const EdgeInsets.symmetric(vertical: 24),
          child: Column(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
            Row(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                _ModeChip(
                  icon: Icons.wifi,
                  label: 'Local Network',
                  selected: _mode == TransferMode.local,
                  onTap: () => _switchMode(TransferMode.local),
                ),
                const SizedBox(width: 12),
                _ModeChip(
                  icon: Icons.public,
                  label: 'Internet',
                  selected: _mode == TransferMode.internet,
                  onTap: () => _switchMode(TransferMode.internet),
                ),
              ],
            ),
            const SizedBox(height: 32),

            if (_mode == TransferMode.local)
              GestureDetector(
                onTap: _isListening ? null : _startLocalReceiver,
                child: AnimatedRadar(isListening: _isListening),
              )
            else
              GestureDetector(
                onTap: _remoteStarted ? null : _setupRemoteReceiver,
                child: AnimatedRadar(isListening: _remoteStarted),
              ),

            const SizedBox(height: 40),
            Text(
              _statusMessage,
              style: const TextStyle(fontSize: 16, color: AppTheme.textSecondary),
              textAlign: TextAlign.center,
            ),
            
            if (_isListening || _remoteStarted)
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: TextButton.icon(
                  onPressed: _stopReceiving,
                  icon: const Icon(Icons.stop_circle, color: AppTheme.accentPrimary),
                  label: const Text('Stop Receiving', style: TextStyle(color: AppTheme.accentPrimary)),
                ),
              ),

            if (_statusMessage.startsWith('Error:'))
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: ElevatedButton.icon(
                  onPressed: () {
                    _stopReceiving();
                    if (_mode == TransferMode.local) {
                      _startLocalReceiver();
                    } else {
                      _setupRemoteReceiver();
                    }
                  },
                  icon: const Icon(Icons.refresh),
                  label: const Text('Retry'),
                ),
              ),

            if (_mode == TransferMode.local && _pin != null)
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
                    Text(
                      _requirePinActive
                          ? 'PIN required — senders must enter this code'
                          : 'Pairing code — senders can use this to find you',
                      style: const TextStyle(fontSize: 12, color: AppTheme.textSecondary),
                      textAlign: TextAlign.center,
                    ),
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
                    if (_localAddress != null) ...[
                      const SizedBox(height: 12),
                      Text('Your address: $_localAddress', style: const TextStyle(fontSize: 12, color: AppTheme.textSecondary)),
                    ],
                  ],
                ),
              ),

            if (_mode == TransferMode.internet && _roomCode != null)
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
                        const SizedBox(width: 8),
                        GestureDetector(
                          onTap: () {
                            Share.share('Use this code to send files on Plenum: $_roomCode');
                          },
                          child: Container(
                            padding: const EdgeInsets.all(8),
                            decoration: BoxDecoration(
                              color: AppTheme.bgSidebar,
                              borderRadius: BorderRadius.circular(8),
                            ),
                            child: const Icon(Icons.share, size: 20, color: AppTheme.textSecondary),
                          ),
                        ),
                      ],
                    ),
                    const SizedBox(height: 12),
                    const Text('Code valid while this screen is open', style: TextStyle(fontSize: 12, color: AppTheme.textSecondary)),
                  ],
                ),
              ),

            if (_progress != null)
              Container(
                margin: const EdgeInsets.only(top: 24),
                padding: const EdgeInsets.all(16),
                width: 300,
                decoration: BoxDecoration(
                  color: AppTheme.bgSidebar,
                  borderRadius: BorderRadius.circular(12),
                ),
                child: Column(
                  children: [
                    ClipRRect(
                      borderRadius: BorderRadius.circular(4),
                      child: LinearProgressIndicator(
                        value: _progress,
                        minHeight: 8,
                        backgroundColor: AppTheme.bgApp,
                        valueColor: const AlwaysStoppedAnimation<Color>(AppTheme.accentPrimary),
                      ),
                    ),
                    const SizedBox(height: 8),
                    Row(
                      mainAxisAlignment: MainAxisAlignment.spaceBetween,
                      children: [
                        Text(
                          '${(_progress! * 100).toStringAsFixed(1)}%',
                          style: const TextStyle(color: AppTheme.textSecondary, fontSize: 12),
                        ),
                        if (_speedText != null && _etaText != null)
                          Text(
                            '$_speedText • $_etaText',
                            style: const TextStyle(color: AppTheme.textSecondary, fontSize: 12),
                          ),
                      ],
                    ),
                    if (_progress == 1.0 && _savedFilePath != null) ...[
                      const SizedBox(height: 16),
                      Text(
                        _savedLocation != null ? 'Saved to $_savedLocation' : 'Saving to Downloads...',
                        style: const TextStyle(color: AppTheme.textSecondary, fontSize: 12),
                      ),
                      const SizedBox(height: 16),
                      Row(
                        mainAxisAlignment: MainAxisAlignment.center,
                        children: [
                          ElevatedButton(
                            onPressed: () => OpenFilex.open(_savedFilePath!),
                            child: const Text('Open'),
                          ),
                          const SizedBox(width: 8),
                          ElevatedButton(
                            onPressed: () => Share.shareXFiles([XFile(_savedFilePath!)]),
                            child: const Text('Share'),
                          ),
                        ],
                      ),
                      const SizedBox(height: 8),
                      TextButton(
                        onPressed: () {
                          _stopReceiving();
                          if (_mode == TransferMode.local) {
                            _startLocalReceiver();
                          } else {
                            _setupRemoteReceiver();
                          }
                        },
                        child: const Text('Receive another'),
                      ),
                    ]
                  ],
                ),
              ),
          ],
        ),
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
