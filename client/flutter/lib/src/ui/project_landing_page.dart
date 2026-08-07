import 'package:flutter/material.dart';

import '../io/path_picker.dart';
import '../project/project_error_message.dart';
import '../project/project_models.dart';
import '../server/server_client.dart';
import 'ide_theme.dart';
import 'import_project_dialog.dart';

/// The app's entry screen: lists the last 5 projects syntax-bridge was used
/// with (if any) and offers ways into a project other than starting from an
/// empty "Create project" form — reopening a recent one, or importing a
/// project that already exists on disk.
class ProjectLandingPage extends StatefulWidget {
  const ProjectLandingPage({
    super.key,
    required this.serverClient,
    required this.pathPicker,
    required this.onProjectReady,
    required this.onNewProject,
  });

  final ServerClient serverClient;
  final PathPicker pathPicker;

  /// Called with the loaded project once the user opens a recent project or
  /// imports one.
  final ValueChanged<CreatedProject> onProjectReady;

  /// Called when the user chooses to start a new project instead.
  final VoidCallback onNewProject;

  @override
  State<ProjectLandingPage> createState() => _ProjectLandingPageState();
}

class _ProjectLandingPageState extends State<ProjectLandingPage> {
  late Future<List<RecentProject>> _recentProjects;
  String? _openingProjectDir;
  Object? _openError;

  @override
  void initState() {
    super.initState();
    _recentProjects = widget.serverClient.listRecentProjects();
  }

  Future<void> _openRecent(RecentProject recent) async {
    setState(() {
      _openingProjectDir = recent.projectDir;
      _openError = null;
    });

    try {
      final project = await widget.serverClient.openProject(recent.projectDir);
      if (!mounted) {
        return;
      }

      widget.onProjectReady(project);
    } catch (error) {
      if (!mounted) {
        return;
      }

      setState(() {
        _openError = error;
        _openingProjectDir = null;
      });
    }
  }

  /// Drops a project the server reported as gone from the list. The directory
  /// itself is left alone: the user already deleted or moved it, and this only
  /// stops the app from offering something it cannot open.
  Future<void> _forget(RecentProject project) async {
    setState(() {
      _openError = null;
    });

    try {
      await widget.serverClient.forgetProject(project.projectDir);
      if (!mounted) {
        return;
      }

      setState(() {
        _recentProjects = widget.serverClient.listRecentProjects();
      });
    } catch (error) {
      if (!mounted) {
        return;
      }

      setState(() {
        _openError = error;
      });
    }
  }

  Future<void> _import() async {
    final project = await showDialog<CreatedProject>(
      context: context,
      barrierDismissible: false,
      builder: (context) => ImportProjectDialog(
        serverClient: widget.serverClient,
        pathPicker: widget.pathPicker,
      ),
    );

    if (project != null) {
      widget.onProjectReady(project);
    }
  }

  @override
  Widget build(BuildContext context) {
    final textTheme = Theme.of(context).textTheme;

    return Scaffold(
      backgroundColor: IdePalette.background,
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 520),
          child: SingleChildScrollView(
            padding: const EdgeInsets.all(24),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Row(
                  children: [
                    const Icon(
                      Icons.grid_view_rounded,
                      size: 26,
                      color: IdePalette.teal,
                    ),
                    const SizedBox(width: 10),
                    Text(
                      'Syntax Bridge',
                      style: textTheme.headlineMedium?.copyWith(
                        fontWeight: FontWeight.w700,
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 8),
                const Text(
                  'C/C++ to Dart transpilation IDE',
                  style: TextStyle(color: IdePalette.muted),
                ),
                const SizedBox(height: 32),
                Text('Recent projects', style: textTheme.titleMedium),
                const SizedBox(height: 12),
                FutureBuilder<List<RecentProject>>(
                  future: _recentProjects,
                  builder: (context, snapshot) {
                    if (snapshot.connectionState != ConnectionState.done) {
                      return const Padding(
                        padding: EdgeInsets.symmetric(vertical: 12),
                        child: Center(
                          child: SizedBox.square(
                            dimension: 20,
                            child: CircularProgressIndicator(strokeWidth: 2),
                          ),
                        ),
                      );
                    }

                    if (snapshot.hasError) {
                      return Text(
                        'Failed to load recent projects: ${projectErrorMessage(snapshot.error!)}',
                        style: const TextStyle(color: IdePalette.muted),
                      );
                    }

                    final projects = snapshot.data ?? const <RecentProject>[];
                    if (projects.isEmpty) {
                      return const Text(
                        'No recent projects yet',
                        style: TextStyle(color: IdePalette.muted),
                      );
                    }

                    return Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        for (final project in projects)
                          _RecentProjectTile(
                            project: project,
                            opening: _openingProjectDir == project.projectDir,
                            onTap:
                                project.available && _openingProjectDir == null
                                ? () => _openRecent(project)
                                : null,
                            onForget: project.available
                                ? null
                                : () => _forget(project),
                          ),
                      ],
                    );
                  },
                ),
                if (_openError != null) ...[
                  const SizedBox(height: 12),
                  Text(
                    'Failed to open project: ${projectErrorMessage(_openError!)}',
                    style: const TextStyle(color: Color(0xFFB3261E)),
                  ),
                ],
                const SizedBox(height: 28),
                Row(
                  children: [
                    Expanded(
                      child: OutlinedButton.icon(
                        onPressed: _import,
                        icon: const Icon(Icons.folder_open),
                        label: const Text('Import project'),
                      ),
                    ),
                    const SizedBox(width: 12),
                    Expanded(
                      child: FilledButton.icon(
                        onPressed: widget.onNewProject,
                        icon: const Icon(Icons.create_new_folder),
                        label: const Text('New project'),
                      ),
                    ),
                  ],
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _RecentProjectTile extends StatelessWidget {
  const _RecentProjectTile({
    required this.project,
    required this.opening,
    required this.onTap,
    required this.onForget,
  });

  final RecentProject project;
  final bool opening;
  final VoidCallback? onTap;

  /// Set only for projects whose directory is gone: offers to drop the entry
  /// instead of leaving a row that fails whenever it is clicked.
  final VoidCallback? onForget;

  @override
  Widget build(BuildContext context) {
    final missing = onForget != null;

    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: Material(
        color: IdePalette.panel,
        borderRadius: BorderRadius.circular(6),
        child: InkWell(
          borderRadius: BorderRadius.circular(6),
          onTap: onTap,
          child: Container(
            padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
            decoration: BoxDecoration(
              border: Border.all(color: IdePalette.border),
              borderRadius: BorderRadius.circular(6),
            ),
            child: Row(
              children: [
                Icon(
                  missing ? Icons.folder_off_rounded : Icons.folder_rounded,
                  color: missing ? IdePalette.muted : IdePalette.teal,
                  size: 20,
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        project.name,
                        style: TextStyle(
                          color: missing ? IdePalette.muted : IdePalette.text,
                          fontWeight: FontWeight.w600,
                        ),
                      ),
                      Text(
                        project.projectDir,
                        overflow: TextOverflow.ellipsis,
                        style: const TextStyle(
                          color: IdePalette.muted,
                          fontSize: 12,
                        ),
                      ),
                      if (missing)
                        const Text(
                          'Folder no longer exists',
                          style: TextStyle(
                            color: IdePalette.amber,
                            fontSize: 12,
                          ),
                        ),
                    ],
                  ),
                ),
                if (opening)
                  const SizedBox.square(
                    dimension: 16,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                else if (missing)
                  IconButton(
                    tooltip: 'Remove ${project.name} from the list',
                    onPressed: onForget,
                    icon: const Icon(
                      Icons.delete_outline_rounded,
                      color: IdePalette.muted,
                    ),
                  )
                else
                  const Icon(
                    Icons.chevron_right_rounded,
                    color: IdePalette.muted,
                  ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
