import 'package:flutter/material.dart';

import '../project/project_models.dart';
import 'ide_theme.dart';

class SourceFilesView extends StatelessWidget {
  const SourceFilesView({
    super.key,
    required this.project,
    required this.onFileSelected,
    this.selectedPath,
    this.onMarkFileExternal,
  });

  final CreatedProject project;
  final ValueChanged<SourceFile> onFileSelected;
  final String? selectedPath;

  /// Marks every type/function declared in a file external in one action
  /// (decision 3, `docs/plans/lista-de-externos.md`: expands to that file's
  /// usrs *now*, a one-time snapshot — not an ongoing "this file is
  /// external" state, so unlike [TypesView]/[FunctionsView]'s per-row
  /// control this isn't a toggle with an on/off icon). `null` hides the
  /// per-row action entirely.
  final ValueChanged<SourceFile>? onMarkFileExternal;

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

              return ListTile(
                contentPadding: EdgeInsets.zero,
                selected: file.path == selectedPath,
                leading: Icon(
                  file.kind == SourceFileKind.translationUnit
                      ? Icons.description
                      : Icons.article_outlined,
                ),
                title: Text(relativePath),
                trailing: onMarkFileExternal == null
                    ? null
                    : IconButton(
                        iconSize: 16,
                        visualDensity: VisualDensity.compact,
                        tooltip: 'Marcar tudo neste arquivo como externo',
                        color: IdePalette.muted,
                        icon: const Icon(Icons.link),
                        onPressed: () => onMarkFileExternal!(file),
                      ),
                onTap: () => onFileSelected(file),
              );
            },
          ),
        ),
      ],
    );
  }

  String _projectRelativeFile(String file) {
    final normalizedProjectDir = _stripTrailingSlash(project.projectDir);
    if (normalizedProjectDir.isEmpty) {
      return file;
    }

    final prefix = '$normalizedProjectDir/';
    if (file.startsWith(prefix)) {
      return file.substring(prefix.length);
    }

    return file;
  }

  String _stripTrailingSlash(String path) {
    var result = path;
    while (result.endsWith('/') && result.length > 1) {
      result = result.substring(0, result.length - 1);
    }
    return result;
  }
}
