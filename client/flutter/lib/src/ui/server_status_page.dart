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
import 'screen_capturer.dart';
import 'server_connection_status.dart';
import 'source_file_viewer.dart';
import 'source_files_view.dart';

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
  final Set<_IdePanel> _openPanels = {_IdePanel.explorer, _IdePanel.log};
  final Map<_IdePanel, DockSide> _panelSides = {
    _IdePanel.explorer: DockSide.left,
    _IdePanel.log: DockSide.right,
  };
  final _screenshotBoundaryKey = GlobalKey();
  SourceFile? _selectedSourceFile;
  Future<String>? _selectedSourceContent;

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

  void _selectSourceFile(SourceFile file) {
    _addLog('Opening ${file.path}');
    setState(() {
      _selectedSourceFile = file;
      _selectedSourceContent = _loadSourceFileContent(
        widget.project.projectDir,
        file,
      );
    });
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
    return RepaintBoundary(
      key: _screenshotBoundaryKey,
      child: Scaffold(
        body: Column(
          children: [
            _TitleBar(onRefresh: _refresh, onCaptureScreen: _captureScreen),
            Expanded(
              child: LayoutBuilder(
                builder: (context, constraints) {
                  final panels = _buildPanels();

                  return ColoredBox(
                    color: IdePalette.background,
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
                        project: widget.project,
                        onRefresh: _refresh,
                        selectedFile: _selectedSourceFile,
                        selectedFileContent: _selectedSourceContent,
                      ),
                    ),
                  );
                },
              ),
            ),
            const _StatusBar(),
          ],
        ),
      ),
    );
  }

  Map<_IdePanel, Widget> _buildPanels() {
    return {
      _IdePanel.explorer: DockablePanel(
        title: _IdePanel.explorer.title,
        icon: _IdePanel.explorer.icon,
        side: _panelSides[_IdePanel.explorer] ?? DockSide.left,
        onClose: () => _closePanel(_IdePanel.explorer),
        onDockSide: (side) => _dockPanel(_IdePanel.explorer, side),
        child: SourceFilesView(
          project: widget.project,
          onFileSelected: _selectSourceFile,
          selectedPath: _selectedSourceFile?.path,
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

enum _IdePanel { explorer, log }

extension _IdePanelMetadata on _IdePanel {
  String get title {
    return switch (this) {
      _IdePanel.explorer => 'Explorer',
      _IdePanel.log => 'Execution log',
    };
  }

  IconData get icon {
    return switch (this) {
      _IdePanel.explorer => Icons.folder_open_outlined,
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

    return Row(
      children: [
        const _ActivityRail(),
        Expanded(
          child: Padding(
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
                          child: _PanelColumn(
                            stretch: true,
                            children: leftPanels,
                          ),
                        ),
                        const SizedBox(width: 8),
                      ],
                      Expanded(child: child),
                      if (rightPanels.isNotEmpty) ...[
                        const SizedBox(width: 8),
                        SizedBox(
                          width: 360,
                          child: _PanelColumn(
                            stretch: true,
                            children: rightPanels,
                          ),
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
          ),
        ),
      ],
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

class _ActivityRail extends StatelessWidget {
  const _ActivityRail();

  @override
  Widget build(BuildContext context) {
    return Container(
      width: 48,
      color: IdePalette.activity,
      child: const Column(
        children: [
          SizedBox(height: 10),
          _RailIcon(
            Icons.file_copy_rounded,
            tooltip: 'Explorer',
            selected: true,
          ),
          _RailIcon(Icons.search_rounded, tooltip: 'Search'),
          _RailIcon(Icons.account_tree_rounded, tooltip: 'Source control'),
          _RailIcon(Icons.bug_report_rounded, tooltip: 'Debug'),
          _RailIcon(Icons.extension_rounded, tooltip: 'Extensions'),
          Spacer(),
          _RailIcon(Icons.person_rounded, tooltip: 'Account'),
          _RailIcon(Icons.manage_accounts_rounded, tooltip: 'Settings'),
          SizedBox(height: 8),
        ],
      ),
    );
  }
}

class _RailIcon extends StatelessWidget {
  const _RailIcon(this.icon, {required this.tooltip, this.selected = false});

  final IconData icon;
  final String tooltip;
  final bool selected;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: tooltip,
      child: Container(
        width: 48,
        height: 44,
        decoration: BoxDecoration(
          border: Border(
            left: BorderSide(
              color: selected ? IdePalette.teal : Colors.transparent,
              width: 3,
            ),
          ),
        ),
        child: Icon(
          icon,
          size: 21,
          color: selected ? IdePalette.text : IdePalette.muted,
        ),
      ),
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
      child: LayoutBuilder(
        builder: (context, constraints) {
          final compact = constraints.maxWidth < 920;

          return Row(
            children: [
              const SizedBox(width: 12),
              const Icon(
                Icons.grid_view_rounded,
                size: 18,
                color: IdePalette.teal,
              ),
              const SizedBox(width: 10),
              if (compact)
                const Expanded(
                  child: Text(
                    'Syntax Bridge',
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(fontWeight: FontWeight.w700),
                  ),
                )
              else ...[
                const Text(
                  'Syntax Bridge',
                  style: TextStyle(fontWeight: FontWeight.w700),
                ),
                const SizedBox(width: 22),
                const _MenuText('File'),
                const _MenuText('Edit'),
                const _MenuText('Search'),
                const _MenuText('Run'),
                const _MenuText('Window'),
                const Spacer(),
                const _SearchBox(),
                const SizedBox(width: 12),
              ],
              const IdeToolbarIcon(
                icon: Icons.play_arrow_rounded,
                tooltip: 'Run pipeline',
                color: IdePalette.green,
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
              const IdeToolbarIcon(
                icon: Icons.settings_rounded,
                tooltip: 'Settings',
              ),
              const SizedBox(width: 8),
            ],
          );
        },
      ),
    );
  }
}

class _SearchBox extends StatelessWidget {
  const _SearchBox();

  @override
  Widget build(BuildContext context) {
    return ConstrainedBox(
      constraints: const BoxConstraints(maxWidth: 380, minWidth: 180),
      child: Container(
        height: 30,
        padding: const EdgeInsets.symmetric(horizontal: 10),
        decoration: BoxDecoration(
          color: IdePalette.editor,
          border: Border.all(color: IdePalette.border),
          borderRadius: BorderRadius.circular(6),
        ),
        child: const Row(
          children: [
            Icon(Icons.search_rounded, size: 16, color: IdePalette.muted),
            SizedBox(width: 8),
            Expanded(
              child: Text(
                'Search workspace',
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: IdePalette.muted, fontSize: 13),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _MenuText extends StatelessWidget {
  const _MenuText(this.label);

  final String label;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 8),
      child: Text(
        label,
        style: const TextStyle(color: IdePalette.softText, fontSize: 13),
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
  });

  final Future<ServerStatus> status;
  final CreatedProject project;
  final VoidCallback onRefresh;
  final SourceFile? selectedFile;
  final Future<String>? selectedFileContent;

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
                    const SizedBox(height: 18),
                    const _WorkspaceCodePreview(),
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
              Container(
                height: 40,
                padding: const EdgeInsets.symmetric(horizontal: 14),
                decoration: const BoxDecoration(
                  color: IdePalette.tabBar,
                  border: Border(bottom: BorderSide(color: IdePalette.border)),
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
                        selectedFile?.path.split('/').last ??
                            'workspace.syntax-bridge',
                        overflow: TextOverflow.ellipsis,
                        style: const TextStyle(
                          color: IdePalette.softText,
                          fontSize: 13,
                          fontWeight: FontWeight.w700,
                        ),
                      ),
                    ),
                    const Icon(
                      Icons.more_vert_rounded,
                      color: IdePalette.muted,
                      size: 18,
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

class _WorkspaceCodePreview extends StatelessWidget {
  const _WorkspaceCodePreview();

  @override
  Widget build(BuildContext context) {
    const lines = [
      'input:',
      '  language: c_cpp',
      '  analysis: clang + cmake',
      '',
      'intermediate_model:',
      '  build_graph: ready',
      '  compilation_units: pending',
      '',
      'output:',
      '  language: dart',
      '  validation: unit_tests',
    ];

    return DecoratedBox(
      decoration: BoxDecoration(
        color: const Color(0xFF0E1013),
        border: Border.all(color: IdePalette.border),
        borderRadius: BorderRadius.circular(6),
      ),
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 10),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            for (final (index, line) in lines.indexed)
              _CodePreviewLine(number: index + 1, code: line),
          ],
        ),
      ),
    );
  }
}

class _CodePreviewLine extends StatelessWidget {
  const _CodePreviewLine({required this.number, required this.code});

  final int number;
  final String code;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      height: 24,
      child: Row(
        children: [
          SizedBox(
            width: 42,
            child: Text(
              '$number',
              textAlign: TextAlign.right,
              style: const TextStyle(
                color: IdePalette.lineNumber,
                fontFamily: 'monospace',
                fontSize: 13,
              ),
            ),
          ),
          const SizedBox(width: 14),
          Expanded(
            child: Text(
              code,
              softWrap: false,
              overflow: TextOverflow.fade,
              style: TextStyle(
                color: _codeColor(code),
                fontFamily: 'monospace',
                fontSize: 13,
                height: 1.3,
              ),
            ),
          ),
        ],
      ),
    );
  }

  Color _codeColor(String code) {
    final trimmed = code.trimLeft();
    if (trimmed.endsWith(':')) {
      return IdePalette.blue;
    }
    if (trimmed.contains('dart') || trimmed.contains('ready')) {
      return IdePalette.green;
    }
    if (trimmed.contains('clang') || trimmed.contains('cmake')) {
      return IdePalette.amber;
    }
    return IdePalette.codeText;
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

class _StatusBar extends StatelessWidget {
  const _StatusBar();

  @override
  Widget build(BuildContext context) {
    return Container(
      height: 26,
      color: IdePalette.status,
      padding: const EdgeInsets.symmetric(horizontal: 10),
      child: LayoutBuilder(
        builder: (context, constraints) {
          final wide = constraints.maxWidth >= 900;

          return Row(
            children: [
              const Icon(Icons.account_tree_rounded, size: 14),
              const SizedBox(width: 6),
              const Text('main', style: TextStyle(fontSize: 12)),
              const SizedBox(width: 18),
              const Icon(Icons.sync_rounded, size: 14),
              const SizedBox(width: 6),
              const Flexible(
                child: Text(
                  '0 errors, 0 warnings',
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(fontSize: 12),
                ),
              ),
              const Spacer(),
              const Text('C/C++ -> Dart', style: TextStyle(fontSize: 12)),
              if (wide) ...[
                const SizedBox(width: 18),
                const Text('UTF-8', style: TextStyle(fontSize: 12)),
                const SizedBox(width: 18),
                const Text('Flatpak pending', style: TextStyle(fontSize: 12)),
              ],
            ],
          );
        },
      ),
    );
  }
}
