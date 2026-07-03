import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:file_picker/file_picker.dart';
import 'package:mobile/src/rust/api/plenum_api.dart';
import '../theme.dart';

class SendScreen extends StatefulWidget {
  const SendScreen({super.key});

  @override
  State<SendScreen> createState() => _SendScreenState();
}

class _SendScreenState extends State<SendScreen> {
  String? _selectedFile;
  final List<Map<String, dynamic>> _peers = [];
  bool _isDiscovering = false;
  String _transferStatus = '';
  double? _progress;

  @override
  void initState() {
    super.initState();
    _startDiscovery();
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
        if (discEvent['PeerFound'] != null) {
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

  void _sendToPeer(String address) {
    if (_selectedFile == null) return;
    
    startSend(filePath: _selectedFile!, peerAddress: address).listen((eventJson) {
      final event = jsonDecode(eventJson);
      if (event['Transfer'] != null) {
        final trans = event['Transfer'];
        if (trans['StateChanged'] != null) {
          if (trans['StateChanged']['state'] != 'Closed') {
            setState(() => _transferStatus = trans['StateChanged']['state']);
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
    });
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Send'),
        actions: [
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
            // Action Card (like Desktop)
            GestureDetector(
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
            ),
            
            const SizedBox(height: 32),
            const Text('Discovered Devices', style: TextStyle(fontWeight: FontWeight.w600, color: AppTheme.textPrimary, fontSize: 14)),
            const SizedBox(height: 16),
            
            if (_peers.isEmpty && !_isDiscovering)
              const Center(child: Padding(
                padding: EdgeInsets.all(32.0),
                child: Text('No devices found on local network.', style: TextStyle(color: AppTheme.textSecondary)),
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
                        onPressed: _selectedFile == null ? null : () => _sendToPeer(peer['address']),
                      ),
                    ),
                  );
                },
              ),
            ),

            if (_transferStatus.isNotEmpty)
              Container(
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
              ),
          ],
        ),
      ),
    );
  }
}
