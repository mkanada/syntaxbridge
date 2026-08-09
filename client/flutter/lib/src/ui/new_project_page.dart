import 'package:flutter/material.dart';

import '../io/path_picker.dart';
import '../project/project_models.dart';
import 'ide_theme.dart';
import 'project_creation_pane.dart';

/// Full-screen "create a new project" step shown before the IDE loads, so
/// there is nothing else to interact with until a project exists.
///
/// This page only collects and validates the form's parameters — submitting
/// hands the input off via [onSubmit] rather than calling the server itself,
/// so the caller can move to a progress screen immediately (`assim que os
/// parâmetros... estão válidos, ir para a próxima tela`) instead of leaving
/// the user staring at a blocked button while the server ingests the
/// archive and runs `libclang` extraction, which can take minutes for a
/// real project.
class NewProjectPage extends StatefulWidget {
  const NewProjectPage({
    super.key,
    required this.pathPicker,
    required this.onSubmit,
    required this.onCancel,
  });

  final PathPicker pathPicker;
  final ValueChanged<CreateProjectInput> onSubmit;
  final VoidCallback onCancel;

  @override
  State<NewProjectPage> createState() => _NewProjectPageState();
}

class _NewProjectPageState extends State<NewProjectPage> {
  final _nameController = TextEditingController();
  final _workspaceDirController = TextEditingController();
  final _archivePathController = TextEditingController();

  @override
  void dispose() {
    _nameController.dispose();
    _workspaceDirController.dispose();
    _archivePathController.dispose();
    super.dispose();
  }

  bool get _canCreateProject {
    return _nameController.text.trim().isNotEmpty &&
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

  void _submit() {
    widget.onSubmit(
      CreateProjectInput(
        name: _nameController.text.trim(),
        workspaceDir: _workspaceDirController.text.trim(),
        archivePath: _archivePathController.text.trim(),
      ),
    );
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
                  canCreateProject: _canCreateProject,
                  onChanged: () => setState(() {}),
                  onChooseWorkspaceDirectory: _chooseWorkspaceDirectory,
                  onChooseSourceArchive: _chooseSourceArchive,
                  onCreateProject: _submit,
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
