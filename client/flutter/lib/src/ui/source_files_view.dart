import 'package:flutter/material.dart';

import '../project/project_models.dart';
import 'external_toggle_button.dart';

class SourceFilesView extends StatelessWidget {
  const SourceFilesView({
    super.key,
    required this.project,
    required this.onFileSelected,
    this.selectedPath,
    this.externalFiles = const <String>{},
    this.onToggleFileExternal,
  });

  final CreatedProject project;
  final ValueChanged<SourceFile> onFileSelected;
  final String? selectedPath;

  /// Every file currently holding a persistent external mark (item 3,
  /// `docs/prompts/2026-08-19-mudanca-interacao.md` — a reversal of decision
  /// 3's cascade-snapshot behavior in `docs/plans/lista-de-externos.md`),
  /// keyed by [SourceFile.path]. Drives each row's toggle icon state.
  final Set<String> externalFiles;

  /// Toggles a whole file's persistent external mark. `null` hides the
  /// per-row control entirely.
  final ValueChanged<SourceFile>? onToggleFileExternal;

  @override
  Widget build(BuildContext context) {
    final files = project.sourceFiles;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text('Source files', style: Theme.of(context).textTheme.headlineSmall),
        const SizedBox(height: 4),
        Text(project.projectDir, style: Theme.of(context).textTheme.bodyMedium),
        const SizedBox(height: 16),
        Expanded(
          child: ListView.separated(
            itemCount: files.length,
            separatorBuilder: (context, index) => const Divider(height: 1),
            itemBuilder: (context, index) {
              final file = files[index];
              final relativePath = _projectRelativeFile(file.path);
              final isExternal = externalFiles.contains(file.path);

              return ListTile(
                contentPadding: EdgeInsets.zero,
                selected: file.path == selectedPath,
                leading: Icon(
                  file.kind == SourceFileKind.translationUnit
                      ? Icons.description
                      : Icons.article_outlined,
                ),
                title: Text(relativePath),
                trailing: onToggleFileExternal == null
                    ? null
                    : ExternalToggleButton(
                        isExternal: isExternal,
                        onPressed: () => onToggleFileExternal!(file),
                      ),
                onTap: () => onFileSelected(file),
              );
            },
          ),
        ),
      ],
    );
  }

  /// Displays [file] relative to the C++ project itself: strips
  /// `project.inputSourceDir` (where the uploaded archive was unpacked) and
  /// the archive's own root folder, since neither is meaningful to the user.
  /// A file sitting directly under `input-source/`, with no archive-root
  /// folder above it, is left as-is past that prefix.
  String _projectRelativeFile(String file) {
    final normalizedInputSourceDir = _stripTrailingSlash(
      project.inputSourceDir,
    );
    if (normalizedInputSourceDir.isEmpty) {
      return file;
    }

    final prefix = '$normalizedInputSourceDir/';
    if (!file.startsWith(prefix)) {
      return file;
    }

    final rest = file.substring(prefix.length);
    final archiveRootEnd = rest.indexOf('/');
    if (archiveRootEnd == -1) {
      return rest;
    }

    return rest.substring(archiveRootEnd + 1);
  }

  String _stripTrailingSlash(String path) {
    var result = path;
    while (result.endsWith('/') && result.length > 1) {
      result = result.substring(0, result.length - 1);
    }
    return result;
  }
}
