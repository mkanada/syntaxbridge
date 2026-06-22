import 'package:flutter/material.dart';

import 'execution_log.dart';

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
            color: const Color(0xFFFFFFFF),
            border: Border.all(color: const Color(0xFFD0D7D9)),
            borderRadius: BorderRadius.circular(8),
          ),
          child: ConstrainedBox(
            constraints: const BoxConstraints(minHeight: 160, maxHeight: 280),
            child: entries.isEmpty
                ? const Padding(
                    padding: EdgeInsets.all(16),
                    child: Text('No events yet'),
                  )
                : SingleChildScrollView(
                    padding: const EdgeInsets.symmetric(vertical: 8),
                    child: Column(
                      children: [
                        for (final (index, entry) in entries.indexed) ...[
                          if (index > 0) const Divider(height: 1),
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
      title: Text(entry.message),
      subtitle: Text(_formattedTime(entry.timestamp)),
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
      ExecutionLogLevel.info => const Color(0xFF3F6068),
      ExecutionLogLevel.success => const Color(0xFF237A57),
      ExecutionLogLevel.warning => const Color(0xFF7B6324),
      ExecutionLogLevel.error => const Color(0xFFB3261E),
    };
  }

  String _formattedTime(DateTime timestamp) {
    String twoDigits(int value) => value.toString().padLeft(2, '0');

    return '${twoDigits(timestamp.hour)}:${twoDigits(timestamp.minute)}:${twoDigits(timestamp.second)}';
  }
}
