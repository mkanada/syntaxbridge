import 'package:flutter/material.dart';

import '../project/project_error_message.dart';
import '../project/project_models.dart';
import 'ide_theme.dart';
import 'source_file_viewer.dart';

/// Shows the transpiled Dart file that corresponds to whichever C++ source
/// file is currently open (US-8/PR6, `docs/plans/primeiro-corte-e01-e03.md`)
/// — reuses [SourceFileViewer] for the actual text rendering rather than
/// duplicating it, since displaying read-only source text is exactly what
/// that widget already does.
class DartOutputView extends StatelessWidget {
  const DartOutputView({
    super.key,
    required this.package,
    required this.selectedCppPath,
  });

  /// `null` before "Transpile" has been triggered at all.
  final Future<TranspiledPackage>? package;

  /// The currently open C++ file's project-relative path, used to find the
  /// matching `lib/<stem>.dart` entry in [package]'s files — `null` when no
  /// C++ file is open.
  final String? selectedCppPath;

  @override
  Widget build(BuildContext context) {
    final future = package;
    if (future == null) {
      return const _Placeholder(
        message: 'Click "Transpile" to generate Dart for this project.',
      );
    }

    return FutureBuilder<TranspiledPackage>(
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
              'Failed to transpile: ${projectErrorMessage(error)}',
              style: const TextStyle(color: IdePalette.red),
            ),
          );
        }

        final cppPath = selectedCppPath;
        if (cppPath == null) {
          return const _Placeholder(
            message: 'Open a C++ source file to see its generated Dart.',
          );
        }

        final dartPath = matchingDartPath(cppPath);
        final content = snapshot.data?.files[dartPath];
        if (content == null) {
          return _Placeholder(
            message: 'No generated Dart for this file yet ($dartPath).',
          );
        }

        return SourceFileViewer(path: dartPath, content: Future.value(content));
      },
    );
  }
}

/// Mirrors `emit::dart::file_stem`'s naming convention in
/// `crates/server/src/emit/dart.rs` (one `.dart` file per C++ source file,
/// named after its stem, sanitized the same way) exactly — a stem with a
/// hyphen, space, or accented letter must land on the same key on both
/// sides, or this panel would report "no generated Dart" for a file that
/// really did transpile successfully.
String matchingDartPath(String cppPath) {
  final fileName = cppPath.split('/').last;
  final dotIndex = fileName.lastIndexOf('.');
  final rawStem = dotIndex > 0 ? fileName.substring(0, dotIndex) : fileName;
  final stem = _sanitizeIdentifier(rawStem);
  return 'lib/${stem.isEmpty ? 'output' : stem}.dart';
}

/// The Latin diacritics `emit::dart::fold_diacritic` folds to their base
/// ASCII letter — kept in lockstep with that function, not general Unicode
/// normalization.
const _diacriticFolds = <String, String>{
  'à': 'a',
  'á': 'a',
  'â': 'a',
  'ã': 'a',
  'ä': 'a',
  'å': 'a',
  'è': 'e',
  'é': 'e',
  'ê': 'e',
  'ë': 'e',
  'ì': 'i',
  'í': 'i',
  'î': 'i',
  'ï': 'i',
  'ò': 'o',
  'ó': 'o',
  'ô': 'o',
  'õ': 'o',
  'ö': 'o',
  'ù': 'u',
  'ú': 'u',
  'û': 'u',
  'ü': 'u',
  'ý': 'y',
  'ÿ': 'y',
  'ñ': 'n',
  'ç': 'c',
};

final _asciiAlphanumeric = RegExp(r'^[a-z0-9]$');
final _leadingDigit = RegExp(r'^[0-9]');

/// Mirrors `sanitize_identifier` in `crates/server/src/emit/dart.rs`:
/// lowercase, fold diacritics, replace every remaining character outside
/// `[a-z0-9_]` with `_` (collapsing repeats), trim leading/trailing `_`,
/// and prefix a leading digit with `_`.
String _sanitizeIdentifier(String input) {
  final buffer = StringBuffer();
  var previousWasUnderscore = false;
  for (final rune in input.toLowerCase().runes) {
    final ch =
        _diacriticFolds[String.fromCharCode(rune)] ?? String.fromCharCode(rune);
    final normalized = _asciiAlphanumeric.hasMatch(ch) ? ch : '_';
    if (normalized == '_' && previousWasUnderscore) {
      continue;
    }
    previousWasUnderscore = normalized == '_';
    buffer.write(normalized);
  }

  final trimmed = buffer.toString().replaceAll(RegExp(r'^_+|_+$'), '');
  if (trimmed.isEmpty) {
    return '';
  }
  return _leadingDigit.hasMatch(trimmed) ? '_$trimmed' : trimmed;
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
