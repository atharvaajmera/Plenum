import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:file_picker/file_picker.dart';
import 'package:mobile/src/rust/api/plenum_api.dart';
import '../config.dart';
import '../services/internet_settings.dart';
import '../theme.dart';

enum _TransferMode { local, internet }

String _friendlyState(String state) {
  switch (state) {
    case 'Discovering':
      return 'Searching for devices...';
    case 'Listening':
      return 'Ready to send';
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

class SendScreen extends StatefulWidget {
  const SendScreen({super.key});

  @override
  State<SendScreen> createState() => _SendScreenState();
}

class _SendScreenState extends State<SendScreen> {
  _TransferMode _mode = _TransferMode.local;
  String? _selectedFile;
  final List<Map<String, dynamic>> _peers = [];
  bool _isDiscovering = false;
  String _transferStatus = '';
  double? _progress;

  final TextEditingController _roomCodeController = TextEditingController();
  bool _isConnectingRemote = false;

  @override
  void initState() {
    super.initState();
    _startDiscovery();
  }

  @override
  void dispose() {
    _roomCodeController.dispose();
    super.dispose();
  }

  void _startDiscovery() {
    setState(() {
      _isDiscovering = true;
      _peers.clear();
      _transferStatus = '';
      _progress = null;
    });

    startDiscovery(timeoutSecs: BigInt.from(10)).listen((eventJson) {
      final event = jsonDecode(eventJson);
      if (event['Discovery'] != null) {
        final discEvent = event['Discovery'];
        if (discEvent is Map && discEvent['PeerFound'] != null) {
          setState(() {
            // Avoid duplicates
            if (!_peers.any((p) => p['token'] == discEvent['PeerFound']['token'])) {
              _peers.add(discEvent['PeerFound']);
            }
          });
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
      });
    }
  }

  void _handleTransferEvent(String eventJson) {
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
          setState(() => _transferStatus = _friendlyState(trans['StateChanged']['state']));
        }
      } else if (trans['Started'] != null) {
        setState(() {
          _transferStatus = 'Sending ${trans['Started']['file_name']}...';
          _progress = 0.0;
        });
      } else if (trans['Progress'] != null) {
        setState(() {
          _progress = trans['Progress']['transferred_bytes'] / trans['Progress']['total_bytes'];
        });
      } else if (trans['Completed'] != null) {
        setState(() {
          _transferStatus = 'Sent successfully!';
          _progress = 1.0;
        });
      }
    }
  }

  void _sendToPeer(String address, String? pin) {
    if (_selectedFile == null) return;

    startSend(filePath: _selectedFile!, peerAddress: address, optionalPin: pin)
        .listen(_handleTransferEvent);
  }

  Future<void> _handleRoomCodeConnect() async {
    if (_selectedFile == null) {
      setState(() => _transferStatus = 'Please select a file first');
      return;
    }
    final roomCode = _roomCodeController.text.trim();
    if (roomCode.isEmpty) {
      setState(() => _transferStatus = 'Please enter a room code');
      return;
    }

    const relayServerUrl = PlenumConfig.relayServerUrl;
    final iceServers = PlenumConfig.defaultIceServers();

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
      startSendRemote(
        filePath: _selectedFile!,
        relayServerUrl: relayServerUrl,
        sessionId: roomCode.toUpperCase(),
        myPeerId: myPeerId,
        iceServersJson: iceServersJson,
        connectTimeoutSecs: BigInt.from(30),
      ).listen(
        _handleTransferEvent,
        onDone: () {
          if (mounted) setState(() => _isConnectingRemote = false);
        },
        onError: (e) {
          if (mounted) {
            setState(() {
              _transferStatus = 'Error: $e';
              _progress = null;
              _isConnectingRemote = false;
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

  void _showPinDialog(String address, String hostname) {
    if (_selectedFile == null) return;

    final TextEditingController pinController = TextEditingController();

    showDialog(
      context: context,
      builder: (context) {
        return AlertDialog(
          backgroundColor: AppTheme.bgCard,
          title: Text('Send to $hostname', style: const TextStyle(color: AppTheme.textPrimary)),
          content: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              const Text('If the receiver requires a PIN, enter it below. Otherwise, leave blank.', style: TextStyle(color: AppTheme.textSecondary, fontSize: 14)),
              const SizedBox(height: 16),
              TextField(
                controller: pinController,
                decoration: const InputDecoration(
                  labelText: 'PIN (Optional)',
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
                final pin = pinController.text.trim();
                _sendToPeer(address, pin.isNotEmpty ? pin : null);
              },
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
            selected: _mode == _TransferMode.local,
            onTap: () => setState(() => _mode = _TransferMode.local),
          ),
        ),
        const SizedBox(width: 12),
        Expanded(
          child: _ModeCard(
            icon: Icons.public,
            label: 'Internet',
            selected: _mode == _TransferMode.internet,
            onTap: () => setState(() => _mode = _TransferMode.internet),
          ),
        ),
      ],
    );
  }

  Widget _buildFilePicker() {
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
            Icon(
              _selectedFile == null ? Icons.upload_file : Icons.check_circle,
              size: 48,
              color: _selectedFile == null ? AppTheme.textSecondary : AppTheme.accentPrimary,
            ),
            const SizedBox(height: 16),
            Text(
              _selectedFile == null ? 'Select File to Send' : _selectedFile!.split(RegExp(r'[\\/]')).last,
              style: const TextStyle(fontWeight: FontWeight.w600, fontSize: 16, color: AppTheme.textPrimary),
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
          ]
        ],
      ),
    );
  }

  Widget _buildInternetPanel() {
    return Expanded(
      child: SingleChildScrollView(
        child: Column(
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
                const SizedBox(width: 12),
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
        ),
      ),
    );
  }

  Widget _buildLocalPanel() {
    return Expanded(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              const Text('Discovered Devices', style: TextStyle(fontWeight: FontWeight.w600, color: AppTheme.textPrimary, fontSize: 14)),
              GestureDetector(
                onTap: _showManualIpDialog,
                child: Container(
                  padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
                  decoration: BoxDecoration(
                    border: Border.all(color: AppTheme.accentPrimary),
                    borderRadius: BorderRadius.circular(8),
                  ),
                  child: const Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Icon(Icons.add, size: 16, color: AppTheme.accentPrimary),
                      SizedBox(width: 4),
                      Text('Manual IP', style: TextStyle(color: AppTheme.accentPrimary, fontSize: 12, fontWeight: FontWeight.w600)),
                    ],
                  ),
                ),
              ),
            ],
          ),
          const SizedBox(height: 16),

          if (_peers.isEmpty && !_isDiscovering)
            const Center(child: Padding(
              padding: EdgeInsets.all(32.0),
              child: Text('No devices found.\nTry "Manual IP" to connect directly.', textAlign: TextAlign.center, style: TextStyle(color: AppTheme.textSecondary)),
            )),

          Expanded(
            child: ListView.separated(
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
                    trailing: IconButton(
                      icon: const Icon(Icons.send_rounded, color: AppTheme.accentPrimary),
                      onPressed: _selectedFile == null ? null : () => _showPinDialog(peer['address'], peer['hostname'] ?? 'Unknown Device'),
                    ),
                  ),
                );
              },
            ),
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
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Send'),
        actions: [
          if (_mode == _TransferMode.local)
            IconButton(
              icon: _isDiscovering
                  ? const SizedBox(width: 20, height: 20, child: CircularProgressIndicator(strokeWidth: 2, color: AppTheme.accentPrimary))
                  : const Icon(Icons.refresh),
              onPressed: _isDiscovering ? null : _startDiscovery,
            )
        ],
      ),
      body: Padding(
        padding: const EdgeInsets.all(24.0),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            _buildModeToggle(),
            const SizedBox(height: 24),
            _buildFilePicker(),
            const SizedBox(height: 32),
            _mode == _TransferMode.local ? _buildLocalPanel() : _buildInternetPanel(),
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
