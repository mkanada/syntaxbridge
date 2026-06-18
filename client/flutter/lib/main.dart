import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';

void main() {
  runApp(SyntaxBridgeApp());
}

class SyntaxBridgeApp extends StatelessWidget {
  const SyntaxBridgeApp({super.key, this.serverClient});

  final ServerClient? serverClient;

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
      ),
    );
  }
}

abstract class ServerClient {
  Future<ServerStatus> health();
}

class ServerStatus {
  const ServerStatus({required this.service, required this.status});

  factory ServerStatus.fromJson(Map<String, Object?> json) {
    return ServerStatus(
      service: json['service'] as String? ?? 'unknown',
      status: json['status'] as String? ?? 'unknown',
    );
  }

  final String service;
  final String status;
}

class HttpServerClient implements ServerClient {
  HttpServerClient(this.baseUrl, {HttpClient? httpClient})
    : _httpClient = httpClient ?? HttpClient();

  factory HttpServerClient.fromEnvironment() {
    final value = Platform.environment['SYNTAX_BRIDGE_SERVER_URL'];
    return HttpServerClient(
      Uri.parse(value == null || value.isEmpty ? _defaultServerUrl : value),
    );
  }

  static const _defaultServerUrl = 'http://127.0.0.1:37651';

  final Uri baseUrl;
  final HttpClient _httpClient;

  @override
  Future<ServerStatus> health() async {
    final request = await _httpClient.getUrl(baseUrl.resolve('/health'));
    final response = await request.close();
    final body = await utf8.decoder.bind(response).join();

    if (response.statusCode != HttpStatus.ok) {
      throw HttpException('Unexpected server status: ${response.statusCode}');
    }

    return ServerStatus.fromJson(jsonDecode(body) as Map<String, Object?>);
  }
}

class ServerStatusPage extends StatefulWidget {
  const ServerStatusPage({super.key, required this.serverClient});

  final ServerClient serverClient;

  @override
  State<ServerStatusPage> createState() => _ServerStatusPageState();
}

class _ServerStatusPageState extends State<ServerStatusPage> {
  late Future<ServerStatus> _status;

  @override
  void initState() {
    super.initState();
    _status = widget.serverClient.health();
  }

  void _refresh() {
    setState(() {
      _status = widget.serverClient.health();
    });
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Syntax Bridge')),
      body: Padding(
        padding: const EdgeInsets.all(24),
        child: Align(
          alignment: Alignment.topLeft,
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 560),
            child: FutureBuilder<ServerStatus>(
              future: _status,
              builder: (context, snapshot) {
                final connected =
                    snapshot.hasData && snapshot.data?.status == 'ok';
                final failed = snapshot.hasError;

                return Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      'Server connection',
                      style: Theme.of(context).textTheme.headlineSmall,
                    ),
                    const SizedBox(height: 20),
                    Row(
                      children: [
                        Icon(
                          connected
                              ? Icons.check_circle
                              : failed
                              ? Icons.error
                              : Icons.sync,
                          color: connected
                              ? const Color(0xFF237A57)
                              : failed
                              ? const Color(0xFFB3261E)
                              : const Color(0xFF7B6324),
                        ),
                        const SizedBox(width: 10),
                        Text(
                          connected
                              ? 'Connected'
                              : failed
                              ? 'Disconnected'
                              : 'Connecting',
                          style: Theme.of(context).textTheme.titleMedium,
                        ),
                        const Spacer(),
                        IconButton(
                          tooltip: 'Refresh',
                          onPressed: _refresh,
                          icon: const Icon(Icons.refresh),
                        ),
                      ],
                    ),
                    const Divider(height: 32),
                    Text(
                      snapshot.data?.service ?? 'syntax-bridge-server',
                      style: Theme.of(context).textTheme.bodyLarge,
                    ),
                  ],
                );
              },
            ),
          ),
        ),
      ),
    );
  }
}
