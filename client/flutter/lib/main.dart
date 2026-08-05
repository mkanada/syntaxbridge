import 'dart:io';

import 'package:flutter/material.dart';

import 'src/io/path_picker.dart';
import 'src/io/screenshot_storage.dart';
import 'src/logging/cli_log.dart';
import 'src/project/project_models.dart';
import 'src/server/http_server_client.dart';
import 'src/server/server_client.dart';
import 'src/ui/ide_theme.dart';
import 'src/ui/new_project_page.dart';
import 'src/ui/project_landing_page.dart';
import 'src/ui/screen_capturer.dart';
import 'src/ui/server_status_page.dart';

export 'src/io/path_picker.dart';
export 'src/io/screenshot_storage.dart';
export 'src/project/project_creation_exception.dart';
export 'src/project/project_models.dart';
export 'src/server/http_server_client.dart';
export 'src/server/server_client.dart';
export 'src/ui/new_project_page.dart';
export 'src/ui/project_landing_page.dart';
export 'src/ui/screen_capturer.dart';
export 'src/ui/server_status_page.dart';

void main() {
  cliLog('app starting');
  cliLog(
    'SYNTAX_BRIDGE_SERVER_URL=${Platform.environment['SYNTAX_BRIDGE_SERVER_URL'] ?? '<unset>'}',
  );
  cliLog(
    'SYNTAX_BRIDGE_SERVER_ADDR=${Platform.environment['SYNTAX_BRIDGE_SERVER_ADDR'] ?? '<unset>'}',
  );
  runApp(SyntaxBridgeApp());
}

class SyntaxBridgeApp extends StatelessWidget {
  const SyntaxBridgeApp({
    super.key,
    this.serverClient,
    this.pathPicker,
    this.screenshotStorage,
    this.screenCapturer,
  });

  final ServerClient? serverClient;
  final PathPicker? pathPicker;
  final ScreenshotStorage? screenshotStorage;
  final ScreenCapturer? screenCapturer;

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Syntax Bridge',
      debugShowCheckedModeBanner: false,
      theme: ThemeData(
        useMaterial3: true,
        brightness: Brightness.dark,
        colorScheme: ColorScheme.fromSeed(
          seedColor: IdePalette.teal,
          brightness: Brightness.dark,
        ),
        scaffoldBackgroundColor: IdePalette.background,
        canvasColor: IdePalette.panel,
        dividerColor: IdePalette.border,
        textTheme: ThemeData.dark().textTheme.apply(
          bodyColor: IdePalette.text,
          displayColor: IdePalette.text,
        ),
        inputDecorationTheme: InputDecorationTheme(
          filled: true,
          fillColor: IdePalette.editor,
          labelStyle: const TextStyle(color: IdePalette.muted),
          prefixIconColor: IdePalette.muted,
          suffixIconColor: IdePalette.softText,
          enabledBorder: OutlineInputBorder(
            borderSide: const BorderSide(color: IdePalette.border),
            borderRadius: BorderRadius.circular(6),
          ),
          focusedBorder: OutlineInputBorder(
            borderSide: const BorderSide(color: IdePalette.teal),
            borderRadius: BorderRadius.circular(6),
          ),
          disabledBorder: OutlineInputBorder(
            borderSide: const BorderSide(color: IdePalette.border),
            borderRadius: BorderRadius.circular(6),
          ),
        ),
        filledButtonTheme: FilledButtonThemeData(
          style: FilledButton.styleFrom(
            backgroundColor: IdePalette.teal,
            foregroundColor: IdePalette.background,
            disabledBackgroundColor: IdePalette.selection,
            disabledForegroundColor: IdePalette.muted,
            shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(6),
            ),
          ),
        ),
        outlinedButtonTheme: OutlinedButtonThemeData(
          style: OutlinedButton.styleFrom(
            foregroundColor: IdePalette.softText,
            side: const BorderSide(color: IdePalette.border),
            shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(6),
            ),
          ),
        ),
      ),
      home: _AppRoot(
        serverClient: serverClient ?? HttpServerClient.fromEnvironment(),
        pathPicker: pathPicker ?? const FilePickerPathPicker(),
        screenshotStorage: screenshotStorage ?? const FileScreenshotStorage(),
        screenCapturer: screenCapturer ?? const RenderScreenCapturer(),
      ),
    );
  }
}

/// Switches between the recent-projects landing screen, the dedicated
/// new-project step, and the IDE once a project has been chosen (opened,
/// imported, or created). Nothing but the current stage is ever mounted, so
/// there is no way to interact with the IDE before a project exists.
class _AppRoot extends StatefulWidget {
  const _AppRoot({
    required this.serverClient,
    required this.pathPicker,
    required this.screenshotStorage,
    required this.screenCapturer,
  });

  final ServerClient serverClient;
  final PathPicker pathPicker;
  final ScreenshotStorage screenshotStorage;
  final ScreenCapturer screenCapturer;

  @override
  State<_AppRoot> createState() => _AppRootState();
}

enum _AppStage { landing, creatingProject, ide }

class _AppRootState extends State<_AppRoot> {
  _AppStage _stage = _AppStage.landing;
  CreatedProject? _project;

  void _startNewProject() {
    setState(() {
      _stage = _AppStage.creatingProject;
    });
  }

  void _cancelNewProject() {
    setState(() {
      _stage = _AppStage.landing;
    });
  }

  void _handleProjectReady(CreatedProject project) {
    setState(() {
      _project = project;
      _stage = _AppStage.ide;
    });
  }

  @override
  Widget build(BuildContext context) {
    switch (_stage) {
      case _AppStage.landing:
        return ProjectLandingPage(
          serverClient: widget.serverClient,
          pathPicker: widget.pathPicker,
          onProjectReady: _handleProjectReady,
          onNewProject: _startNewProject,
        );
      case _AppStage.creatingProject:
        return NewProjectPage(
          serverClient: widget.serverClient,
          pathPicker: widget.pathPicker,
          onProjectCreated: _handleProjectReady,
          onCancel: _cancelNewProject,
        );
      case _AppStage.ide:
        return ServerStatusPage(
          serverClient: widget.serverClient,
          screenshotStorage: widget.screenshotStorage,
          screenCapturer: widget.screenCapturer,
          project: _project!,
        );
    }
  }
}
