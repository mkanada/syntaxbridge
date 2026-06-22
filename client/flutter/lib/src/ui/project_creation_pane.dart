import 'package:flutter/material.dart';

import '../project/project_models.dart';
import 'compilation_units_view.dart';

class ProjectCreationPane extends StatelessWidget {
  const ProjectCreationPane({
    super.key,
    this.showTitle = true,
    required this.nameController,
    required this.workspaceDirController,
    required this.archivePathController,
    required this.creating,
    required this.createError,
    required this.project,
    required this.canCreateProject,
    required this.onChanged,
    required this.onChooseWorkspaceDirectory,
    required this.onChooseSourceArchive,
    required this.onCreateProject,
    required this.errorMessage,
  });

  final bool showTitle;
  final TextEditingController nameController;
  final TextEditingController workspaceDirController;
  final TextEditingController archivePathController;
  final bool creating;
  final Object? createError;
  final CreatedProject? project;
  final bool canCreateProject;
  final VoidCallback onChanged;
  final Future<void> Function() onChooseWorkspaceDirectory;
  final Future<void> Function() onChooseSourceArchive;
  final Future<void> Function() onCreateProject;
  final String Function(Object error) errorMessage;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        if (showTitle) ...[
          Text(
            'Create project',
            style: Theme.of(context).textTheme.headlineSmall,
          ),
          const SizedBox(height: 16),
        ],
        TextField(
          controller: nameController,
          decoration: const InputDecoration(
            border: OutlineInputBorder(),
            labelText: 'Project name',
            prefixIcon: Icon(Icons.drive_file_rename_outline),
          ),
          onChanged: (_) => onChanged(),
        ),
        const SizedBox(height: 12),
        TextField(
          controller: workspaceDirController,
          decoration: InputDecoration(
            border: const OutlineInputBorder(),
            labelText: 'Workspace directory',
            prefixIcon: const Icon(Icons.folder_open),
            suffixIcon: IconButton(
              tooltip: 'Choose workspace directory',
              onPressed: () {
                onChooseWorkspaceDirectory();
              },
              icon: const Icon(Icons.more_horiz),
            ),
          ),
          onChanged: (_) => onChanged(),
        ),
        const SizedBox(height: 12),
        TextField(
          controller: archivePathController,
          decoration: InputDecoration(
            border: const OutlineInputBorder(),
            labelText: 'Source archive',
            prefixIcon: const Icon(Icons.archive),
            suffixIcon: IconButton(
              tooltip: 'Choose source archive',
              onPressed: () {
                onChooseSourceArchive();
              },
              icon: const Icon(Icons.upload_file),
            ),
          ),
          onChanged: (_) => onChanged(),
        ),
        const SizedBox(height: 16),
        FilledButton.icon(
          onPressed: canCreateProject
              ? () {
                  onCreateProject();
                }
              : null,
          icon: creating
              ? const SizedBox.square(
                  dimension: 18,
                  child: CircularProgressIndicator(strokeWidth: 2),
                )
              : const Icon(Icons.create_new_folder),
          label: Text(creating ? 'Creating' : 'Create project'),
        ),
        if (createError != null) ...[
          const SizedBox(height: 16),
          Text(
            'Project creation failed',
            style: Theme.of(
              context,
            ).textTheme.titleMedium?.copyWith(color: const Color(0xFFB3261E)),
          ),
          const SizedBox(height: 4),
          Text(errorMessage(createError!)),
        ],
        if (project != null) ...[
          const Divider(height: 40),
          CompilationUnitsView(project: project!),
        ],
      ],
    );
  }
}
