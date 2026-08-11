import 'package:flutter/material.dart';

import '../project/project_models.dart';
import 'ide_theme.dart';

/// Every recorded caller of the function currently selected in
/// [FunctionsView] (US-5 criterion 5): each row is an accordion that expands
/// in place to show the surrounding code, fetched lazily through
/// [loadCallerSource] — mirrors [UsagesView], US-4's own panel. Jumping the
/// main viewer to a call site is a separate, explicit action via
/// [onCallerSelected], triggered by that row's "open in editor" button
/// rather than by expanding the row.
class CallersView extends StatelessWidget {
  const CallersView({
    super.key,
    required this.selectedFunction,
    required this.callers,
    required this.onCallerSelected,
    required this.loadCallerSource,
  });

  final FunctionDeclaration? selectedFunction;
  final List<CallEdge> callers;
  final ValueChanged<CallEdge> onCallerSelected;

  /// Fetches the full contents of the file a call occurs in, so the
  /// accordion can show the code around it. Mirrors
  /// [UsagesView.loadUsageSource].
  final Future<String> Function(CallEdge caller) loadCallerSource;

  @override
  Widget build(BuildContext context) {
    final selectedFunction = this.selectedFunction;
    if (selectedFunction == null) {
      return const Padding(
        padding: EdgeInsets.all(16),
        child: Text(
          'Select a function to see its callers',
          style: TextStyle(color: IdePalette.muted),
        ),
      );
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text('Callers', style: Theme.of(context).textTheme.headlineSmall),
        const SizedBox(height: 4),
        Text(
          '${callers.length} ${callers.length == 1 ? 'caller' : 'callers'} of '
          '${selectedFunction.qualifiedName}',
          style: Theme.of(context).textTheme.bodyMedium,
        ),
        const SizedBox(height: 16),
        Expanded(
          child: callers.isEmpty
              ? const Padding(
                  padding: EdgeInsets.only(top: 8),
                  child: Text(
                    'No callers found',
                    style: TextStyle(color: IdePalette.muted),
                  ),
                )
              : ListView.separated(
                  itemCount: callers.length,
                  separatorBuilder: (context, index) =>
                      const Divider(height: 1),
                  itemBuilder: (context, index) {
                    final caller = callers[index];
                    return _CallerAccordionItem(
                      // Keyed by the call site's own identity (not just list
                      // index) so Flutter doesn't reuse a row's `State` —
                      // including its cached source snippet and expanded
                      // flag — for an unrelated caller when the selected
                      // function changes and this list is rebuilt.
                      key: ValueKey(
                        '${caller.callerUsr}@${caller.file}:${caller.line}:'
                        '${caller.column}',
                      ),
                      caller: caller,
                      onOpenInEditor: () => onCallerSelected(caller),
                      loadSource: loadCallerSource,
                    );
                  },
                ),
        ),
      ],
    );
  }
}

String _fileName(String path) => path.split('/').last;

/// A short label for a call edge's resolution (US-5 criteria 3/6): flags
/// virtual dispatch and unresolved calls, which are recorded but can't point
/// to a single definitive callee the way a direct call does.
String _resolutionLabel(CallResolution resolution) {
  if (!resolution.isResolved) {
    return 'unresolved: ${resolution.unresolvedReason}';
  }
  return resolution.isDynamicDispatch ? 'dynamic dispatch' : 'direct call';
}

class _CallerAccordionItem extends StatefulWidget {
  const _CallerAccordionItem({
    super.key,
    required this.caller,
    required this.onOpenInEditor,
    required this.loadSource,
  });

  final CallEdge caller;
  final VoidCallback onOpenInEditor;
  final Future<String> Function(CallEdge caller) loadSource;

  @override
  State<_CallerAccordionItem> createState() => _CallerAccordionItemState();
}

class _CallerAccordionItemState extends State<_CallerAccordionItem> {
  Future<String>? _source;
  bool _expanded = false;

  @override
  Widget build(BuildContext context) {
    final caller = widget.caller;
    return ExpansionTile(
      tilePadding: EdgeInsets.zero,
      childrenPadding: const EdgeInsets.only(bottom: 8),
      leading: const Icon(Icons.call_made),
      title: Text('${_fileName(caller.file)}:${caller.line}'),
      subtitle: Text(
        _resolutionLabel(caller.resolution),
        style: const TextStyle(color: IdePalette.muted),
      ),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          IconButton(
            key: ValueKey('caller-open-${caller.file}:${caller.line}'),
            icon: const Icon(Icons.open_in_new, size: 18),
            tooltip: 'Open in editor',
            onPressed: widget.onOpenInEditor,
          ),
          Icon(_expanded ? Icons.expand_less : Icons.expand_more),
        ],
      ),
      onExpansionChanged: (expanded) {
        setState(() {
          _expanded = expanded;
          if (expanded) {
            _source ??= widget.loadSource(caller);
          }
        });
      },
      children: [
        FutureBuilder<String>(
          future: _source,
          builder: (context, snapshot) {
            if (snapshot.connectionState != ConnectionState.done) {
              return const Padding(
                padding: EdgeInsets.symmetric(vertical: 16),
                child: Center(
                  child: SizedBox.square(
                    dimension: 18,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  ),
                ),
              );
            }

            final error = snapshot.error;
            if (error != null) {
              return Padding(
                padding: const EdgeInsets.all(8),
                child: Text(
                  'Failed to load ${caller.file}: $error',
                  style: const TextStyle(color: IdePalette.red),
                ),
              );
            }

            return Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                for (final line in _snippetLines(
                  snapshot.data ?? '',
                  caller.line,
                ))
                  _CodeSnippetLine(
                    number: line.key,
                    code: line.value,
                    highlighted: line.key == caller.line,
                  ),
              ],
            );
          },
        ),
      ],
    );
  }
}

/// The lines around [line] (1-indexed), [context] on either side, clamped to
/// the bounds of [content]. Mirrors `usages_view.dart`'s own helper.
List<MapEntry<int, String>> _snippetLines(
  String content,
  int line, {
  int context = 2,
}) {
  final lines = content.split('\n');
  if (lines.isEmpty) {
    return const [];
  }

  final target = line.clamp(1, lines.length);
  final start = (target - context).clamp(1, lines.length);
  final end = (target + context).clamp(1, lines.length);

  return [
    for (var number = start; number <= end; number++)
      MapEntry(number, lines[number - 1]),
  ];
}

class _CodeSnippetLine extends StatelessWidget {
  const _CodeSnippetLine({
    required this.number,
    required this.code,
    this.highlighted = false,
  });

  final int number;
  final String code;
  final bool highlighted;

  @override
  Widget build(BuildContext context) {
    return ColoredBox(
      color: highlighted ? IdePalette.selection : Colors.transparent,
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 1),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            SizedBox(
              width: 40,
              child: Text(
                '$number',
                textAlign: TextAlign.right,
                style: const TextStyle(
                  color: IdePalette.lineNumber,
                  fontFamily: 'monospace',
                  fontSize: 12,
                ),
              ),
            ),
            const SizedBox(width: 10),
            Expanded(
              child: Text(
                code,
                softWrap: false,
                overflow: TextOverflow.fade,
                style: const TextStyle(
                  color: IdePalette.codeText,
                  fontFamily: 'monospace',
                  fontSize: 12,
                  height: 1.3,
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
