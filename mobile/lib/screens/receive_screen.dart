import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';
import 'package:mobile/src/rust/api/plenum_api.dart';
import '../theme.dart';
import '../widgets/animated_radar.dart';

class ReceiveScreen extends StatefulWidget {
  const ReceiveScreen({super.key});

  @override
  State<ReceiveScreen> createState() => _ReceiveScreenState();
}

class _ReceiveScreenState extends State<ReceiveScreen> {
  bool _isListening = false;
  String _statusMessage = 'Tap radar to start receiving';
  String? _pin;
  double? _progress;
  bool _copied = false;

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
          setState(() => _statusMessage = 'Receiving ${transEvent['Started']['file_name']}...');
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

  void _copyPin() {
    if (_pin != null) {
      Clipboard.setData(ClipboardData(text: _pin!));
      setState(() => _copied = true);
      Future.delayed(const Duration(seconds: 2), () {
        if (mounted) setState(() => _copied = false);
      });
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
            GestureDetector(
              onTap: _isListening ? null : _startReceiving,
              child: AnimatedRadar(isListening: _isListening),
            ),
            const SizedBox(height: 40),
            Text(
              _statusMessage, 
              style: const TextStyle(fontSize: 16, color: AppTheme.textSecondary),
              textAlign: TextAlign.center,
            ),
            
            if (_pin != null)
              Container(
                margin: const EdgeInsets.only(top: 24),
                padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 16),
                decoration: BoxDecoration(
                  color: AppTheme.bgCard,
                  borderRadius: BorderRadius.circular(12),
                  border: Border.all(color: AppTheme.accentPrimary, width: 1, style: BorderStyle.solid), // Dashed isn't native, using solid for now
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
