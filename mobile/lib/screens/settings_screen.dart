import 'package:flutter/material.dart';
import '../theme.dart';

class SettingsScreen extends StatefulWidget {
  const SettingsScreen({super.key});

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  bool _requirePin = true;
  bool _backgroundTransfer = false;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Settings')),
      body: ListView(
        padding: const EdgeInsets.all(24),
        children: [
          const Text('Security', style: TextStyle(fontWeight: FontWeight.w600, color: AppTheme.textPrimary, fontSize: 14)),
          const SizedBox(height: 16),
          Container(
            decoration: BoxDecoration(
              color: AppTheme.bgCard,
              borderRadius: BorderRadius.circular(12),
              border: Border.all(color: AppTheme.borderColor),
            ),
            child: Column(
              children: [
                SwitchListTile(
                  title: const Text('Require PIN for transfers', style: TextStyle(color: AppTheme.textPrimary)),
                  subtitle: const Text('Ask for a PIN when devices try to send you files', style: TextStyle(color: AppTheme.textSecondary, fontSize: 12)),
                  value: _requirePin,
                  activeColor: AppTheme.accentPrimary,
                  onChanged: (bool value) {
                    setState(() => _requirePin = value);
                  },
                ),
                const Divider(color: AppTheme.borderColor, height: 1),
                SwitchListTile(
                  title: const Text('Allow background transfers', style: TextStyle(color: AppTheme.textPrimary)),
                  subtitle: const Text('Continue sending/receiving when the app is in the background', style: TextStyle(color: AppTheme.textSecondary, fontSize: 12)),
                  value: _backgroundTransfer,
                  activeColor: AppTheme.accentPrimary,
                  onChanged: (bool value) {
                    setState(() => _backgroundTransfer = value);
                  },
                ),
              ],
            ),
          ),
          const SizedBox(height: 32),
          const Text('About', style: TextStyle(fontWeight: FontWeight.w600, color: AppTheme.textPrimary, fontSize: 14)),
          const SizedBox(height: 16),
          Container(
            decoration: BoxDecoration(
              color: AppTheme.bgCard,
              borderRadius: BorderRadius.circular(12),
              border: Border.all(color: AppTheme.borderColor),
            ),
            child: const ListTile(
              title: Text('Plenum Mobile', style: TextStyle(color: AppTheme.textPrimary)),
              subtitle: Text('Version 0.1.0', style: TextStyle(color: AppTheme.textSecondary, fontSize: 12)),
            ),
          ),
        ],
      ),
    );
  }
}
