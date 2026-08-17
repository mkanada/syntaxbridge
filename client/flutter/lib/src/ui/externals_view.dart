import 'package:flutter/material.dart';

import '../project/project_models.dart';
import 'ide_theme.dart';

/// The "Extern" navigator (`docs/plans/lista-de-externos.md`): every usr
/// currently in the effective external set, with the source(s) that put it
/// there, plus both live regexp rule lists. By papel this is a **Decisão**
/// list (`docs/plans/ui-lists.md`) — the marks and patterns *are* the
/// product of this screen — but it's implemented as an ordinary docked
/// navigator panel for this delivery, since the center-document mechanism
/// that family calls for was never built for anything, not even US-7 which
/// motivated it (see the design doc's own "Onde isso mora na UI" section).
class ExternalsView extends StatefulWidget {
  const ExternalsView({
    super.key,
    required this.listing,
    required this.onToggleExternal,
    required this.onAddNameRegex,
    required this.onRemoveNameRegex,
    required this.onAddPathRegex,
    required this.onRemovePathRegex,
  });

  final ExternalListing listing;

  /// Manual override for one usr, in either direction (decision 5) — the
  /// same action a regex-matched or auto-detected item's row exposes to
  /// contradict its source.
  final void Function(String usr, bool external) onToggleExternal;

  final ValueChanged<String> onAddNameRegex;
  final ValueChanged<int> onRemoveNameRegex;
  final ValueChanged<String> onAddPathRegex;
  final ValueChanged<int> onRemovePathRegex;

  @override
  State<ExternalsView> createState() => _ExternalsViewState();
}

class _ExternalsViewState extends State<ExternalsView> {
  final _nameRegexController = TextEditingController();
  final _pathRegexController = TextEditingController();

  @override
  void dispose() {
    _nameRegexController.dispose();
    _pathRegexController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final statuses = List<ExternalStatus>.of(widget.listing.statuses)
      ..sort((a, b) => a.usr.compareTo(b.usr));
    final effectiveCount = statuses.where((status) => status.effective).length;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text('Extern', style: Theme.of(context).textTheme.headlineSmall),
        const SizedBox(height: 4),
        Text(
          '$effectiveCount de ${statuses.length} itens marcados como extern',
          style: Theme.of(context).textTheme.bodyMedium,
        ),
        const SizedBox(height: 16),
        _RegexSection(
          title: 'Regexp de nome',
          hint: '^humlib::',
          controller: _nameRegexController,
          rules: widget.listing.nameRegexes,
          statuses: statuses,
          matchKind: ExternalSourceKind.nameRegex,
          onAdd: widget.onAddNameRegex,
          onRemove: widget.onRemoveNameRegex,
        ),
        const SizedBox(height: 12),
        _RegexSection(
          title: 'Regexp de caminho',
          hint: '^third_party/',
          controller: _pathRegexController,
          rules: widget.listing.pathRegexes,
          statuses: statuses,
          matchKind: ExternalSourceKind.pathRegex,
          onAdd: widget.onAddPathRegex,
          onRemove: widget.onRemovePathRegex,
        ),
        const SizedBox(height: 16),
        Expanded(
          child: statuses.isEmpty
              ? const Padding(
                  padding: EdgeInsets.only(top: 8),
                  child: Text(
                    'Nenhum item externo ainda',
                    style: TextStyle(color: IdePalette.muted),
                  ),
                )
              : ListView(
                  children: [
                    for (final status in statuses) ...[
                      _StatusRow(
                        status: status,
                        onToggleExternal: widget.onToggleExternal,
                      ),
                      const Divider(height: 1),
                    ],
                  ],
                ),
        ),
      ],
    );
  }
}

class _StatusRow extends StatelessWidget {
  const _StatusRow({required this.status, required this.onToggleExternal});

  final ExternalStatus status;
  final void Function(String usr, bool external) onToggleExternal;

  @override
  Widget build(BuildContext context) {
    return ListTile(
      contentPadding: EdgeInsets.zero,
      leading: Icon(
        status.effective ? Icons.link : Icons.link_off,
        color: status.effective ? IdePalette.violet : IdePalette.muted,
      ),
      title: Text(status.usr, overflow: TextOverflow.ellipsis),
      subtitle: Wrap(
        spacing: 6,
        children: [
          for (final source in status.sources)
            Chip(
              label: Text(
                source.pattern == null
                    ? source.kind.label
                    : '${source.kind.label}: ${source.pattern}',
                style: const TextStyle(fontSize: 11),
              ),
              visualDensity: VisualDensity.compact,
              materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
            ),
        ],
      ),
      trailing: TextButton(
        onPressed: () => onToggleExternal(status.usr, !status.effective),
        child: Text(status.effective ? 'Excluir' : 'Incluir'),
      ),
    );
  }
}

class _RegexSection extends StatelessWidget {
  const _RegexSection({
    required this.title,
    required this.hint,
    required this.controller,
    required this.rules,
    required this.statuses,
    required this.matchKind,
    required this.onAdd,
    required this.onRemove,
  });

  final String title;
  final String hint;
  final TextEditingController controller;
  final List<Object> rules;
  final List<ExternalStatus> statuses;
  final ExternalSourceKind matchKind;
  final ValueChanged<String> onAdd;
  final ValueChanged<int> onRemove;

  int _idOf(Object rule) {
    return rule is NameRegexRule ? rule.id : (rule as PathRegexRule).id;
  }

  String _patternOf(Object rule) {
    return rule is NameRegexRule
        ? rule.pattern
        : (rule as PathRegexRule).pattern;
  }

  int _matchCount(String pattern) {
    return statuses.where((status) {
      return status.sources.any(
        (source) => source.kind == matchKind && source.pattern == pattern,
      );
    }).length;
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(title, style: Theme.of(context).textTheme.labelLarge),
        const SizedBox(height: 4),
        Row(
          children: [
            Expanded(
              child: TextField(
                controller: controller,
                decoration: InputDecoration(
                  hintText: hint,
                  isDense: true,
                  border: const OutlineInputBorder(),
                ),
              ),
            ),
            const SizedBox(width: 8),
            FilledButton(
              onPressed: () {
                final pattern = controller.text.trim();
                if (pattern.isEmpty) {
                  return;
                }
                onAdd(pattern);
                controller.clear();
              },
              child: const Text('Adicionar'),
            ),
          ],
        ),
        for (final rule in rules)
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: Row(
              children: [
                Expanded(
                  child: Text(
                    _patternOf(rule),
                    style: const TextStyle(fontFamily: 'monospace'),
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
                Text(
                  '${_matchCount(_patternOf(rule))} itens',
                  style: const TextStyle(color: IdePalette.muted, fontSize: 12),
                ),
                IconButton(
                  iconSize: 16,
                  visualDensity: VisualDensity.compact,
                  tooltip: 'Remover',
                  icon: const Icon(Icons.close),
                  onPressed: () => onRemove(_idOf(rule)),
                ),
              ],
            ),
          ),
      ],
    );
  }
}
