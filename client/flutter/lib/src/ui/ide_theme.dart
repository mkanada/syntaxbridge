import 'package:flutter/material.dart';

abstract final class IdePalette {
  static const background = Color(0xFF151618);
  static const topBar = Color(0xFF202124);
  static const sideBar = Color(0xFF24262B);
  static const tabBar = Color(0xFF202227);
  static const editor = Color(0xFF101114);
  static const preview = Color(0xFF1A1C20);
  static const panel = Color(0xFF22242A);
  static const border = Color(0xFF343741);
  static const divider = Color(0xFF181A1F);
  static const dividerHover = Color(0xFF26313A);
  static const selection = Color(0xFF2D3A46);
  static const text = Color(0xFFF1F4F8);
  static const softText = Color(0xFFC8CDD5);
  static const muted = Color(0xFF8E96A3);
  static const lineNumber = Color(0xFF5E6571);
  static const codeText = Color(0xFFD9DEE7);
  static const teal = Color(0xFF35D0BA);
  static const green = Color(0xFF8BD46E);
  static const amber = Color(0xFFE8B95E);
  static const blue = Color(0xFF6FA8FF);
  static const violet = Color(0xFFB58CFF);
  static const red = Color(0xFFFF7A7A);
}

/// Toolbar button for the IDE title bar.
///
/// [onPressed] is required and passed straight through: a button that does
/// nothing must be spelled `onPressed: null` so it renders disabled, instead
/// of looking active while silently doing nothing.
class IdeToolbarIcon extends StatelessWidget {
  const IdeToolbarIcon({
    super.key,
    required this.icon,
    required this.tooltip,
    required this.onPressed,
    this.color,
  });

  final IconData icon;
  final String tooltip;
  final VoidCallback? onPressed;
  final Color? color;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: tooltip,
      child: IconButton(
        visualDensity: VisualDensity.compact,
        iconSize: 19,
        color: color ?? IdePalette.softText,
        onPressed: onPressed,
        icon: Icon(icon),
      ),
    );
  }
}
