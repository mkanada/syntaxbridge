import 'package:flutter/material.dart';

import '../project/project_models.dart';
import 'ide_theme.dart';

enum _SortField { name, kind }

/// The type catalog navigator (US-3): every struct, class, union, enum,
/// typedef, type alias, and macro declared in the project, with its kind.
/// Sortable by name or by kind, and each kind can be hidden entirely via its
/// filter chip — handy on a large project where only a few kinds matter at a
/// time.
///
/// Functions, methods and macro-as-callable belong to US-5's own navigator,
/// not here — mixing declarations and callables in one list is exactly what
/// `docs/plans/User Steps.md` flags as a modeling mistake to avoid.
class TypesView extends StatefulWidget {
  const TypesView({
    super.key,
    required this.types,
    required this.onTypeSelected,
    this.selectedType,
  });

  final List<TypeDeclaration> types;
  final ValueChanged<TypeDeclaration> onTypeSelected;
  final TypeDeclaration? selectedType;

  @override
  State<TypesView> createState() => _TypesViewState();
}

class _TypesViewState extends State<TypesView> {
  _SortField _sortField = _SortField.name;
  bool _ascending = true;
  final Set<TypeDeclarationKind> _hiddenKinds = {};

  @override
  Widget build(BuildContext context) {
    final eligible = widget.types
        .where((type) => type.kind.isUserVisible)
        .toList();
    final presentKinds = [
      for (final kind in TypeDeclarationKind.values)
        if (kind.isUserVisible && eligible.any((type) => type.kind == kind))
          kind,
    ];
    final visible = eligible
        .where((type) => !_hiddenKinds.contains(type.kind))
        .toList();

    final namedTypes = visible.where((type) => !type.kind.isMacro).toList();
    final macros = visible.where((type) => type.kind.isMacro).toList();
    _sort(namedTypes);
    _sort(macros);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text('Types', style: Theme.of(context).textTheme.headlineSmall),
        const SizedBox(height: 4),
        Text(
          '${visible.length} declared',
          style: Theme.of(context).textTheme.bodyMedium,
        ),
        const SizedBox(height: 8),
        Row(
          children: [
            Text(
              'Sort by ',
              style: Theme.of(
                context,
              ).textTheme.bodyMedium?.copyWith(color: IdePalette.muted),
            ),
            _sortButton(context, _SortField.name, 'Name'),
            _sortButton(context, _SortField.kind, 'Kind'),
          ],
        ),
        if (presentKinds.isNotEmpty) ...[
          const SizedBox(height: 4),
          Wrap(
            spacing: 8,
            runSpacing: 4,
            children: [
              for (final kind in presentKinds)
                FilterChip(
                  label: Text(kind.label),
                  selected: !_hiddenKinds.contains(kind),
                  onSelected: (selected) => setState(() {
                    if (selected) {
                      _hiddenKinds.remove(kind);
                    } else {
                      _hiddenKinds.add(kind);
                    }
                  }),
                ),
            ],
          ),
        ],
        const SizedBox(height: 16),
        Expanded(
          child: visible.isEmpty
              ? const Padding(
                  padding: EdgeInsets.only(top: 8),
                  child: Text(
                    'No types found',
                    style: TextStyle(color: IdePalette.muted),
                  ),
                )
              : ListView(
                  children: [
                    ..._rows(namedTypes),
                    // Only worth calling out as its own section when there
                    // are named types to set it apart from — otherwise it's
                    // just "the list", exactly like before macros were
                    // classified.
                    if (macros.isNotEmpty && namedTypes.isNotEmpty)
                      Padding(
                        padding: const EdgeInsets.only(bottom: 4),
                        child: Text(
                          'Constants & macros',
                          style: Theme.of(context).textTheme.labelLarge
                              ?.copyWith(color: IdePalette.muted),
                        ),
                      ),
                    ..._rows(macros),
                  ],
                ),
        ),
      ],
    );
  }

  /// A button toggling [field] as the active sort: tapping the inactive
  /// field switches to it (ascending); tapping the already-active field
  /// flips its direction, so both fields stay reachable without a separate
  /// direction control.
  Widget _sortButton(BuildContext context, _SortField field, String label) {
    final bool active = _sortField == field;

    return TextButton.icon(
      onPressed: () => setState(() {
        if (_sortField == field) {
          _ascending = !_ascending;
        } else {
          _sortField = field;
          _ascending = true;
        }
      }),
      icon: Icon(
        _ascending ? Icons.arrow_upward : Icons.arrow_downward,
        size: 14,
        color: active ? IdePalette.text : Colors.transparent,
      ),
      label: Text(
        label,
        style: TextStyle(color: active ? IdePalette.text : IdePalette.muted),
      ),
    );
  }

  void _sort(List<TypeDeclaration> declarations) {
    declarations.sort((a, b) {
      final int comparison = switch (_sortField) {
        _SortField.name => _compareByName(a, b),
        _SortField.kind => _compareByKindThenName(a, b),
      };
      return _ascending ? comparison : -comparison;
    });
  }

  /// One row per declaration, each divided from the next.
  List<Widget> _rows(List<TypeDeclaration> declarations) {
    return [
      for (final type in declarations) ...[
        ListTile(
          contentPadding: EdgeInsets.zero,
          selected: type == widget.selectedType,
          leading: const Icon(Icons.data_object),
          title: Text(_qualifiedName(type)),
          trailing: Text(
            type.kind.label,
            style: const TextStyle(color: IdePalette.muted),
          ),
          onTap: () => widget.onTypeSelected(type),
        ),
        const Divider(height: 1),
      ],
    ];
  }
}

int _compareByName(TypeDeclaration a, TypeDeclaration b) {
  return _qualifiedName(
    a,
  ).toLowerCase().compareTo(_qualifiedName(b).toLowerCase());
}

int _compareByKindThenName(TypeDeclaration a, TypeDeclaration b) {
  final int kindComparison = a.kind.label.compareTo(b.kind.label);
  return kindComparison != 0 ? kindComparison : _compareByName(a, b);
}

/// The type's name, qualified with its enclosing namespace when it has one,
/// so homonym types declared in different namespaces read as distinct rows.
String _qualifiedName(TypeDeclaration type) {
  return type.namespace.isEmpty ? type.name : '${type.namespace}::${type.name}';
}
