import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:mobile/src/rust/frb_generated.dart';

import 'screens/send_screen.dart';
import 'screens/receive_screen.dart';

import 'theme.dart';

import 'package:provider/provider.dart';
import 'services/settings_service.dart';
import 'services/share_intent_handler.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();

  final settingsService = await SettingsService.init();

  runApp(
    ChangeNotifierProvider.value(
      value: settingsService,
      child: const PlenumApp(),
    ),
  );
}

class PlenumApp extends StatelessWidget {
  const PlenumApp({super.key});

  @override
  Widget build(BuildContext context) {
    return Consumer<SettingsService>(
      builder: (context, settings, _) {
        return MaterialApp(
          title: 'Plenum',
          theme: AppTheme.lightTheme,
          darkTheme: AppTheme.darkTheme,
          themeMode: settings.themeMode,
          home: const MainScreen(),
        );
      },
    );
  }
}

class MainScreen extends StatefulWidget {
  const MainScreen({super.key});

  @override
  State<MainScreen> createState() => _MainScreenState();
}

class _MainScreenState extends State<MainScreen> {
  int _selectedIndex = 0;
  DateTime? _lastBackPressTime;
  StreamSubscription<List<SharedFile>>? _shareIntentSub;

  static const List<Widget> _screens = <Widget>[
    SendScreen(),
    ReceiveScreen(),
  ];

  @override
  void initState() {
    super.initState();
    _shareIntentSub = ShareIntentHandler.getSharingStream().listen((files) {
      if (files.isNotEmpty && mounted) {
        setState(() {
          _selectedIndex = 0;
        });
      }
    });
    ShareIntentHandler.getInitialSharing().then((files) {
      if (files.isNotEmpty && mounted) {
        setState(() {
          _selectedIndex = 0;
        });
      }
    });
  }

  @override
  void dispose() {
    _shareIntentSub?.cancel();
    super.dispose();
  }

  void _onItemTapped(int index) {
    setState(() {
      _selectedIndex = index;
    });
  }

  @override
  Widget build(BuildContext context) {
    return PopScope(
      canPop: false,
      onPopInvokedWithResult: (didPop, result) {
        if (didPop) return;

        // If on the Receive tab, navigate to the primary Send tab first
        if (_selectedIndex != 0) {
          setState(() {
            _selectedIndex = 0;
          });
          return;
        }

        final now = DateTime.now();
        if (_lastBackPressTime == null ||
            now.difference(_lastBackPressTime!) > const Duration(seconds: 2)) {
          _lastBackPressTime = now;
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(
              content: const Text('Press back again to exit'),
              duration: const Duration(seconds: 2),
              behavior: SnackBarBehavior.floating,
              shape: RoundedRectangleBorder(
                borderRadius: BorderRadius.circular(10),
              ),
            ),
          );
        } else {
          SystemNavigator.pop();
        }
      },
      child: Scaffold(
        body: IndexedStack(
          index: _selectedIndex,
          children: _screens,
        ),
        bottomNavigationBar: NavigationBar(
          selectedIndex: _selectedIndex,
          onDestinationSelected: _onItemTapped,
          destinations: const <NavigationDestination>[
            NavigationDestination(
              icon: Icon(Icons.send_outlined),
              selectedIcon: Icon(Icons.send),
              label: 'Send',
            ),
            NavigationDestination(
              icon: Icon(Icons.download_outlined),
              selectedIcon: Icon(Icons.download),
              label: 'Receive',
            ),
          ],
        ),
      ),
    );
  }
}
