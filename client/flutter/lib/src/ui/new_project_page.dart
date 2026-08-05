import 'package:flutter/material.dart';

import '../io/path_picker.dart';
import '../project/project_error_message.dart';
import '../project/project_models.dart';
import '../server/server_client.dart';
import 'ide_theme.dart';
import 'project_creation_pane.dart';

/// Full-screen "create a new project" step shown before the IDE loads, so
/// there is nothing else to interact with until a project exists.
class NewProjectPage extends StatefulWidget {
  const NewProjectPage({
    super.key,
    required this.serverClient,
    required this.pathPicker,
    required this.onProjectCreated,
    required this.onCancel,
  });

  final ServerClient serverClient;
  final PathPicker pathPicker;
  final ValueChanged<CreatedProject> onProjectCreated;
  final VoidCallback onCancel;

  @override
  State<NewProjectPage> createState() => _NewProjectPageState();
}

class _NewProjectPageState extends State<NewProjectPage> {
  final _nameController = TextEditingController();
  final _workspaceDirController = TextEditingController();
  final _archivePathController = TextEditingController();
  bool _creating = false;
  Object? _createError;

  @override
  void dispose() {
    _nameController.dispose();
    _workspaceDirController.dispose();
    _archivePathController.dispose();
    super.dispose();
  }

  bool get _canCreateProject {
    return !_creating &&
        _nameController.text.trim().isNotEmpty &&
        _workspaceDirController.text.trim().isNotEmpty &&
        _archivePathController.text.trim().isNotEmpty;
  }

  Future<void> _chooseWorkspaceDirectory() async {
    final selected = await widget.pathPicker.pickWorkspaceDirectory();
    if (!mounted || selected == null) {
      return;
    }

    _setControllerText(_workspaceDirController, selected);
  }

  Future<void> _chooseSourceArchive() async {
    final selected = await widget.pathPicker.pickSourceArchive();
    if (!mounted || selected == null) {
      return;
    }

    _setControllerText(_archivePathController, selected);
  }

  void _setControllerText(TextEditingController controller, String value) {
    controller.value = TextEditingValue(
      text: value,
      selection: TextSelection.collapsed(offset: value.length),
    );
    setState(() {});
  }

  Future<void> _createProject() async {
    final input = CreateProjectInput(
      name: _nameController.text.trim(),
      workspaceDir: _workspaceDirController.text.trim(),
      archivePath: _archivePathController.text.trim(),
    );

    setState(() {
      _creating = true;
      _createError = null;
    });

    try {
      final project = await widget.serverClient.createProject(input);
      if (!mounted) {
        return;
      }

      widget.onProjectCreated(project);
    } catch (error) {
      if (!mounted) {
        return;
      }

      setState(() {
        _createError = error;
      });
    } finally {
      if (mounted) {
        setState(() {
          _creating = false;
        });
      }
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
                    IconButton(
                      onPressed: widget.onCancel,
                      tooltip: 'Back',
                      icon: const Icon(Icons.arrow_back),
                    ),
                    const SizedBox(width: 4),
                    Text(
                      'New project',
                      style: textTheme.headlineMedium?.copyWith(
                        fontWeight: FontWeight.w700,
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 24),
                ProjectCreationPane(
                  showTitle: false,
                  nameController: _nameController,
                  workspaceDirController: _workspaceDirController,
                  archivePathController: _archivePathController,
                  creating: _creating,
                  createError: _createError,
                  canCreateProject: _canCreateProject,
                  onChanged: () => setState(() {}),
                  onChooseWorkspaceDirectory: _chooseWorkspaceDirectory,
                  onChooseSourceArchive: _chooseSourceArchive,
                  onCreateProject: _createProject,
                  errorMessage: projectErrorMessage,
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
