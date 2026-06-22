import 'dart:io';

import 'package:flutter/material.dart';

import 'src/io/path_picker.dart';
import 'src/logging/cli_log.dart';
import 'src/server/http_server_client.dart';
import 'src/server/server_client.dart';
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
        colorScheme: ColorScheme.fromSeed(
          seedColor: const Color(0xFF006A6A),
          brightness: Brightness.light,
        ),
        scaffoldBackgroundColor: const Color(0xFFF6F7F8),
      ),
      home: ServerStatusPage(
        serverClient: serverClient ?? HttpServerClient.fromEnvironment(),
        pathPicker: pathPicker ?? const FilePickerPathPicker(),
      ),
    );
  }
}
