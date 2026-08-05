import 'package:flutter/material.dart';

import 'ide_theme.dart';

class SourceFileViewer extends StatelessWidget {
  const SourceFileViewer({
    super.key,
    required this.path,
    required this.content,
  });

  final String path;
  final Future<String> content;

  @override
  Widget build(BuildContext context) {
    return FutureBuilder<String>(
      future: content,
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
              'Failed to load $path: ${snapshot.error}',
              style: const TextStyle(color: Color(0xFFB3261E)),
            ),
          );
        }

        final lines = (snapshot.data ?? '').split('\n');

        return ListView.builder(
          padding: const EdgeInsets.symmetric(vertical: 10),
          itemCount: lines.length,
          itemBuilder: (context, index) {
            return _SourceLine(number: index + 1, code: lines[index]);
          },
        );
      },
    );
  }
}

class _SourceLine extends StatelessWidget {
  const _SourceLine({required this.number, required this.code});

  final int number;
  final String code;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      height: 22,
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
    );
  }
}
