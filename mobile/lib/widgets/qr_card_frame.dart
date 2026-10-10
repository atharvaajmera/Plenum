import 'package:flutter/material.dart';
import '../theme.dart';

/// Reusable QR card frame with stylized glowing viewfinder corner brackets
/// matching the desktop Receive page QR design format.
class QrCardFrame extends StatelessWidget {
  final Widget child;
  final double cardRadius;
  final double cornerLength;
  final double borderWidth;
  final double cornerOffset;
  final EdgeInsetsGeometry padding;
  final Color cornerColor;
  final Color backgroundColor;
  final bool showCorners;

  const QrCardFrame({
    super.key,
    required this.child,
    this.cardRadius = 18,
    this.cornerLength = 24,
    this.borderWidth = 3.5,
    this.cornerOffset = 4,
    this.padding = const EdgeInsets.all(10),
    this.cornerColor = AppTheme.accentPrimary,
    this.backgroundColor = Colors.white,
    this.showCorners = true,
  });

  @override
  Widget build(BuildContext context) {
    final card = Container(
      padding: padding,
      decoration: BoxDecoration(
        color: backgroundColor,
        borderRadius: BorderRadius.circular(cardRadius),
        boxShadow: [
          BoxShadow(
            color: Colors.black.withValues(alpha: showCorners ? 0.36 : 0.22),
            blurRadius: showCorners ? 24 : 14,
            offset: Offset(0, showCorners ? 8 : 4),
          ),
          if (showCorners)
            BoxShadow(
              color: cornerColor.withValues(alpha: 0.16),
              blurRadius: 18,
              spreadRadius: 1,
            ),
        ],
      ),
      child: child,
    );

    if (!showCorners) {
      return card;
    }

    return CustomPaint(
      foregroundPainter: _QrCardCornersPainter(
        cornerColor: cornerColor,
        borderWidth: borderWidth,
        cornerLength: cornerLength,
        cardRadius: cardRadius,
        cornerOffset: cornerOffset,
      ),
      child: card,
    );
  }
}

class _QrCardCornersPainter extends CustomPainter {
  final Color cornerColor;
  final double borderWidth;
  final double cornerLength;
  final double cardRadius;
  final double cornerOffset;

  _QrCardCornersPainter({
    required this.cornerColor,
    required this.borderWidth,
    required this.cornerLength,
    required this.cardRadius,
    required this.cornerOffset,
  });

  @override
  void paint(Canvas canvas, Size size) {
    final l = cornerLength;
    final r = cardRadius.clamp(2.0, l - 3.0);

    final glowPaint = Paint()
      ..color = cornerColor.withValues(alpha: 0.45)
      ..strokeWidth = borderWidth + 1.2
      ..style = PaintingStyle.stroke
      ..strokeCap = StrokeCap.butt
      ..strokeJoin = StrokeJoin.round
      ..maskFilter = const MaskFilter.blur(BlurStyle.normal, 2.5);

    final cornerPaint = Paint()
      ..color = cornerColor
      ..strokeWidth = borderWidth
      ..style = PaintingStyle.stroke
      ..strokeCap = StrokeCap.butt
      ..strokeJoin = StrokeJoin.round;

    final left = -cornerOffset;
    final top = -cornerOffset;
    final right = size.width + cornerOffset;
    final bottom = size.height + cornerOffset;

    // Top-Left: from left arm upward, curving clockwise to top arm
    final pathTL = Path()
      ..moveTo(left, top + l)
      ..lineTo(left, top + r)
      ..arcToPoint(Offset(left + r, top), radius: Radius.circular(r), clockwise: true)
      ..lineTo(left + l, top);

    // Top-Right: from top arm rightward, curving clockwise to right arm
    final pathTR = Path()
      ..moveTo(right - l, top)
      ..lineTo(right - r, top)
      ..arcToPoint(Offset(right, top + r), radius: Radius.circular(r), clockwise: true)
      ..lineTo(right, top + l);

    // Bottom-Right: from right arm downward, curving clockwise to bottom arm
    final pathBR = Path()
      ..moveTo(right, bottom - l)
      ..lineTo(right, bottom - r)
      ..arcToPoint(Offset(right - r, bottom), radius: Radius.circular(r), clockwise: true)
      ..lineTo(right - l, bottom);

    // Bottom-Left: from bottom arm leftward, curving clockwise to left arm
    final pathBL = Path()
      ..moveTo(left + l, bottom)
      ..lineTo(left + r, bottom)
      ..arcToPoint(Offset(left, bottom - r), radius: Radius.circular(r), clockwise: true)
      ..lineTo(left, bottom - l);

    for (final path in [pathTL, pathTR, pathBR, pathBL]) {
      canvas.drawPath(path, glowPaint);
      canvas.drawPath(path, cornerPaint);
    }
  }

  @override
  bool shouldRepaint(covariant _QrCardCornersPainter oldDelegate) {
    return oldDelegate.cornerColor != cornerColor ||
        oldDelegate.borderWidth != borderWidth ||
        oldDelegate.cornerLength != cornerLength ||
        oldDelegate.cardRadius != cardRadius ||
        oldDelegate.cornerOffset != cornerOffset;
  }
}
