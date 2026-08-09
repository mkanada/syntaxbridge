import 'package:flutter/material.dart';

class ProjectCreationPane extends StatelessWidget {
  const ProjectCreationPane({
    super.key,
    this.showTitle = true,
    required this.nameController,
    required this.workspaceDirController,
    required this.archivePathController,
    required this.canCreateProject,
    required this.onChanged,
    required this.onChooseWorkspaceDirectory,
    required this.onChooseSourceArchive,
    required this.onCreateProject,
  });

  final bool showTitle;
  final TextEditingController nameController;
  final TextEditingController workspaceDirController;
  final TextEditingController archivePathController;
  final bool canCreateProject;
  final VoidCallback onChanged;
  final Future<void> Function() onChooseWorkspaceDirectory;
  final Future<void> Function() onChooseSourceArchive;
  final VoidCallback onCreateProject;

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
          onPressed: canCreateProject ? onCreateProject : null,
          icon: const Icon(Icons.create_new_folder),
          label: const Text('Create project'),
        ),
      ],
    );
  }
}
