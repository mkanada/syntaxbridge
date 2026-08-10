import 'package:flutter/material.dart';

import 'ide_theme.dart';

/// Where a dockable panel can be placed. [center] docks it into the main
/// workspace split, side by side with whatever else lives there (the source
/// viewer, and any other center-docked panel) — see [ServerStatusPage]'s
/// center split.
enum DockSide { left, right, top, bottom, center }

extension DockSideLabel on DockSide {
  String get label {
    return switch (this) {
      DockSide.left => 'left',
      DockSide.right => 'right',
      DockSide.top => 'top',
      DockSide.bottom => 'bottom',
      DockSide.center => 'center',
    };
  }

  IconData get icon {
    return switch (this) {
      DockSide.left => Icons.vertical_align_center,
      DockSide.right => Icons.vertical_align_center,
      DockSide.top => Icons.horizontal_rule,
      DockSide.bottom => Icons.horizontal_rule,
      DockSide.center => Icons.view_column_outlined,
    };
  }
}

/// Moldura de painel do workspace.
///
/// O painel limita a altura do corpo e **nao rola**: quem rola e o filho. Sem
/// isso, uma lista dentro do painel viveria em altura ilimitada e teria de
/// construir todas as linhas em todo frame, o que nao se sustenta a partir do
/// catalogo de tipos (US-3).
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

  /// Altura do corpo quando o pai nao a limita — o caso do modo compacto, em
  /// que os paineis ficam empilhados dentro de uma area rolavel.
  static const double unboundedBodyHeight = 420;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final bounded = constraints.maxHeight.isFinite;
        final body = Padding(padding: const EdgeInsets.all(14), child: child);

        return Material(
          color: IdePalette.sideBar,
          clipBehavior: Clip.antiAlias,
          shape: RoundedRectangleBorder(
            side: const BorderSide(color: IdePalette.border),
            borderRadius: BorderRadius.circular(6),
          ),
          child: Column(
            mainAxisSize: bounded ? MainAxisSize.max : MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              _DockablePanelHeader(
                title: title,
                icon: icon,
                side: side,
                onClose: onClose,
                onDockSide: onDockSide,
              ),
              const Divider(height: 1, color: IdePalette.border),
              if (bounded)
                Expanded(child: body)
              else
                SizedBox(height: unboundedBodyHeight, child: body),
            ],
          ),
        );
      },
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
    return Padding(
      padding: const EdgeInsetsDirectional.only(start: 12, end: 4),
      child: SizedBox(
        height: 44,
        child: Row(
          children: [
            Icon(icon, size: 18, color: IdePalette.muted),
            const SizedBox(width: 8),
            Expanded(
              child: Text(
                title,
                overflow: TextOverflow.ellipsis,
                style: const TextStyle(
                  color: IdePalette.text,
                  fontSize: 13,
                  fontWeight: FontWeight.w700,
                ),
              ),
            ),
            PopupMenuButton<DockSide>(
              tooltip: 'Dock $title panel',
              initialValue: side,
              color: IdePalette.panel,
              icon: const Icon(Icons.dock_outlined, color: IdePalette.muted),
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
              icon: const Icon(Icons.close, color: IdePalette.muted),
            ),
          ],
        ),
      ),
    );
  }
}
