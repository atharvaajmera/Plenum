import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:package_info_plus/package_info_plus.dart';
import '../theme.dart';
import '../services/settings_service.dart';
import 'transfer_history_screen.dart';

class SettingsScreen extends StatefulWidget {
  const SettingsScreen({super.key});

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  String _appVersion = 'Loading...';

  @override
  void initState() {
    super.initState();
    _loadVersion();
  }

  Future<void> _loadVersion() async {
    final info = await PackageInfo.fromPlatform();
    setState(() {
      _appVersion = '${info.version}+${info.buildNumber}';
    });
  }

  void _editDeviceName(SettingsService settings) {
    final controller = TextEditingController(text: settings.deviceName);
    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        backgroundColor: AppTheme.bgCard,
        title: const Text('Device Name', style: TextStyle(color: AppTheme.textPrimary)),
        content: TextField(
          controller: controller,
          style: const TextStyle(color: AppTheme.textPrimary),
          decoration: const InputDecoration(
            enabledBorder: UnderlineInputBorder(borderSide: BorderSide(color: AppTheme.borderColor)),
            focusedBorder: UnderlineInputBorder(borderSide: BorderSide(color: AppTheme.accentPrimary)),
          ),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context), child: const Text('Cancel', style: TextStyle(color: AppTheme.textSecondary))),
          TextButton(
            onPressed: () {
              settings.setDeviceName(controller.text.trim());
              Navigator.pop(context);
            },
            child: const Text('Save', style: TextStyle(color: AppTheme.accentPrimary)),
          ),
        ],
      ),
    );
  }

  void _editRelayConfig(SettingsService settings, bool isUrl) {
    final controller = TextEditingController(text: isUrl ? settings.relayServerUrl : settings.iceServers);
    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        backgroundColor: AppTheme.bgCard,
        title: Text(isUrl ? 'Relay Server URL' : 'ICE Servers (STUN/TURN)', style: const TextStyle(color: AppTheme.textPrimary)),
        content: TextField(
          controller: controller,
          maxLines: isUrl ? 1 : 4,
          style: const TextStyle(color: AppTheme.textPrimary),
          decoration: const InputDecoration(
            enabledBorder: UnderlineInputBorder(borderSide: BorderSide(color: AppTheme.borderColor)),
            focusedBorder: UnderlineInputBorder(borderSide: BorderSide(color: AppTheme.accentPrimary)),
          ),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context), child: const Text('Cancel', style: TextStyle(color: AppTheme.textSecondary))),
          TextButton(
            onPressed: () {
              if (isUrl) {
                settings.setRelayServerUrl(controller.text.trim());
              } else {
                settings.setIceServers(controller.text.trim());
              }
              Navigator.pop(context);
            },
            child: const Text('Save', style: TextStyle(color: AppTheme.accentPrimary)),
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final settings = context.watch<SettingsService>();

    return Scaffold(
      appBar: AppBar(
        title: const Text('Settings', style: TextStyle(fontWeight: FontWeight.w900, color: AppTheme.accentPrimary, letterSpacing: -0.5)),
      ),
      body: ListView(
        padding: const EdgeInsets.all(24),
        children: [
          const Text('General', style: TextStyle(fontWeight: FontWeight.w600, color: AppTheme.textPrimary, fontSize: 14)),
          const SizedBox(height: 16),
          _buildCard([
            ListTile(
              title: const Text('Device Name', style: TextStyle(color: AppTheme.textPrimary)),
              subtitle: Text(settings.deviceName, style: const TextStyle(color: AppTheme.textSecondary, fontSize: 12)),
              trailing: const Icon(Icons.edit, color: AppTheme.textSecondary, size: 20),
              onTap: () => _editDeviceName(settings),
            ),
            _divider(),
            SwitchListTile(
              title: const Text('Require PIN for incoming', style: TextStyle(color: AppTheme.textPrimary)),
              subtitle: const Text('Senders must enter your code', style: TextStyle(color: AppTheme.textSecondary, fontSize: 12)),
              value: settings.requirePin,
              activeColor: AppTheme.accentPrimary,
              onChanged: settings.setRequirePin,
            ),
            _divider(),
            SwitchListTile(
              title: const Text('Auto-accept files', style: TextStyle(color: AppTheme.textPrimary)),
              subtitle: const Text('Automatically receive incoming files', style: TextStyle(color: AppTheme.textSecondary, fontSize: 12)),
              value: settings.autoAccept,
              activeColor: AppTheme.accentPrimary,
              onChanged: settings.setAutoAccept,
            ),
            _divider(),
            const ListTile(
              title: Text('Save Location', style: TextStyle(color: AppTheme.textPrimary)),
              subtitle: Text('Saved to Downloads after transfer', style: TextStyle(color: AppTheme.textSecondary, fontSize: 12)),
              trailing: Icon(Icons.folder, color: AppTheme.textSecondary, size: 20),
            ),
          ]),
          
          const SizedBox(height: 24),
          const Text('Preferences', style: TextStyle(fontWeight: FontWeight.w600, color: AppTheme.textPrimary, fontSize: 14)),
          const SizedBox(height: 16),
          _buildCard([
            ListTile(
              title: const Text('Theme', style: TextStyle(color: AppTheme.textPrimary)),
              trailing: DropdownButton<ThemeMode>(
                value: settings.themeMode,
                dropdownColor: AppTheme.bgCardHover,
                underline: const SizedBox(),
                style: const TextStyle(color: AppTheme.textPrimary),
                items: const [
                  DropdownMenuItem(value: ThemeMode.system, child: Text('System')),
                  DropdownMenuItem(value: ThemeMode.light, child: Text('Light')),
                  DropdownMenuItem(value: ThemeMode.dark, child: Text('Dark')),
                ],
                onChanged: (mode) {
                  if (mode != null) settings.setThemeMode(mode);
                },
              ),
            ),
            _divider(),
            ListTile(
              title: const Text('Default Mode', style: TextStyle(color: AppTheme.textPrimary)),
              trailing: DropdownButton<int>(
                value: settings.defaultTransferMode,
                dropdownColor: AppTheme.bgCardHover,
                underline: const SizedBox(),
                style: const TextStyle(color: AppTheme.textPrimary),
                items: const [
                  DropdownMenuItem(value: 0, child: Text('Local Network')),
                  DropdownMenuItem(value: 1, child: Text('Internet')),
                ],
                onChanged: (val) {
                  if (val != null) settings.setDefaultTransferMode(val);
                },
              ),
            ),
          ]),

          const SizedBox(height: 24),
          const Text('History', style: TextStyle(fontWeight: FontWeight.w600, color: AppTheme.textPrimary, fontSize: 14)),
          const SizedBox(height: 16),
          _buildCard([
            ListTile(
              title: const Text('Transfer History', style: TextStyle(color: AppTheme.textPrimary)),
              trailing: const Icon(Icons.chevron_right, color: AppTheme.textSecondary),
              onTap: () {
                Navigator.push(context, MaterialPageRoute(builder: (context) => const TransferHistoryScreen()));
              },
            ),
          ]),

          const SizedBox(height: 24),
          const Text('Advanced', style: TextStyle(fontWeight: FontWeight.w600, color: AppTheme.textPrimary, fontSize: 14)),
          const SizedBox(height: 16),
          _buildCard([
            ExpansionTile(
              title: const Text('Network Configuration', style: TextStyle(color: AppTheme.textPrimary)),
              collapsedIconColor: AppTheme.textSecondary,
              iconColor: AppTheme.accentPrimary,
              children: [
                ListTile(
                  title: const Text('Relay Server URL', style: TextStyle(color: AppTheme.textPrimary, fontSize: 13)),
                  subtitle: Text(settings.relayServerUrl, style: const TextStyle(color: AppTheme.textSecondary, fontSize: 11)),
                  trailing: const Icon(Icons.edit, color: AppTheme.textSecondary, size: 16),
                  onTap: () => _editRelayConfig(settings, true),
                ),
                ListTile(
                  title: const Text('ICE Servers', style: TextStyle(color: AppTheme.textPrimary, fontSize: 13)),
                  subtitle: Text(settings.iceServers.replaceAll('\n', ', '), style: const TextStyle(color: AppTheme.textSecondary, fontSize: 11), maxLines: 1, overflow: TextOverflow.ellipsis),
                  trailing: const Icon(Icons.edit, color: AppTheme.textSecondary, size: 16),
                  onTap: () => _editRelayConfig(settings, false),
                ),
              ],
            )
          ]),

          const SizedBox(height: 24),
          const Text('About', style: TextStyle(fontWeight: FontWeight.w600, color: AppTheme.textPrimary, fontSize: 14)),
          const SizedBox(height: 16),
          _buildCard([
            ListTile(
              title: const Text('Plenum Mobile', style: TextStyle(color: AppTheme.textPrimary)),
              subtitle: Text('Version $_appVersion', style: const TextStyle(color: AppTheme.textSecondary, fontSize: 12)),
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
      child: Column(
        children: children,
      ),
    );
  }

  Widget _divider() {
    return const Divider(height: 1, thickness: 1, color: AppTheme.borderColor);
  }
}
