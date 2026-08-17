import 'package:flutter/material.dart';

import '../project/project_error_message.dart';
import '../project/project_models.dart';
import 'ide_theme.dart';

/// Shows the `dart analyze` findings against the transpiled project (US-9,
/// `POST /projects/validate`), each translated back to its C++ origin when
/// [DartDiagnostic.origin] could resolve one — see that type's doc comment
/// (`crates/server/src/validate/dart.rs`) for the granularity that
/// resolves at (a whole top-level declaration, not the exact statement).
///
/// Errors and warnings are both shown — an ERROR-severity finding means the
/// generated Dart doesn't compile, but nothing about this panel or the
/// route behind it blocks the user from continuing anyway: US-9's roteiro
/// (`docs/plans/User Steps.md`) explicitly chose "warnings inform, errors
/// block" only in the sense of visual weight (errors sort first, in red),
/// not as a hard gate — the same "never block on incomplete proof" spirit
/// AGENTS.md already applies to US-6.
class DiagnosticsView extends StatelessWidget {
  const DiagnosticsView({
    super.key,
    required this.diagnostics,
    required this.onDiagnosticSelected,
  });

  /// `null` before "Validate" has been triggered at all.
  final Future<List<DartDiagnostic>>? diagnostics;

  /// Fired when a diagnostic with a resolvable [DartDiagnostic.origin] is
  /// clicked — the caller navigates the main source viewer there, the same
  /// shape [onUsageSelected]/`onCallerSelected` already use elsewhere.
  final ValueChanged<DartDiagnostic> onDiagnosticSelected;

  @override
  Widget build(BuildContext context) {
    final future = diagnostics;
    if (future == null) {
      return const _Placeholder(
        message: 'Click "Validate" to run dart analyze on this project.',
      );
    }

    return FutureBuilder<List<DartDiagnostic>>(
      future: future,
      builder: (context, snapshot) {
        if (snapshot.connectionState != ConnectionState.done) {
          return const Center(
            child: Padding(
              padding: EdgeInsets.all(16),
              child: CircularProgressIndicator(),
            ),
          );
        }

        final error = snapshot.error;
        if (error != null) {
          return Padding(
            padding: const EdgeInsets.all(16),
            child: Text(
              'Failed to validate: ${projectErrorMessage(error)}',
              style: const TextStyle(color: IdePalette.red),
            ),
          );
        }

        final items = List<DartDiagnostic>.of(snapshot.data ?? const []);
        if (items.isEmpty) {
          return const _Placeholder(message: 'No issues found');
        }

        items.sort(_bySeverityThenLocation);

        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'Validation',
              style: Theme.of(context).textTheme.headlineSmall,
            ),
            const SizedBox(height: 4),
            Text(
              _summary(items),
              style: Theme.of(context).textTheme.bodyMedium,
            ),
            const SizedBox(height: 16),
            Expanded(
              child: ListView.separated(
                itemCount: items.length,
                separatorBuilder: (context, index) => const Divider(height: 1),
                itemBuilder: (context, index) {
                  final diagnostic = items[index];
                  return _DiagnosticTile(
                    diagnostic: diagnostic,
                    onSelected: diagnostic.origin == null
                        ? null
                        : () => onDiagnosticSelected(diagnostic),
                  );
                },
              ),
            ),
          ],
        );
      },
    );
  }
}

int _bySeverityThenLocation(DartDiagnostic a, DartDiagnostic b) {
  final severityOrder = _severityRank(
    a.severity,
  ).compareTo(_severityRank(b.severity));
  if (severityOrder != 0) {
    return severityOrder;
  }
  final fileOrder = a.dartFile.compareTo(b.dartFile);
  if (fileOrder != 0) {
    return fileOrder;
  }
  return a.dartLine.compareTo(b.dartLine);
}

int _severityRank(DartDiagnosticSeverity severity) {
  switch (severity) {
    case DartDiagnosticSeverity.error:
      return 0;
    case DartDiagnosticSeverity.warning:
      return 1;
    case DartDiagnosticSeverity.info:
      return 2;
  }
}

String _summary(List<DartDiagnostic> items) {
  final errors = items
      .where((d) => d.severity == DartDiagnosticSeverity.error)
      .length;
  final warnings = items
      .where((d) => d.severity == DartDiagnosticSeverity.warning)
      .length;
  final parts = <String>[
    if (errors > 0) '$errors ${errors == 1 ? 'error' : 'errors'}',
    if (warnings > 0) '$warnings ${warnings == 1 ? 'warning' : 'warnings'}',
  ];
  if (parts.isEmpty) {
    return '${items.length} ${items.length == 1 ? 'finding' : 'findings'}';
  }
  return parts.join(', ');
}

class _DiagnosticTile extends StatelessWidget {
  const _DiagnosticTile({required this.diagnostic, required this.onSelected});

  final DartDiagnostic diagnostic;
  final VoidCallback? onSelected;

  @override
  Widget build(BuildContext context) {
    final origin = diagnostic.origin;
    return ListTile(
      contentPadding: EdgeInsets.zero,
      leading: _severityIcon(diagnostic.severity),
      title: Text(diagnostic.message),
      subtitle: Text(
        origin == null
            ? '${diagnostic.dartFile}:${diagnostic.dartLine}'
            : '${diagnostic.dartFile}:${diagnostic.dartLine}  →  '
                  '${_fileName(origin.file)}:${origin.line}',
        style: const TextStyle(color: IdePalette.muted),
      ),
      trailing: onSelected == null
          ? null
          : const Icon(Icons.open_in_new, size: 18),
      onTap: onSelected,
      enabled: onSelected != null,
    );
  }
}

String _fileName(String path) => path.split('/').last;

Icon _severityIcon(DartDiagnosticSeverity severity) {
  switch (severity) {
    case DartDiagnosticSeverity.error:
      return const Icon(Icons.error, color: IdePalette.red, size: 18);
    case DartDiagnosticSeverity.warning:
      return const Icon(Icons.warning_amber, color: Colors.amber, size: 18);
    case DartDiagnosticSeverity.info:
      return const Icon(Icons.info_outline, color: IdePalette.muted, size: 18);
  }
}

class _Placeholder extends StatelessWidget {
  const _Placeholder({required this.message});

  final String message;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(16),
      child: Text(message, style: const TextStyle(color: IdePalette.muted)),
    );
  }
}
