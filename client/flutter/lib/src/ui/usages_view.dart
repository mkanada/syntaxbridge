import 'package:flutter/material.dart';

import '../project/project_models.dart';
import 'ide_theme.dart';

/// Every recorded usage of the type currently selected in [TypesView]
/// (US-4): each row is an accordion that expands in place to show the
/// surrounding code, fetched lazily through [loadUsageSource] — this is
/// purely local to the row and does *not* touch the main source viewer.
/// Jumping the main viewer to a usage (US-4 criterion 5) is a separate,
/// explicit action via [onUsageSelected], triggered by that row's
/// "open in editor" button rather than by expanding the row — the two
/// were briefly conflated (expanding a row also jumped the main viewer),
/// which surprised users who just wanted to peek at the code inline.
/// Clicking a type in the catalog navigates to its own declaration (US-3)
/// *and* populates this panel — the two stay in sync through the shared
/// [selectedType], without this widget owning any fetching itself.
class UsagesView extends StatelessWidget {
  const UsagesView({
    super.key,
    required this.selectedType,
    required this.usages,
    required this.onUsageSelected,
    required this.loadUsageSource,
  });

  final TypeDeclaration? selectedType;
  final List<TypeUsage> usages;
  final ValueChanged<TypeUsage> onUsageSelected;

  /// Fetches the full contents of the file a usage occurs in, so the
  /// accordion can show the code around it. Returns the whole file (mirrors
  /// [ServerClient.readSourceFile]) rather than a pre-cut snippet, so the
  /// snippet window itself is decided here, next to where it's rendered.
  final Future<String> Function(TypeUsage usage) loadUsageSource;

  @override
  Widget build(BuildContext context) {
    final selectedType = this.selectedType;
    if (selectedType == null) {
      return const Padding(
        padding: EdgeInsets.all(16),
        child: Text(
          'Select a type to see where it is used',
          style: TextStyle(color: IdePalette.muted),
        ),
      );
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text('Usages', style: Theme.of(context).textTheme.headlineSmall),
        const SizedBox(height: 4),
        Text(
          '${usages.length} ${usages.length == 1 ? 'use' : 'uses'} of '
          '${_qualifiedName(selectedType)}',
          style: Theme.of(context).textTheme.bodyMedium,
        ),
        const SizedBox(height: 16),
        Expanded(
          child: usages.isEmpty
              ? const Padding(
                  padding: EdgeInsets.only(top: 8),
                  child: Text(
                    'No usages found',
                    style: TextStyle(color: IdePalette.muted),
                  ),
                )
              : ListView.separated(
                  itemCount: usages.length,
                  separatorBuilder: (context, index) =>
                      const Divider(height: 1),
                  itemBuilder: (context, index) {
                    final usage = usages[index];
                    return _UsageAccordionItem(
                      usage: usage,
                      onOpenInEditor: () => onUsageSelected(usage),
                      loadSource: loadUsageSource,
                    );
                  },
                ),
        ),
      ],
    );
  }
}

String _qualifiedName(TypeDeclaration type) {
  return type.namespace.isEmpty ? type.name : '${type.namespace}::${type.name}';
}

String _fileName(String path) => path.split('/').last;

/// One usage in the list, collapsed to its file:line by default. Expanding
/// it only affects this row: it lazily fetches its file via [loadSource]
/// (cached afterwards, so re-collapsing and reopening doesn't refetch) and
/// renders the surrounding code beneath the row. Jumping the main source
/// viewer to this usage (US-4 criterion 5) is a distinct action, fired by
/// [onOpenInEditor] via the row's own "open in editor" button rather than by
/// expanding it.
class _UsageAccordionItem extends StatefulWidget {
  const _UsageAccordionItem({
    required this.usage,
    required this.onOpenInEditor,
    required this.loadSource,
  });

  final TypeUsage usage;
  final VoidCallback onOpenInEditor;
  final Future<String> Function(TypeUsage usage) loadSource;

  @override
  State<_UsageAccordionItem> createState() => _UsageAccordionItemState();
}

class _UsageAccordionItemState extends State<_UsageAccordionItem> {
  Future<String>? _source;
  bool _expanded = false;

  @override
  Widget build(BuildContext context) {
    final usage = widget.usage;
    return ExpansionTile(
      tilePadding: EdgeInsets.zero,
      childrenPadding: const EdgeInsets.only(bottom: 8),
      leading: const Icon(Icons.place_outlined),
      title: Text('${_fileName(usage.file)}:${usage.line}'),
      subtitle: Text(
        usage.kind.label,
        style: const TextStyle(color: IdePalette.muted),
      ),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          IconButton(
            key: ValueKey('usage-open-${usage.file}:${usage.line}'),
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
            _source ??= widget.loadSource(usage);
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
                  'Failed to load ${usage.file}: $error',
                  style: const TextStyle(color: IdePalette.red),
                ),
              );
            }

            return Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                for (final line in _snippetLines(
                  snapshot.data ?? '',
                  usage.line,
                ))
                  _CodeSnippetLine(
                    number: line.key,
                    code: line.value,
                    highlighted: line.key == usage.line,
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
/// the bounds of [content] — a short window rather than the whole file,
/// since the accordion shows this inline alongside every other usage.
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
