import 'dart:async';
import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:file_picker/file_picker.dart';
import 'package:permission_handler/permission_handler.dart';
import 'package:mobile/src/rust/api/plenum_api.dart';
import '../services/internet_settings.dart';
import '../theme.dart';
import 'package:provider/provider.dart';
import '../services/settings_service.dart';

import '../utils/transfer_status.dart';
import '../utils/formatters.dart';
import 'settings_screen.dart';

class SendScreen extends StatefulWidget {
  const SendScreen({super.key});

  @override
  State<SendScreen> createState() => _SendScreenState();
}

class _SendScreenState extends State<SendScreen> {
  TransferMode _mode = TransferMode.local;
  String? _selectedFile;
  final List<Map<String, dynamic>> _peers = [];
  bool _isDiscovering = false;
  String _transferStatus = '';
  String? _currentTransferPeerName;
  double? _progress;
  int? _selectedFileSize;

  int? _totalBytes;
  int? _transferredBytes;
  DateTime? _transferStartTime;
  String? _speedText;
  String? _etaText;

  final TextEditingController _roomCodeController = TextEditingController();
  bool _isConnectingRemote = false;
  String? _sessionToken;
  StreamSubscription<String>? _transferSub;
  bool _transferActive = false;

  @override
  void initState() {
    super.initState();
    _startDiscovery();
  }

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

  @override
  void dispose() {
    final token = _sessionToken;
    if (token != null) {
      try {
        cancelSession(sessionToken: token);
      } catch (_) {}
    }
    _transferSub?.cancel();
    _roomCodeController.dispose();
    super.dispose();
  }

  /// Cancels an in-flight transfer: flips the Rust-side flag; the engine
  /// sends `Close` to the peer, emits `Cancelled`, and returns.
  void _cancelTransfer() {
    final token = _sessionToken;
    if (token != null) {
      try {
        cancelSession(sessionToken: token);
      } catch (_) {}
    }
  }

  void _startDiscovery() async {
    await Permission.nearbyWifiDevices.request();
    if (!mounted) return;

    setState(() {
      _peers.clear();
      _transferStatus = '';
      _progress = null;
    });

    startDiscovery(timeoutSecs: BigInt.from(10)).listen((eventJson) {
      if (!mounted) return;
      final event = jsonDecode(eventJson);
      if (event['Discovery'] != null) {
        final discEvent = event['Discovery'];
        if (discEvent == 'PeerNotFound') {
          setState(() {
            _isDiscovering = false;
            _transferStatus = 'No devices found';
          });
        } else if (discEvent is Map) {
          if (discEvent['PeerFound'] != null) {
            setState(() {
              final found = discEvent['PeerFound'];
              final token = found['token'];
              // PIN-required peers announce an empty token, so only dedup by
              // token when it is non-empty; address is always unique per peer.
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
            });
          }
        }
      }
    }, onDone: () {
      if (mounted) setState(() => _isDiscovering = false);
    });
  }

  Future<void> _pickFile() async {
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
      final level = log['level'];
      if (level == 'Warn' || level == 'Error') {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content: Text(log['message'] ?? 'Error occurred'),
          backgroundColor: level == 'Error' ? Colors.redAccent : Colors.orange,
        ));
      }
      return;
    }
    if (event['Transfer'] != null) {
      final trans = event['Transfer'];
      if (trans['StateChanged'] != null) {
        final state = trans['StateChanged']['state'];
        if (state == 'Closed') {
          setState(() {
            _transferStatus = '';
            _progress = null;
            _totalBytes = null;
            _transferredBytes = null;
            _transferStartTime = null;
            _speedText = null;
            _etaText = null;
            _isConnectingRemote = false;
            _transferActive = false;
          });
        } else {
          setState(() => _transferStatus = friendlyState(state));
        }
      } else if (trans['AwaitingApproval'] != null) {
        setState(() {
          _transferStatus = 'Waiting for the receiver to accept...';
          _transferActive = true;
        });
      } else if (trans['Cancelled'] != null) {
        setState(() {
          _transferStatus = 'Transfer cancelled';
          _progress = null;
          _isConnectingRemote = false;
          _transferActive = false;
        });
      } else if (trans['Declined'] != null) {
        final reason = trans['Declined']['reason'];
        setState(() {
          _transferStatus = switch (reason) {
            'pin_rejected' => 'Wrong pairing code — check the code on the receiver\'s screen',
            'cancelled' => 'The receiver cancelled the transfer',
            _ => 'The receiver declined the transfer',
          };
          _progress = null;
          _isConnectingRemote = false;
          _transferActive = false;
        });
      } else if (trans['Started'] != null) {
        setState(() {
          _transferStatus = 'Sending ${trans['Started']['file_name']}...';
          _progress = 0.0;
          _totalBytes = trans['Started']['total_bytes'];
          _transferStartTime = DateTime.now();
          _speedText = null;
          _etaText = null;
          _transferActive = true;
        });
      } else if (trans['Resumed'] != null) {
        setState(() {
          final resumedBytes = trans['Resumed']['resumed_bytes'] ?? 0;
          final percent = _totalBytes != null && _totalBytes! > 0 ? (resumedBytes / _totalBytes! * 100).toStringAsFixed(1) : '0';
          _transferStatus = 'Resuming from $percent%...';
        });
      } else if (trans['Progress'] != null) {
        setState(() {
          _transferredBytes = trans['Progress']['transferred_bytes'];
          _totalBytes = trans['Progress']['total_bytes'];
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
      } else if (trans['Completed'] != null) {
        final summary = trans['Completed'];
        final settings = context.read<SettingsService>();
        settings.addTransferHistory({
          'direction': 'send',
          'fileName': summary['file_name'] ?? _selectedFile?.split(RegExp(r'[\\/]')).last ?? 'Unknown file',
          'size': summary['total_bytes'] ?? _selectedFileSize,
          'peerName': summary['peer'] ?? _currentTransferPeerName ?? 'Unknown device',
          'timestamp': DateTime.now().toIso8601String(),
        });
        setState(() {
          _transferStatus = 'Sent successfully!';
          _progress = 1.0;
          _isConnectingRemote = false;
          _transferActive = false;
        });
      }
    }
  }

  void _sendToPeer(String address, String hostname, String? pin) {
    if (_selectedFile == null) return;
    _currentTransferPeerName = hostname;

    final sessionToken = DateTime.now().millisecondsSinceEpoch.toString();
    _sessionToken = sessionToken;
    setState(() => _transferActive = true);
    _transferSub = startSend(
      filePath: _selectedFile!,
      peerAddress: address,
      optionalPin: pin,
      sessionToken: sessionToken,
    ).listen(
      _handleTransferEvent,
      onDone: () {
        if (mounted) setState(() => _transferActive = false);
      },
      onError: (e) {
        if (mounted) {
          setState(() {
            _transferStatus = 'Error: $e';
            _progress = null;
            _transferActive = false;
          });
        }
      },
    );
  }

  Future<void> _handleRoomCodeConnect() async {
    if (_selectedFile == null) {
      ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Please select a file first')));
      return;
    }
    final roomCode = _roomCodeController.text.trim();
    if (roomCode.isEmpty) {
      ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Please enter a room code')));
      return;
    }

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
      _transferStatus = 'Connecting to relay...';
      _isConnectingRemote = true;
    });

    try {
      final myPeerId = generatePeerIdSync();
      final iceServersJson = await InternetSettings.buildIceServersJsonWithTurn(
        relayServerUrl,
        myPeerId,
        iceServers,
      );
      final sessionToken = DateTime.now().millisecondsSinceEpoch.toString();
      _sessionToken = sessionToken;
      setState(() => _transferActive = true);
      _transferSub = startSendRemote(
        filePath: _selectedFile!,
        relayServerUrl: relayServerUrl,
        sessionId: roomCode.toUpperCase(),
        myPeerId: myPeerId,
        iceServersJson: iceServersJson,
        connectTimeoutSecs: BigInt.from(30),
        sessionToken: sessionToken,
      ).listen(
        _handleTransferEvent,
        onDone: () {
          if (mounted) setState(() {
            _isConnectingRemote = false;
            _transferActive = false;
          });
        },
        onError: (e) {
          if (mounted) {
            setState(() {
              _transferStatus = 'Error: $e';
              _progress = null;
              _isConnectingRemote = false;
              _transferActive = false;
            });
          }
        },
      );
    } catch (e) {
      setState(() {
        _transferStatus = 'Error: $e';
        _isConnectingRemote = false;
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
      _sendToPeer(address, hostname, pin.isNotEmpty ? pin : null);
    }

    showDialog(
      context: context,
      builder: (context) {
        return AlertDialog(
          backgroundColor: AppTheme.bgCard,
          title: Text('Send to $hostname', style: const TextStyle(color: AppTheme.textPrimary)),
          content: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(
                pinRequired
                    ? 'This device requires a pairing code. Enter the code shown on its screen.'
                    : 'If the receiver requires a pairing code, enter it below. Otherwise, leave blank.',
                style: const TextStyle(color: AppTheme.textSecondary, fontSize: 14),
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
                ),
                style: const TextStyle(color: AppTheme.textPrimary),
                onSubmitted: (_) => submit(context),
              ),
            ],
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context),
              child: const Text('Cancel', style: TextStyle(color: AppTheme.textSecondary)),
            ),
            ElevatedButton(
              onPressed: () => submit(context),
              style: ElevatedButton.styleFrom(backgroundColor: AppTheme.accentPrimary),
              child: const Text('Send'),
            ),
          ],
        );
      }
    );
  }

  void _showManualIpDialog() {
    final TextEditingController ipController = TextEditingController();
    final TextEditingController portController = TextEditingController(text: '8080');

    showDialog(
      context: context,
      builder: (context) {
        return AlertDialog(
          backgroundColor: AppTheme.bgCard,
          title: const Text('Connect by IP', style: TextStyle(color: AppTheme.textPrimary)),
          content: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              const Text(
                'Enter the IP address shown on the receiver\'s screen.',
                style: TextStyle(color: AppTheme.textSecondary, fontSize: 13),
              ),
              const SizedBox(height: 16),
              TextField(
                controller: ipController,
                keyboardType: TextInputType.number,
                decoration: const InputDecoration(
                  labelText: 'IP Address',
                  hintText: '192.168.1.5',
                  border: OutlineInputBorder(),
                  focusedBorder: OutlineInputBorder(borderSide: BorderSide(color: AppTheme.accentPrimary)),
                ),
                style: const TextStyle(color: AppTheme.textPrimary),
              ),
              const SizedBox(height: 12),
              TextField(
                controller: portController,
                keyboardType: TextInputType.number,
                decoration: const InputDecoration(
                  labelText: 'Port',
                  border: OutlineInputBorder(),
                  focusedBorder: OutlineInputBorder(borderSide: BorderSide(color: AppTheme.accentPrimary)),
                ),
                style: const TextStyle(color: AppTheme.textPrimary),
              ),
            ],
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context),
              child: const Text('Cancel', style: TextStyle(color: AppTheme.textSecondary)),
            ),
            ElevatedButton(
              onPressed: () {
                Navigator.pop(context);
                final ip = ipController.text.trim();
                final port = portController.text.trim();
                if (ip.isNotEmpty) {
                  final address = '$ip:$port';
                  setState(() {
                    _peers.add({'hostname': ip, 'address': address, 'token': 'manual'});
                  });
                }
              },
              style: ElevatedButton.styleFrom(backgroundColor: AppTheme.accentPrimary),
              child: const Text('Add'),
            ),
          ],
        );
      },
    );
  }

  Widget _buildModeToggle() {
    return Row(
      children: [
        Expanded(
          child: _ModeCard(
            icon: Icons.wifi,
            label: 'Local Network',
            selected: _mode == TransferMode.local,
            onTap: () => setState(() => _mode = TransferMode.local),
          ),
        ),
        const SizedBox(width: 12),
        Expanded(
          child: _ModeCard(
            icon: Icons.public,
            label: 'Internet',
            selected: _mode == TransferMode.internet,
            onTap: () => setState(() => _mode = TransferMode.internet),
          ),
        ),
      ],
    );
  }

  Widget _buildFilePicker() {
    if (_selectedFile != null) {
      return Container(
        width: double.infinity,
        padding: const EdgeInsets.all(16),
        decoration: BoxDecoration(
          color: AppTheme.bgCard,
          borderRadius: BorderRadius.circular(16),
          border: Border.all(color: AppTheme.accentPrimary),
        ),
        child: Row(
          children: [
            const Icon(Icons.insert_drive_file, color: AppTheme.accentPrimary, size: 32),
            const SizedBox(width: 16),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    _selectedFile!.split(RegExp(r'[\\/]')).last,
                    style: const TextStyle(fontWeight: FontWeight.w600, color: AppTheme.textPrimary),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                  const SizedBox(height: 4),
                  if (_selectedFileSize != null)
                    Text(formatBytes(_selectedFileSize!), style: const TextStyle(color: AppTheme.textSecondary, fontSize: 12)),
                ],
              ),
            ),
            IconButton(
              icon: const Icon(Icons.close, color: AppTheme.textSecondary),
              onPressed: () {
                setState(() {
                  _selectedFile = null;
                  _selectedFileSize = null;
                });
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
        padding: const EdgeInsets.symmetric(vertical: 32),
        decoration: BoxDecoration(
          gradient: const LinearGradient(
            begin: Alignment.topLeft,
            end: Alignment.bottomRight,
            colors: [AppTheme.bgCard, Color(0xFF1E2835)],
          ),
          borderRadius: BorderRadius.circular(16),
          border: Border.all(color: AppTheme.borderColor),
          boxShadow: [
            BoxShadow(color: Colors.black.withOpacity(0.1), blurRadius: 6, offset: const Offset(0, 4))
          ],
        ),
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            const Icon(
              Icons.upload_file,
              size: 48,
              color: AppTheme.textSecondary,
            ),
            const SizedBox(height: 16),
            const Text(
              'Select File to Send',
              style: TextStyle(fontWeight: FontWeight.w600, fontSize: 16, color: AppTheme.textPrimary),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildStatusCard() {
    if (_transferStatus.isEmpty) return const SizedBox.shrink();
    return Container(
      margin: const EdgeInsets.only(top: 16),
      padding: const EdgeInsets.all(16),
      decoration: BoxDecoration(
        color: AppTheme.bgSidebar,
        borderRadius: BorderRadius.circular(12),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(_transferStatus, style: const TextStyle(color: AppTheme.textPrimary, fontSize: 14), textAlign: TextAlign.center),
          if (_isConnectingRemote) ...[
            const SizedBox(height: 12),
            const Center(child: CircularProgressIndicator(color: AppTheme.accentPrimary)),
          ],
          if (_progress != null) ...[
            const SizedBox(height: 12),
            ClipRRect(
              borderRadius: BorderRadius.circular(4),
              child: LinearProgressIndicator(
                value: _progress,
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
          ],
          if (_transferActive && _progress != 1.0)
            Padding(
              padding: const EdgeInsets.only(top: 12),
              child: TextButton.icon(
                onPressed: _cancelTransfer,
                icon: const Icon(Icons.cancel, color: AppTheme.accentPrimary, size: 18),
                label: const Text('Cancel transfer', style: TextStyle(color: AppTheme.accentPrimary)),
              ),
            ),
          if (_progress == 1.0)
            Padding(
              padding: const EdgeInsets.only(top: 16),
              child: ElevatedButton(
                onPressed: () {
                  setState(() {
                    _transferStatus = '';
                    _progress = null;
                    _totalBytes = null;
                    _transferredBytes = null;
                    _transferStartTime = null;
                    _speedText = null;
                    _etaText = null;
                  });
                },
                child: const Text('Send another file'),
              ),
            )
        ],
      ),
    );
  }

  Widget _buildInternetPanel() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
            const Text('Connect via room code', style: TextStyle(fontWeight: FontWeight.w600, color: AppTheme.textPrimary, fontSize: 14)),
            const SizedBox(height: 16),
            Row(
              children: [
                Expanded(
                  child: TextField(
                    controller: _roomCodeController,
                    textCapitalization: TextCapitalization.characters,
                    decoration: const InputDecoration(
                      hintText: 'Enter room code',
                      border: OutlineInputBorder(),
                      focusedBorder: OutlineInputBorder(borderSide: BorderSide(color: AppTheme.accentPrimary)),
                    ),
                    style: const TextStyle(color: AppTheme.textPrimary, letterSpacing: 2),
                    onSubmitted: (_) => _handleRoomCodeConnect(),
                  ),
                ),
                const SizedBox(width: 8),
                IconButton(
                  icon: const Icon(Icons.paste, color: AppTheme.accentPrimary),
                  onPressed: () async {
                    final data = await Clipboard.getData(Clipboard.kTextPlain);
                    if (data != null && data.text != null) {
                      _roomCodeController.text = data.text!;
                    }
                  },
                ),
                const SizedBox(width: 8),
                ElevatedButton(
                  onPressed: _isConnectingRemote ? null : _handleRoomCodeConnect,
                  child: const Text('Connect'),
                ),
              ],
            ),
            _buildStatusCard(),
            const SizedBox(height: 32),
            const Text(
              'Ask the receiver for their room code, then tap Connect to send over the internet.',
              style: TextStyle(fontSize: 13, color: AppTheme.textSecondary),
              textAlign: TextAlign.center,
            ),
          ],
    );
  }

  Widget _buildLocalPanel() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
          Row(
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              const Text('Discovered Devices', style: TextStyle(fontWeight: FontWeight.w600, color: AppTheme.textPrimary, fontSize: 14)),
              IconButton(
                icon: _isDiscovering
                    ? const SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2, color: AppTheme.accentPrimary))
                    : const Icon(Icons.refresh, color: AppTheme.accentPrimary),
                onPressed: _isDiscovering ? null : _startDiscovery,
              ),
            ],
          ),
          const SizedBox(height: 16),

          if (_peers.isEmpty)
            if (_isDiscovering)
              const Center(child: Padding(
                padding: EdgeInsets.all(32.0),
                child: CircularProgressIndicator(color: AppTheme.accentPrimary),
              ))
            else
              Center(child: Padding(
                padding: const EdgeInsets.all(32.0),
                child: Column(
                  children: [
                    const Text('No devices found.', textAlign: TextAlign.center, style: TextStyle(color: AppTheme.textSecondary)),
                    const SizedBox(height: 16),
                    ElevatedButton(
                      onPressed: _startDiscovery,
                      child: const Text('Search again'),
                    ),
                  ],
                ),
              )),

          ListView.separated(
            shrinkWrap: true,
            physics: const NeverScrollableScrollPhysics(),
            itemCount: _peers.length,
              separatorBuilder: (context, index) => const SizedBox(height: 12),
              itemBuilder: (context, index) {
                final peer = _peers[index];
                return Container(
                  decoration: BoxDecoration(
                    color: AppTheme.bgCard,
                    borderRadius: BorderRadius.circular(12),
                    border: Border.all(color: AppTheme.borderColor),
                  ),
                  child: ListTile(
                    contentPadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
                    onTap: () {
                      if (_selectedFile == null) {
                        ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Please select a file first')));
                        return;
                      }
                      _showPinDialog(
                        peer['address'],
                        peer['hostname'] ?? 'Unknown Device',
                        pinRequired: peer['pin_required'] == true,
                      );
                    },

                    leading: Container(
                      padding: const EdgeInsets.all(10),
                      decoration: BoxDecoration(
                        color: AppTheme.bgSidebar,
                        borderRadius: BorderRadius.circular(8),
                      ),
                      child: const Icon(Icons.computer, color: AppTheme.accentPrimary),
                    ),
                    title: Text(peer['hostname'] ?? 'Unknown Device', style: const TextStyle(fontWeight: FontWeight.w600, color: AppTheme.textPrimary)),
                    subtitle: Text(peer['address'] ?? '', style: const TextStyle(color: AppTheme.textSecondary, fontSize: 12)),
                    trailing: const Icon(Icons.send_rounded, color: AppTheme.accentPrimary),
                  ),
                );
              },
            ),

          _buildStatusCard(),

          const SizedBox(height: 16),
          Center(
            child: Column(
              children: [
                const Text(
                  'Please ensure that the desired target is also on the same Wi-Fi network.',
                  style: TextStyle(fontSize: 13, color: AppTheme.textSecondary),
                  textAlign: TextAlign.center,
                ),
              ],
            ),
          ),
        ],
    );
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
      body: SingleChildScrollView(
        padding: const EdgeInsets.all(24.0),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            _buildModeToggle(),
            const SizedBox(height: 24),
            _buildFilePicker(),
            const SizedBox(height: 32),
            _mode == TransferMode.local ? _buildLocalPanel() : _buildInternetPanel(),
          ],
        ),
      ),
    );
  }
}

class _ModeCard extends StatelessWidget {
  final IconData icon;
  final String label;
  final bool selected;
  final VoidCallback onTap;

  const _ModeCard({required this.icon, required this.label, required this.selected, required this.onTap});

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(vertical: 16),
        decoration: BoxDecoration(
          color: AppTheme.bgCard,
          borderRadius: BorderRadius.circular(12),
          border: Border.all(color: selected ? AppTheme.accentPrimary : AppTheme.borderColor, width: selected ? 2 : 1),
        ),
        child: Column(
          children: [
            Icon(icon, color: selected ? AppTheme.accentPrimary : AppTheme.textSecondary),
            const SizedBox(height: 8),
            Text(label, style: TextStyle(color: selected ? AppTheme.accentPrimary : AppTheme.textSecondary, fontWeight: FontWeight.w600, fontSize: 13)),
          ],
        ),
      ),
    );
  }
}
