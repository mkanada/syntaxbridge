import 'package:flutter/material.dart';

import '../project/project_models.dart';
import 'ide_theme.dart';

enum _SortField { name, kind, shape }

/// The pointer catalog navigator (Parte 1 of
/// `docs/plans/catalogo-de-ponteiros-e-solver-tfa.md`): every raw `T*`
/// pointer declared in the project — parameter, field, local variable,
/// function return type — with what it points at and its shape (`T*`,
/// `T**`, or a function pointer). Sortable by name, site or shape, and each
/// site kind can be hidden via its filter chip, mirroring [TypesView].
///
/// This exists to let a user inspect the fact base the mapping solver's
/// pointer rules are meant to grow into consulting — it doesn't yet drive
/// `mapping::pointer_options_for` itself (see the plan doc, Parte 2).
class PointersView extends StatefulWidget {
  const PointersView({super.key, required this.pointers});

  final List<PointerDeclaration> pointers;

  @override
  State<PointersView> createState() => _PointersViewState();
}

class _PointersViewState extends State<PointersView> {
  _SortField _sortField = _SortField.name;
  bool _ascending = true;
  final Set<PointerDeclarationKind> _hiddenKinds = {};

  /// Filtered and sorted once per actual change, not on every `build()`.
  /// A project's pointer catalog can hold tens of thousands of entries
  /// (Verovio has ~16k), and re-filtering/re-sorting that on every rebuild —
  /// including ones unrelated to this widget, like a host panel being
  /// resized — is what crashed the app while this tab was open.
  late List<PointerDeclaration> _visible;

  @override
  void initState() {
    super.initState();
    _visible = _computeVisible();
  }

  @override
  void didUpdateWidget(covariant PointersView oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.pointers != widget.pointers) {
      _visible = _computeVisible();
    }
  }

  List<PointerDeclaration> _computeVisible() {
    final visible = widget.pointers
        .where((pointer) => !_hiddenKinds.contains(pointer.kind))
        .toList();
    _sort(visible);
    return visible;
  }

  void _recompute() {
    setState(() {
      _visible = _computeVisible();
    });
  }

  @override
  Widget build(BuildContext context) {
    final presentKinds = [
      for (final kind in PointerDeclarationKind.values)
        if (widget.pointers.any((pointer) => pointer.kind == kind)) kind,
    ];
    final visible = _visible;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text('Pointers', style: Theme.of(context).textTheme.headlineSmall),
        const SizedBox(height: 4),
        Text(
          '${visible.length} declared',
          style: Theme.of(context).textTheme.bodyMedium,
        ),
        const SizedBox(height: 8),
        Wrap(
          crossAxisAlignment: WrapCrossAlignment.center,
          children: [
            Text(
              'Sort by ',
              style: Theme.of(
                context,
              ).textTheme.bodyMedium?.copyWith(color: IdePalette.muted),
            ),
            _sortButton(context, _SortField.name, 'Name'),
            _sortButton(context, _SortField.kind, 'Site'),
            _sortButton(context, _SortField.shape, 'Shape'),
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
                  onSelected: (selected) {
                    if (selected) {
                      _hiddenKinds.remove(kind);
                    } else {
                      _hiddenKinds.add(kind);
                    }
                    _recompute();
                  },
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
                    'No pointers found',
                    style: TextStyle(color: IdePalette.muted),
                  ),
                )
              : ListView.separated(
                  itemCount: visible.length,
                  separatorBuilder: (context, index) =>
                      const Divider(height: 1),
                  itemBuilder: (context, index) => _row(visible[index]),
                ),
        ),
      ],
    );
  }

  /// A button toggling [field] as the active sort — mirrors
  /// `TypesView._sortButton`.
  Widget _sortButton(BuildContext context, _SortField field, String label) {
    final bool active = _sortField == field;

    return TextButton.icon(
      onPressed: () {
        if (_sortField == field) {
          _ascending = !_ascending;
        } else {
          _sortField = field;
          _ascending = true;
        }
        _recompute();
      },
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

  void _sort(List<PointerDeclaration> declarations) {
    declarations.sort((a, b) {
      final int comparison = switch (_sortField) {
        _SortField.name => _compareByName(a, b),
        _SortField.kind => _compareByKindThenName(a, b),
        _SortField.shape => _compareByShapeThenName(a, b),
      };
      return _ascending ? comparison : -comparison;
    });
  }

  Widget _row(PointerDeclaration pointer) {
    return ListTile(
      contentPadding: EdgeInsets.zero,
      leading: const Icon(Icons.arrow_right_alt),
      title: Text(
        pointer.name.isEmpty ? '(unnamed)' : pointer.name,
        overflow: TextOverflow.ellipsis,
      ),
      subtitle: Text(
        '${pointer.pointeeTypeName}${_shapeSuffix(pointer.shape)}',
        overflow: TextOverflow.ellipsis,
      ),
      trailing: SizedBox(
        width: 120,
        child: Wrap(
          alignment: WrapAlignment.end,
          crossAxisAlignment: WrapCrossAlignment.center,
          spacing: 8,
          children: [
            Text(
              pointer.shape.label,
              style: const TextStyle(color: IdePalette.muted, fontSize: 12),
            ),
            Text(
              pointer.kind.label,
              style: const TextStyle(color: IdePalette.muted, fontSize: 12),
            ),
          ],
        ),
      ),
    );
  }
}

/// `*`/`**` after the pointee's own spelling, matching C++ declaration order
/// (`Forma*`, `Forma**`) — a function pointer's spelling already reads as a
/// function type on its own, so it gets no extra suffix.
String _shapeSuffix(PointerShape shape) {
  return switch (shape) {
    PointerShape.scalar => '*',
    PointerShape.doublePointer => '**',
    PointerShape.functionPointer => '',
  };
}

int _compareByName(PointerDeclaration a, PointerDeclaration b) {
  return a.name.toLowerCase().compareTo(b.name.toLowerCase());
}

int _compareByKindThenName(PointerDeclaration a, PointerDeclaration b) {
  final int kindComparison = a.kind.label.compareTo(b.kind.label);
  return kindComparison != 0 ? kindComparison : _compareByName(a, b);
}

int _compareByShapeThenName(PointerDeclaration a, PointerDeclaration b) {
  final int shapeComparison = a.shape.label.compareTo(b.shape.label);
  return shapeComparison != 0 ? shapeComparison : _compareByName(a, b);
}
