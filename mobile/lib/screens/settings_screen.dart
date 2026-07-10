import 'package:flutter/material.dart';
import '../services/internet_settings.dart';
import '../theme.dart';

class SettingsScreen extends StatefulWidget {
  const SettingsScreen({super.key});

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  bool _requirePin = true;
  bool _backgroundTransfer = false;

  final TextEditingController _relayUrlController = TextEditingController();
  List<IceServerSetting> _iceServers = [];
  bool _loadingInternetSettings = true;
  bool _showSaved = false;

  @override
  void initState() {
    super.initState();
    _loadInternetSettings();
  }

  @override
  void dispose() {
    _relayUrlController.dispose();
    super.dispose();
  }

  Future<void> _loadInternetSettings() async {
    final url = await InternetSettings.loadRelayServerUrl();
    final servers = await InternetSettings.loadIceServers();
    if (!mounted) return;
    setState(() {
      _relayUrlController.text = url;
      _iceServers = servers;
      _loadingInternetSettings = false;
    });
  }

  Future<void> _saveInternetSettings() async {
    await InternetSettings.saveRelayServerUrl(_relayUrlController.text.trim());
    await InternetSettings.saveIceServers(_iceServers);
    if (!mounted) return;
    setState(() => _showSaved = true);
    Future.delayed(const Duration(seconds: 2), () {
      if (mounted) setState(() => _showSaved = false);
    });
  }

  void _addIceServer() {
    setState(() => _iceServers.add(IceServerSetting(urls: '')));
  }

  void _removeIceServer(int index) {
    setState(() => _iceServers.removeAt(index));
  }

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
          const Text('Internet Transfers', style: TextStyle(fontWeight: FontWeight.w600, color: AppTheme.textPrimary, fontSize: 14)),
          const SizedBox(height: 16),
          Container(
            decoration: BoxDecoration(
              color: AppTheme.bgCard,
              borderRadius: BorderRadius.circular(12),
              border: Border.all(color: AppTheme.borderColor),
            ),
            padding: const EdgeInsets.all(16),
            child: _loadingInternetSettings
                ? const Padding(
                    padding: EdgeInsets.symmetric(vertical: 16),
                    child: Center(child: CircularProgressIndicator(color: AppTheme.accentPrimary)),
                  )
                : Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      const Text('Relay Server URL', style: TextStyle(color: AppTheme.textPrimary, fontWeight: FontWeight.w600, fontSize: 13)),
                      const SizedBox(height: 8),
                      TextField(
                        controller: _relayUrlController,
                        decoration: const InputDecoration(
                          hintText: 'wss://your-relay.example.com/ws',
                          hintStyle: TextStyle(color: AppTheme.textSecondary, fontSize: 13),
                          border: OutlineInputBorder(),
                          focusedBorder: OutlineInputBorder(borderSide: BorderSide(color: AppTheme.accentPrimary)),
                        ),
                        style: const TextStyle(color: AppTheme.textPrimary, fontSize: 13),
                      ),
                      const SizedBox(height: 20),
                      const Text('ICE Servers', style: TextStyle(color: AppTheme.textPrimary, fontWeight: FontWeight.w600, fontSize: 13)),
                      const SizedBox(height: 12),
                      for (int i = 0; i < _iceServers.length; i++) ...[
                        _IceServerRow(
                          server: _iceServers[i],
                          onChanged: () => setState(() {}),
                          onRemove: () => _removeIceServer(i),
                        ),
                        const SizedBox(height: 12),
                      ],
                      OutlinedButton.icon(
                        onPressed: _addIceServer,
                        icon: const Icon(Icons.add, size: 16, color: AppTheme.accentPrimary),
                        label: const Text('Add ICE server', style: TextStyle(color: AppTheme.accentPrimary)),
                        style: OutlinedButton.styleFrom(
                          side: const BorderSide(color: AppTheme.accentPrimary),
                        ),
                      ),
                      const SizedBox(height: 12),
                      const Text(
                        'STUN alone works for some NAT types; add a TURN server for symmetric NAT (see relay-server deployment docs).',
                        style: TextStyle(color: AppTheme.textSecondary, fontSize: 12),
                      ),
                      const SizedBox(height: 16),
                      Align(
                        alignment: Alignment.centerRight,
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.end,
                          children: [
                            if (_showSaved)
                              const Padding(
                                padding: EdgeInsets.only(bottom: 8),
                                child: Text('Settings saved!', style: TextStyle(color: AppTheme.accentPrimary, fontWeight: FontWeight.w500)),
                              ),
                            ElevatedButton(
                              onPressed: _saveInternetSettings,
                              child: const Text('Save'),
                            ),
                          ],
                        ),
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

class _IceServerRow extends StatefulWidget {
  final IceServerSetting server;
  final VoidCallback onChanged;
  final VoidCallback onRemove;

  const _IceServerRow({required this.server, required this.onChanged, required this.onRemove});

  @override
  State<_IceServerRow> createState() => _IceServerRowState();
}

class _IceServerRowState extends State<_IceServerRow> {
  late final TextEditingController _urlsController;
  late final TextEditingController _usernameController;
  late final TextEditingController _credentialController;

  @override
  void initState() {
    super.initState();
    _urlsController = TextEditingController(text: widget.server.urls);
    _usernameController = TextEditingController(text: widget.server.username ?? '');
    _credentialController = TextEditingController(text: widget.server.credential ?? '');
  }

  @override
  void dispose() {
    _urlsController.dispose();
    _usernameController.dispose();
    _credentialController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: AppTheme.bgSidebar,
        borderRadius: BorderRadius.circular(8),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          TextField(
            controller: _urlsController,
            onChanged: (v) {
              widget.server.urls = v;
              widget.onChanged();
            },
            decoration: const InputDecoration(
              labelText: 'STUN/TURN URL',
              hintText: 'stun:stun.l.google.com:19302 or turn:host:port',
              isDense: true,
              border: OutlineInputBorder(),
            ),
            style: const TextStyle(color: AppTheme.textPrimary, fontSize: 13),
          ),
          const SizedBox(height: 8),
          Row(
            children: [
              Expanded(
                child: TextField(
                  controller: _usernameController,
                  onChanged: (v) {
                    widget.server.username = v;
                    widget.onChanged();
                  },
                  decoration: const InputDecoration(
                    labelText: 'Username (optional)',
                    isDense: true,
                    border: OutlineInputBorder(),
                  ),
                  style: const TextStyle(color: AppTheme.textPrimary, fontSize: 13),
                ),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: TextField(
                  controller: _credentialController,
                  obscureText: true,
                  onChanged: (v) {
                    widget.server.credential = v;
                    widget.onChanged();
                  },
                  decoration: const InputDecoration(
                    labelText: 'Credential (optional)',
                    isDense: true,
                    border: OutlineInputBorder(),
                  ),
                  style: const TextStyle(color: AppTheme.textPrimary, fontSize: 13),
                ),
              ),
              IconButton(
                onPressed: widget.onRemove,
                icon: const Icon(Icons.delete_outline, color: AppTheme.textSecondary),
              ),
            ],
          ),
        ],
      ),
    );
  }
}
