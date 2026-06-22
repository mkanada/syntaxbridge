import 'package:flutter/material.dart';

enum DockSide { left, right, top, bottom }

extension DockSideLabel on DockSide {
  String get label {
    return switch (this) {
      DockSide.left => 'left',
      DockSide.right => 'right',
      DockSide.top => 'top',
      DockSide.bottom => 'bottom',
    };
  }

  IconData get icon {
    return switch (this) {
      DockSide.left => Icons.vertical_align_center,
      DockSide.right => Icons.vertical_align_center,
      DockSide.top => Icons.horizontal_rule,
      DockSide.bottom => Icons.horizontal_rule,
    };
  }
}

class DockablePanel extends StatelessWidget {
  const DockablePanel({
    super.key,
    required this.title,
    required this.icon,
    required this.side,
    required this.onClose,
    required this.onDockSide,
    required this.child,
  });

  final String title;
  final IconData icon;
  final DockSide side;
  final VoidCallback onClose;
  final ValueChanged<DockSide> onDockSide;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    return Material(
      color: const Color(0xFFFFFFFF),
      clipBehavior: Clip.antiAlias,
      shape: RoundedRectangleBorder(
        side: const BorderSide(color: Color(0xFFD0D7D9)),
        borderRadius: BorderRadius.circular(8),
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _DockablePanelHeader(
            title: title,
            icon: icon,
            side: side,
            onClose: onClose,
            onDockSide: onDockSide,
          ),
          const Divider(height: 1),
          Padding(padding: const EdgeInsets.all(16), child: child),
        ],
      ),
    );
  }
}

class _DockablePanelHeader extends StatelessWidget {
  const _DockablePanelHeader({
    required this.title,
    required this.icon,
    required this.side,
    required this.onClose,
    required this.onDockSide,
  });

  final String title;
  final IconData icon;
  final DockSide side;
  final VoidCallback onClose;
  final ValueChanged<DockSide> onDockSide;

  @override
  Widget build(BuildContext context) {
    final textTheme = Theme.of(context).textTheme;

    return Padding(
      padding: const EdgeInsetsDirectional.only(start: 12, end: 4),
      child: SizedBox(
        height: 44,
        child: Row(
          children: [
            Icon(icon, size: 20),
            const SizedBox(width: 8),
            Expanded(
              child: Text(
                title,
                overflow: TextOverflow.ellipsis,
                style: textTheme.titleMedium,
              ),
            ),
            PopupMenuButton<DockSide>(
              tooltip: 'Dock $title panel',
              initialValue: side,
              icon: const Icon(Icons.dock_outlined),
              onSelected: onDockSide,
              itemBuilder: (context) => [
                for (final dockSide in DockSide.values)
                  PopupMenuItem(
                    value: dockSide,
                    child: Row(
                      children: [
                        Icon(dockSide.icon, size: 18),
                        const SizedBox(width: 8),
                        Text('Dock ${dockSide.label}'),
                      ],
                    ),
                  ),
              ],
            ),
            IconButton(
              tooltip: 'Close $title panel',
              onPressed: onClose,
              icon: const Icon(Icons.close),
            ),
          ],
        ),
      ),
    );
  }
}
