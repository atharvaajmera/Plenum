import 'package:flutter/material.dart';
import 'package:package_info_plus/package_info_plus.dart';
import '../theme.dart';

class AboutScreen extends StatefulWidget {
  const AboutScreen({super.key});

  @override
  State<AboutScreen> createState() => _AboutScreenState();
}

class _AboutScreenState extends State<AboutScreen> {
  String _appVersion = '…';

  @override
  void initState() {
    super.initState();
    _loadVersion();
  }

  Future<void> _loadVersion() async {
    final info = await PackageInfo.fromPlatform();
    if (!mounted) return;
    setState(() {
      _appVersion = info.version;
    });
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('About', style: TextStyle(fontWeight: FontWeight.w900, color: AppTheme.accentPrimary, letterSpacing: -0.5)),
      ),
      body: ListView(
        padding: const EdgeInsets.all(24),
        children: [
          _buildCard([
            const ListTile(
              title: Text('Plenum', style: TextStyle(color: AppTheme.textPrimary, fontWeight: FontWeight.w700, fontSize: 18)),
              subtitle: Text(
                'Peer-to-peer file transfer. No account required.',
                style: TextStyle(color: AppTheme.textSecondary, fontSize: 13),
              ),
            ),
            _divider(),
            ListTile(
              title: const Text('Version', style: TextStyle(color: AppTheme.textPrimary)),
              trailing: Text(_appVersion, style: const TextStyle(color: AppTheme.textSecondary, fontSize: 13)),
            ),
          ]),

          const SizedBox(height: 24),
          const Text('How it works', style: TextStyle(fontWeight: FontWeight.w600, color: AppTheme.textPrimary, fontSize: 14)),
          const SizedBox(height: 16),
          _buildCard([
            const ListTile(
              leading: Icon(Icons.wifi, color: AppTheme.accentPrimary),
              title: Text('Local Network', style: TextStyle(color: AppTheme.textPrimary)),
              subtitle: Text(
                'Send to nearby devices on the same Wi‑Fi. Fastest option.',
                style: TextStyle(color: AppTheme.textSecondary, fontSize: 12),
              ),
            ),
            _divider(),
            const ListTile(
              leading: Icon(Icons.public, color: AppTheme.accentPrimary),
              title: Text('Internet', style: TextStyle(color: AppTheme.textPrimary)),
              subtitle: Text(
                'Share a room code to send across different networks.',
                style: TextStyle(color: AppTheme.textSecondary, fontSize: 12),
              ),
            ),
          ]),

          const SizedBox(height: 24),
          const Text('Internet transfers', style: TextStyle(fontWeight: FontWeight.w600, color: AppTheme.textPrimary, fontSize: 14)),
          const SizedBox(height: 16),
          _buildCard([
            const ListTile(
              leading: Icon(Icons.route, color: AppTheme.accentPrimary),
              title: Text('If a direct link isn’t possible', style: TextStyle(color: AppTheme.textPrimary)),
              subtitle: Text(
                'On a VPN, mobile data, or a strict network, Plenum routes through a secure relay so the transfer still works.',
                style: TextStyle(color: AppTheme.textSecondary, fontSize: 12),
              ),
            ),
            _divider(),
            const ListTile(
              leading: Icon(Icons.lock, color: AppTheme.accentPrimary),
              title: Text('Your files stay private', style: TextStyle(color: AppTheme.textPrimary)),
              subtitle: Text(
                'Transfers are encrypted end to end. The relay can’t open or read your files.',
                style: TextStyle(color: AppTheme.textSecondary, fontSize: 12),
              ),
            ),
            _divider(),
            const ListTile(
              leading: Icon(Icons.speed, color: AppTheme.accentPrimary),
              title: Text('Slower on relay', style: TextStyle(color: AppTheme.textPrimary)),
              subtitle: Text(
                'Relay mode is slower and less ideal for very large files. Use Local Network on the same Wi‑Fi when you can.',
                style: TextStyle(color: AppTheme.textSecondary, fontSize: 12),
              ),
            ),
          ]),

          const SizedBox(height: 24),
          const Text('Files & privacy', style: TextStyle(fontWeight: FontWeight.w600, color: AppTheme.textPrimary, fontSize: 14)),
          const SizedBox(height: 16),
          _buildCard([
            const ListTile(
              leading: Icon(Icons.folder, color: AppTheme.accentPrimary),
              title: Text('Save location', style: TextStyle(color: AppTheme.textPrimary)),
              subtitle: Text(
                'Received files go to Downloads on this device.',
                style: TextStyle(color: AppTheme.textSecondary, fontSize: 12),
              ),
            ),
            _divider(),
            const ListTile(
              leading: Icon(Icons.lock_outline, color: AppTheme.accentPrimary),
              title: Text('Privacy', style: TextStyle(color: AppTheme.textPrimary)),
              subtitle: Text(
                'No account needed. Optional PIN for local receives.',
                style: TextStyle(color: AppTheme.textSecondary, fontSize: 12),
              ),
            ),
          ]),
          const SizedBox(height: 32),
        ],
      ),
    );
  }

  Widget _buildCard(List<Widget> children) {
    return Container(
      decoration: BoxDecoration(
        color: AppTheme.bgCard,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: AppTheme.borderColor),
      ),
      child: Column(children: children),
    );
  }

  Widget _divider() {
    return const Divider(height: 1, thickness: 1, color: AppTheme.borderColor);
  }
}
