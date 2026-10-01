import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:mobile_scanner/mobile_scanner.dart';
import 'package:permission_handler/permission_handler.dart';
import '../theme.dart';
import '../utils/qr_payload.dart';

/// Fullscreen scanner screen for scanning Plenum QR codes using the device camera.
class QrScannerSheet extends StatefulWidget {
  final String title;

  const QrScannerSheet({
    super.key,
    required this.title,
  });

  /// Opens the QR scanner and returns the scanned [QrPayload], or `null` if dismissed.
  static Future<QrPayload?> show(
    BuildContext context, {
    String title = 'Scan QR Code',
  }) {
    return Navigator.of(context).push<QrPayload>(
      MaterialPageRoute(
        fullscreenDialog: true,
        builder: (context) => QrScannerSheet(title: title),
      ),
    );
  }

  @override
  State<QrScannerSheet> createState() => _QrScannerSheetState();
}

class _QrScannerSheetState extends State<QrScannerSheet>
    with WidgetsBindingObserver, SingleTickerProviderStateMixin {
  late final MobileScannerController _controller;
  bool _hasDetected = false;
  bool _torchEnabled = false;
  bool _isCheckingPermission = true;
  PermissionStatus _permissionStatus = PermissionStatus.denied;

  late final AnimationController _animController;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);

    _animController = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 2200),
    )..repeat(reverse: true);

    _controller = MobileScannerController(
      detectionSpeed: DetectionSpeed.normal,
      formats: const [BarcodeFormat.qrCode],
      returnImage: false,
    );

    _checkPermission();
  }

  Future<void> _checkPermission() async {
    final status = await Permission.camera.status;
    if (status.isGranted) {
      if (mounted) {
        setState(() {
          _permissionStatus = status;
          _isCheckingPermission = false;
        });
      }
      return;
    }

    final requested = await Permission.camera.request();
    if (mounted) {
      setState(() {
        _permissionStatus = requested;
        _isCheckingPermission = false;
      });
    }
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (!_controller.value.isInitialized) return;
    if (state == AppLifecycleState.inactive || state == AppLifecycleState.paused) {
      _controller.stop();
    } else if (state == AppLifecycleState.resumed && _permissionStatus.isGranted) {
      _controller.start();
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _animController.dispose();
    _controller.dispose();
    super.dispose();
  }

  void _onDetect(BarcodeCapture capture) {
    if (_hasDetected) return;

    for (final barcode in capture.barcodes) {
      final rawValue = barcode.rawValue ?? barcode.displayValue;
      if (rawValue == null || rawValue.trim().isEmpty) continue;

      debugPrint('[QR Scanner] Detected raw barcode: $rawValue');
      final payload = QrPayload.parse(rawValue);
      if (payload != null) {
        _hasDetected = true;
        HapticFeedback.mediumImpact();
        Navigator.of(context).pop(payload);
        break;
      }
    }
  }

  Future<void> _toggleTorch() async {
    try {
      await _controller.toggleTorch();
      setState(() {
        _torchEnabled = !_torchEnabled;
      });
    } catch (_) {}
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: Colors.black,
      appBar: AppBar(
        backgroundColor: Colors.black,
        elevation: 0,
        leading: IconButton(
          icon: const Icon(Icons.close, color: Colors.white),
          tooltip: 'Close',
          onPressed: () => Navigator.of(context).pop(),
        ),
        title: Text(
          widget.title,
          style: const TextStyle(
            color: Colors.white,
            fontWeight: FontWeight.w600,
            fontSize: 18,
          ),
        ),
        actions: [
          if (_permissionStatus.isGranted)
            IconButton(
              icon: Icon(
                _torchEnabled ? Icons.flash_on : Icons.flash_off,
                color: _torchEnabled ? AppTheme.accentPrimary : Colors.white70,
              ),
              tooltip: 'Toggle Flashlight',
              onPressed: _toggleTorch,
            ),
        ],
      ),
      body: _buildBody(),
    );
  }

  Widget _buildBody() {
    if (_isCheckingPermission) {
      return const Center(
        child: CircularProgressIndicator(color: AppTheme.accentPrimary),
      );
    }

    if (!_permissionStatus.isGranted) {
      return Center(
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 32),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Container(
                padding: const EdgeInsets.all(20),
                decoration: BoxDecoration(
                  color: AppTheme.bgCardOf(context),
                  shape: BoxShape.circle,
                  border: Border.all(color: AppTheme.borderColorOf(context)),
                ),
                child: const Icon(
                  Icons.videocam_off_outlined,
                  size: 48,
                  color: AppTheme.accentPrimary,
                ),
              ),
              const SizedBox(height: 20),
              Text(
                'Camera Access Required',
                style: TextStyle(
                  fontSize: 18,
                  fontWeight: FontWeight.bold,
                  color: AppTheme.textPrimaryOf(context),
                ),
              ),
              const SizedBox(height: 10),
              Text(
                'Plenum needs camera access to scan receiver QR codes and establish zero-friction transfers.',
                textAlign: TextAlign.center,
                style: TextStyle(
                  fontSize: 14,
                  color: AppTheme.textSecondaryOf(context),
                  height: 1.4,
                ),
              ),
              const SizedBox(height: 24),
              ElevatedButton.icon(
                onPressed: () async {
                  await openAppSettings();
                },
                style: ElevatedButton.styleFrom(
                  backgroundColor: AppTheme.accentPrimary,
                  padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 12),
                  shape: RoundedRectangleBorder(
                    borderRadius: BorderRadius.circular(10),
                  ),
                ),
                icon: const Icon(Icons.settings, size: 18),
                label: const Text(
                  'Open Settings',
                  style: TextStyle(fontWeight: FontWeight.w600),
                ),
              ),
            ],
          ),
        ),
      );
    }

    return LayoutBuilder(
      builder: (context, constraints) {
        final boxSize = (constraints.maxWidth * 0.72).clamp(220.0, 300.0);
        final left = (constraints.maxWidth - boxSize) / 2;
        final top = (constraints.maxHeight - boxSize) / 2 - 30;
        final cutoutRect = Rect.fromLTWH(left, top, boxSize, boxSize);

        return Stack(
          fit: StackFit.expand,
          children: [
            // Live Camera Preview
            MobileScanner(
              controller: _controller,
              fit: BoxFit.cover,
              onDetect: _onDetect,
              errorBuilder: (context, error, child) {
                return Center(
                  child: Padding(
                    padding: const EdgeInsets.all(24),
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        const Icon(
                          Icons.error_outline,
                          size: 44,
                          color: Colors.redAccent,
                        ),
                        const SizedBox(height: 12),
                        const Text(
                          'Camera Error',
                          style: TextStyle(
                            fontSize: 16,
                            fontWeight: FontWeight.bold,
                            color: Colors.white,
                          ),
                        ),
                        const SizedBox(height: 6),
                        Text(
                          error.errorDetails?.message ?? error.errorCode.name,
                          textAlign: TextAlign.center,
                          style: const TextStyle(
                            fontSize: 13,
                            color: Colors.white70,
                          ),
                        ),
                      ],
                    ),
                  ),
                );
              },
            ),

            // Clean CustomPainter Cutout (Zero White-Screen Blend Bugs)
            CustomPaint(
              size: Size(constraints.maxWidth, constraints.maxHeight),
              painter: _ScannerOverlayPainter(
                cutoutRect: cutoutRect,
                borderRadius: 16,
                borderColor: AppTheme.accentPrimary,
                cornerLength: 28,
                borderWidth: 3.5,
              ),
            ),

            // Animated Laser Scanning Reticle
            AnimatedBuilder(
              animation: _animController,
              builder: (context, child) {
                final scanLineY = top + (_animController.value * boxSize);
                return Positioned(
                  left: left + 8,
                  top: scanLineY,
                  width: boxSize - 16,
                  child: Container(
                    height: 2.5,
                    decoration: BoxDecoration(
                      gradient: LinearGradient(
                        colors: [
                          AppTheme.accentPrimary.withValues(alpha: 0.0),
                          AppTheme.accentPrimary,
                          AppTheme.accentPrimary.withValues(alpha: 0.0),
                        ],
                      ),
                      boxShadow: [
                        BoxShadow(
                          color: AppTheme.accentPrimary.withValues(alpha: 0.55),
                          blurRadius: 8,
                          spreadRadius: 2,
                        ),
                      ],
                    ),
                  ),
                );
              },
            ),

            // Helpful Instructions
            Positioned(
              bottom: 40,
              left: 24,
              right: 24,
              child: Container(
                padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
                decoration: BoxDecoration(
                  color: AppTheme.bgCardOf(context).withValues(alpha: 0.85),
                  borderRadius: BorderRadius.circular(14),
                  border: Border.all(color: AppTheme.borderColorOf(context)),
                ),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    const Icon(
                      Icons.qr_code_scanner,
                      size: 18,
                      color: AppTheme.accentPrimary,
                    ),
                    const SizedBox(width: 10),
                    Flexible(
                      child: Text(
                        'Align receiver\'s QR code within the frame',
                        textAlign: TextAlign.center,
                        style: TextStyle(
                          fontSize: 13,
                          color: AppTheme.textPrimaryOf(context),
                          fontWeight: FontWeight.w500,
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ],
        );
      },
    );
  }
}

/// CustomPainter that cuts a transparent hole in a dark overlay without using broken blend modes.
class _ScannerOverlayPainter extends CustomPainter {
  static const Color overlayColor = Color(0x99000000);

  final Rect cutoutRect;
  final double borderRadius;
  final Color borderColor;
  final double borderWidth;
  final double cornerLength;

  _ScannerOverlayPainter({
    required this.cutoutRect,
    this.borderRadius = 16,
    required this.borderColor,
    this.borderWidth = 3.5,
    this.cornerLength = 28,
  });

  @override
  void paint(Canvas canvas, Size size) {
    // 1. Dark semi-transparent scrim with transparent cutout hole
    final bgPath = Path()..addRect(Rect.fromLTWH(0, 0, size.width, size.height));
    final holePath = Path()
      ..addRRect(RRect.fromRectAndRadius(cutoutRect, Radius.circular(borderRadius)));
    final overlayPath = Path.combine(PathOperation.difference, bgPath, holePath);
    canvas.drawPath(overlayPath, Paint()..color = overlayColor);

    // 2. Corner brackets around the cutout
    final cornerPaint = Paint()
      ..color = borderColor
      ..strokeWidth = borderWidth
      ..style = PaintingStyle.stroke
      ..strokeCap = StrokeCap.round;

    final left = cutoutRect.left;
    final top = cutoutRect.top;
    final right = cutoutRect.right;
    final bottom = cutoutRect.bottom;
    final r = borderRadius;
    final l = cornerLength;

    // Top-Left
    final pathTL = Path()
      ..moveTo(left, top + l)
      ..lineTo(left, top + r)
      ..arcToPoint(Offset(left + r, top), radius: Radius.circular(r))
      ..lineTo(left + l, top);
    canvas.drawPath(pathTL, cornerPaint);

    // Top-Right
    final pathTR = Path()
      ..moveTo(right - l, top)
      ..lineTo(right - r, top)
      ..arcToPoint(Offset(right, top + r), radius: Radius.circular(r))
      ..lineTo(right, top + l);
    canvas.drawPath(pathTR, cornerPaint);

    // Bottom-Left
    final pathBL = Path()
      ..moveTo(left, bottom - l)
      ..lineTo(left, bottom - r)
      ..arcToPoint(Offset(left + r, bottom), radius: Radius.circular(r))
      ..lineTo(left + l, bottom);
    canvas.drawPath(pathBL, cornerPaint);

    // Bottom-Right
    final pathBR = Path()
      ..moveTo(right - l, bottom)
      ..lineTo(right - r, bottom)
      ..arcToPoint(Offset(right, bottom - r), radius: Radius.circular(r))
      ..lineTo(right, bottom - l);
    canvas.drawPath(pathBR, cornerPaint);
  }

  @override
  bool shouldRepaint(covariant _ScannerOverlayPainter oldDelegate) {
    return oldDelegate.cutoutRect != cutoutRect ||
        oldDelegate.borderColor != borderColor;
  }
}
