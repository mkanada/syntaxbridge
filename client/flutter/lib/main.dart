import 'dart:io';

import 'package:flutter/material.dart';

import 'src/io/path_picker.dart';
import 'src/logging/cli_log.dart';
import 'src/server/http_server_client.dart';
import 'src/server/server_client.dart';
import 'src/ui/ide_theme.dart';
import 'src/ui/server_status_page.dart';

export 'src/io/path_picker.dart';
export 'src/project/project_creation_exception.dart';
export 'src/project/project_models.dart';
export 'src/server/http_server_client.dart';
export 'src/server/server_client.dart';
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
  const SyntaxBridgeApp({super.key, this.serverClient, this.pathPicker});

  final ServerClient? serverClient;
  final PathPicker? pathPicker;

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
      home: ServerStatusPage(
        serverClient: serverClient ?? HttpServerClient.fromEnvironment(),
        pathPicker: pathPicker ?? const FilePickerPathPicker(),
      ),
    );
  }
}
