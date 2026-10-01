import 'dart:async';
import 'dart:convert';
import 'dart:math';
import 'package:flutter/material.dart';
import 'package:file_picker/file_picker.dart';
import 'package:permission_handler/permission_handler.dart';
import 'package:mobile/src/rust/api/plenum_api.dart';
import '../services/internet_settings.dart';
import '../theme.dart';
import 'package:provider/provider.dart';
import '../services/settings_service.dart';
import '../services/transfer_lock.dart';

import '../utils/transfer_status.dart';
import '../utils/formatters.dart';
import '../utils/transfer_metrics.dart';
import '../widgets/success_check.dart';
import '../widgets/qr_scanner_sheet.dart';
import '../utils/qr_payload.dart';
import 'settings_screen.dart';

class SendScreen extends StatefulWidget {
  const SendScreen({super.key});

  @override
  State<SendScreen> createState() => _SendScreenState();
}

class _SendScreenState extends State<SendScreen> {
  TransferUiPhase _phase = TransferUiPhase.idle;
  bool _terminalEventReceived = false;
  String? _selectedFile;
  final List<Map<String, dynamic>> _peers = [];
  bool _isDiscovering = false;
  String _transferStatus = '';
  String? _currentTransferPeerName;
  double? _progress;
  int? _selectedFileSize;

  int? _totalBytes;
  int? _transferredBytes;
  int? _resumeBaselineBytes;
  TransferMetricsState? _metrics;
  String? _speedText;
  String? _etaText;

  final TextEditingController _roomCodeController = TextEditingController();
  bool _isConnectingRemote = false;
  String? _sessionToken;
  /// Ownership token for the currently-held [TransferLock], if any.
  Object? _lockToken;
  StreamSubscription<String>? _transferSub;
  StreamSubscription<String>? _discoverySub;
  bool _transferActive = false;
  bool _showSuccess = false;
  Timer? _autoResetTimer;
  
  String? _completedFileName;
  String? _completedPeerName;
  String? _completedDuration;
  String? _completedMode;
  String? _activePathBadge;

  @override
  void initState() {
    super.initState();
    _startDiscovery();
  }

  @override
  void dispose() {
    final token = _sessionToken;
    if (token != null && !_transferActive) {
      try {
        cancelSession(sessionToken: token);
      } catch (_) {}
    }
    if (!_transferActive) _transferSub?.cancel();
    _discoverySub?.cancel();
    _autoResetTimer?.cancel();
    _roomCodeController.dispose();
    if (!_transferActive) {
      TransferLock.release(_lockToken);
      _lockToken = null;
      unawaited(FilePicker.clearTemporaryFiles());
    }
    super.dispose();
  }

  /// Cancels an in-flight transfer: flips the Rust-side flag; the engine
  /// sends `Close` to the peer, emits `Cancelled`, and returns.
  void _cancelTransfer() {
    if (_phase == TransferUiPhase.cancelling || _phase.isTerminal) return;
    final token = _sessionToken;
    if (token != null) {
      setState(() {
        _phase = TransferUiPhase.cancelling;
        _transferStatus = 'Cancelling transfer...';
      });
      try {
        cancelSession(sessionToken: token);
      } catch (_) {}
    }
  }

  void _resetTransferUi() {
    _autoResetTimer?.cancel();
    _roomCodeController.clear();
    setState(() {
      _phase = TransferUiPhase.idle;
      _terminalEventReceived = false;
      _transferStatus = '';
      _progress = null;
      _totalBytes = null;
      _transferredBytes = null;
      _resumeBaselineBytes = null;
      _metrics = null;
      _speedText = null;
      _etaText = null;
      _showSuccess = false;
      _completedFileName = null;
      _completedPeerName = null;
      _completedDuration = null;
      _completedMode = null;
      _activePathBadge = null;
      _sessionToken = null;
      _isConnectingRemote = false;
      _transferActive = false;
      _currentTransferPeerName = null;
    });
    _clearSelectedTemporaryFile();
  }

  Future<void> _clearSelectedTemporaryFile() async {
    try {
      final cleared = await FilePicker.clearTemporaryFiles();
      if (cleared != true) {
        debugPrint('File picker temporary files were not cleared');
      }
    } catch (error) {
      debugPrint('Failed to clear file picker cache: $error');
    }

    if (!mounted) return;

    setState(() {
      _selectedFile = null;
      _selectedFileSize = null;
    });
  }

  void _startDiscovery() async {
    _discoverySub?.cancel();
    _discoverySub = null;

    await Permission.nearbyWifiDevices.request();
    if (!mounted) return;

    setState(() {
      _peers.clear();
      _transferStatus = '';
      _progress = null;
      if (!_phase.isTerminal && _phase != TransferUiPhase.transferring && _phase != TransferUiPhase.connecting) {
        _phase = TransferUiPhase.discovering;
      }
    });

    _discoverySub = startDiscovery(timeoutSecs: BigInt.from(10)).listen((eventJson) {
      if (!mounted) return;
      final event = jsonDecode(eventJson);
      if (event['Discovery'] != null) {
        final discEvent = event['Discovery'];
        if (discEvent == 'PeerNotFound') {
          setState(() {
            _isDiscovering = false;
            if (_phase == TransferUiPhase.discovering) {
              _phase = TransferUiPhase.idle;
            }
            _transferStatus = 'No devices found';
          });
        } else if (discEvent is Map) {
          if (discEvent['PeerFound'] != null) {
            setState(() {
              final found = discEvent['PeerFound'];
              final token = found['token'];
              final duplicate = _peers.any((p) =>
                  p['address'] == found['address'] ||
                  (token != null && token.toString().isNotEmpty && p['token'] == token));
              if (!duplicate) {
                _peers.add(found);
              }
            });
          } else if (discEvent['SearchStarted'] != null) {
            setState(() {
              _isDiscovering = true;
              if (!_phase.isTerminal && _phase != TransferUiPhase.transferring && _phase != TransferUiPhase.connecting) {
                _phase = TransferUiPhase.discovering;
              }
            });
          }
        }
      }
    }, onDone: () {
      if (mounted) {
        setState(() {
          _isDiscovering = false;
          if (_phase == TransferUiPhase.discovering) {
            _phase = TransferUiPhase.idle;
          }
        });
      }
    });
  }

  Future<void> _pickFile() async {
    if (!_transferActive) {
      await FilePicker.clearTemporaryFiles();
    }
    FilePickerResult? result = await FilePicker.pickFiles();
    if (result != null) {
      setState(() {
        _selectedFile = result.files.single.path;
        _selectedFileSize = result.files.single.size;
      });
    }
  }

  void _handleTransferEvent(String eventJson) {
    if (!mounted) return;
    final event = jsonDecode(eventJson);
    if (event['Log'] != null) {
      final log = event['Log'];
      final level = log['level'] ?? 'Info';
      final message = log['message'] ?? '';
      debugPrint('[$level] $message');
      return;
    }
    if (event['Transfer'] != null) {
      final trans = event['Transfer'];
      if (trans['PathSelected'] != null) {
        final path = trans['PathSelected']['path'];
        setState(() {
          if (path is Map && path['Local'] != null) {
            final addr = path['Local']['address'] ?? '';
            _activePathBadge = '🟢 Local Wi-Fi ($addr)';
          } else if (path == 'DirectLan' || (path is Map && path.containsKey('DirectLan'))) {
            _activePathBadge = '🟢 Local Wi-Fi (Direct)';
          } else if (path is Map && path['Internet'] != null) {
            _activePathBadge = '🌐 Internet (${path['Internet']['connection_type'] ?? 'P2P'})';
          } else {
            _activePathBadge = '🌐 Internet ($path)';
          }
        });
      }
      if (trans['StateChanged'] != null) {
        if (_terminalEventReceived) return;
        final state = trans['StateChanged']['state'];
        if (state == 'Closed') {
          if (!_showSuccess && !_phase.isTerminal) {
            setState(() {
              _phase = TransferUiPhase.idle;
              _transferStatus = '';
              _progress = null;
              _totalBytes = null;
              _transferredBytes = null;
              _speedText = null;
              _etaText = null;
              _isConnectingRemote = false;
              _transferActive = false;
            });
          }
        } else {
          setState(() {
            _phase = TransferUiPhase.connecting;
            _transferStatus = friendlyState(state);
          });
        }
      } else if (trans['AwaitingApproval'] != null) {
        if (_terminalEventReceived) return;
        setState(() {
          _phase = TransferUiPhase.awaitingApproval;
          _transferStatus = 'Waiting for the receiver to accept...';
          _transferActive = true;
        });
      } else if (trans['Cancelled'] != null) {
        setState(() {
          _terminalEventReceived = true;
          _phase = TransferUiPhase.cancelled;
          _transferStatus = 'Transfer cancelled\nThe partial file can be resumed later.';
          _isConnectingRemote = false;
          _transferActive = false;
        });
        _autoResetTimer?.cancel();
        _autoResetTimer = Timer(const Duration(seconds: 2), () {
          if (mounted && _phase == TransferUiPhase.cancelled) _resetTransferUi();
        });
      } else if (trans['Declined'] != null) {
        final reason = trans['Declined']['reason'];
        setState(() {
          _terminalEventReceived = true;
          _phase = TransferUiPhase.failed;
          _transferStatus = switch (reason) {
            'pin_rejected' => 'Wrong pairing code — check the code on the receiver\'s screen',
            'cancelled' => 'The receiver cancelled the transfer',
            _ => 'The receiver declined the transfer',
          };
          _progress = null;
          _isConnectingRemote = false;
          _transferActive = false;
        });
        _autoResetTimer?.cancel();
        _autoResetTimer = Timer(const Duration(seconds: 2), () {
          if (mounted && _phase == TransferUiPhase.failed) _resetTransferUi();
        });
      } else if (trans['Failed'] != null) {
        setState(() {
          _terminalEventReceived = true;
          _phase = TransferUiPhase.failed;
          _transferStatus = friendlyError(trans['Failed']['message']);
          _progress = null;
          _isConnectingRemote = false;
          _transferActive = false;
        });
      } else if (trans['Started'] != null) {
        if (_terminalEventReceived) return;
        final started = trans['Started'];
        final total = started['total_bytes'] as int? ?? 0;
        final resumed = started['resumed_bytes'] as int? ?? 0;
        setState(() {
          _phase = TransferUiPhase.transferring;
          _transferStatus = resumed > 0
              ? 'Resuming ${started['file_name']} from ${formatBytes(resumed)}...'
              : 'Sending ${started['file_name']}...';
          _resumeBaselineBytes = resumed;
          _transferredBytes = resumed;
          _totalBytes = total;
          _metrics = TransferMetricsState.start(
            totalBytes: total,
            resumedBytes: resumed,
          );
          _progress = total > 0 ? (resumed / total).clamp(0.0, 1.0) : 0.0;
          _speedText = null;
          _etaText = null;
          _transferActive = true;
        });
      } else if (trans['Resumed'] != null) {
        if (_terminalEventReceived) return;
        setState(() {
          _phase = TransferUiPhase.transferring;
          final resumedBytes = trans['Resumed']['resumed_bytes'] ?? 0;
          final percent = _totalBytes != null && _totalBytes! > 0 ? (resumedBytes / _totalBytes! * 100).toStringAsFixed(1) : '0';
          _transferStatus = 'Resuming from $percent%...';
        });
      } else if (trans['Progress'] != null) {
        if (_terminalEventReceived) return;
        final currentTransferred = trans['Progress']['transferred_bytes'] as int? ?? 0;
        final total = trans['Progress']['total_bytes'] as int? ?? _totalBytes ?? 0;
        setState(() {
          _phase = TransferUiPhase.transferring;
          _transferredBytes = currentTransferred;
          _totalBytes = total;
          if (_metrics != null) {
            final update = _metrics!.update(currentTransferred);
            _progress = update.progressFraction;
            if (update.speedBps != null) {
              _speedText = '${formatBytes(update.speedBps!.round())}/s';
            }
            _etaText = update.etaSeconds != null && update.etaSeconds! > 0
                ? '${update.etaSeconds}s left'
                : '';
          } else if (_totalBytes != null && _totalBytes! > 0) {
            _progress = (currentTransferred / _totalBytes!).clamp(0.0, 1.0);
          }
        });
      } else if (trans['Completed'] != null) {
        _terminalEventReceived = true;
        final summary = trans['Completed'];
        final settings = context.read<SettingsService>();
        final peerName = summary['peer_name'] ??
            summary['peer'] ??
            _currentTransferPeerName ??
            'Unknown device';
        final elapsedMs = summary['elapsed_ms'];
        final mode = formatTransferMode(summary['mode']);
        final resumedBytes = summary['resumed_bytes'] as int? ?? _resumeBaselineBytes ?? 0;
        final totalBytes = summary['total_bytes'] as int? ?? _selectedFileSize ?? 0;
        final sessionBytes = max(0, totalBytes - resumedBytes);
        settings.addTransferHistory({
          'direction': 'send',
          'fileName': summary['file_name'] ?? _selectedFile?.split(RegExp(r'[\\/]')).last ?? 'Unknown file',
          'size': totalBytes,
          'resumedBytes': resumedBytes,
          'sessionBytes': sessionBytes,
          'peerName': peerName,
          'durationMs': elapsedMs,
          'mode': summary['mode'],
          'timestamp': DateTime.now().toIso8601String(),
        });
        _roomCodeController.clear();
        setState(() {
          _phase = TransferUiPhase.succeeded;
          _transferStatus = elapsedMs != null
              ? (resumedBytes > 0
                  ? 'Sent to $peerName in ${formatDuration(elapsedMs)} (Resumed from ${formatBytes(resumedBytes)})'
                  : 'Sent to $peerName in ${formatDuration(elapsedMs)}')
              : 'Sent to $peerName';
          _progress = 1.0;
          _showSuccess = true;
          _isConnectingRemote = false;
          _transferActive = false;
          _completedFileName = summary['file_name'] ?? _selectedFile?.split(RegExp(r'[\\/]')).last ?? 'Unknown file';
          _completedPeerName = peerName;
          _completedDuration = elapsedMs != null ? formatDuration(elapsedMs) : null;
          _completedMode = mode;
          _metrics = null;
        });
        _autoResetTimer?.cancel();
        _autoResetTimer = Timer(const Duration(seconds: 5), () {
          if (mounted && _phase == TransferUiPhase.succeeded) _resetTransferUi();
        });
      }
    }
  }

  Future<void> _sendToPeer(String address, String hostname, String? pin) async {
    if (_selectedFile == null) return;
    _currentTransferPeerName = hostname;
    final deviceName = context.read<SettingsService>().deviceName;

    final sessionToken = DateTime.now().millisecondsSinceEpoch.toString();
    _sessionToken = sessionToken;
    _autoResetTimer?.cancel();
    setState(() {
      _phase = TransferUiPhase.connecting;
      _terminalEventReceived = false;
      _transferActive = true;
      _showSuccess = false;
      _transferStatus = 'Connecting to $hostname...';
    });
    Object? lockToken;
    try {
      lockToken = await TransferLock.acquire();
      _lockToken = lockToken;
    } catch (error) {
      if (mounted) {
        setState(() {
          _terminalEventReceived = true;
          _phase = TransferUiPhase.failed;
          _transferStatus = 'Transfer lock unavailable';
          _transferActive = false;
        });
      }
      return;
    }
    _transferSub = startSend(
      filePath: _selectedFile!,
      peerAddress: address,
      optionalPin: pin,
      deviceName: deviceName,
      sessionToken: sessionToken,
    ).listen(
      _handleTransferEvent,
      onDone: () {
        TransferLock.release(lockToken);
        _lockToken = null;
        if (mounted) {
          setState(() {
            _transferActive = false;
            if (!_terminalEventReceived && !_phase.isTerminal) {
              _phase = TransferUiPhase.idle;
            }
          });
        }
        unawaited(_clearSelectedTemporaryFile());
      },
      onError: (e) {
        TransferLock.release(lockToken);
        _lockToken = null;
        if (mounted) {
          if (_terminalEventReceived || _phase.isTerminal) {
            // The semantic transfer event already updated the UI.
            return;
          }
          setState(() {
            _transferActive = false;
            _terminalEventReceived = true;
            _phase = TransferUiPhase.failed;
            _transferStatus = friendlyError(e);
            _progress = null;
          });
        }
        unawaited(_clearSelectedTemporaryFile());
      },
    );
  }

  Future<void> _handleUnifiedConnect(QrPayload payload) async {
    if (_selectedFile == null) {
      ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Please select a file first')));
      return;
    }
    if (_isConnectingRemote || _phase == TransferUiPhase.connecting || _phase == TransferUiPhase.transferring) return;

    final unified = payload.unifiedPayload;
    if (unified == null) return;

    final settings = context.read<SettingsService>();
    final relayServerUrl = settings.relayServerUrl;
    final iceServers = settings.iceServers
        .split('\n')
        .map((e) => e.trim())
        .where((e) => e.isNotEmpty)
        .map((e) => IceServerSetting(urls: e))
        .toList();
    final myPeerId = generatePeerIdSync();
    final iceServersJson = await InternetSettings.buildIceServersJsonWithTurn(
      relayServerUrl,
      myPeerId,
      iceServers,
    );

    _currentTransferPeerName = unified.deviceName ?? 'Receiver (${unified.roomCode})';
    final sessionToken = DateTime.now().millisecondsSinceEpoch.toString();
    _sessionToken = sessionToken;
    _autoResetTimer?.cancel();

    setState(() {
      _phase = TransferUiPhase.connecting;
      _terminalEventReceived = false;
      _transferActive = true;
      _showSuccess = false;
      _activePathBadge = null;
      _transferStatus = 'Connecting to ${unified.roomCode}...';
      _isConnectingRemote = true;
    });

    Object? lockToken;
    try {
      lockToken = await TransferLock.acquire();
      _lockToken = lockToken;

      final payloadStr = payload.rawUri ?? payload.unifiedPayload!.toUri();

      _transferSub = startSendUnified(
        sessionToken: sessionToken,
        filePath: _selectedFile!,
        payloadUriOrJson: payloadStr,
        relayServerUrl: relayServerUrl,
        myPeerId: myPeerId,
        iceServersJson: iceServersJson,
        connectTimeoutSecs: BigInt.from(30),
        deviceName: settings.deviceName,
      ).listen(
        _handleTransferEvent,
        onDone: () {
          TransferLock.release(lockToken);
          _lockToken = null;
          if (mounted) {
            setState(() {
              _isConnectingRemote = false;
              _transferActive = false;
              if (!_terminalEventReceived && !_phase.isTerminal) {
                _phase = TransferUiPhase.idle;
              }
            });
          }
          unawaited(_clearSelectedTemporaryFile());
        },
        onError: (e) {
          TransferLock.release(lockToken);
          _lockToken = null;
          if (mounted) {
            if (_terminalEventReceived || _phase.isTerminal) return;
            setState(() {
              _isConnectingRemote = false;
              _transferActive = false;
              _terminalEventReceived = true;
              _phase = TransferUiPhase.failed;
              _transferStatus = friendlyError(e);
              _progress = null;
            });
          }
          unawaited(_clearSelectedTemporaryFile());
        },
      );
    } catch (e) {
      TransferLock.release(lockToken);
      if (identical(_lockToken, lockToken)) _lockToken = null;
      setState(() {
        _terminalEventReceived = true;
        _phase = TransferUiPhase.failed;
        _transferStatus = friendlyError(e);
        _isConnectingRemote = false;
        _transferActive = false;
      });
    }
  }

  Future<void> _handleRoomCodeConnect() async {
    if (_selectedFile == null) {
      ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Please select a file first')));
      return;
    }
    final rawText = _roomCodeController.text.trim();
    if (rawText.isEmpty) {
      ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Please enter a room code')));
      return;
    }
    final parsed = QrPayload.parse(rawText);
    if (parsed != null && parsed.type == QrPayloadType.unified) {
      return _handleUnifiedConnect(parsed);
    }
    final roomCode = rawText.toUpperCase();
    if (!RegExp(r'^[A-Z0-9]{9}$').hasMatch(roomCode)) {
      ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Room codes are 9 letters or numbers')));
      return;
    }
    if (_isConnectingRemote || _phase == TransferUiPhase.connecting || _phase == TransferUiPhase.transferring) return;

    final settings = context.read<SettingsService>();
    final relayServerUrl = settings.relayServerUrl;
    final iceServers = settings.iceServers
        .split('\n')
        .map((e) => e.trim())
        .where((e) => e.isNotEmpty)
        .map((e) => IceServerSetting(urls: e))
        .toList();
    _currentTransferPeerName = 'Remote Device ($roomCode)';

    setState(() {
      _phase = TransferUiPhase.connecting;
      _terminalEventReceived = false;
      _transferStatus = 'Finding room…';
      _isConnectingRemote = true;
    });

    Object? lockToken;
    try {
      final lookup = await InternetSettings.lookupRoomWithGracePeriod(
        relayServerUrl,
        roomCode,
        timeout: const Duration(seconds: 4),
        isCancelled: () => !mounted || _phase.isTerminal,
      );
      if (!lookup.exists) {
        if (mounted) {
          setState(() {
            _terminalEventReceived = true;
            _phase = TransferUiPhase.failed;
            _transferStatus = lookup.userMessage;
            _isConnectingRemote = false;
          });
        }
        return;
      }

      if (mounted) {
        setState(() {
          _transferStatus = 'Room found. Connecting…';
        });
      }

      final myPeerId = generatePeerIdSync();
      final iceServersJson = await InternetSettings.buildIceServersJsonWithTurn(
        relayServerUrl,
        myPeerId,
        iceServers,
      );
      final sessionToken = DateTime.now().millisecondsSinceEpoch.toString();
      _sessionToken = sessionToken;
      _autoResetTimer?.cancel();
      setState(() {
        _phase = TransferUiPhase.connecting;
        _terminalEventReceived = false;
        _transferActive = true;
        _showSuccess = false;
      });
      lockToken = await TransferLock.acquire();
      _lockToken = lockToken;
      _transferSub = startSendRemote(
        filePath: _selectedFile!,
        relayServerUrl: relayServerUrl,
        sessionId: roomCode,
        myPeerId: myPeerId,
        iceServersJson: iceServersJson,
        connectTimeoutSecs: BigInt.from(30),
        deviceName: settings.deviceName,
        sessionToken: sessionToken,
      ).listen(
        _handleTransferEvent,
        onDone: () {
          TransferLock.release(lockToken);
          _lockToken = null;
          if (mounted) {
            setState(() {
              _isConnectingRemote = false;
              _transferActive = false;
              if (!_terminalEventReceived && !_phase.isTerminal) {
                _phase = TransferUiPhase.idle;
              }
            });
          }
          unawaited(_clearSelectedTemporaryFile());
        },
        onError: (e) {
          TransferLock.release(lockToken);
          _lockToken = null;
          if (mounted) {
            if (_terminalEventReceived || _phase.isTerminal) {
              // The semantic transfer event already updated the UI.
              return;
            }
            setState(() {
              _isConnectingRemote = false;
              _transferActive = false;
              _terminalEventReceived = true;
              _phase = TransferUiPhase.failed;
              _transferStatus = friendlyError(e);
              _progress = null;
            });
          }
          unawaited(_clearSelectedTemporaryFile());
        },
      );
    } catch (e) {
      TransferLock.release(lockToken);
      if (identical(_lockToken, lockToken)) _lockToken = null;
      setState(() {
        _terminalEventReceived = true;
        _phase = TransferUiPhase.failed;
        _transferStatus = friendlyError(e);
        _isConnectingRemote = false;
        _transferActive = false;
      });
    }
  }

  void _showPinDialog(String address, String hostname, {bool pinRequired = false}) {
    if (_selectedFile == null) return;

    final TextEditingController pinController = TextEditingController();

    void submit(BuildContext dialogContext) {
      final pin = pinController.text.trim();
      if (pinRequired && pin.isEmpty) return; // must enter a code
      Navigator.pop(dialogContext);
      unawaited(_sendToPeer(address, hostname, pin.isNotEmpty ? pin : null));
    }

    showDialog(
      context: context,
      builder: (dialogCtx) {
        return AlertDialog(
          backgroundColor: AppTheme.bgCardOf(dialogCtx),
          title: Text('Send to $hostname', style: TextStyle(color: AppTheme.textPrimaryOf(dialogCtx))),
          content: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(
                pinRequired
                    ? 'This device requires a pairing code. Enter the code shown on its screen or scan its QR code.'
                    : 'If the receiver requires a pairing code, enter it below or scan its QR code. Otherwise, leave blank.',
                style: TextStyle(color: AppTheme.textSecondaryOf(dialogCtx), fontSize: 14),
              ),
              const SizedBox(height: 16),
              TextField(
                controller: pinController,
                autofocus: true,
                textCapitalization: TextCapitalization.characters,
                decoration: InputDecoration(
                  labelText: pinRequired ? 'Pairing Code' : 'Pairing Code (Optional)',
                  border: const OutlineInputBorder(),
                  focusedBorder: const OutlineInputBorder(borderSide: BorderSide(color: AppTheme.accentPrimary)),
                  suffixIcon: IconButton(
                    icon: const Icon(Icons.qr_code_scanner, color: AppTheme.accentPrimary),
                    tooltip: 'Scan PIN QR',
                    onPressed: () async {
                      final payload = await QrScannerSheet.show(dialogCtx, title: 'Scan Pairing PIN');
                      if (payload != null && dialogCtx.mounted) {
                        pinController.text = payload.code;
                        submit(dialogCtx);
                      }
                    },
                  ),
                ),
                style: TextStyle(color: AppTheme.textPrimaryOf(dialogCtx)),
                onSubmitted: (_) => submit(dialogCtx),
              ),
            ],
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(dialogCtx),
              child: Text('Cancel', style: TextStyle(color: AppTheme.textSecondaryOf(dialogCtx))),
            ),
            ElevatedButton(
              onPressed: () => submit(dialogCtx),
              style: ElevatedButton.styleFrom(backgroundColor: AppTheme.accentPrimary),
              child: const Text('Send'),
            ),
          ],
        );
      },
    );
  }


  Widget _buildFilePicker() {
    if (_selectedFile != null) {
      return Container(
        width: double.infinity,
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
        decoration: BoxDecoration(
          color: AppTheme.bgCardOf(context),
          borderRadius: BorderRadius.circular(12),
          border: Border.all(color: AppTheme.accentPrimary),
        ),
        child: Row(
          children: [
            const Icon(Icons.insert_drive_file, color: AppTheme.accentPrimary, size: 28),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    _selectedFile!.split(RegExp(r'[\\/]')).last,
                    style: TextStyle(fontWeight: FontWeight.w600, color: AppTheme.textPrimaryOf(context), fontSize: 14),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                  if (_selectedFileSize != null) ...[
                    const SizedBox(height: 2),
                    Text(formatBytes(_selectedFileSize!), style: TextStyle(color: AppTheme.textSecondaryOf(context), fontSize: 12)),
                  ],
                ],
              ),
            ),
            IconButton(
              visualDensity: VisualDensity.compact,
              icon: Icon(Icons.close, color: AppTheme.textSecondaryOf(context)),
              onPressed: () {
                setState(() {
                  _selectedFile = null;
                  _selectedFileSize = null;
                });
                unawaited(FilePicker.clearTemporaryFiles());
              },
            ),
          ],
        ),
      );
    }

    return GestureDetector(
      onTap: _pickFile,
      child: Container(
        width: double.infinity,
        padding: const EdgeInsets.symmetric(vertical: 14),
        decoration: BoxDecoration(
          gradient: LinearGradient(
            begin: Alignment.topLeft,
            end: Alignment.bottomRight,
            colors: [
              AppTheme.bgCardOf(context),
              AppTheme.isDark(context) ? const Color(0xFF1E2835) : const Color(0xFFEFF3F6),
            ],
          ),
          borderRadius: BorderRadius.circular(12),
          border: Border.all(color: AppTheme.borderColorOf(context)),
        ),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.upload_file, size: 32, color: AppTheme.textSecondaryOf(context)),
            const SizedBox(height: 8),
            Text(
              'Select File to Send',
              style: TextStyle(fontWeight: FontWeight.w600, fontSize: 14, color: AppTheme.textPrimaryOf(context)),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildStatusCard() {
    if (_phase == TransferUiPhase.succeeded || _showSuccess) {
      return Container(
        margin: const EdgeInsets.only(top: 8),
        padding: const EdgeInsets.all(16),
        decoration: BoxDecoration(
          color: AppTheme.bgSidebarOf(context),
          borderRadius: BorderRadius.circular(16),
          border: Border.all(color: AppTheme.accentPrimary.withValues(alpha: 0.3)),
        ),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const Center(child: SuccessCheck()),
            const SizedBox(height: 12),
            Text(
              'Sent ${_completedFileName ?? "file"}',
              style: TextStyle(color: AppTheme.textPrimaryOf(context), fontSize: 16, fontWeight: FontWeight.bold),
              textAlign: TextAlign.center,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
            ),
            const SizedBox(height: 12),
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
              decoration: BoxDecoration(
                color: AppTheme.bgCardOf(context),
                borderRadius: BorderRadius.circular(8),
                border: Border.all(color: AppTheme.borderColorOf(context)),
              ),
              child: Column(
                children: [
                  if (_completedPeerName != null) ...[
                    Row(
                      children: [
                        Icon(Icons.monitor, size: 16, color: AppTheme.textSecondaryOf(context)),
                        const SizedBox(width: 8),
                        Expanded(child: Text(_completedPeerName!, style: TextStyle(color: AppTheme.textSecondaryOf(context), fontSize: 13))),
                      ],
                    ),
                    const SizedBox(height: 8),
                  ],
                  Row(
                    children: [
                      Icon(Icons.timer_outlined, size: 16, color: AppTheme.textSecondaryOf(context)),
                      const SizedBox(width: 8),
                      Expanded(
                        child: Text(
                          '${_completedDuration ?? "-"} • ${_completedMode ?? "Unknown"}',
                          style: TextStyle(color: AppTheme.textSecondaryOf(context), fontSize: 13),
                        ),
                      ),
                    ],
                  ),
                ],
              ),
            ),
            const SizedBox(height: 16),
            SizedBox(
              width: double.infinity,
              child: ElevatedButton(
                onPressed: _resetTransferUi,
                style: ElevatedButton.styleFrom(
                  backgroundColor: AppTheme.accentPrimary,
                  padding: const EdgeInsets.symmetric(vertical: 12),
                ),
                child: const Text('Send Another File', style: TextStyle(fontWeight: FontWeight.w600)),
              ),
            ),
          ],
        ),
      );
    }

    if (_transferStatus.isEmpty) return const SizedBox.shrink();

    final isInFlight = _phase == TransferUiPhase.connecting ||
        _phase == TransferUiPhase.awaitingApproval ||
        _phase == TransferUiPhase.transferring ||
        _phase == TransferUiPhase.cancelling ||
        _transferActive;

    return Container(
      margin: const EdgeInsets.only(top: 8),
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
      decoration: BoxDecoration(
        color: AppTheme.bgSidebarOf(context),
        borderRadius: BorderRadius.circular(12),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: MainAxisSize.min,
        children: [
          if (_activePathBadge != null) ...[
            Center(
              child: Container(
                margin: const EdgeInsets.only(bottom: 8),
                padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
                decoration: BoxDecoration(
                  color: _activePathBadge!.contains('Local')
                      ? const Color(0xFF166534).withValues(alpha: 0.3)
                      : const Color(0xFF1E40AF).withValues(alpha: 0.3),
                  borderRadius: BorderRadius.circular(12),
                  border: Border.all(
                    color: _activePathBadge!.contains('Local')
                        ? const Color(0xFF22C55E).withValues(alpha: 0.5)
                        : const Color(0xFF3B82F6).withValues(alpha: 0.5),
                  ),
                ),
                child: Text(
                  _activePathBadge!,
                  style: TextStyle(
                    fontSize: 12,
                    fontWeight: FontWeight.w600,
                    color: _activePathBadge!.contains('Local')
                        ? const Color(0xFF4ADE80)
                        : const Color(0xFF60A5FA),
                  ),
                ),
              ),
            ),
          ],
          Text(
            _transferStatus,
            style: TextStyle(color: AppTheme.textPrimaryOf(context), fontSize: 13),
            textAlign: TextAlign.center,
            maxLines: 3,
            overflow: TextOverflow.ellipsis,
          ),
          if (_isConnectingRemote || _phase == TransferUiPhase.connecting || _phase == TransferUiPhase.cancelling) ...[
            const SizedBox(height: 8),
            const Center(child: SizedBox(width: 20, height: 20, child: CircularProgressIndicator(strokeWidth: 2, color: AppTheme.accentPrimary))),
          ],
          if (_progress != null) ...[
            const SizedBox(height: 8),
            ClipRRect(
              borderRadius: BorderRadius.circular(4),
              child: LinearProgressIndicator(
                value: _progress,
                backgroundColor: AppTheme.bgAppOf(context),
                valueColor: const AlwaysStoppedAnimation<Color>(AppTheme.accentPrimary),
              ),
            ),
            const SizedBox(height: 6),
            Row(
              children: [
                Text(
                  '${(_progress! * 100).toStringAsFixed(1)}%  •  ${formatBytes(_transferredBytes ?? 0)} / ${formatBytes(_totalBytes ?? 0)}',
                  style: TextStyle(color: AppTheme.textSecondaryOf(context), fontSize: 12),
                ),
                if (_speedText != null && _etaText != null) ...[
                  const Spacer(),
                  Flexible(
                    child: Text(
                      '$_speedText • $_etaText',
                      style: TextStyle(color: AppTheme.textSecondaryOf(context), fontSize: 12),
                      overflow: TextOverflow.ellipsis,
                      textAlign: TextAlign.end,
                    ),
                  ),
                ],
              ],
            ),
          ],
          if (isInFlight && _progress != 1.0)
            Padding(
              padding: const EdgeInsets.only(top: 4),
              child: TextButton.icon(
                onPressed: _phase == TransferUiPhase.cancelling ? null : _cancelTransfer,
                style: TextButton.styleFrom(
                  visualDensity: VisualDensity.compact,
                  tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                ),
                icon: Icon(
                  Icons.cancel,
                  color: _phase == TransferUiPhase.cancelling ? AppTheme.textSecondaryOf(context) : AppTheme.accentPrimary,
                  size: 18,
                ),
                label: Text(
                  _phase == TransferUiPhase.cancelling ? 'Cancelling...' : 'Cancel transfer',
                  style: TextStyle(
                    color: _phase == TransferUiPhase.cancelling ? AppTheme.textSecondaryOf(context) : AppTheme.accentPrimary,
                  ),
                ),
              ),
            ),
          if (_progress == 1.0)
            Padding(
              padding: const EdgeInsets.only(top: 8),
              child: ElevatedButton(
                onPressed: _resetTransferUi,
                child: const Text('Send another file'),
              ),
            )
        ],
      ),
    );
  }

  Future<void> _scanRoomCode() async {
    if (_selectedFile == null) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('Please select a file first')),
      );
      return;
    }
    final payload = await QrScannerSheet.show(context, title: 'Scan Receiver QR');
    if (payload != null && mounted) {
      _roomCodeController.text = payload.code;
      if (payload.type == QrPayloadType.unified) {
        _handleUnifiedConnect(payload);
      } else if (payload.type == QrPayloadType.room || payload.type == QrPayloadType.raw) {
        _handleRoomCodeConnect();
      }
    }
  }

  Widget _buildUnifiedConnectCard() {
    final isBusy = _isConnectingRemote || _transferActive;
    return Container(
      padding: const EdgeInsets.all(16),
      decoration: BoxDecoration(
        color: AppTheme.bgCardOf(context),
        borderRadius: BorderRadius.circular(16),
        border: Border.all(color: AppTheme.borderColorOf(context)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          // 1. Hero QR Scanner Button
          InkWell(
            borderRadius: BorderRadius.circular(12),
            onTap: isBusy ? null : _scanRoomCode,
            child: Ink(
              decoration: BoxDecoration(
                gradient: LinearGradient(
                  colors: [
                    AppTheme.accentPrimary,
                    AppTheme.accentPrimary.withValues(alpha: 0.85),
                  ],
                  begin: Alignment.topLeft,
                  end: Alignment.bottomRight,
                ),
                borderRadius: BorderRadius.circular(12),
                boxShadow: [
                  BoxShadow(
                    color: AppTheme.accentPrimary.withValues(alpha: 0.25),
                    blurRadius: 8,
                    offset: const Offset(0, 3),
                  ),
                ],
              ),
              padding: const EdgeInsets.symmetric(vertical: 14, horizontal: 16),
              child: const Row(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  Icon(Icons.qr_code_scanner, color: Colors.white, size: 22),
                  SizedBox(width: 10),
                  Text(
                    'Scan Receiver QR Code',
                    style: TextStyle(
                      color: Colors.white,
                      fontWeight: FontWeight.bold,
                      fontSize: 15,
                    ),
                  ),
                ],
              ),
            ),
          ),

          const SizedBox(height: 14),

          // 2. OR divider
          Row(
            children: [
              Expanded(child: Divider(color: AppTheme.borderColorOf(context), height: 1)),
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 10),
                child: Text(
                  'OR',
                  style: TextStyle(
                    color: AppTheme.textSecondaryOf(context).withValues(alpha: 0.8),
                    fontSize: 11,
                    fontWeight: FontWeight.w600,
                    letterSpacing: 1,
                  ),
                ),
              ),
              Expanded(child: Divider(color: AppTheme.borderColorOf(context), height: 1)),
            ],
          ),

          const SizedBox(height: 14),

          // 3. Room Code Input Row
          Row(
            children: [
              Expanded(
                child: TextField(
                  controller: _roomCodeController,
                  textCapitalization: TextCapitalization.characters,
                  decoration: InputDecoration(
                    isDense: true,
                    hintText: 'Enter 9-character room code',
                    hintStyle: TextStyle(
                      color: AppTheme.textSecondaryOf(context),
                      fontSize: 13,
                      letterSpacing: 0,
                    ),
                    border: OutlineInputBorder(
                      borderRadius: BorderRadius.circular(10),
                      borderSide: BorderSide(color: AppTheme.borderColorOf(context)),
                    ),
                    enabledBorder: OutlineInputBorder(
                      borderRadius: BorderRadius.circular(10),
                      borderSide: BorderSide(color: AppTheme.borderColorOf(context)),
                    ),
                    focusedBorder: OutlineInputBorder(
                      borderRadius: BorderRadius.circular(10),
                      borderSide: const BorderSide(color: AppTheme.accentPrimary),
                    ),
                    contentPadding: const EdgeInsets.symmetric(horizontal: 12, vertical: 12),
                    prefixIcon: Icon(
                      Icons.tag,
                      size: 18,
                      color: AppTheme.textSecondaryOf(context),
                    ),
                  ),
                  style: TextStyle(
                    color: AppTheme.textPrimaryOf(context),
                    fontSize: 14,
                    letterSpacing: 1.5,
                    fontWeight: FontWeight.w600,
                  ),
                  onSubmitted: (_) => _handleRoomCodeConnect(),
                ),
              ),
              const SizedBox(width: 8),
              ElevatedButton(
                onPressed: isBusy ? null : _handleRoomCodeConnect,
                style: ElevatedButton.styleFrom(
                  backgroundColor: AppTheme.accentPrimary,
                  padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 13),
                  shape: RoundedRectangleBorder(
                    borderRadius: BorderRadius.circular(10),
                  ),
                ),
                child: const Text('Connect', style: TextStyle(fontWeight: FontWeight.w600)),
              ),
            ],
          ),
        ],
      ),
    );
  }

  Widget _buildNearbyDevicesSection() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      mainAxisSize: MainAxisSize.min,
      children: [
        Row(
          children: [
            Icon(Icons.devices, size: 18, color: AppTheme.textSecondaryOf(context)),
            const SizedBox(width: 8),
            Text(
              'Nearby Devices',
              style: TextStyle(
                fontWeight: FontWeight.w600,
                color: AppTheme.textPrimaryOf(context),
                fontSize: 14,
              ),
            ),
            const Spacer(),
            IconButton(
              visualDensity: VisualDensity.compact,
              padding: EdgeInsets.zero,
              constraints: const BoxConstraints(minWidth: 32, minHeight: 32),
              icon: _isDiscovering
                  ? const SizedBox(
                      width: 14,
                      height: 14,
                      child: CircularProgressIndicator(
                        strokeWidth: 2,
                        color: AppTheme.accentPrimary,
                      ),
                    )
                  : const Icon(Icons.refresh, size: 18, color: AppTheme.accentPrimary),
              tooltip: 'Refresh Nearby Devices',
              onPressed: _isDiscovering ? null : _startDiscovery,
            ),
          ],
        ),
        const SizedBox(height: 8),
        if (_peers.isEmpty) ...[
          Container(
            padding: const EdgeInsets.symmetric(vertical: 16, horizontal: 16),
            decoration: BoxDecoration(
              color: AppTheme.bgCardOf(context),
              borderRadius: BorderRadius.circular(12),
              border: Border.all(color: AppTheme.borderColorOf(context)),
            ),
            child: Row(
              children: [
                Icon(
                  Icons.wifi_tethering,
                  size: 24,
                  color: AppTheme.textSecondaryOf(context).withValues(alpha: 0.7),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: Text(
                    _isDiscovering
                        ? 'Searching for devices on same Wi-Fi...'
                        : 'No nearby devices found. Make sure receiver is open on the same Wi-Fi, or scan its QR code above.',
                    style: TextStyle(
                      color: AppTheme.textSecondaryOf(context),
                      fontSize: 12.5,
                      height: 1.3,
                    ),
                  ),
                ),
              ],
            ),
          ),
        ] else ...[
          ListView.separated(
            shrinkWrap: true,
            physics: const NeverScrollableScrollPhysics(),
            padding: EdgeInsets.zero,
            itemCount: _peers.length,
            separatorBuilder: (context, index) => const SizedBox(height: 8),
            itemBuilder: (context, index) {
              final peer = _peers[index];
              return Container(
                decoration: BoxDecoration(
                  color: AppTheme.bgCardOf(context),
                  borderRadius: BorderRadius.circular(12),
                  border: Border.all(color: AppTheme.borderColorOf(context)),
                ),
                child: ListTile(
                  dense: true,
                  contentPadding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
                  onTap: () {
                    if (_selectedFile == null) {
                      ScaffoldMessenger.of(context).showSnackBar(
                        const SnackBar(content: Text('Please select a file first')),
                      );
                      return;
                    }
                    _showPinDialog(
                      peer['address'],
                      peer['hostname'] ?? 'Unknown Device',
                      pinRequired: peer['pin_required'] == true,
                    );
                  },
                  leading: Container(
                    padding: const EdgeInsets.all(8),
                    decoration: BoxDecoration(
                      color: AppTheme.bgSidebarOf(context),
                      borderRadius: BorderRadius.circular(8),
                    ),
                    child: const Icon(Icons.computer, color: AppTheme.accentPrimary, size: 22),
                  ),
                  title: Text(
                    peer['hostname'] ?? 'Unknown Device',
                    style: TextStyle(fontWeight: FontWeight.w600, color: AppTheme.textPrimaryOf(context)),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                  subtitle: Text(
                    peer['address'] ?? '',
                    style: TextStyle(color: AppTheme.textSecondaryOf(context), fontSize: 12),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                  trailing: const Icon(Icons.send_rounded, color: AppTheme.accentPrimary),
                ),
              );
            },
          ),
        ],
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final isSuccess = _phase == TransferUiPhase.succeeded || _showSuccess;

    return Scaffold(
      appBar: AppBar(
        title: const Text(
          'Plenum',
          style: TextStyle(
            fontWeight: FontWeight.w900,
            color: AppTheme.accentPrimary,
            letterSpacing: -0.5,
          ),
        ),
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
      body: SafeArea(
        child: SingleChildScrollView(
          padding: const EdgeInsets.fromLTRB(16, 10, 16, 20),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (isSuccess) ...[
                _buildStatusCard(),
              ] else ...[
                _buildFilePicker(),
                const SizedBox(height: 14),
                _buildUnifiedConnectCard(),
                if (_transferStatus.isNotEmpty) ...[
                  const SizedBox(height: 12),
                  _buildStatusCard(),
                ],
                const SizedBox(height: 20),
                _buildNearbyDevicesSection(),
              ],
            ],
          ),
        ),
      ),
    );
  }
}
