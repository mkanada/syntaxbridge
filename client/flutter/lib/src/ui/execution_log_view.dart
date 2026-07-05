import 'package:flutter/material.dart';

import 'execution_log.dart';
import 'ide_theme.dart';

class ExecutionLogView extends StatelessWidget {
  const ExecutionLogView({
    super.key,
    required this.entries,
    this.showTitle = true,
  });

  final List<ExecutionLogEntry> entries;
  final bool showTitle;

  @override
  Widget build(BuildContext context) {
    final textTheme = Theme.of(context).textTheme;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        if (showTitle) ...[
          Text('Execution log', style: textTheme.headlineSmall),
          const SizedBox(height: 12),
        ],
        DecoratedBox(
          decoration: BoxDecoration(
            color: IdePalette.editor,
            border: Border.all(color: IdePalette.border),
            borderRadius: BorderRadius.circular(6),
          ),
          child: ConstrainedBox(
            constraints: const BoxConstraints(minHeight: 160, maxHeight: 280),
            child: entries.isEmpty
                ? const Padding(
                    padding: EdgeInsets.all(16),
                    child: Text(
                      'No events yet',
                      style: TextStyle(color: IdePalette.muted),
                    ),
                  )
                : SingleChildScrollView(
                    padding: const EdgeInsets.symmetric(vertical: 8),
                    child: Column(
                      children: [
                        for (final (index, entry) in entries.indexed) ...[
                          if (index > 0)
                            const Divider(height: 1, color: IdePalette.border),
                          ExecutionLogRow(entry: entry),
                        ],
                      ],
                    ),
                  ),
          ),
        ),
      ],
    );
  }
}

class ExecutionLogRow extends StatelessWidget {
  const ExecutionLogRow({super.key, required this.entry});

  final ExecutionLogEntry entry;

  @override
  Widget build(BuildContext context) {
    return ListTile(
      dense: true,
      minLeadingWidth: 24,
      leading: Icon(_icon, color: _color),
      title: Text(
        entry.message,
        style: const TextStyle(color: IdePalette.codeText),
      ),
      subtitle: Text(
        _formattedTime(entry.timestamp),
        style: const TextStyle(color: IdePalette.muted),
      ),
    );
  }

  IconData get _icon {
    return switch (entry.level) {
      ExecutionLogLevel.info => Icons.info_outline,
      ExecutionLogLevel.success => Icons.check_circle_outline,
      ExecutionLogLevel.warning => Icons.warning_amber_outlined,
      ExecutionLogLevel.error => Icons.error_outline,
    };
  }

  Color get _color {
    return switch (entry.level) {
      ExecutionLogLevel.info => IdePalette.blue,
      ExecutionLogLevel.success => IdePalette.green,
      ExecutionLogLevel.warning => IdePalette.amber,
      ExecutionLogLevel.error => IdePalette.red,
    };
  }

  String _formattedTime(DateTime timestamp) {
    String twoDigits(int value) => value.toString().padLeft(2, '0');

    return '${twoDigits(timestamp.hour)}:${twoDigits(timestamp.minute)}:${twoDigits(timestamp.second)}';
  }
}
