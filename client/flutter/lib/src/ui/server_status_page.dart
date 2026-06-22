import 'dart:io';

import 'package:flutter/material.dart';

import '../io/path_picker.dart';
import '../logging/cli_log.dart';
import '../project/project_creation_exception.dart';
import '../project/project_models.dart';
import '../server/server_client.dart';
import 'dockable_panel.dart';
import 'execution_log.dart';
import 'execution_log_view.dart';
import 'project_creation_pane.dart';
import 'server_connection_status.dart';

class ServerStatusPage extends StatefulWidget {
  const ServerStatusPage({
    super.key,
    required this.serverClient,
    required this.pathPicker,
  });

  final ServerClient serverClient;
  final PathPicker pathPicker;

  @override
  State<ServerStatusPage> createState() => _ServerStatusPageState();
}

class _ServerStatusPageState extends State<ServerStatusPage> {
  late Future<ServerStatus> _status;
  final _nameController = TextEditingController();
  final _workspaceDirController = TextEditingController();
  final _archivePathController = TextEditingController();
  final List<ExecutionLogEntry> _logs = [];
  final Set<_IdePanel> _openPanels = {_IdePanel.project, _IdePanel.log};
  final Map<_IdePanel, DockSide> _panelSides = {
    _IdePanel.project: DockSide.left,
    _IdePanel.log: DockSide.right,
  };
  bool _creating = false;
  CreatedProject? _project;
  Object? _createError;

  @override
  void initState() {
    super.initState();
    _status = _loadServerStatus(notify: false);
  }

  void _refresh() {
    setState(() {
      _status = _loadServerStatus();
    });
  }

  @override
  void dispose() {
    _nameController.dispose();
    _workspaceDirController.dispose();
    _archivePathController.dispose();
    super.dispose();
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
    _addLog("Creating project '${input.name}'");
    _addLog('Workspace directory: ${input.workspaceDir}');
    _addLog('Source archive: ${input.archivePath}');
    _addLog('Requesting project creation from server');

    try {
      final project = await widget.serverClient.createProject(input);

      if (!mounted) {
        return;
      }

      setState(() {
        _project = project;
      });
      _addLog(
        'Project created: ${project.projectDir}',
        level: ExecutionLogLevel.success,
      );
      _addLog(
        'Compilation units found: ${project.compilationUnits.length}',
        level: ExecutionLogLevel.success,
      );
      _addLog(
        'Build layers found: ${project.buildLayers.length}',
        level: ExecutionLogLevel.success,
      );
    } catch (error, stackTrace) {
      if (!mounted) {
        return;
      }

      setState(() {
        _createError = error;
      });
      cliLog('project creation exception: $error');
      cliLog('project creation stack: $stackTrace');
      _addLog(
        'Project creation failed: ${_errorMessage(error)}',
        level: ExecutionLogLevel.error,
      );
    } finally {
      if (mounted) {
        setState(() {
          _creating = false;
        });
      }
    }
  }

  Future<void> _chooseWorkspaceDirectory() async {
    _addLog('Opening workspace directory picker');
    final selected = await widget.pathPicker.pickWorkspaceDirectory();
    if (!mounted) {
      return;
    }

    if (selected == null) {
      _addLog(
        'Workspace directory selection cancelled',
        level: ExecutionLogLevel.warning,
      );
      return;
    }

    _setControllerText(_workspaceDirController, selected);
    _addLog(
      'Workspace directory selected: $selected',
      level: ExecutionLogLevel.success,
    );
  }

  Future<void> _chooseSourceArchive() async {
    _addLog('Opening source archive picker');
    final selected = await widget.pathPicker.pickSourceArchive();
    if (!mounted) {
      return;
    }

    if (selected == null) {
      _addLog(
        'Source archive selection cancelled',
        level: ExecutionLogLevel.warning,
      );
      return;
    }

    _setControllerText(_archivePathController, selected);
    _addLog(
      'Source archive selected: $selected',
      level: ExecutionLogLevel.success,
    );
  }

  void _setControllerText(TextEditingController controller, String value) {
    controller.value = TextEditingValue(
      text: value,
      selection: TextSelection.collapsed(offset: value.length),
    );
    setState(() {});
  }

  Future<ServerStatus> _loadServerStatus({bool notify = true}) async {
    _addLog('Checking server connection', notify: notify);

    try {
      final status = await widget.serverClient.health();
      _addLog(
        'Server connection ready: ${status.service}',
        level: ExecutionLogLevel.success,
      );
      return status;
    } catch (error, stackTrace) {
      cliLog('server connection exception: $error');
      cliLog('server connection stack: $stackTrace');
      _addLog(
        'Server connection failed: ${_errorMessage(error)}',
        level: ExecutionLogLevel.error,
      );
      rethrow;
    }
  }

  void _addLog(
    String message, {
    ExecutionLogLevel level = ExecutionLogLevel.info,
    bool notify = true,
  }) {
    final entry = ExecutionLogEntry(
      timestamp: DateTime.now(),
      level: level,
      message: message,
    );
    cliLog('execution_log level=${level.name} notify=$notify message=$message');

    if (!mounted || !notify) {
      _logs.add(entry);
      return;
    }

    setState(() {
      _logs.add(entry);
    });
  }

  String _errorMessage(Object error) {
    return switch (error) {
      ProjectCreationException(:final message) => message,
      HttpException(:final message) => message,
      _ => error.toString(),
    };
  }

  bool get _canCreateProject {
    return !_creating &&
        _nameController.text.trim().isNotEmpty &&
        _workspaceDirController.text.trim().isNotEmpty &&
        _archivePathController.text.trim().isNotEmpty;
  }

  void _openPanel(_IdePanel panel) {
    setState(() {
      _openPanels.add(panel);
    });
  }

  void _closePanel(_IdePanel panel) {
    setState(() {
      _openPanels.remove(panel);
    });
  }

  void _dockPanel(_IdePanel panel, DockSide side) {
    setState(() {
      _panelSides[panel] = side;
      _openPanels.add(panel);
    });
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Syntax Bridge')),
      body: LayoutBuilder(
        builder: (context, constraints) {
          final panels = _buildPanels();

          return SingleChildScrollView(
            padding: const EdgeInsets.all(24),
            child: Center(
              child: ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 1280),
                child: _DockedWorkspace(
                  compact: constraints.maxWidth < 980,
                  closedPanels: [
                    for (final panel in _IdePanel.values)
                      if (!_openPanels.contains(panel))
                        _ClosedPanelButton(
                          title: panel.title,
                          icon: panel.icon,
                          onPressed: () => _openPanel(panel),
                        ),
                  ],
                  topPanels: _panelsFor(
                    panels,
                    DockSide.top,
                    constraints.maxWidth,
                  ),
                  leftPanels: _panelsFor(
                    panels,
                    DockSide.left,
                    constraints.maxWidth,
                  ),
                  rightPanels: _panelsFor(
                    panels,
                    DockSide.right,
                    constraints.maxWidth,
                  ),
                  bottomPanels: _panelsFor(
                    panels,
                    DockSide.bottom,
                    constraints.maxWidth,
                  ),
                  child: _WorkspaceCenter(
                    status: _status,
                    project: _project,
                    onRefresh: _refresh,
                  ),
                ),
              ),
            ),
          );
        },
      ),
    );
  }

  Map<_IdePanel, Widget> _buildPanels() {
    return {
      _IdePanel.project: DockablePanel(
        title: _IdePanel.project.title,
        icon: _IdePanel.project.icon,
        side: _panelSides[_IdePanel.project] ?? DockSide.left,
        onClose: () => _closePanel(_IdePanel.project),
        onDockSide: (side) => _dockPanel(_IdePanel.project, side),
        child: ProjectCreationPane(
          showTitle: false,
          nameController: _nameController,
          workspaceDirController: _workspaceDirController,
          archivePathController: _archivePathController,
          creating: _creating,
          createError: _createError,
          project: _project,
          canCreateProject: _canCreateProject,
          onChanged: () => setState(() {}),
          onChooseWorkspaceDirectory: _chooseWorkspaceDirectory,
          onChooseSourceArchive: _chooseSourceArchive,
          onCreateProject: _createProject,
          errorMessage: _errorMessage,
        ),
      ),
      _IdePanel.log: DockablePanel(
        title: _IdePanel.log.title,
        icon: _IdePanel.log.icon,
        side: _panelSides[_IdePanel.log] ?? DockSide.right,
        onClose: () => _closePanel(_IdePanel.log),
        onDockSide: (side) => _dockPanel(_IdePanel.log, side),
        child: ExecutionLogView(entries: _logs, showTitle: false),
      ),
    };
  }

  List<Widget> _panelsFor(
    Map<_IdePanel, Widget> panels,
    DockSide side,
    double screenWidth,
  ) {
    final compact = screenWidth < 980;
    return [
      for (final panel in _IdePanel.values)
        if (_openPanels.contains(panel) &&
            (_panelSides[panel] ?? DockSide.left) == side)
          _ConstrainedDockPanel(
            side: compact ? DockSide.top : side,
            child: panels[panel]!,
          ),
    ];
  }
}

enum _IdePanel { project, log }

extension _IdePanelMetadata on _IdePanel {
  String get title {
    return switch (this) {
      _IdePanel.project => 'Project setup',
      _IdePanel.log => 'Execution log',
    };
  }

  IconData get icon {
    return switch (this) {
      _IdePanel.project => Icons.create_new_folder_outlined,
      _IdePanel.log => Icons.receipt_long_outlined,
    };
  }
}

class _DockedWorkspace extends StatelessWidget {
  const _DockedWorkspace({
    required this.compact,
    required this.closedPanels,
    required this.topPanels,
    required this.leftPanels,
    required this.rightPanels,
    required this.bottomPanels,
    required this.child,
  });

  final bool compact;
  final List<Widget> closedPanels;
  final List<Widget> topPanels;
  final List<Widget> leftPanels;
  final List<Widget> rightPanels;
  final List<Widget> bottomPanels;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    if (compact) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (closedPanels.isNotEmpty) ...[
            Wrap(spacing: 8, runSpacing: 8, children: closedPanels),
            const SizedBox(height: 16),
          ],
          if (topPanels.isNotEmpty) ...[
            _PanelColumn(stretch: true, children: topPanels),
            const SizedBox(height: 16),
          ],
          if (leftPanels.isNotEmpty) ...[
            _PanelColumn(stretch: true, children: leftPanels),
            const SizedBox(height: 16),
          ],
          if (rightPanels.isNotEmpty) ...[
            _PanelColumn(stretch: true, children: rightPanels),
            const SizedBox(height: 16),
          ],
          child,
          if (bottomPanels.isNotEmpty) ...[
            const SizedBox(height: 16),
            _PanelColumn(stretch: true, children: bottomPanels),
          ],
        ],
      );
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (closedPanels.isNotEmpty) ...[
          Wrap(spacing: 8, runSpacing: 8, children: closedPanels),
          const SizedBox(height: 16),
        ],
        if (topPanels.isNotEmpty) ...[
          _PanelColumn(stretch: true, children: topPanels),
          const SizedBox(height: 16),
        ],
        Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            if (leftPanels.isNotEmpty) ...[
              _PanelColumn(children: leftPanels),
              const SizedBox(width: 16),
            ],
            Expanded(child: child),
            if (rightPanels.isNotEmpty) ...[
              const SizedBox(width: 16),
              _PanelColumn(children: rightPanels),
            ],
          ],
        ),
        if (bottomPanels.isNotEmpty) ...[
          const SizedBox(height: 16),
          _PanelColumn(stretch: true, children: bottomPanels),
        ],
      ],
    );
  }
}

class _WorkspaceCenter extends StatelessWidget {
  const _WorkspaceCenter({
    required this.status,
    required this.project,
    required this.onRefresh,
  });

  final Future<ServerStatus> status;
  final CreatedProject? project;
  final VoidCallback onRefresh;

  @override
  Widget build(BuildContext context) {
    final textTheme = Theme.of(context).textTheme;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        ServerConnectionStatus(status: status, onRefresh: onRefresh),
        const Divider(height: 40),
        Text('Syntax Bridge workspace', style: textTheme.headlineSmall),
        const SizedBox(height: 8),
        Text(
          project == null ? 'No project loaded' : 'Project: ${project!.name}',
          style: textTheme.bodyLarge,
        ),
      ],
    );
  }
}

class _PanelColumn extends StatelessWidget {
  const _PanelColumn({this.stretch = false, required this.children});

  final bool stretch;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: stretch
          ? CrossAxisAlignment.stretch
          : CrossAxisAlignment.start,
      children: [
        for (final (index, child) in children.indexed) ...[
          if (index > 0) const SizedBox(height: 16),
          child,
        ],
      ],
    );
  }
}

class _ConstrainedDockPanel extends StatelessWidget {
  const _ConstrainedDockPanel({required this.side, required this.child});

  final DockSide side;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      width: switch (side) {
        DockSide.left || DockSide.right => 360,
        DockSide.top || DockSide.bottom => null,
      },
      child: child,
    );
  }
}

class _ClosedPanelButton extends StatelessWidget {
  const _ClosedPanelButton({
    required this.title,
    required this.icon,
    required this.onPressed,
  });

  final String title;
  final IconData icon;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    return OutlinedButton.icon(
      onPressed: onPressed,
      icon: Icon(icon),
      label: Text(title),
    );
  }
}
