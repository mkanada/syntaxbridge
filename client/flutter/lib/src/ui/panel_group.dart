import 'package:flutter/material.dart';

import 'dockable_panel.dart';
import 'ide_theme.dart';

/// One navigator hosted inside a [TabbedPanelGroup].
///
/// Carries the same affordances as a standalone [DockablePanel] (close, dock)
/// because only the active tab is visible at a time — there is no other place
/// for those controls to live.
class PanelTab {
  const PanelTab({
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
}

/// Groups several navigators on the same side behind a tab strip, showing
/// exactly one at a time.
///
/// Stacking full [DockablePanel]s on one side divides the height equally
/// between them; past two navigators none is usable. This is the
/// alternative: one shared frame, one visible body, switched by tab.
class TabbedPanelGroup extends StatefulWidget {
  const TabbedPanelGroup({super.key, required this.tabs});

  final List<PanelTab> tabs;

  @override
  State<TabbedPanelGroup> createState() => _TabbedPanelGroupState();
}

class _TabbedPanelGroupState extends State<TabbedPanelGroup> {
  int _activeIndex = 0;

  @override
  void didUpdateWidget(TabbedPanelGroup oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (_activeIndex >= widget.tabs.length) {
      _activeIndex = widget.tabs.isEmpty ? 0 : widget.tabs.length - 1;
    }
  }

  @override
  Widget build(BuildContext context) {
    final active = widget.tabs[_activeIndex];

    return LayoutBuilder(
      builder: (context, constraints) {
        final bounded = constraints.maxHeight.isFinite;
        final body = Padding(
          padding: const EdgeInsets.all(14),
          child: active.child,
        );

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
              _TabStrip(
                tabs: widget.tabs,
                activeIndex: _activeIndex,
                onSelect: (index) => setState(() => _activeIndex = index),
                onClose: active.onClose,
                onDockSide: active.onDockSide,
              ),
              const Divider(height: 1, color: IdePalette.border),
              if (bounded)
                Expanded(child: body)
              else
                SizedBox(
                  height: DockablePanel.unboundedBodyHeight,
                  child: body,
                ),
            ],
          ),
        );
      },
    );
  }
}

class _TabStrip extends StatelessWidget {
  const _TabStrip({
    required this.tabs,
    required this.activeIndex,
    required this.onSelect,
    required this.onClose,
    required this.onDockSide,
  });

  final List<PanelTab> tabs;
  final int activeIndex;
  final ValueChanged<int> onSelect;
  final VoidCallback onClose;
  final ValueChanged<DockSide> onDockSide;

  @override
  Widget build(BuildContext context) {
    final active = tabs[activeIndex];

    return Padding(
      padding: const EdgeInsetsDirectional.only(start: 4, end: 4),
      child: SizedBox(
        height: 44,
        child: Row(
          children: [
            Expanded(
              child: ListView.builder(
                scrollDirection: Axis.horizontal,
                itemCount: tabs.length,
                itemBuilder: (context, index) {
                  final tab = tabs[index];
                  return _TabButton(
                    title: tab.title,
                    icon: tab.icon,
                    active: index == activeIndex,
                    onTap: () => onSelect(index),
                  );
                },
              ),
            ),
            PopupMenuButton<DockSide>(
              tooltip: 'Dock ${active.title} panel',
              initialValue: active.side,
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
              tooltip: 'Close ${active.title} panel',
              onPressed: onClose,
              icon: const Icon(Icons.close, color: IdePalette.muted),
            ),
          ],
        ),
      ),
    );
  }
}

class _TabButton extends StatelessWidget {
  const _TabButton({
    required this.title,
    required this.icon,
    required this.active,
    required this.onTap,
  });

  final String title;
  final IconData icon;
  final bool active;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return InkWell(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 10),
        decoration: BoxDecoration(
          border: Border(
            bottom: BorderSide(
              color: active ? IdePalette.teal : Colors.transparent,
              width: 2,
            ),
          ),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(
              icon,
              size: 16,
              color: active ? IdePalette.text : IdePalette.muted,
            ),
            const SizedBox(width: 6),
            Text(
              title,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(
                color: active ? IdePalette.text : IdePalette.muted,
                fontSize: 13,
                fontWeight: active ? FontWeight.w700 : FontWeight.w500,
              ),
            ),
          ],
        ),
      ),
    );
  }
}
