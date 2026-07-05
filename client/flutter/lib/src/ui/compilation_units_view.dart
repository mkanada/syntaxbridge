import 'package:flutter/material.dart';

import '../project/project_models.dart';

class CompilationUnitsView extends StatelessWidget {
  const CompilationUnitsView({super.key, required this.project});

  final CreatedProject project;

  @override
  Widget build(BuildContext context) {
    final units = project.compilationUnits;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          'Compilation units',
          style: Theme.of(context).textTheme.headlineSmall,
        ),
        const SizedBox(height: 4),
        Text(project.projectDir, style: Theme.of(context).textTheme.bodyMedium),
        const SizedBox(height: 16),
        ListView.separated(
          shrinkWrap: true,
          physics: const NeverScrollableScrollPhysics(),
          itemCount: units.length,
          separatorBuilder: (context, index) => const Divider(height: 1),
          itemBuilder: (context, index) {
            final unit = units[index];
            final file = _projectRelativeFile(unit.file);

            return ListTile(
              contentPadding: EdgeInsets.zero,
              leading: const Icon(Icons.description),
              title: Text(file),
            );
          },
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
