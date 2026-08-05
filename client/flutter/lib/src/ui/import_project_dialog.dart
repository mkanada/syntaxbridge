import 'package:flutter/material.dart';

import '../io/path_picker.dart';
import '../project/project_error_message.dart';
import '../server/server_client.dart';
import 'ide_theme.dart';

/// A non-dismissible modal that imports an existing syntax-bridge project
/// from an arbitrary directory on disk: interacting with the rest of the app
/// before an import completes or is cancelled doesn't make sense, since
/// nothing has been imported yet.
class ImportProjectDialog extends StatefulWidget {
  const ImportProjectDialog({
    super.key,
    required this.serverClient,
    required this.pathPicker,
  });

  final ServerClient serverClient;
  final PathPicker pathPicker;

  @override
  State<ImportProjectDialog> createState() => _ImportProjectDialogState();
}

class _ImportProjectDialogState extends State<ImportProjectDialog> {
  final _directoryController = TextEditingController();
  bool _importing = false;
  Object? _error;

  @override
  void dispose() {
    _directoryController.dispose();
    super.dispose();
  }

  Future<void> _chooseDirectory() async {
    final selected = await widget.pathPicker.pickExistingProjectDirectory();
    if (selected == null) {
      return;
    }

    setState(() {
      _directoryController.text = selected;
    });
  }

  Future<void> _import() async {
    final projectDir = _directoryController.text.trim();

    setState(() {
      _importing = true;
      _error = null;
    });

    try {
      final project = await widget.serverClient.openProject(projectDir);
      if (!mounted) {
        return;
      }

      Navigator.of(context).pop(project);
    } catch (error) {
      if (!mounted) {
        return;
      }

      setState(() {
        _error = error;
        _importing = false;
      });
    }
  }

  bool get _canImport => !_importing && _directoryController.text.trim().isNotEmpty;

  @override
  Widget build(BuildContext context) {
    return PopScope(
      canPop: false,
      child: Dialog(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 480),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(
                  'Import project',
                  style: Theme.of(context).textTheme.headlineSmall,
                ),
                const SizedBox(height: 8),
                const Text(
                  'Point to a directory that already contains a syntax-bridge project.',
                  style: TextStyle(color: IdePalette.softText),
                ),
                const SizedBox(height: 16),
                TextField(
                  controller: _directoryController,
                  autofocus: true,
                  enabled: !_importing,
                  decoration: InputDecoration(
                    border: const OutlineInputBorder(),
                    labelText: 'Project directory',
                    prefixIcon: const Icon(Icons.folder_open),
                    suffixIcon: IconButton(
                      tooltip: 'Choose project directory',
                      onPressed: _importing ? null : _chooseDirectory,
                      icon: const Icon(Icons.more_horiz),
                    ),
                  ),
                  onChanged: (_) => setState(() {}),
                ),
                if (_error != null) ...[
                  const SizedBox(height: 12),
                  Text(
                    'Import failed: ${projectErrorMessage(_error!)}',
                    style: const TextStyle(color: Color(0xFFB3261E)),
                  ),
                ],
                const SizedBox(height: 20),
                Row(
                  mainAxisAlignment: MainAxisAlignment.end,
                  children: [
                    TextButton(
                      onPressed: _importing
                          ? null
                          : () => Navigator.of(context).pop(),
                      child: const Text('Cancel'),
                    ),
                    const SizedBox(width: 8),
                    FilledButton.icon(
                      onPressed: _canImport ? _import : null,
                      icon: _importing
                          ? const SizedBox.square(
                              dimension: 18,
                              child: CircularProgressIndicator(strokeWidth: 2),
                            )
                          : const Icon(Icons.download_outlined),
                      label: Text(_importing ? 'Importing' : 'Import'),
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
