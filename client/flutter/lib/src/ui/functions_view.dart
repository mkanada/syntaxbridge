import 'package:flutter/material.dart';

import '../project/project_models.dart';
import 'ide_theme.dart';

enum _SortField { name, kind, callerCount }

/// The function catalog navigator (US-5): every free function, method,
/// constructor, destructor and function-like macro declared in the project,
/// with its full signature. Sortable by name, kind, or number of callers —
/// mirrors [TypesView], US-3/US-4's own navigator, kept as a separate panel
/// since mixing declarations and callables in one list is exactly what
/// `docs/plans/User Steps.md` flags as a modeling mistake to avoid.
class FunctionsView extends StatefulWidget {
  const FunctionsView({
    super.key,
    required this.functions,
    required this.onFunctionSelected,
    this.selectedFunction,
    this.callerCounts = const <String, int>{},
  });

  final List<FunctionDeclaration> functions;
  final ValueChanged<FunctionDeclaration> onFunctionSelected;
  final FunctionDeclaration? selectedFunction;

  /// Each function's caller count (US-5 criterion 5), keyed by its `usr`. A
  /// function with no entry (or an empty `usr`) shows as 0 callers.
  final Map<String, int> callerCounts;

  @override
  State<FunctionsView> createState() => _FunctionsViewState();
}

class _FunctionsViewState extends State<FunctionsView> {
  _SortField _sortField = _SortField.name;
  bool _ascending = true;

  @override
  Widget build(BuildContext context) {
    final visible = List<FunctionDeclaration>.of(widget.functions);
    _sort(visible);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text('Functions', style: Theme.of(context).textTheme.headlineSmall),
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
            _sortButton(context, _SortField.kind, 'Kind'),
            // Labeled "Called", not "Callers", so it doesn't collide with
            // the Callers panel's own heading (mirrors TypesView's "Uses"
            // sort button, deliberately distinct from the Usages panel's
            // heading for the same reason).
            _sortButton(context, _SortField.callerCount, 'Called'),
          ],
        ),
        const SizedBox(height: 16),
        Expanded(
          child: visible.isEmpty
              ? const Padding(
                  padding: EdgeInsets.only(top: 8),
                  child: Text(
                    'No functions found',
                    style: TextStyle(color: IdePalette.muted),
                  ),
                )
              : ListView(children: _rows(visible)),
        ),
      ],
    );
  }

  /// A button toggling [field] as the active sort: tapping the inactive
  /// field switches to it (ascending); tapping the already-active field
  /// flips its direction, mirroring [TypesView]'s own sort buttons.
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

  void _sort(List<FunctionDeclaration> declarations) {
    declarations.sort((a, b) {
      final int comparison = switch (_sortField) {
        _SortField.name => _compareByName(a, b),
        _SortField.kind => _compareByKindThenName(a, b),
        _SortField.callerCount => _compareByCallerCount(a, b),
      };
      return _ascending ? comparison : -comparison;
    });
  }

  int _callerCount(FunctionDeclaration function) =>
      widget.callerCounts[function.usr] ?? 0;

  /// Ties break by name so sorting stays stable for equal counts, mirroring
  /// [TypesView]'s usage-count sort (US-4 criterion 4).
  int _compareByCallerCount(FunctionDeclaration a, FunctionDeclaration b) {
    final int countComparison = _callerCount(a).compareTo(_callerCount(b));
    return countComparison != 0 ? countComparison : _compareByName(a, b);
  }

  List<Widget> _rows(List<FunctionDeclaration> declarations) {
    return [
      for (final function in declarations) ...[
        ListTile(
          contentPadding: EdgeInsets.zero,
          selected: function == widget.selectedFunction,
          leading: Icon(
            function.kind == FunctionDeclarationKind.functionMacro
                ? Icons.tag
                : Icons.functions,
          ),
          title: Text(function.qualifiedName, overflow: TextOverflow.ellipsis),
          subtitle: Text(
            function.signature,
            overflow: TextOverflow.ellipsis,
            style: const TextStyle(color: IdePalette.muted, fontSize: 12),
          ),
          trailing: SizedBox(
            width: 110,
            child: Wrap(
              alignment: WrapAlignment.end,
              crossAxisAlignment: WrapCrossAlignment.center,
              spacing: 8,
              children: [
                if (function.isVirtual)
                  const Text(
                    'virtual',
                    style: TextStyle(color: IdePalette.muted, fontSize: 12),
                  ),
                Text(
                  '${_callerCount(function)} '
                  '${_callerCount(function) == 1 ? 'caller' : 'callers'}',
                  style: const TextStyle(color: IdePalette.muted, fontSize: 12),
                ),
                Text(
                  function.kind.label,
                  style: const TextStyle(color: IdePalette.muted, fontSize: 12),
                ),
              ],
            ),
          ),
          onTap: () => widget.onFunctionSelected(function),
        ),
        const Divider(height: 1),
      ],
    ];
  }
}

int _compareByName(FunctionDeclaration a, FunctionDeclaration b) {
  return a.qualifiedName.toLowerCase().compareTo(b.qualifiedName.toLowerCase());
}

int _compareByKindThenName(FunctionDeclaration a, FunctionDeclaration b) {
  final int kindComparison = a.kind.label.compareTo(b.kind.label);
  return kindComparison != 0 ? kindComparison : _compareByName(a, b);
}
