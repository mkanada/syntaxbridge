import 'package:flutter/material.dart';

import '../io/screenshot_storage.dart';
import '../logging/cli_log.dart';
import '../project/project_error_message.dart';
import '../project/project_models.dart';
import '../server/server_client.dart';
import 'dockable_panel.dart';
import 'execution_log.dart';
import 'execution_log_view.dart';
import 'ide_theme.dart';
import 'panel_descriptor.dart';
import 'panel_group.dart';
import 'screen_capturer.dart';
import 'server_connection_status.dart';
import 'source_file_viewer.dart';
import 'source_files_view.dart';
import 'types_view.dart';

class ServerStatusPage extends StatefulWidget {
  const ServerStatusPage({
    super.key,
    required this.serverClient,
    required this.screenshotStorage,
    required this.screenCapturer,
    required this.project,
  });

  final ServerClient serverClient;
  final ScreenshotStorage screenshotStorage;
  final ScreenCapturer screenCapturer;
  final CreatedProject project;

  @override
  State<ServerStatusPage> createState() => _ServerStatusPageState();
}

class _ServerStatusPageState extends State<ServerStatusPage> {
  late Future<ServerStatus> _status;
  final List<ExecutionLogEntry> _logs = [];
  final Set<String> _openPanels = {'explorer', 'types', 'log'};
  final Map<String, DockSide> _panelSides = {
    'explorer': DockSide.left,
    'types': DockSide.left,
    'log': DockSide.right,
  };
  final _screenshotBoundaryKey = GlobalKey();
  SourceFile? _selectedSourceFile;
  Future<String>? _selectedSourceContent;
  TypeDeclaration? _selectedType;
  int? _highlightStartLine;
  int? _highlightEndLine;
  late Future<List<TypeDeclaration>> _types;

  @override
  void initState() {
    super.initState();
    _status = _loadServerStatus(notify: false);
    _types = _loadTypes();
  }

  void _refresh() {
    setState(() {
      _status = _loadServerStatus();
    });
  }

  Future<void> _captureScreen() async {
    try {
      final pixelRatio = MediaQuery.of(context).devicePixelRatio;
      final pngBytes = await widget.screenCapturer.capture(
        _screenshotBoundaryKey,
        pixelRatio: pixelRatio,
      );
      final path = await widget.screenshotStorage.save(pngBytes);
      if (!mounted) {
        return;
      }

      _addLog('Screenshot saved: $path', level: ExecutionLogLevel.success);
    } catch (error, stackTrace) {
      if (!mounted) {
        return;
      }

      cliLog('screenshot capture exception: $error');
      cliLog('screenshot capture stack: $stackTrace');
      _addLog(
        'Screenshot capture failed: ${projectErrorMessage(error)}',
        level: ExecutionLogLevel.error,
      );
    }
  }

  void _selectSourceFile(
    SourceFile file, {
    TypeDeclaration? selectedType,
    int? highlightStartLine,
    int? highlightEndLine,
  }) {
    _addLog('Opening ${file.path}');
    setState(() {
      _selectedSourceFile = file;
      _selectedSourceContent = _loadSourceFileContent(
        widget.project.projectDir,
        file,
      );
      _selectedType = selectedType;
      _highlightStartLine = highlightStartLine;
      _highlightEndLine = highlightEndLine;
    });
  }

  /// Opens the file where [type] is declared and highlights its body, so
  /// clicking a type in the Types navigator jumps straight to its
  /// definition instead of just naming it.
  void _selectType(TypeDeclaration type) {
    final sourceFile = widget.project.sourceFiles.firstWhere(
      (file) => file.path == type.file,
      orElse: () => SourceFile(path: type.file, kind: SourceFileKind.header),
    );

    _selectSourceFile(
      sourceFile,
      selectedType: type,
      highlightStartLine: type.line,
      highlightEndLine: type.endLine > 0 ? type.endLine : type.line,
    );
  }

  Future<String> _loadSourceFileContent(
    String projectDir,
    SourceFile file,
  ) async {
    try {
      final content = await widget.serverClient.readSourceFile(
        projectDir: projectDir,
        path: file.path,
      );
      _addLog('Loaded ${file.path}', level: ExecutionLogLevel.success);
      return content;
    } catch (error, stackTrace) {
      cliLog('read source file exception: $error');
      cliLog('read source file stack: $stackTrace');
      _addLog(
        'Failed to load ${file.path}: ${projectErrorMessage(error)}',
        level: ExecutionLogLevel.error,
      );
      rethrow;
    }
  }

  Future<List<TypeDeclaration>> _loadTypes() async {
    try {
      final types = await widget.serverClient.listTypes(
        widget.project.projectDir,
      );
      _addLog(
        'Loaded ${types.length} types',
        level: ExecutionLogLevel.success,
        notify: false,
      );
      return types;
    } catch (error, stackTrace) {
      cliLog('list types exception: $error');
      cliLog('list types stack: $stackTrace');
      _addLog(
        'Failed to load types: ${projectErrorMessage(error)}',
        level: ExecutionLogLevel.error,
        notify: false,
      );
      rethrow;
    }
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
        'Server connection failed: ${projectErrorMessage(error)}',
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

  void _openPanel(String id) {
    setState(() {
      _openPanels.add(id);
    });
  }

  void _closePanel(String id) {
    setState(() {
      _openPanels.remove(id);
    });
  }

  void _dockPanel(String id, DockSide side) {
    setState(() {
      _panelSides[id] = side;
      _openPanels.add(id);
    });
  }

  @override
  Widget build(BuildContext context) {
    return RepaintBoundary(
      key: _screenshotBoundaryKey,
      child: Scaffold(
        body: Column(
          children: [
            _TitleBar(onRefresh: _refresh, onCaptureScreen: _captureScreen),
            Expanded(
              child: LayoutBuilder(
                builder: (context, constraints) {
                  final descriptors = _panelDescriptors();

                  return ColoredBox(
                    color: IdePalette.background,
                    child: _DockedWorkspace(
                      compact: constraints.maxWidth < 980,
                      closedPanels: [
                        for (final descriptor in descriptors)
                          if (!_openPanels.contains(descriptor.id))
                            _ClosedPanelButton(
                              title: descriptor.title,
                              icon: descriptor.icon,
                              onPressed: () => _openPanel(descriptor.id),
                            ),
                      ],
                      topPanels: _panelsFor(
                        context,
                        descriptors,
                        DockSide.top,
                        constraints.maxWidth,
                      ),
                      leftPanels: _panelsFor(
                        context,
                        descriptors,
                        DockSide.left,
                        constraints.maxWidth,
                      ),
                      rightPanels: _panelsFor(
                        context,
                        descriptors,
                        DockSide.right,
                        constraints.maxWidth,
                      ),
                      bottomPanels: _panelsFor(
                        context,
                        descriptors,
                        DockSide.bottom,
                        constraints.maxWidth,
                      ),
                      child: _WorkspaceCenter(
                        status: _status,
                        project: widget.project,
                        onRefresh: _refresh,
                        selectedFile: _selectedSourceFile,
                        selectedFileContent: _selectedSourceContent,
                        highlightStartLine: _highlightStartLine,
                        highlightEndLine: _highlightEndLine,
                      ),
                    ),
                  );
                },
              ),
            ),
          ],
        ),
      ),
    );
  }

  /// The registry of panels the workspace can show. Adding a panel means
  /// adding one entry here — nothing else in this class or in
  /// [_DockedWorkspace] names a specific panel.
  List<PanelDescriptor> _panelDescriptors() {
    return [
      PanelDescriptor(
        id: 'explorer',
        title: 'Explorer',
        icon: Icons.folder_open_outlined,
        defaultSide: DockSide.left,
        builder: (context) => SourceFilesView(
          project: widget.project,
          onFileSelected: _selectSourceFile,
          selectedPath: _selectedSourceFile?.path,
        ),
      ),
      PanelDescriptor(
        id: 'types',
        title: 'Types',
        icon: Icons.data_object,
        defaultSide: DockSide.left,
        builder: (context) => FutureBuilder<List<TypeDeclaration>>(
          future: _types,
          builder: (context, snapshot) {
            if (snapshot.connectionState != ConnectionState.done) {
              return const Center(
                child: Padding(
                  padding: EdgeInsets.all(16),
                  child: CircularProgressIndicator(),
                ),
              );
            }

            final error = snapshot.error;
            if (error != null) {
              return Padding(
                padding: const EdgeInsets.all(16),
                child: Text(
                  'Failed to load types: ${projectErrorMessage(error)}',
                  style: const TextStyle(color: IdePalette.red),
                ),
              );
            }

            return TypesView(
              types: snapshot.data ?? const [],
              onTypeSelected: _selectType,
              selectedType: _selectedType,
            );
          },
        ),
      ),
      PanelDescriptor(
        id: 'log',
        title: 'Execution log',
        icon: Icons.receipt_long_outlined,
        defaultSide: DockSide.right,
        builder: (context) =>
            ExecutionLogView(entries: _logs, showTitle: false),
      ),
    ];
  }

  /// The open panels docked to [side]. A single one renders as a standalone
  /// [DockablePanel]; two or more share a [TabbedPanelGroup] so each keeps
  /// full height instead of splitting it.
  List<Widget> _panelsFor(
    BuildContext context,
    List<PanelDescriptor> descriptors,
    DockSide side,
    double screenWidth,
  ) {
    final openOnSide = [
      for (final descriptor in descriptors)
        if (_openPanels.contains(descriptor.id) &&
            (_panelSides[descriptor.id] ?? descriptor.defaultSide) == side)
          descriptor,
    ];

    if (openOnSide.isEmpty) {
      return const [];
    }

    final compact = screenWidth < 980;
    final displaySide = compact ? DockSide.top : side;

    if (openOnSide.length == 1) {
      final descriptor = openOnSide.single;
      return [
        _ConstrainedDockPanel(
          side: displaySide,
          child: DockablePanel(
            title: descriptor.title,
            icon: descriptor.icon,
            side: _panelSides[descriptor.id] ?? descriptor.defaultSide,
            onClose: () => _closePanel(descriptor.id),
            onDockSide: (newSide) => _dockPanel(descriptor.id, newSide),
            child: descriptor.builder(context),
          ),
        ),
      ];
    }

    return [
      _ConstrainedDockPanel(
        side: displaySide,
        child: TabbedPanelGroup(
          tabs: [
            for (final descriptor in openOnSide)
              PanelTab(
                title: descriptor.title,
                icon: descriptor.icon,
                side: _panelSides[descriptor.id] ?? descriptor.defaultSide,
                onClose: () => _closePanel(descriptor.id),
                onDockSide: (newSide) => _dockPanel(descriptor.id, newSide),
                child: descriptor.builder(context),
              ),
          ],
        ),
      ),
    ];
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
      return SingleChildScrollView(
        padding: const EdgeInsets.all(12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (closedPanels.isNotEmpty) ...[
              _ClosedPanelsBar(children: closedPanels),
              const SizedBox(height: 12),
            ],
            if (topPanels.isNotEmpty) ...[
              _PanelColumn(stretch: true, children: topPanels),
              const SizedBox(height: 12),
            ],
            if (leftPanels.isNotEmpty) ...[
              _PanelColumn(stretch: true, children: leftPanels),
              const SizedBox(height: 12),
            ],
            if (rightPanels.isNotEmpty) ...[
              _PanelColumn(stretch: true, children: rightPanels),
              const SizedBox(height: 12),
            ],
            child,
            if (bottomPanels.isNotEmpty) ...[
              const SizedBox(height: 12),
              _PanelColumn(stretch: true, children: bottomPanels),
            ],
          ],
        ),
      );
    }

    return Padding(
      padding: const EdgeInsets.all(8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (closedPanels.isNotEmpty) ...[
            _ClosedPanelsBar(children: closedPanels),
            const SizedBox(height: 8),
          ],
          if (topPanels.isNotEmpty) ...[
            SizedBox(
              height: 180,
              child: _PanelColumn(stretch: true, children: topPanels),
            ),
            const SizedBox(height: 8),
          ],
          Expanded(
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                if (leftPanels.isNotEmpty) ...[
                  SizedBox(
                    width: 360,
                    child: _PanelColumn(stretch: true, children: leftPanels),
                  ),
                  const SizedBox(width: 8),
                ],
                Expanded(child: child),
                if (rightPanels.isNotEmpty) ...[
                  const SizedBox(width: 8),
                  SizedBox(
                    width: 360,
                    child: _PanelColumn(stretch: true, children: rightPanels),
                  ),
                ],
              ],
            ),
          ),
          if (bottomPanels.isNotEmpty) ...[
            const SizedBox(height: 8),
            SizedBox(
              height: 230,
              child: _PanelColumn(stretch: true, children: bottomPanels),
            ),
          ],
        ],
      ),
    );
  }
}

class _ClosedPanelsBar extends StatelessWidget {
  const _ClosedPanelsBar({required this.children});

  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(8),
      decoration: BoxDecoration(
        color: IdePalette.tabBar,
        border: Border.all(color: IdePalette.border),
        borderRadius: BorderRadius.circular(6),
      ),
      child: Wrap(spacing: 8, runSpacing: 8, children: children),
    );
  }
}

class _TitleBar extends StatelessWidget {
  const _TitleBar({required this.onRefresh, required this.onCaptureScreen});

  final VoidCallback onRefresh;
  final VoidCallback onCaptureScreen;

  @override
  Widget build(BuildContext context) {
    return Container(
      height: 46,
      decoration: const BoxDecoration(
        color: IdePalette.topBar,
        border: Border(bottom: BorderSide(color: IdePalette.border)),
      ),
      child: Row(
        children: [
          const SizedBox(width: 12),
          const Icon(Icons.grid_view_rounded, size: 18, color: IdePalette.teal),
          const SizedBox(width: 10),
          const Expanded(
            child: Text(
              'Syntax Bridge',
              overflow: TextOverflow.ellipsis,
              style: TextStyle(fontWeight: FontWeight.w700),
            ),
          ),
          IdeToolbarIcon(
            icon: Icons.refresh_rounded,
            tooltip: 'Refresh',
            onPressed: onRefresh,
          ),
          IdeToolbarIcon(
            icon: Icons.screenshot_monitor_rounded,
            tooltip: 'Capture screen',
            onPressed: onCaptureScreen,
          ),
          const SizedBox(width: 8),
        ],
      ),
    );
  }
}

class _WorkspaceCenter extends StatelessWidget {
  const _WorkspaceCenter({
    required this.status,
    required this.project,
    required this.onRefresh,
    this.selectedFile,
    this.selectedFileContent,
    this.highlightStartLine,
    this.highlightEndLine,
  });

  final Future<ServerStatus> status;
  final CreatedProject project;
  final VoidCallback onRefresh;
  final SourceFile? selectedFile;
  final Future<String>? selectedFileContent;
  final int? highlightStartLine;
  final int? highlightEndLine;

  @override
  Widget build(BuildContext context) {
    final textTheme = Theme.of(context).textTheme;
    final selectedFile = this.selectedFile;
    final selectedFileContent = this.selectedFileContent;

    return LayoutBuilder(
      builder: (context, constraints) {
        final body = selectedFile != null && selectedFileContent != null
            ? SourceFileViewer(
                path: selectedFile.path,
                content: selectedFileContent,
                highlightStartLine: highlightStartLine,
                highlightEndLine: highlightEndLine,
              )
            : SingleChildScrollView(
                padding: const EdgeInsets.all(18),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    ServerConnectionStatus(
                      status: status,
                      onRefresh: onRefresh,
                    ),
                    const Divider(height: 40, color: IdePalette.border),
                    Text(
                      'Syntax Bridge workspace',
                      style: textTheme.headlineSmall,
                    ),
                    const SizedBox(height: 8),
                    Text(
                      'Project: ${project.name}',
                      style: textTheme.bodyLarge?.copyWith(
                        color: IdePalette.softText,
                      ),
                    ),
                  ],
                ),
              );

        return DecoratedBox(
          decoration: BoxDecoration(
            color: IdePalette.editor,
            border: Border.all(color: IdePalette.border),
            borderRadius: BorderRadius.circular(6),
          ),
          child: Column(
            mainAxisSize: constraints.maxHeight.isFinite
                ? MainAxisSize.max
                : MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              // The tab strip names the file actually open; with nothing open
              // there is no tab to show.
              if (selectedFile != null)
                Container(
                  height: 40,
                  padding: const EdgeInsets.symmetric(horizontal: 14),
                  decoration: const BoxDecoration(
                    color: IdePalette.tabBar,
                    border: Border(
                      bottom: BorderSide(color: IdePalette.border),
                    ),
                  ),
                  child: Row(
                    children: [
                      const Icon(
                        Icons.code_rounded,
                        color: IdePalette.teal,
                        size: 17,
                      ),
                      const SizedBox(width: 8),
                      Expanded(
                        child: Text(
                          selectedFile.path.split('/').last,
                          overflow: TextOverflow.ellipsis,
                          style: const TextStyle(
                            color: IdePalette.softText,
                            fontSize: 13,
                            fontWeight: FontWeight.w700,
                          ),
                        ),
                      ),
                    ],
                  ),
                ),
              if (constraints.maxHeight.isFinite)
                Expanded(child: body)
              else
                SizedBox(height: 480, child: body),
            ],
          ),
        );
      },
    );
  }
}

class _PanelColumn extends StatelessWidget {
  const _PanelColumn({this.stretch = false, required this.children});

  final bool stretch;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final canExpand = stretch && constraints.maxHeight.isFinite;

        return Column(
          crossAxisAlignment: stretch
              ? CrossAxisAlignment.stretch
              : CrossAxisAlignment.start,
          children: [
            for (final (index, child) in children.indexed) ...[
              if (index > 0) const SizedBox(height: 16),
              if (canExpand) Expanded(child: child) else child,
            ],
          ],
        );
      },
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
