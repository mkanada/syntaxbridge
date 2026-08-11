import 'package:flutter/material.dart';

import 'dockable_panel.dart';
import 'ide_theme.dart';

/// One navigator hosted inside an [AccordionPanelGroup].
///
/// Carries the same affordances as a standalone [DockablePanel] (close, dock)
/// because, unlike a tab strip, every item's header stays visible at once —
/// there is no separate "active tab" bar to host those controls.
class AccordionItem {
  const AccordionItem({
    required this.id,
    required this.title,
    required this.icon,
    required this.side,
    required this.onClose,
    required this.onDockSide,
    required this.child,
  });

  /// Stable across rebuilds — [PanelDescriptor.id] — so expand/collapse
  /// state survives the list reshuffling as panels open, close, or dock
  /// elsewhere.
  final String id;

  final String title;
  final IconData icon;
  final DockSide side;
  final VoidCallback onClose;
  final ValueChanged<DockSide> onDockSide;
  final Widget child;
}

/// Stacks several navigators on the same side as collapsible sections, all
/// headers visible at once, each opening to show its own content on click.
///
/// Like [TabbedPanelGroup], only one item's content shows at a time — opening
/// one collapses whichever other item was open — but unlike a tab strip,
/// every item's own header stays in view rather than being switched to.
class AccordionPanelGroup extends StatefulWidget {
  const AccordionPanelGroup({super.key, required this.items});

  final List<AccordionItem> items;

  @override
  State<AccordionPanelGroup> createState() => _AccordionPanelGroupState();
}

class _AccordionPanelGroupState extends State<AccordionPanelGroup> {
  String? _expandedId;

  @override
  void initState() {
    super.initState();
    // Mirrors a tab group's initial active tab.
    if (widget.items.isNotEmpty) {
      _expandedId = widget.items.first.id;
    }
  }

  @override
  void didUpdateWidget(AccordionPanelGroup oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!widget.items.any((item) => item.id == _expandedId)) {
      _expandedId = widget.items.isEmpty ? null : widget.items.first.id;
    }
  }

  void _toggle(String id) {
    setState(() {
      _expandedId = _expandedId == id ? null : id;
    });
  }

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final bounded = constraints.maxHeight.isFinite;

        return Material(
          color: IdePalette.sideBar,
          clipBehavior: Clip.antiAlias,
          shape: RoundedRectangleBorder(
            side: const BorderSide(color: IdePalette.border),
            borderRadius: BorderRadius.circular(6),
          ),
          child: Column(
            mainAxisSize: bounded ? MainAxisSize.max : MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              for (final (index, item) in widget.items.indexed) ...[
                if (index > 0)
                  const Divider(height: 1, color: IdePalette.border),
                _AccordionHeader(
                  item: item,
                  expanded: _expandedId == item.id,
                  onToggle: () => _toggle(item.id),
                ),
                if (_expandedId == item.id) ...[
                  const Divider(height: 1, color: IdePalette.border),
                  if (bounded)
                    Expanded(child: _AccordionBody(child: item.child))
                  else
                    SizedBox(
                      height: DockablePanel.unboundedBodyHeight,
                      child: _AccordionBody(child: item.child),
                    ),
                ],
              ],
            ],
          ),
        );
      },
    );
  }
}

class _AccordionBody extends StatelessWidget {
  const _AccordionBody({required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context) {
    return Padding(padding: const EdgeInsets.all(14), child: child);
  }
}

class _AccordionHeader extends StatelessWidget {
  const _AccordionHeader({
    required this.item,
    required this.expanded,
    required this.onToggle,
  });

  final AccordionItem item;
  final bool expanded;
  final VoidCallback onToggle;

  @override
  Widget build(BuildContext context) {
    return Material(
      color: Colors.transparent,
      child: InkWell(
        onTap: onToggle,
        child: Padding(
          padding: const EdgeInsetsDirectional.only(start: 8, end: 4),
          child: SizedBox(
            height: 44,
            child: Row(
              children: [
                Icon(
                  expanded ? Icons.expand_more : Icons.chevron_right,
                  size: 18,
                  color: IdePalette.muted,
                ),
                const SizedBox(width: 4),
                Icon(item.icon, size: 18, color: IdePalette.muted),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    item.title,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(
                      color: IdePalette.text,
                      fontSize: 13,
                      fontWeight: FontWeight.w700,
                    ),
                  ),
                ),
                PopupMenuButton<DockSide>(
                  tooltip: 'Dock ${item.title} panel',
                  initialValue: item.side,
                  color: IdePalette.panel,
                  icon: const Icon(
                    Icons.dock_outlined,
                    color: IdePalette.muted,
                  ),
                  onSelected: item.onDockSide,
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
                  tooltip: 'Close ${item.title} panel',
                  onPressed: item.onClose,
                  icon: const Icon(Icons.close, color: IdePalette.muted),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
