import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';
import 'package:mobile/src/rust/api/plenum_api.dart';

class ReceiveScreen extends StatefulWidget {
  const ReceiveScreen({super.key});

  @override
  State<ReceiveScreen> createState() => _ReceiveScreenState();
}

class _ReceiveScreenState extends State<ReceiveScreen> {
  bool _isListening = false;
  String _statusMessage = 'Tap to start receiving';
  String? _pin;
  double? _progress;

  void _startReceiving() async {
    final dir = await getApplicationDocumentsDirectory();
    setState(() {
      _isListening = true;
      _statusMessage = 'Listening for incoming files...';
    });

    startReceive(outputDir: dir.path, port: 8080, announce: true).listen((eventJson) {
      final event = jsonDecode(eventJson);
      
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
          setState(() => _statusMessage = 'Receiving ${transEvent['Started']['file_name']}');
        } else if (transEvent['Progress'] != null) {
          final p = transEvent['Progress'];
          setState(() {
            _progress = p['transferred_bytes'] / p['total_bytes'];
          });
        } else if (transEvent['Completed'] != null) {
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

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Receive Files')),
      body: Center(
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            GestureDetector(
              onTap: _isListening ? null : _startReceiving,
              child: Icon(
                _isListening ? Icons.radar : Icons.wifi_tethering, 
                size: 64, 
                color: _isListening ? Colors.blue : Colors.grey
              ),
            ),
            const SizedBox(height: 20),
            Text(_statusMessage, style: const TextStyle(fontSize: 18)),
            const SizedBox(height: 10),
            if (_pin != null)
              Text('PIN: $_pin', style: const TextStyle(fontSize: 24, fontWeight: FontWeight.bold, letterSpacing: 4)),
            if (_progress != null)
              Padding(
                padding: const EdgeInsets.all(20.0),
                child: LinearProgressIndicator(value: _progress),
              ),
          ],
        ),
      ),
    );
  }
}
