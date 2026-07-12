import 'package:flutter/material.dart';
import '../theme.dart';

class SettingsScreen extends StatefulWidget {
  const SettingsScreen({super.key});

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Settings')),
      body: ListView(
        padding: const EdgeInsets.all(24),
        children: [
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
