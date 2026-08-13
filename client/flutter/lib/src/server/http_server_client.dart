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
  Future<String> startCreateProject(CreateProjectInput input) async {
    final url = baseUrl.resolve('/projects');
    final payload = jsonEncode(input.toJson());
    cliLog('HTTP POST $url payload=$payload');
    final request = await _httpClient.postUrl(url);
    request.headers.contentType = ContentType.json;
    request.write(payload);

    final response = await request.close();
    final body = await utf8.decoder.bind(response).join();
    cliLog('HTTP POST $url -> ${response.statusCode} body=$body');

    if (response.statusCode != HttpStatus.accepted) {
      throw ProjectCreationException(_errorMessageFromBody(body));
    }

    final json = jsonDecode(body) as Map<String, Object?>;
    return json['job_id'] as String? ?? '';
  }

  @override
  Future<ProjectCreationJobStatus> pollCreateProjectJob(String jobId) async {
    final url = baseUrl.resolve('/projects/jobs/$jobId');
    cliLog('HTTP GET $url');
    final request = await _httpClient.getUrl(url);
    final response = await request.close();
    final body = await utf8.decoder.bind(response).join();
    cliLog('HTTP GET $url -> ${response.statusCode} body=$body');

    if (response.statusCode != HttpStatus.ok) {
      throw ProjectCreationException(_errorMessageFromBody(body));
    }

    return ProjectCreationJobStatus.fromJson(
      jsonDecode(body) as Map<String, Object?>,
    );
  }

  @override
  Future<void> cancelCreateProject(String jobId) async {
    final url = baseUrl.resolve('/projects/jobs/$jobId');
    cliLog('HTTP DELETE $url');
    final request = await _httpClient.deleteUrl(url);
    final response = await request.close();
    final body = await utf8.decoder.bind(response).join();
    cliLog('HTTP DELETE $url -> ${response.statusCode} body=$body');

    if (response.statusCode != HttpStatus.accepted) {
      throw HttpException(_errorMessageFromBody(body));
    }
  }

  @override
  Future<List<RecentProject>> listRecentProjects() async {
    final url = baseUrl.resolve('/projects');
    cliLog('HTTP GET $url');
    final request = await _httpClient.getUrl(url);
    final response = await request.close();
    final body = await utf8.decoder.bind(response).join();
    cliLog('HTTP GET $url -> ${response.statusCode} body=$body');

    if (response.statusCode != HttpStatus.ok) {
      throw HttpException('Unexpected server status: ${response.statusCode}');
    }

    final json = jsonDecode(body) as Map<String, Object?>;
    final projectsJson =
        json['projects'] as List<Object?>? ?? const <Object?>[];
    return projectsJson
        .whereType<Map<String, Object?>>()
        .map(RecentProject.fromJson)
        .toList();
  }

  @override
  Future<void> forgetProject(String projectDir) async {
    final url = baseUrl.resolve('/projects');
    final payload = jsonEncode({'project_dir': projectDir});
    cliLog('HTTP DELETE $url payload=$payload');
    final request = await _httpClient.deleteUrl(url);
    request.headers.contentType = ContentType.json;
    request.write(payload);

    final response = await request.close();
    final body = await utf8.decoder.bind(response).join();
    cliLog('HTTP DELETE $url -> ${response.statusCode} body=$body');

    if (response.statusCode != HttpStatus.ok) {
      throw HttpException(_errorMessageFromBody(body));
    }
  }

  @override
  Future<CreatedProject> openProject(String projectDir) async {
    final url = baseUrl.resolve('/projects/open');
    final payload = jsonEncode({'project_dir': projectDir});
    cliLog('HTTP POST $url payload=$payload');
    final request = await _httpClient.postUrl(url);
    request.headers.contentType = ContentType.json;
    request.write(payload);

    final response = await request.close();
    final body = await utf8.decoder.bind(response).join();
    cliLog('HTTP POST $url -> ${response.statusCode} body=$body');

    if (response.statusCode != HttpStatus.ok) {
      throw ProjectCreationException(_errorMessageFromBody(body));
    }

    return CreatedProject.fromJson(jsonDecode(body) as Map<String, Object?>);
  }

  @override
  Future<String> readSourceFile({
    required String projectDir,
    required String path,
  }) async {
    final url = baseUrl
        .resolve('/projects/source-file')
        .replace(queryParameters: {'project_dir': projectDir, 'path': path});
    cliLog('HTTP GET $url');
    final request = await _httpClient.getUrl(url);
    final response = await request.close();
    final body = await utf8.decoder.bind(response).join();
    cliLog('HTTP GET $url -> ${response.statusCode} body=$body');

    if (response.statusCode != HttpStatus.ok) {
      throw HttpException(_errorMessageFromBody(body));
    }

    final json = jsonDecode(body) as Map<String, Object?>;
    return json['content'] as String? ?? '';
  }

  @override
  Future<TypeCatalogListing> listTypes(String projectDir) async {
    final url = baseUrl
        .resolve('/projects/types')
        .replace(queryParameters: {'project_dir': projectDir});
    cliLog('HTTP GET $url');
    final request = await _httpClient.getUrl(url);
    final response = await request.close();
    final body = await utf8.decoder.bind(response).join();
    cliLog('HTTP GET $url -> ${response.statusCode} body=$body');

    if (response.statusCode != HttpStatus.ok) {
      throw HttpException(_errorMessageFromBody(body));
    }

    return TypeCatalogListing.fromJson(
      jsonDecode(body) as Map<String, Object?>,
    );
  }

  @override
  Future<List<TypeUsage>> listTypeUsages({
    required String projectDir,
    required String typeUsr,
  }) async {
    final url = baseUrl
        .resolve('/projects/types/usages')
        .replace(queryParameters: {'project_dir': projectDir, 'usr': typeUsr});
    cliLog('HTTP GET $url');
    final request = await _httpClient.getUrl(url);
    final response = await request.close();
    final body = await utf8.decoder.bind(response).join();
    cliLog('HTTP GET $url -> ${response.statusCode} body=$body');

    if (response.statusCode != HttpStatus.ok) {
      throw HttpException(_errorMessageFromBody(body));
    }

    final json = jsonDecode(body) as Map<String, Object?>;
    final usagesJson = json['usages'] as List<Object?>? ?? const <Object?>[];
    return usagesJson
        .whereType<Map<String, Object?>>()
        .map(TypeUsage.fromJson)
        .toList();
  }

  @override
  Future<FunctionCatalogListing> listFunctions(String projectDir) async {
    final url = baseUrl
        .resolve('/projects/functions')
        .replace(queryParameters: {'project_dir': projectDir});
    cliLog('HTTP GET $url');
    final request = await _httpClient.getUrl(url);
    final response = await request.close();
    final body = await utf8.decoder.bind(response).join();
    cliLog('HTTP GET $url -> ${response.statusCode} body=$body');

    if (response.statusCode != HttpStatus.ok) {
      throw HttpException(_errorMessageFromBody(body));
    }

    return FunctionCatalogListing.fromJson(
      jsonDecode(body) as Map<String, Object?>,
    );
  }

  @override
  Future<List<CallEdge>> listCallers({
    required String projectDir,
    required String functionUsr,
  }) async {
    final url = baseUrl
        .resolve('/projects/functions/callers')
        .replace(
          queryParameters: {'project_dir': projectDir, 'usr': functionUsr},
        );
    cliLog('HTTP GET $url');
    final request = await _httpClient.getUrl(url);
    final response = await request.close();
    final body = await utf8.decoder.bind(response).join();
    cliLog('HTTP GET $url -> ${response.statusCode} body=$body');

    if (response.statusCode != HttpStatus.ok) {
      throw HttpException(_errorMessageFromBody(body));
    }

    final json = jsonDecode(body) as Map<String, Object?>;
    final callersJson = json['callers'] as List<Object?>? ?? const <Object?>[];
    return callersJson
        .whereType<Map<String, Object?>>()
        .map(CallEdge.fromJson)
        .toList();
  }

  @override
  Future<List<CallEdge>> listCallsInFile({
    required String projectDir,
    required String file,
  }) async {
    final url = baseUrl
        .resolve('/projects/functions/calls-in-file')
        .replace(queryParameters: {'project_dir': projectDir, 'file': file});
    cliLog('HTTP GET $url');
    final request = await _httpClient.getUrl(url);
    final response = await request.close();
    final body = await utf8.decoder.bind(response).join();
    cliLog('HTTP GET $url -> ${response.statusCode} body=$body');

    if (response.statusCode != HttpStatus.ok) {
      throw HttpException(_errorMessageFromBody(body));
    }

    final json = jsonDecode(body) as Map<String, Object?>;
    final callsJson = json['calls'] as List<Object?>? ?? const <Object?>[];
    return callsJson
        .whereType<Map<String, Object?>>()
        .map(CallEdge.fromJson)
        .toList();
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
