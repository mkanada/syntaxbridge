import 'package:flutter/material.dart';

import 'dockable_panel.dart';

/// Registers one navigator, inspector, or result panel of the workspace.
///
/// Adding a panel to the workspace means adding one [PanelDescriptor] to the
/// page's list — the layout code that places panels by side and groups them
/// into tabs iterates the list generically and never names a specific panel.
class PanelDescriptor {
  const PanelDescriptor({
    required this.id,
    required this.title,
    required this.icon,
    required this.defaultSide,
    required this.builder,
  });

  /// Stable across the app's lifetime; used as the key for open/closed state
  /// and dock side, so it must not change once a panel ships.
  final String id;

  final String title;
  final IconData icon;
  final DockSide defaultSide;
  final WidgetBuilder builder;
}
