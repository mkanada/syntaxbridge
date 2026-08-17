import 'package:flutter/material.dart';

import 'ide_theme.dart';

/// The per-row "mark as external" control shared by [TypesView],
/// [FunctionsView], and [SourceFilesView] — `docs/plans/lista-de-externos.md`'s
/// three manual-marking entry points. No shared row/list widget exists yet
/// in this codebase (`docs/plans/ui-lists.md`'s proposed `CatalogList<T>`
/// isn't built), so this is deliberately the smallest possible shared piece:
/// just the toggle icon, not a whole row template.
class ExternalToggleButton extends StatelessWidget {
  const ExternalToggleButton({
    super.key,
    required this.isExternal,
    required this.onPressed,
  });

  final bool isExternal;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    return IconButton(
      iconSize: 16,
      visualDensity: VisualDensity.compact,
      padding: EdgeInsets.zero,
      constraints: const BoxConstraints(minWidth: 28, minHeight: 28),
      tooltip: isExternal
          ? 'Remover marcação de externo'
          : 'Marcar como externo',
      // `violet`: one of the palette colors `docs/plans/ui-lists.md`'s
      // `KindBadge` section calls "declaradas e mortas" — no widget used it
      // before this one.
      color: isExternal ? IdePalette.violet : IdePalette.muted,
      icon: Icon(isExternal ? Icons.link_off : Icons.link),
      onPressed: onPressed,
    );
  }
}
