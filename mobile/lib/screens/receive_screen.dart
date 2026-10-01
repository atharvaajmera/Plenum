import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math';
import 'package:open_filex/open_filex.dart';
import 'package:share_plus/share_plus.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:mobile/src/rust/api/plenum_api.dart';
import 'package:permission_handler/permission_handler.dart';
import '../services/receive_storage.dart';
import '../services/internet_settings.dart';
import '../services/transfer_lock.dart';
import '../theme.dart';
import '../widgets/animated_radar.dart';
import 'package:provider/provider.dart';
import '../services/settings_service.dart';

import '../utils/transfer_status.dart';
import '../utils/formatters.dart';
import '../utils/transfer_metrics.dart';
import '../widgets/success_check.dart';
import 'package:qr_flutter/qr_flutter.dart';
import '../utils/qr_payload.dart';
import 'settings_screen.dart';

class ReceiveScreen extends StatefulWidget {
  const ReceiveScreen({super.key});

  @override
  State<ReceiveScreen> createState() => _ReceiveScreenState();
}

class _ReceiveScreenState extends State<ReceiveScreen> {
  TransferUiPhase _phase = TransferUiPhase.idle;
  bool _terminalEventReceived = false;

  bool _isListening = false;
  String _statusMessage = 'Tap radar to start receiving';
  String? _pin;
  double? _progress;

  String? _roomCode;
  bool _roomCodeCopied = false;
  String? _unifiedQrPayload;
  String? _selectedPathBadge;

  StreamSubscription<String>? _localSub;
  StreamSubscription<String>? _remoteSub;
  String? _sessionToken;
  Object? _lockToken;
  bool _requirePinActive = false;
  bool _autoAcceptActive = true;

  int? _totalBytes;
  int? _transferredBytes;
  int? _resumeBaselineBytes;
  TransferMetricsState? _metrics;
  String? _speedText;
  String? _etaText;
  String? _savedFilePath;
  String? _savedLocation;
  Timer? _reArmTimer;

  // Summary fields kept for the success card after completion.
  String? _completedPeerName;
  String? _completedDuration;
  String? _completedMode;

  void _handleLogEvent(dynamic log) {
    final level = log['level'] ?? 'Info';
    final message = log['message'] ?? '';
    debugPrint('[$level] $message');
  }

  @override
  void dispose() {
    final active = _isListening;
    final token = _sessionToken;
    if (token != null && !active) {
      try {
        cancelSession(sessionToken: token);
      } catch (_) {}
    }
    // Clean up app-dir copy if it was already exported to Downloads.
    _deleteAppDirCopyIfExported();
    if (!active) {
      TransferLock.release(_lockToken);
      _lockToken = null;
      _localSub?.cancel();
      _remoteSub?.cancel();
    }
    _reArmTimer?.cancel();
    super.dispose();
  }

  /// Deletes the app-dir copy of the received file if it was successfully
  /// exported to public Downloads. The app-dir copy is only needed for
  /// Open/Share; once the user moves on, reclaim the space.
  void _deleteAppDirCopyIfExported() {
    final path = _savedFilePath;
    if (path != null && _savedLocation != null) {
      try {
        final f = File(path);
        if (f.existsSync()) f.deleteSync();
      } catch (_) {}
    }
  }

  void _stopReceiving({Object? lockToken, bool resetPhase = true}) {
    TransferLock.release(lockToken ?? _lockToken);
    _lockToken = null;
    final token = _sessionToken;
    if (token != null) {
      try {
        cancelSession(sessionToken: token);
      } catch (_) {}
    }
    _sessionToken = null;
    _localSub?.cancel();
    _remoteSub?.cancel();
    _reArmTimer?.cancel();
    setState(() {
      _isListening = false;
      if (resetPhase) {
        _phase = TransferUiPhase.idle;
        _terminalEventReceived = false;
      }
      _statusMessage = 'Tap radar to start receiving';
      _pin = null;
      _unifiedQrPayload = null;
      _selectedPathBadge = null;
      _requirePinActive = false;
      _roomCode = null;
      _progress = null;
      _totalBytes = null;
      _transferredBytes = null;
      _resumeBaselineBytes = null;
      _metrics = null;
      _speedText = null;
      _etaText = null;
      _savedFilePath = null;
      _savedLocation = null;
    });
  }

  Future<void> _startUnifiedReceiver() async {
    if (_isListening) return;

    final grantedStorage = await ReceiveStorage.ensurePermission();
    if (!grantedStorage) {
      if (mounted) {
        setState(() {
          _terminalEventReceived = true;
          _phase = TransferUiPhase.failed;
          _statusMessage = 'Storage permission needed to save files';
        });
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
    if (!mounted) return;
    final settings = context.read<SettingsService>();
    final deviceName = settings.deviceName;
    final requirePin = settings.requirePin;
    final autoAccept = settings.autoAccept;
    final relayServerUrl = settings.relayServerUrl;

    final code = generateRoomCodeSync();
    final myPeerId = generatePeerIdSync();
    final localIps = getLocalIpsSync();

    final iceServers = settings.iceServers
        .split('\n')
        .map((e) => e.trim())
        .where((e) => e.isNotEmpty)
        .map((e) => IceServerSetting(urls: e))
        .toList();

    final iceServersJson = await InternetSettings.buildIceServersJsonWithTurn(
      relayServerUrl,
      myPeerId,
      iceServers,
    );

    if (!mounted) return;

    setState(() {
      _isListening = true;
      _phase = TransferUiPhase.listening;
      _terminalEventReceived = false;
      _requirePinActive = requirePin;
      _autoAcceptActive = autoAccept;
      _roomCode = code;
      _selectedPathBadge = null;
      _statusMessage = 'Ready to receive files (Wi-Fi & Internet active)';
      _unifiedQrPayload = QrPayload.encodeUnified(UnifiedSharePayload(
        roomCode: code,
        lanIps: localIps,
        peerId: myPeerId,
        deviceName: deviceName,
      ));
    });

    Object? lockToken;
    try {
      lockToken = await TransferLock.acquire();
      _lockToken = lockToken;
    } catch (error) {
      if (mounted) {
        _handleLogEvent({
          'level': 'Warn',
          'message': 'Transfer lock unavailable: $error',
        });
      }
    }

    final sessionToken = DateTime.now().millisecondsSinceEpoch.toString();
    _sessionToken = sessionToken;
    _localSub = startReceiveUnified(
      sessionToken: sessionToken,
      outputDir: outputDir,
      relayServerUrl: relayServerUrl,
      sessionId: code,
      myPeerId: myPeerId,
      iceServersJson: iceServersJson,
      connectTimeoutSecs: BigInt.from(600),
      port: 0,
      requirePin: requirePin,
      autoAccept: autoAccept,
      deviceName: deviceName,
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
          final token = discEvent['BroadcastStarted']['token']?.toString();
          final port = discEvent['BroadcastStarted']['port'] as int?;
          setState(() {
            _pin = token;
            _statusMessage = 'Ready to receive files';
            _unifiedQrPayload = QrPayload.encodeUnified(UnifiedSharePayload(
              roomCode: code,
              lanIps: localIps,
              port: port ?? 0,
              pin: token,
              peerId: myPeerId,
              deviceName: deviceName,
            ));
          });
        }
      } else if (event['Transfer'] != null) {
        final trans = event['Transfer'];
        if (trans['PathSelected'] != null) {
          final path = trans['PathSelected']['path'];
          setState(() {
            if (path is Map && path['Local'] != null) {
              final addr = path['Local']['address'] ?? '';
              _selectedPathBadge = 'Local Wi-Fi ($addr)';
            } else if (path == 'DirectLan' || (path is Map && path.containsKey('DirectLan'))) {
              _selectedPathBadge = 'Local Wi-Fi (Direct)';
            } else if (path is Map && path['Internet'] != null) {
              _selectedPathBadge = 'Internet (${path['Internet']['connection_type'] ?? 'P2P'})';
            } else {
              _selectedPathBadge = 'Internet ($path)';
            }
          });
        }
        _handleTransferEvent(trans, outputDir, lockToken);
      }
    }, onDone: () {
      TransferLock.release(lockToken);
      _lockToken = null;
      if (mounted) {
        setState(() {
          _isListening = false;
          if (!_terminalEventReceived && !_phase.isTerminal) {
            _phase = TransferUiPhase.idle;
          }
        });
      }
    }, onError: (e) {
      TransferLock.release(lockToken);
      _lockToken = null;
      if (mounted) {
        if (_terminalEventReceived || _phase.isTerminal) return;
        setState(() {
          _terminalEventReceived = true;
          _phase = TransferUiPhase.failed;
          _statusMessage = friendlyError(e);
          _isListening = false;
        });
      }
    });
  }



  void _handleTransferEvent(dynamic transEvent, String outputDir, Object? lockToken) {
    if (transEvent['StateChanged'] != null) {
      if (_terminalEventReceived) return;
      final state = transEvent['StateChanged']['state'];
      if (state != 'Closed') {
        setState(() {
          _statusMessage = friendlyState(state);
        });
      }
    } else if (transEvent['ConnectionEstablished'] != null) {
      if (_terminalEventReceived) return;
      final mode = transEvent['ConnectionEstablished']['mode'];
      setState(() {
        _statusMessage = 'Connected via ${friendlyConnectionMode(mode)}';
      });
    } else if (transEvent['IncomingRequest'] != null) {
      if (_terminalEventReceived) return;
      final req = transEvent['IncomingRequest'];
      final fileName = req['file_name'] ?? 'Unknown file';
      final totalBytes = req['total_bytes'] ?? 0;
      if (_autoAcceptActive) {
        // Auto-accept skips the dialog. Surface the file/sender explicitly so the
        // user has confirmation of what was accepted even if negotiation stalls
        // on a slow network.
        final sender = req['sender_name'] ?? req['peer'] ?? 'device';
        setState(() {
          _phase = TransferUiPhase.connecting;
          _statusMessage = 'Accepting $fileName (${formatBytes(totalBytes)}) from $sender...';
        });
      } else {
        setState(() {
          _phase = TransferUiPhase.awaitingApproval;
        });
        _showIncomingRequestDialog(
          fileName: fileName,
          totalBytes: totalBytes,
          peer: req['peer'],
          senderName: req['sender_name'],
        );
      }
    } else if (transEvent['Cancelled'] != null) {
      TransferLock.release(lockToken);
      _lockToken = null;
      setState(() {
        _terminalEventReceived = true;
        _phase = TransferUiPhase.cancelled;
        _statusMessage = 'Sender cancelled the transfer';
        _progress = null;
        _speedText = null;
        _etaText = null;
      });
      _reArmTimer?.cancel();
      _reArmTimer = Timer(const Duration(seconds: 2), () {
        if (mounted && _phase == TransferUiPhase.cancelled) _reArmReceiving();
      });
    } else if (transEvent['Declined'] != null) {
      TransferLock.release(lockToken);
      _lockToken = null;
      final reason = transEvent['Declined']['reason'];
      setState(() {
        _terminalEventReceived = true;
        _phase = TransferUiPhase.failed;
        _statusMessage = reason == 'cancelled'
            ? 'Sender cancelled the transfer'
            : 'Transfer declined';
        _progress = null;
        _speedText = null;
        _etaText = null;
      });
      _reArmTimer?.cancel();
      _reArmTimer = Timer(const Duration(seconds: 2), () {
        if (mounted && _phase == TransferUiPhase.failed) _reArmReceiving();
      });
    } else if (transEvent['Started'] != null) {
      if (_terminalEventReceived) return;
      final started = transEvent['Started'];
      final total = started['total_bytes'] as int? ?? 0;
      final resumed = started['resumed_bytes'] as int? ?? 0;
      setState(() {
        _phase = TransferUiPhase.transferring;
        _statusMessage = resumed > 0
            ? 'Resuming ${started['file_name']} from ${formatBytes(resumed)}...'
            : 'Receiving ${started['file_name']}...';
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
      });
    } else if (transEvent['Failed'] != null) {
      setState(() {
        _terminalEventReceived = true;
        _phase = TransferUiPhase.failed;
        _statusMessage = friendlyError(transEvent['Failed']['message']);
        _progress = null;
        _speedText = null;
        _etaText = null;
        _metrics = null;
      });
    } else if (transEvent['Resumed'] != null) {
      if (_terminalEventReceived) return;
      setState(() {
        _phase = TransferUiPhase.transferring;
        final resumedBytes = transEvent['Resumed']['resumed_bytes'] ?? 0;
        final percent = _totalBytes != null && _totalBytes! > 0 ? (resumedBytes / _totalBytes! * 100).toStringAsFixed(1) : '0';
        _statusMessage = 'Resuming from $percent%...';
      });
    } else if (transEvent['Progress'] != null) {
      if (_terminalEventReceived) return;
      final p = transEvent['Progress'];
      final currentTransferred = p['transferred_bytes'] as int? ?? 0;
      final total = p['total_bytes'] as int? ?? _totalBytes ?? 0;
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
    } else if (transEvent['Completed'] != null) {
      _terminalEventReceived = true;
      TransferLock.release(lockToken);
      _lockToken = null;
      final summary = transEvent['Completed'];
      final fileName = summary['file_name'];
      final localPath = fileName != null ? '$outputDir/$fileName' : null;
      final peerName = summary['peer_name'] ?? summary['peer'] ?? 'Unknown sender';
      final elapsedMs = summary['elapsed_ms'];
      final mode = formatTransferMode(summary['mode']);
      final resumedBytes = summary['resumed_bytes'] as int? ?? _resumeBaselineBytes ?? 0;
      final totalBytes = summary['total_bytes'] as int? ?? _totalBytes ?? 0;
      final sessionBytes = max(0, totalBytes - resumedBytes);
      final settings = context.read<SettingsService>();
      settings.addTransferHistory({
        'direction': 'receive',
        'fileName': fileName ?? 'Unknown file',
        'size': totalBytes,
        'resumedBytes': resumedBytes,
        'sessionBytes': sessionBytes,
        'peerName': peerName,
        'durationMs': elapsedMs,
        'mode': summary['mode'],
        'path': localPath,
        'timestamp': DateTime.now().toIso8601String(),
      });

      // Build a rich status message with summary details.
      String statusMsg = 'Transfer complete!';
      if (elapsedMs != null) {
        statusMsg = resumedBytes > 0
            ? 'Received from $peerName in ${formatDuration(elapsedMs)} (Resumed from ${formatBytes(resumedBytes)})'
            : 'Received from $peerName in ${formatDuration(elapsedMs)}';
      }

      setState(() {
        _phase = TransferUiPhase.succeeded;
        _statusMessage = statusMsg;
        _progress = 1.0;
        _savedFilePath = localPath;
        _savedLocation = null;
        _completedPeerName = peerName;
        _completedDuration = elapsedMs != null ? formatDuration(elapsedMs) : null;
        _completedMode = mode;
        _metrics = null;
      });

      void scheduleAutoReset() {
        _reArmTimer?.cancel();
        _reArmTimer = Timer(const Duration(seconds: 5), () {
          if (mounted && _phase == TransferUiPhase.succeeded) {
            _reArmReceiving();
          }
        });
      }

      if (localPath != null) {
        ReceiveStorage.exportToDownloads(localPath).then((saved) {
          if (mounted) {
            if (saved != null) {
              setState(() => _savedLocation = saved);
            }
            if (_phase == TransferUiPhase.succeeded) {
              scheduleAutoReset();
            }
          }
        }).catchError((_) {
          if (mounted && _phase == TransferUiPhase.succeeded) {
            scheduleAutoReset();
          }
        });
      } else {
        scheduleAutoReset();
      }
    }
  }

  void _reArmReceiving() {
    _reArmTimer?.cancel();
    // Delete the app-dir copy now that the user has had time to Open/Share.
    _deleteAppDirCopyIfExported();
    _stopReceiving(resetPhase: true);
    _startUnifiedReceiver();
  }

  Future<void> _showIncomingRequestDialog({
    required String fileName,
    required int totalBytes,
    String? peer,
    String? senderName,
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
            Text(formatBytes(totalBytes), style: TextStyle(color: AppTheme.textSecondaryOf(context))),
            if (senderName != null) ...[
              const SizedBox(height: 8),
              Text('From: $senderName',
                  style: TextStyle(color: AppTheme.textPrimaryOf(context), fontWeight: FontWeight.w600, fontSize: 14)),
              if (peer != null) ...[
                const SizedBox(height: 2),
                Text(peer, style: TextStyle(color: AppTheme.textSecondaryOf(context), fontSize: 11)),
              ],
            ] else if (peer != null) ...[
              const SizedBox(height: 4),
              Text('From: $peer', style: TextStyle(color: AppTheme.textSecondaryOf(context), fontSize: 12)),
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


  Future<void> _copyRoomCode() async {
    if (_roomCode == null) return;
    try {
      await Clipboard.setData(ClipboardData(text: _roomCode!));
      if (!mounted) return;
      setState(() => _roomCodeCopied = true);
      Future.delayed(const Duration(seconds: 2), () {
        if (mounted) setState(() => _roomCodeCopied = false);
      });
    } catch (_) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('Could not copy — copy the code manually')),
        );
      }
    }
  }

  void _showFileMissingSnack() {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(const SnackBar(
      content: Text('This file was moved or deleted and can no longer be opened.'),
    ));
  }

  String _openResultMessage(ResultType type) {
    switch (type) {
      case ResultType.noAppToOpen:
        return 'No app is available to open this file type';
      case ResultType.permissionDenied:
        return 'Permission denied while opening the file';
      case ResultType.fileNotFound:
        return 'This file was moved or deleted';
      default:
        return 'Could not open the file';
    }
  }

  Future<void> _openSavedFile() async {
    final path = _savedFilePath;
    if (path == null) return;
    if (!await File(path).exists()) {
      _showFileMissingSnack();
      return;
    }
    final result = await OpenFilex.open(path);
    if (result.type != ResultType.done && mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text(_openResultMessage(result.type))),
      );
    }
  }

  Future<void> _shareSavedFile() async {
    final path = _savedFilePath;
    if (path == null) return;
    if (!await File(path).exists()) {
      _showFileMissingSnack();
      return;
    }
    try {
      // ignore: deprecated_member_use
      await Share.shareXFiles([XFile(path)]);
    } catch (_) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('Could not share the file')),
        );
      }
    }
  }

  Widget _buildUnifiedCard() {
    if (_roomCode == null) return const SizedBox.shrink();
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
      decoration: BoxDecoration(
        color: AppTheme.bgCardOf(context),
        borderRadius: BorderRadius.circular(14),
        border: Border.all(color: AppTheme.accentPrimary),
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (_selectedPathBadge != null) ...[
            Container(
              margin: const EdgeInsets.only(bottom: 8),
              padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
              decoration: BoxDecoration(
                color: _selectedPathBadge!.contains('Local')
                    ? const Color(0xFF166534).withValues(alpha: 0.3)
                    : const Color(0xFF1E40AF).withValues(alpha: 0.3),
                borderRadius: BorderRadius.circular(12),
                border: Border.all(
                  color: _selectedPathBadge!.contains('Local')
                       ? const Color(0xFF22C55E).withValues(alpha: 0.5)
                      : const Color(0xFF3B82F6).withValues(alpha: 0.5),
                ),
              ),
              child: Text(
                _selectedPathBadge!,
                style: TextStyle(
                  fontSize: 12,
                  fontWeight: FontWeight.w600,
                  color: _selectedPathBadge!.contains('Local')
                      ? const Color(0xFF4ADE80)
                      : const Color(0xFF60A5FA),
                ),
              ),
            ),
          ],
          Row(
            children: [
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(
                      'Room Code',
                      style: TextStyle(fontSize: 12, color: AppTheme.textSecondaryOf(context)),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                    ),
                    const SizedBox(height: 4),
                    FittedBox(
                      fit: BoxFit.scaleDown,
                      alignment: Alignment.centerLeft,
                      child: Text(
                        _roomCode!,
                        style: const TextStyle(
                          fontSize: 26,
                          fontWeight: FontWeight.bold,
                          letterSpacing: 3,
                          color: AppTheme.accentPrimary,
                        ),
                        maxLines: 1,
                      ),
                    ),
                    if (_requirePinActive && _pin != null) ...[
                      const SizedBox(height: 2),
                      Text(
                        'PIN: $_pin',
                        style: TextStyle(fontSize: 12, fontWeight: FontWeight.w600, color: AppTheme.textSecondaryOf(context)),
                      ),
                    ],
                    const SizedBox(height: 8),
                    Row(
                      children: [
                        GestureDetector(
                          onTap: _copyRoomCode,
                          child: Container(
                            padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
                            decoration: BoxDecoration(
                              color: AppTheme.bgSidebarOf(context),
                              borderRadius: BorderRadius.circular(8),
                            ),
                            child: Row(
                              mainAxisSize: MainAxisSize.min,
                              children: [
                                Icon(
                                  _roomCodeCopied ? Icons.check : Icons.copy,
                                  size: 16,
                                  color: _roomCodeCopied ? AppTheme.accentPrimary : AppTheme.textSecondaryOf(context),
                                ),
                                const SizedBox(width: 4),
                                Text(
                                  _roomCodeCopied ? 'Copied' : 'Copy',
                                  style: TextStyle(
                                    fontSize: 12,
                                    color: _roomCodeCopied ? AppTheme.accentPrimary : AppTheme.textSecondaryOf(context),
                                    fontWeight: FontWeight.w500,
                                  ),
                                ),
                              ],
                            ),
                          ),
                        ),
                        const SizedBox(width: 8),
                        GestureDetector(
                          // ignore: deprecated_member_use
                          onTap: () => Share.share('Use this code to send files on Plenum: $_roomCode'),
                          child: Container(
                            padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
                            decoration: BoxDecoration(
                              color: AppTheme.bgSidebarOf(context),
                              borderRadius: BorderRadius.circular(8),
                            ),
                            child: Row(
                              mainAxisSize: MainAxisSize.min,
                              children: [
                                Icon(Icons.share, size: 16, color: AppTheme.textSecondaryOf(context)),
                                const SizedBox(width: 4),
                                Text(
                                  'Share',
                                  style: TextStyle(
                                    fontSize: 12,
                                    color: AppTheme.textSecondaryOf(context),
                                    fontWeight: FontWeight.w500,
                                  ),
                                ),
                              ],
                            ),
                          ),
                        ),
                      ],
                    ),
                  ],
                ),
              ),
              if (_unifiedQrPayload != null) ...[
                const SizedBox(width: 12),
                GestureDetector(
                  onTap: () => _showEnlargedQrDialog(context),
                  child: Container(
                    padding: const EdgeInsets.all(6),
                    decoration: BoxDecoration(
                      color: Colors.white,
                      borderRadius: BorderRadius.circular(10),
                      boxShadow: [
                        BoxShadow(
                          color: Colors.black.withValues(alpha: 0.25),
                          blurRadius: 6,
                          offset: const Offset(0, 2),
                        ),
                      ],
                    ),
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        QrImageView(
                          data: _unifiedQrPayload!,
                          version: QrVersions.auto,
                          size: 88,
                          backgroundColor: Colors.white,
                          padding: const EdgeInsets.all(2),
                          eyeStyle: const QrEyeStyle(
                            eyeShape: QrEyeShape.square,
                            color: Colors.black,
                          ),
                          dataModuleStyle: const QrDataModuleStyle(
                            dataModuleShape: QrDataModuleShape.square,
                            color: Colors.black,
                          ),
                        ),
                        const SizedBox(height: 2),
                        const Text(
                          'Tap to enlarge',
                          style: TextStyle(
                            fontSize: 9,
                            fontWeight: FontWeight.w600,
                            color: Colors.black54,
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
              ],
            ],
          ),
          const SizedBox(height: 8),
          Text(
            'Scan QR with sender, or enter room code above',
            style: TextStyle(fontSize: 11, color: AppTheme.textSecondaryOf(context)),
            textAlign: TextAlign.center,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
          ),
        ],
      ),
    );
  }

  void _showEnlargedQrDialog(BuildContext context) {
    if (_unifiedQrPayload == null) return;
    showDialog(
      context: context,
      builder: (ctx) => Dialog(
        backgroundColor: AppTheme.bgCardOf(context),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(20),
          side: const BorderSide(color: AppTheme.accentPrimary),
        ),
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Row(
                mainAxisAlignment: MainAxisAlignment.spaceBetween,
                children: [
                  Text(
                    'Scan to Connect',
                    style: TextStyle(
                      fontSize: 16,
                      fontWeight: FontWeight.bold,
                      color: AppTheme.textPrimaryOf(context),
                    ),
                  ),
                  IconButton(
                    icon: Icon(Icons.close, size: 20, color: AppTheme.textSecondaryOf(context)),
                    onPressed: () => Navigator.of(ctx).pop(),
                  ),
                ],
              ),
              const SizedBox(height: 16),
              Container(
                padding: const EdgeInsets.all(12),
                decoration: BoxDecoration(
                  color: Colors.white,
                  borderRadius: BorderRadius.circular(16),
                  boxShadow: [
                    BoxShadow(
                      color: Colors.black.withValues(alpha: 0.3),
                      blurRadius: 12,
                      offset: const Offset(0, 4),
                    ),
                  ],
                ),
                child: QrImageView(
                  data: _unifiedQrPayload!,
                  version: QrVersions.auto,
                  size: 220,
                  backgroundColor: Colors.white,
                  padding: const EdgeInsets.all(6),
                  eyeStyle: const QrEyeStyle(
                    eyeShape: QrEyeShape.square,
                    color: Colors.black,
                  ),
                  dataModuleStyle: const QrDataModuleStyle(
                    dataModuleShape: QrDataModuleShape.square,
                    color: Colors.black,
                  ),
                ),
              ),
              const SizedBox(height: 16),
              Text(
                'Room: $_roomCode${_pin != null ? ' • PIN: $_pin' : ''}',
                style: const TextStyle(
                  fontSize: 13,
                  fontWeight: FontWeight.w600,
                  color: AppTheme.accentPrimary,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      resizeToAvoidBottomInset: false,
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
      body: Padding(
        padding: const EdgeInsets.fromLTRB(16, 8, 16, 8),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Expanded(
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  GestureDetector(
                    onTap: _isListening ? null : _startUnifiedReceiver,
                    child: AnimatedRadar(
                      isListening: _isListening,
                    ),
                  ),
                  const SizedBox(height: 16),
                  Text(
                    _statusMessage,
                    style: TextStyle(fontSize: 15, color: AppTheme.textSecondaryOf(context)),
                    textAlign: TextAlign.center,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                  ),
                  if (_isListening)
                    TextButton.icon(
                      onPressed: _stopReceiving,
                      style: TextButton.styleFrom(
                        visualDensity: VisualDensity.compact,
                        tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                      ),
                      icon: const Icon(Icons.stop_circle, color: AppTheme.accentPrimary, size: 18),
                      label: const Text('Stop Receiving', style: TextStyle(color: AppTheme.accentPrimary)),
                    ),
                  if (isTransferFailure(_statusMessage))
                    Padding(
                      padding: const EdgeInsets.only(top: 4),
                      child: ElevatedButton.icon(
                        onPressed: () {
                          _stopReceiving();
                          _startUnifiedReceiver();
                        },
                        icon: const Icon(Icons.refresh),
                        label: const Text('Retry'),
                      ),
                    ),
                ],
              ),
            ),
            if (_isListening && _roomCode != null)
              _buildUnifiedCard(),
            if (_progress != null) ...[
              const SizedBox(height: 8),
              Container(
                width: double.infinity,
                padding: const EdgeInsets.all(12),
                decoration: BoxDecoration(
                  color: AppTheme.bgSidebarOf(context),
                  borderRadius: BorderRadius.circular(12),
                ),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    ClipRRect(
                      borderRadius: BorderRadius.circular(4),
                      child: LinearProgressIndicator(
                        value: _progress,
                        minHeight: 8,
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
                    if (_progress == 1.0 && _savedFilePath != null) ...[
                      const SizedBox(height: 8),
                      const Center(child: SuccessCheck()),
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
                      const SizedBox(height: 8),
                      Container(
                        width: double.infinity,
                        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
                        decoration: BoxDecoration(
                          color: AppTheme.accentPrimary.withValues(alpha: 0.1),
                          borderRadius: BorderRadius.circular(8),
                          border: Border.all(color: AppTheme.accentPrimary.withValues(alpha: 0.4)),
                        ),
                        child: Text(
                          _savedLocation != null ? 'Saved to $_savedLocation' : 'Saving to Downloads...',
                          style: TextStyle(color: AppTheme.textPrimaryOf(context), fontSize: 12, fontWeight: FontWeight.w600),
                          textAlign: TextAlign.center,
                          maxLines: 2,
                          overflow: TextOverflow.ellipsis,
                        ),
                      ),
                      const SizedBox(height: 12),
                      Row(
                        children: [
                          Expanded(
                            child: ElevatedButton(
                              onPressed: _openSavedFile,
                              child: const Text('Open'),
                            ),
                          ),
                          const SizedBox(width: 8),
                          Expanded(
                            child: ElevatedButton(
                              onPressed: _shareSavedFile,
                              child: const Text('Share'),
                            ),
                          ),
                        ],
                      ),
                      TextButton(
                        onPressed: _reArmReceiving,
                        style: TextButton.styleFrom(
                          visualDensity: VisualDensity.compact,
                          tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                        ),
                        child: const Text('Receive another'),
                      ),
                    ],
                  ],
                ),
              ),
            ],
          ],
        ),
      ),
    );
  }
}

