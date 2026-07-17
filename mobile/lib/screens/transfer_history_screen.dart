import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:open_filex/open_filex.dart';
import '../theme.dart';
import '../services/settings_service.dart';

import '../utils/formatters.dart';

class TransferHistoryScreen extends StatelessWidget {
  const TransferHistoryScreen({super.key});

  @override
  Widget build(BuildContext context) {
    final settings = context.watch<SettingsService>();
    final history = settings.transferHistory;

    return Scaffold(
      appBar: AppBar(
        title: const Text('Transfer History', style: TextStyle(color: AppTheme.textPrimary, fontSize: 18)),
      ),
      body: history.isEmpty
          ? const Center(
              child: Text('No past transfers', style: TextStyle(color: AppTheme.textSecondary)),
            )
          : ListView.builder(
              itemCount: history.length,
              itemBuilder: (context, index) {
                final item = history[index];
                final isSend = item['direction'] == 'send';
                final fileName = item['fileName'] ?? 'Unknown file';
                final size = item['size'] ?? 0;
                final peer = item['peerName'] ?? 'Unknown device';
                final path = item['path'];
                final timestamp = item['timestamp'] != null ? DateTime.parse(item['timestamp']) : DateTime.now();
                final timeStr = timestamp.toString().substring(0, 16); // yyyy-MM-dd HH:mm

                return ListTile(
                  leading: CircleAvatar(
                    backgroundColor: AppTheme.bgCardHover,
                    child: Icon(
                      isSend ? Icons.upload : Icons.download,
                      color: isSend ? AppTheme.accentPrimary : Colors.blueAccent,
                      size: 18,
                    ),
                  ),
                  title: Text(fileName, style: const TextStyle(color: AppTheme.textPrimary)),
                  subtitle: Text(
                    '${isSend ? 'To' : 'From'} $peer • ${formatBytes(size)}\n$timeStr',
                    style: const TextStyle(color: AppTheme.textSecondary, fontSize: 12),
                  ),
                  isThreeLine: true,
                  onTap: (!isSend && path != null) ? () => OpenFilex.open(path) : null,
                );
              },
            ),
    );
  }
}
