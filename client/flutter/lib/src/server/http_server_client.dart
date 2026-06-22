import 'dart:convert';
import 'dart:io';

import '../logging/cli_log.dart';
import '../project/project_creation_exception.dart';
import '../project/project_models.dart';
import 'server_client.dart';

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
    final url = baseUrl.resolve('/health');
    cliLog('HTTP GET $url');
    final request = await _httpClient.getUrl(url);
    final response = await request.close();
    final body = await utf8.decoder.bind(response).join();
    cliLog('HTTP GET $url -> ${response.statusCode} body=$body');

    if (response.statusCode != HttpStatus.ok) {
      throw HttpException('Unexpected server status: ${response.statusCode}');
    }

    return ServerStatus.fromJson(jsonDecode(body) as Map<String, Object?>);
  }

  @override
  Future<CreatedProject> createProject(CreateProjectInput input) async {
    final url = baseUrl.resolve('/projects');
    final payload = jsonEncode(input.toJson());
    cliLog('HTTP POST $url payload=$payload');
    final request = await _httpClient.postUrl(url);
    request.headers.contentType = ContentType.json;
    request.write(payload);

    final response = await request.close();
    final body = await utf8.decoder.bind(response).join();
    cliLog('HTTP POST $url -> ${response.statusCode} body=$body');

    if (response.statusCode != HttpStatus.created) {
      throw ProjectCreationException(_errorMessageFromBody(body));
    }

    return CreatedProject.fromJson(jsonDecode(body) as Map<String, Object?>);
  }

  String _errorMessageFromBody(String body) {
    try {
      final json = jsonDecode(body);
      if (json is Map<String, Object?>) {
        final message = json['message'] as String?;
        if (message != null && message.isNotEmpty) {
          return message;
        }
      }
    } catch (error, stackTrace) {
      cliLog('failed to parse project creation error body: $error');
      cliLog('error body parse stack: $stackTrace');
    }

    return 'Project creation failed';
  }
}
