import 'dart:async';

import 'package:flutter/material.dart';

import '../project/project_error_message.dart';
import '../project/project_models.dart';
import '../server/server_client.dart';
import 'execution_log.dart';
import 'execution_log_view.dart';
import 'ide_theme.dart';

/// Shown immediately after the new-project form is submitted, while the
/// server ingests the archive and runs `libclang` extraction in the
/// background — which can take minutes for a real project (see
/// `crates/server/tests/verovio_5_7_0_import_diagnosis.rs`). Polls
/// `GET /projects/jobs/{id}` for real progress rather than leaving the user
/// looking at a form with nothing happening.
class CreatingProjectPage extends StatefulWidget {
  const CreatingProjectPage({
    super.key,
    required this.serverClient,
    required this.input,
    required this.onProjectCreated,
    required this.onCancel,
    this.pollInterval = const Duration(milliseconds: 400),
  });

  final ServerClient serverClient;
  final CreateProjectInput input;
  final ValueChanged<CreatedProject> onProjectCreated;
  final VoidCallback onCancel;
  final Duration pollInterval;

  @override
  State<CreatingProjectPage> createState() => _CreatingProjectPageState();
}

class _CreatingProjectPageState extends State<CreatingProjectPage> {
  final List<ExecutionLogEntry> _logs = [];
  Timer? _pollTimer;
  ProjectCreationJobPhase? _lastLoggedPhase;
  ProjectCreationJobStatus? _lastStatus;
  bool _failed = false;
  bool _cancelled = false;
  String? _jobId;
  bool _cancelRequested = false;

  @override
  void initState() {
    super.initState();
    _addLog('Validated project parameters');
    _start();
  }

  @override
  void dispose() {
    _pollTimer?.cancel();
    super.dispose();
  }

  void _addLog(
    String message, {
    ExecutionLogLevel level = ExecutionLogLevel.info,
  }) {
    setState(() {
      _logs.add(
        ExecutionLogEntry(
          timestamp: DateTime.now(),
          level: level,
          message: message,
        ),
      );
    });
  }

  Future<void> _start() async {
    try {
      _addLog('Sending project to server');
      final jobId = await widget.serverClient.startCreateProject(widget.input);
      if (!mounted) {
        return;
      }

      _addLog('Job started');
      _jobId = jobId;
      // Assigned before the first poll runs, not after: `_poll` can resolve
      // (and cancel `_pollTimer`) within the same microtask flush as this
      // call, and if the timer were created afterwards that cancel would be
      // racing against — and losing to — this assignment, leaking a
      // never-cancelled periodic timer.
      _pollTimer = Timer.periodic(widget.pollInterval, (_) => _poll(jobId));
      await _poll(jobId);
    } catch (error) {
      if (!mounted) {
        return;
      }

      _reportFailure(
        'Failed to start project creation: ${projectErrorMessage(error)}',
      );
    }
  }

  Future<void> _poll(String jobId) async {
    try {
      final status = await widget.serverClient.pollCreateProjectJob(jobId);
      if (!mounted) {
        return;
      }

      final phase = status.phase;
      if (phase != null && phase != _lastLoggedPhase) {
        _lastLoggedPhase = phase;
        _addLog(phase.label);
      }
      if (status.state == ProjectCreationJobState.cancelling &&
          _lastStatus?.state != ProjectCreationJobState.cancelling) {
        _addLog('Cancellation requested, stopping...');
      }
      setState(() => _lastStatus = status);

      switch (status.state) {
        case ProjectCreationJobState.running:
          return;
        case ProjectCreationJobState.cancelling:
          return;
        case ProjectCreationJobState.succeeded:
          _pollTimer?.cancel();
          _addLog('Project created', level: ExecutionLogLevel.success);
          widget.onProjectCreated(status.project!);
        case ProjectCreationJobState.failed:
          _pollTimer?.cancel();
          _reportFailure(status.errorMessage ?? 'Project creation failed');
        case ProjectCreationJobState.cancelled:
          _pollTimer?.cancel();
          setState(() => _cancelled = true);
          _addLog('Project creation cancelled', level: ExecutionLogLevel.info);
      }
    } catch (error) {
      if (!mounted) {
        return;
      }

      _pollTimer?.cancel();
      _reportFailure(
        'Failed to check project status: ${projectErrorMessage(error)}',
      );
    }
  }

  void _reportFailure(String message) {
    setState(() => _failed = true);
    _addLog(message, level: ExecutionLogLevel.error);
  }

  /// Requests cancellation (US-4 criterion 7). Sets [_cancelRequested]
  /// immediately, ahead of the next poll's own `cancelling` state, so the
  /// button disables right away instead of staying tappable for another
  /// `pollInterval` while a duplicate request could still go out.
  Future<void> _cancel() async {
    final jobId = _jobId;
    if (jobId == null || _cancelRequested) {
      return;
    }

    setState(() => _cancelRequested = true);
    _addLog('Cancelling...');
    try {
      await widget.serverClient.cancelCreateProject(jobId);
    } catch (error) {
      if (!mounted) {
        return;
      }
      _addLog(
        'Failed to request cancellation: ${projectErrorMessage(error)}',
        level: ExecutionLogLevel.error,
      );
    }
  }

  /// A combined fraction across all three `libclang` passes (US-5 added the
  /// function-catalog pass alongside the original type-catalog/source-catalog
  /// pair), or `null` while the type-catalog total isn't known yet (still
  /// ingesting) — an indeterminate bar is the honest thing to show before
  /// there's a total to divide by.
  double? get _progressFraction {
    final typeCatalog = _lastStatus?.typeCatalogProgress;
    if (typeCatalog == null || typeCatalog.total == 0) {
      return null;
    }

    final sourceCatalog = _lastStatus?.sourceCatalogProgress;
    final sourceTotal = sourceCatalog?.total ?? 0;
    final sourceCompleted = sourceCatalog?.completed ?? 0;
    final functionCatalog = _lastStatus?.functionCatalogProgress;
    final functionTotal = functionCatalog?.total ?? 0;
    final functionCompleted = functionCatalog?.completed ?? 0;
    // Every pass parses the same units, so until a given pass's own total is
    // known, assume it will mirror the type-catalog total rather than
    // showing earlier segments of the bar move faster than later ones.
    final totalUnits =
        typeCatalog.total +
        (sourceTotal > 0 ? sourceTotal : typeCatalog.total) +
        (functionTotal > 0 ? functionTotal : typeCatalog.total);
    final doneUnits =
        typeCatalog.completed + sourceCompleted + functionCompleted;

    return (doneUnits / totalUnits).clamp(0.0, 1.0);
  }

  @override
  Widget build(BuildContext context) {
    final textTheme = Theme.of(context).textTheme;

    return Scaffold(
      backgroundColor: IdePalette.background,
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 640),
          child: Padding(
            padding: const EdgeInsets.all(24),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Row(
                  children: [
                    if (_failed || _cancelled)
                      IconButton(
                        onPressed: widget.onCancel,
                        tooltip: 'Back',
                        icon: const Icon(Icons.arrow_back),
                      )
                    else
                      const SizedBox(width: 48),
                    const SizedBox(width: 4),
                    Expanded(
                      child: Text(
                        'Creating project',
                        style: textTheme.headlineMedium?.copyWith(
                          fontWeight: FontWeight.w700,
                        ),
                      ),
                    ),
                    if (!_failed && !_cancelled)
                      TextButton(
                        onPressed: _cancelRequested ? null : _cancel,
                        child: const Text('Cancel'),
                      ),
                  ],
                ),
                if (_failed) ...[
                  const SizedBox(height: 8),
                  Text(
                    'Project creation failed',
                    style: textTheme.titleMedium?.copyWith(
                      color: IdePalette.red,
                    ),
                  ),
                ],
                if (_cancelled) ...[
                  const SizedBox(height: 8),
                  Text(
                    'Project creation cancelled',
                    style: textTheme.titleMedium?.copyWith(
                      color: IdePalette.muted,
                    ),
                  ),
                ],
                const SizedBox(height: 24),
                ClipRRect(
                  borderRadius: BorderRadius.circular(4),
                  child: LinearProgressIndicator(
                    // A `null` value drives an indeterminate, endlessly
                    // animating bar — appropriate while work is genuinely in
                    // progress, but once failed or cancelled nothing is
                    // progressing anymore, and the page stays mounted so the
                    // user can read the outcome, so a frozen bar (never
                    // animating) is both the honest state and the only one a
                    // widget test's `pumpAndSettle` can converge on.
                    value: (_failed || _cancelled)
                        ? (_progressFraction ?? 0.0)
                        : _progressFraction,
                    minHeight: 6,
                    color: _failed ? IdePalette.red : null,
                    backgroundColor: IdePalette.selection,
                  ),
                ),
                const SizedBox(height: 24),
                SizedBox(
                  height: 320,
                  child: ExecutionLogView(entries: _logs, showTitle: false),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
