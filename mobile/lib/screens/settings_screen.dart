import 'package:flutter/material.dart';

class SettingsScreen extends StatefulWidget {
  const SettingsScreen({super.key});

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  bool requirePin = false;
  bool backgroundTransfer = false;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Settings')),
      body: ListView(
        children: [
          SwitchListTile(
            title: const Text('Require PIN for receiving'),
            subtitle: const Text('Ask sender to enter a PIN to connect'),
            value: requirePin,
            onChanged: (val) => setState(() => requirePin = val),
          ),
          SwitchListTile(
            title: const Text('Allow background transfers'),
            subtitle: const Text('Keep sending/receiving when app is minimized'),
            value: backgroundTransfer,
            onChanged: (val) => setState(() => backgroundTransfer = val),
          ),
        ],
      ),
    );
  }
}
