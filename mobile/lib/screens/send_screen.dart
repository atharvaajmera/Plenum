import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:file_picker/file_picker.dart';
import 'package:mobile/src/rust/api/plenum_api.dart';

class SendScreen extends StatefulWidget {
  const SendScreen({super.key});

  @override
  State<SendScreen> createState() => _SendScreenState();
}

class _SendScreenState extends State<SendScreen> {
  String? _selectedFile;
  final List<Map<String, dynamic>> _peers = [];
  bool _isDiscovering = false;

  void _startDiscovery() {
    setState(() {
      _isDiscovering = true;
      _peers.clear();
    });

    startDiscovery(timeoutSecs: BigInt.from(10)).listen((eventJson) {
      final event = jsonDecode(eventJson);
      if (event['Discovery'] != null) {
        final discEvent = event['Discovery'];
        if (discEvent['PeerFound'] != null) {
          setState(() {
            _peers.add(discEvent['PeerFound']);
          });
        }
      }
    }, onDone: () {
      setState(() => _isDiscovering = false);
    });
  }

  Future<void> _pickFile() async {
    FilePickerResult? result = await FilePicker.pickFiles();
    if (result != null) {
      setState(() {
        _selectedFile = result.files.single.path;
      });
      _startDiscovery();
    }
  }

  void _sendToPeer(String address) {
    if (_selectedFile == null) return;
    
    startSend(filePath: _selectedFile!, peerAddress: address).listen((eventJson) {
      final event = jsonDecode(eventJson);
      // Handle progress and completion
      print(event);
    });
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Send Files')),
      body: Center(
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            ElevatedButton.icon(
              onPressed: _pickFile,
              icon: const Icon(Icons.file_upload),
              label: Text(_selectedFile == null ? 'Select File' : 'Change File'),
            ),
            if (_selectedFile != null)
              Padding(
                padding: const EdgeInsets.all(8.0),
                child: Text('Selected: ${_selectedFile!.split(RegExp(r'[\\/]')).last}'),
              ),
            const SizedBox(height: 20),
            if (_isDiscovering) const CircularProgressIndicator(),
            Expanded(
              child: ListView.builder(
                itemCount: _peers.length,
                itemBuilder: (context, index) {
                  final peer = _peers[index];
                  return ListTile(
                    leading: const Icon(Icons.computer),
                    title: Text(peer['hostname'] ?? 'Unknown Device'),
                    subtitle: Text(peer['address'] ?? ''),
                    trailing: const Icon(Icons.send),
                    onTap: () => _sendToPeer(peer['address']),
                  );
                },
              ),
            ),
          ],
        ),
      ),
    );
  }
}
