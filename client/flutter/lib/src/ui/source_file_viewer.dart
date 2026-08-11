import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';

import 'ide_theme.dart';

const double _lineHeight = 22;

class SourceFileViewer extends StatefulWidget {
  const SourceFileViewer({
    super.key,
    required this.path,
    required this.content,
    this.highlightStartLine,
    this.highlightEndLine,
  });

  final String path;
  final Future<String> content;

  /// A line range to scroll into view and highlight once [content] loads,
  /// 1-indexed and inclusive. `null` leaves the viewer at the top with
  /// nothing highlighted, e.g. when opening a file from the Source Files
  /// panel rather than from a navigator that points at a specific
  /// declaration.
  final int? highlightStartLine;
  final int? highlightEndLine;

  @override
  State<SourceFileViewer> createState() => _SourceFileViewerState();
}

class _SourceFileViewerState extends State<SourceFileViewer> {
  final ScrollController _scrollController = ScrollController();

  @override
  void initState() {
    super.initState();
    _scheduleScrollToHighlight();
  }

  @override
  void didUpdateWidget(covariant SourceFileViewer oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.content != widget.content ||
        oldWidget.highlightStartLine != widget.highlightStartLine) {
      _scheduleScrollToHighlight();
    }
  }

  @override
  void dispose() {
    _scrollController.dispose();
    super.dispose();
  }

  void _scheduleScrollToHighlight() {
    final startLine = widget.highlightStartLine;
    if (startLine == null) {
      return;
    }

    widget.content.then((_) {
      if (!mounted) {
        return;
      }

      SchedulerBinding.instance.addPostFrameCallback((_) {
        if (!mounted || !_scrollController.hasClients) {
          return;
        }

        final position = _scrollController.position;
        final target =
            ((startLine - 1) * _lineHeight - position.viewportDimension / 3)
                .clamp(0.0, position.maxScrollExtent);
        _scrollController.jumpTo(target);
      });
    });
  }

  @override
  Widget build(BuildContext context) {
    return FutureBuilder<String>(
      future: widget.content,
      builder: (context, snapshot) {
        if (snapshot.connectionState != ConnectionState.done) {
          return const Center(
            child: SizedBox.square(
              dimension: 24,
              child: CircularProgressIndicator(strokeWidth: 2),
            ),
          );
        }

        if (snapshot.hasError) {
          return Padding(
            padding: const EdgeInsets.all(16),
            child: Text(
              'Failed to load ${widget.path}: ${snapshot.error}',
              style: const TextStyle(color: Color(0xFFB3261E)),
            ),
          );
        }

        final lines = (snapshot.data ?? '').split('\n');
        final highlightStart = widget.highlightStartLine;
        final highlightEnd = widget.highlightEndLine ?? highlightStart;

        return ListView.builder(
          controller: _scrollController,
          padding: const EdgeInsets.symmetric(vertical: 10),
          itemCount: lines.length,
          itemBuilder: (context, index) {
            final number = index + 1;
            final highlighted =
                highlightStart != null &&
                highlightEnd != null &&
                number >= highlightStart &&
                number <= highlightEnd;

            return _SourceLine(
              number: number,
              code: lines[index],
              highlighted: highlighted,
            );
          },
        );
      },
    );
  }
}

class _SourceLine extends StatelessWidget {
  const _SourceLine({
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
      child: SizedBox(
        height: _lineHeight,
        child: Row(
          children: [
            SizedBox(
              width: 48,
              child: Text(
                '$number',
                textAlign: TextAlign.right,
                style: const TextStyle(
                  color: IdePalette.lineNumber,
                  fontFamily: 'monospace',
                  fontSize: 13,
                ),
              ),
            ),
            const SizedBox(width: 14),
            Expanded(
              child: Text(
                code,
                softWrap: false,
                overflow: TextOverflow.fade,
                style: const TextStyle(
                  color: IdePalette.codeText,
                  fontFamily: 'monospace',
                  fontSize: 13,
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
