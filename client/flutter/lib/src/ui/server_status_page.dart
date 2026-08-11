import 'package:flutter/material.dart';

import '../io/screenshot_storage.dart';
import '../logging/cli_log.dart';
import '../project/project_error_message.dart';
import '../project/project_models.dart';
import '../server/server_client.dart';
import 'callers_view.dart';
import 'dockable_panel.dart';
import 'execution_log.dart';
import 'execution_log_view.dart';
import 'functions_view.dart';
import 'ide_theme.dart';
import 'panel_descriptor.dart';
import 'panel_group.dart';
import 'screen_capturer.dart';
import 'server_connection_status.dart';
import 'source_file_viewer.dart';
import 'source_files_view.dart';
import 'types_view.dart';
import 'usages_view.dart';

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
  // 'usages' starts closed: it opens itself, docked to the center split,
  // only once a type is actually selected (see _selectType) — "entering"
  // the split rather than reserving half the workspace from the outset.
  final Set<String> _openPanels = {'explorer', 'types', 'functions', 'log'};
  final Map<String, DockSide> _panelSides = {
    'explorer': DockSide.left,
    'types': DockSide.left,
    'usages': DockSide.center,
    'functions': DockSide.left,
    'callers': DockSide.center,
    'log': DockSide.right,
  };
  final _screenshotBoundaryKey = GlobalKey();
  SourceFile? _selectedSourceFile;
  Future<String>? _selectedSourceContent;
  TypeDeclaration? _selectedType;
  FunctionDeclaration? _selectedFunction;
  int? _highlightStartLine;
  int? _highlightEndLine;
  late Future<TypeCatalogListing> _types;
  Future<List<TypeUsage>>? _selectedTypeUsages;
  late Future<FunctionCatalogListing> _functions;
  Future<List<CallEdge>>? _selectedFunctionCallers;

  @override
  void initState() {
    super.initState();
    _status = _loadServerStatus(notify: false);
    _types = _loadTypes();
    _functions = _loadFunctions();
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
  /// definition instead of just naming it. Also opens the Usages panel
  /// (US-4), docked to the center split so it appears side by side with the
  /// source viewer, and populates it for this type, keyed by its stable
  /// `usr` (US-3) rather than by position.
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

    setState(() {
      _selectedFunction = null;
      _openPanels.add('usages');
      _panelSides['usages'] = DockSide.center;
      _selectedTypeUsages = type.usr.isEmpty
          ? Future.value(const <TypeUsage>[])
          : _loadUsages(type.usr);
    });
  }

  /// Opens the file at the exact location [usage] occurs, for the Usages
  /// panel's "click a usage, jump to it" navigation (US-4 criterion 5). The
  /// selected type (and therefore the usages panel's contents) stays
  /// unchanged — only the source viewer moves.
  void _selectUsage(TypeUsage usage) {
    final sourceFile = widget.project.sourceFiles.firstWhere(
      (file) => file.path == usage.file,
      orElse: () => SourceFile(path: usage.file, kind: SourceFileKind.header),
    );

    _selectSourceFile(
      sourceFile,
      selectedType: _selectedType,
      highlightStartLine: usage.line,
      highlightEndLine: usage.line,
    );
  }

  Future<List<TypeUsage>> _loadUsages(String typeUsr) async {
    try {
      final usages = await widget.serverClient.listTypeUsages(
        projectDir: widget.project.projectDir,
        typeUsr: typeUsr,
      );
      _addLog(
        'Loaded ${usages.length} usages',
        level: ExecutionLogLevel.success,
        notify: false,
      );
      return usages;
    } catch (error, stackTrace) {
      cliLog('list usages exception: $error');
      cliLog('list usages stack: $stackTrace');
      _addLog(
        'Failed to load usages: ${projectErrorMessage(error)}',
        level: ExecutionLogLevel.error,
        notify: false,
      );
      rethrow;
    }
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

  /// Fetches the file a usage occurs in, for the Usages panel's accordion
  /// (US-4) to show the surrounding code inline. Deliberately silent on
  /// success — unlike [_loadSourceFileContent], this runs once per usage
  /// expanded rather than once per navigation, and would otherwise flood the
  /// execution log.
  Future<String> _loadUsageSource(TypeUsage usage) {
    return widget.serverClient.readSourceFile(
      projectDir: widget.project.projectDir,
      path: usage.file,
    );
  }

  /// Opens the file where [function] is declared and highlights its body,
  /// mirroring [_selectType]. Also opens the Callers panel (US-5), docked to
  /// the center split, and populates it for this function, keyed by its
  /// stable `usr` rather than by position.
  void _selectFunction(FunctionDeclaration function) {
    final sourceFile = widget.project.sourceFiles.firstWhere(
      (file) => file.path == function.file,
      orElse: () =>
          SourceFile(path: function.file, kind: SourceFileKind.header),
    );

    _selectSourceFile(
      sourceFile,
      highlightStartLine: function.line,
      highlightEndLine: function.endLine > 0 ? function.endLine : function.line,
    );

    setState(() {
      _selectedType = null;
      _selectedFunction = function;
      _openPanels.add('callers');
      _panelSides['callers'] = DockSide.center;
      _selectedFunctionCallers = function.usr.isEmpty
          ? Future.value(const <CallEdge>[])
          : _loadCallers(function.usr);
    });
  }

  /// Opens the file at the exact location [caller] occurs, for the Callers
  /// panel's "click a caller, jump to it" navigation (US-5 criterion 5).
  /// Mirrors [_selectUsage].
  void _selectCaller(CallEdge caller) {
    final sourceFile = widget.project.sourceFiles.firstWhere(
      (file) => file.path == caller.file,
      orElse: () => SourceFile(path: caller.file, kind: SourceFileKind.header),
    );

    _selectSourceFile(
      sourceFile,
      highlightStartLine: caller.line,
      highlightEndLine: caller.line,
    );
  }

  Future<List<CallEdge>> _loadCallers(String functionUsr) async {
    try {
      final callers = await widget.serverClient.listCallers(
        projectDir: widget.project.projectDir,
        functionUsr: functionUsr,
      );
      _addLog(
        'Loaded ${callers.length} callers',
        level: ExecutionLogLevel.success,
        notify: false,
      );
      return callers;
    } catch (error, stackTrace) {
      cliLog('list callers exception: $error');
      cliLog('list callers stack: $stackTrace');
      _addLog(
        'Failed to load callers: ${projectErrorMessage(error)}',
        level: ExecutionLogLevel.error,
        notify: false,
      );
      rethrow;
    }
  }

  /// Fetches the file a call occurs in, for the Callers panel's accordion
  /// (US-5) to show the surrounding code inline. Mirrors
  /// [_loadUsageSource]: deliberately silent on success.
  Future<String> _loadCallerSource(CallEdge caller) {
    return widget.serverClient.readSourceFile(
      projectDir: widget.project.projectDir,
      path: caller.file,
    );
  }

  Future<FunctionCatalogListing> _loadFunctions() async {
    try {
      final listing = await widget.serverClient.listFunctions(
        widget.project.projectDir,
      );
      _addLog(
        'Loaded ${listing.functions.length} functions',
        level: ExecutionLogLevel.success,
        notify: false,
      );
      return listing;
    } catch (error, stackTrace) {
      cliLog('list functions exception: $error');
      cliLog('list functions stack: $stackTrace');
      _addLog(
        'Failed to load functions: ${projectErrorMessage(error)}',
        level: ExecutionLogLevel.error,
        notify: false,
      );
      rethrow;
    }
  }

  Future<TypeCatalogListing> _loadTypes() async {
    try {
      final listing = await widget.serverClient.listTypes(
        widget.project.projectDir,
      );
      _addLog(
        'Loaded ${listing.types.length} types',
        level: ExecutionLogLevel.success,
        notify: false,
      );
      return listing;
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

  void _closePanel(String id) {
    setState(() {
      _openPanels.remove(id);
    });
  }

  /// Flips [id] between open and closed — what the Panels menu (in the title
  /// bar) uses both to reopen a closed panel and to close an open one from
  /// a single checklist, instead of the old per-panel "closed" button.
  void _togglePanel(String id) {
    setState(() {
      if (!_openPanels.remove(id)) {
        _openPanels.add(id);
      }
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
    final descriptors = _panelDescriptors();

    return RepaintBoundary(
      key: _screenshotBoundaryKey,
      child: Scaffold(
        body: Column(
          children: [
            _TitleBar(
              onRefresh: _refresh,
              onCaptureScreen: _captureScreen,
              panels: descriptors,
              openPanelIds: _openPanels,
              onTogglePanel: _togglePanel,
            ),
            Expanded(
              child: LayoutBuilder(
                builder: (context, constraints) {
                  return ColoredBox(
                    color: IdePalette.background,
                    child: _DockedWorkspace(
                      compact: constraints.maxWidth < 980,
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
                      centerPanels: _panelsFor(
                        context,
                        descriptors,
                        DockSide.center,
                        constraints.maxWidth,
                        groupAsTabs: false,
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
        builder: (context) => FutureBuilder<TypeCatalogListing>(
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

            final listing = snapshot.data;
            return TypesView(
              types: listing?.types ?? const [],
              usageCounts: listing?.usageCounts ?? const {},
              onTypeSelected: _selectType,
              selectedType: _selectedType,
            );
          },
        ),
      ),
      PanelDescriptor(
        id: 'usages',
        title: 'Usages',
        icon: Icons.travel_explore_outlined,
        // Splits side by side with the source viewer by default, via the
        // center split, so a type's usages sit right next to its code.
        defaultSide: DockSide.center,
        builder: (context) {
          final usagesFuture = _selectedTypeUsages;
          if (usagesFuture == null) {
            return UsagesView(
              selectedType: null,
              usages: const [],
              onUsageSelected: _selectUsage,
              loadUsageSource: _loadUsageSource,
            );
          }

          return FutureBuilder<List<TypeUsage>>(
            future: usagesFuture,
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
                    'Failed to load usages: ${projectErrorMessage(error)}',
                    style: const TextStyle(color: IdePalette.red),
                  ),
                );
              }

              return UsagesView(
                selectedType: _selectedType,
                usages: snapshot.data ?? const [],
                onUsageSelected: _selectUsage,
                loadUsageSource: _loadUsageSource,
              );
            },
          );
        },
      ),
      PanelDescriptor(
        id: 'functions',
        title: 'Functions',
        icon: Icons.functions,
        defaultSide: DockSide.left,
        builder: (context) => FutureBuilder<FunctionCatalogListing>(
          future: _functions,
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
                  'Failed to load functions: ${projectErrorMessage(error)}',
                  style: const TextStyle(color: IdePalette.red),
                ),
              );
            }

            final listing = snapshot.data;
            return FunctionsView(
              functions: listing?.functions ?? const [],
              callerCounts: listing?.callerCounts ?? const {},
              onFunctionSelected: _selectFunction,
              selectedFunction: _selectedFunction,
            );
          },
        ),
      ),
      PanelDescriptor(
        id: 'callers',
        title: 'Callers',
        icon: Icons.call_made,
        // Splits side by side with the source viewer by default, via the
        // center split, mirroring the Usages panel.
        defaultSide: DockSide.center,
        builder: (context) {
          final callersFuture = _selectedFunctionCallers;
          if (callersFuture == null) {
            return CallersView(
              selectedFunction: null,
              callers: const [],
              onCallerSelected: _selectCaller,
              loadCallerSource: _loadCallerSource,
            );
          }

          return FutureBuilder<List<CallEdge>>(
            future: callersFuture,
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
                    'Failed to load callers: ${projectErrorMessage(error)}',
                    style: const TextStyle(color: IdePalette.red),
                  ),
                );
              }

              return CallersView(
                selectedFunction: _selectedFunction,
                callers: snapshot.data ?? const [],
                onCallerSelected: _selectCaller,
                loadCallerSource: _loadCallerSource,
              );
            },
          );
        },
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

  /// The open panels docked to [side]. With [groupAsTabs] (the default), a
  /// single panel renders as a standalone [DockablePanel] and two or more
  /// share a [TabbedPanelGroup] so each keeps full height instead of
  /// splitting it. The center side passes `groupAsTabs: false` instead: its
  /// panels split side by side (the center split), so each one always gets
  /// its own standalone frame rather than being hidden behind a tab.
  List<Widget> _panelsFor(
    BuildContext context,
    List<PanelDescriptor> descriptors,
    DockSide side,
    double screenWidth, {
    bool groupAsTabs = true,
  }) {
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

    if (!groupAsTabs || openOnSide.length == 1) {
      return [
        for (final descriptor in openOnSide)
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
    required this.topPanels,
    required this.leftPanels,
    required this.rightPanels,
    required this.bottomPanels,
    required this.centerPanels,
    required this.child,
  });

  final bool compact;
  final List<Widget> topPanels;
  final List<Widget> leftPanels;
  final List<Widget> rightPanels;
  final List<Widget> bottomPanels;

  /// Panels docked to the center (any panel can be, via its own dock menu):
  /// they split side by side with [child] instead of stacking above/below
  /// or beside it like the other sides do.
  final List<Widget> centerPanels;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    if (compact) {
      return SingleChildScrollView(
        padding: const EdgeInsets.all(12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
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
            // No side-by-side room in compact width, so center panels just
            // stack under the main content like any other group.
            if (centerPanels.isNotEmpty) ...[
              const SizedBox(height: 12),
              _PanelColumn(stretch: true, children: centerPanels),
            ],
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
                Expanded(
                  child: _CenterSplit(main: child, panels: centerPanels),
                ),
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

/// The center of the workspace: [main] (the source viewer/welcome screen)
/// side by side with every center-docked panel, each getting an equal share
/// of the width. With no center-docked panels, this is just [main] — the
/// split only exists once something else asks to live there.
class _CenterSplit extends StatelessWidget {
  const _CenterSplit({required this.main, required this.panels});

  final Widget main;
  final List<Widget> panels;

  @override
  Widget build(BuildContext context) {
    if (panels.isEmpty) {
      return main;
    }

    return Row(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Expanded(child: main),
        for (final panel in panels) ...[
          const SizedBox(width: 8),
          Expanded(child: panel),
        ],
      ],
    );
  }
}

class _TitleBar extends StatelessWidget {
  const _TitleBar({
    required this.onRefresh,
    required this.onCaptureScreen,
    required this.panels,
    required this.openPanelIds,
    required this.onTogglePanel,
  });

  final VoidCallback onRefresh;
  final VoidCallback onCaptureScreen;

  /// Every registered panel, open or closed — what the Panels menu lists.
  final List<PanelDescriptor> panels;
  final Set<String> openPanelIds;
  final ValueChanged<String> onTogglePanel;

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
          PopupMenuButton<String>(
            tooltip: 'Panels',
            color: IdePalette.panel,
            icon: const Icon(
              Icons.view_quilt_outlined,
              color: IdePalette.muted,
            ),
            onSelected: onTogglePanel,
            itemBuilder: (context) => [
              for (final panel in panels)
                CheckedPopupMenuItem<String>(
                  value: panel.id,
                  checked: openPanelIds.contains(panel.id),
                  child: Text(panel.title),
                ),
            ],
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
        DockSide.top || DockSide.bottom || DockSide.center => null,
      },
      child: child,
    );
  }
}
