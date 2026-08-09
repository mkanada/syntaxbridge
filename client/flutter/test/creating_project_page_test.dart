import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/src/project/project_models.dart';
import 'package:syntax_bridge/src/server/server_client.dart';
import 'package:syntax_bridge/src/ui/creating_project_page.dart';

/// Covers the flow the user asked for: submitting the new-project form
/// lands on this screen immediately, which shows a log and a progress bar
/// while polling the server-side job in the background, then hands off the
/// finished project (or reports a failure) once the job settles.
void main() {
  const input = CreateProjectInput(
    name: 'counter',
    workspaceDir: '/tmp/projects',
    archivePath: '/tmp/source.tar.gz',
  );

  testWidgets('starts the job and shows an indeterminate bar while ingesting', (
    tester,
  ) async {
    final client = _ScriptedServerClient(jobId: 'job-1', statuses: []);

    await tester.pumpWidget(_host(client: client, input: input));
    await tester.pump();

    expect(find.text('Creating project'), findsOneWidget);
    final bar = tester.widget<LinearProgressIndicator>(
      find.byType(LinearProgressIndicator),
    );
    expect(bar.value, isNull, reason: 'no totals known yet: indeterminate');
  });

  testWidgets('shows a determinate progress bar once totals are known', (
    tester,
  ) async {
    final client = _ScriptedServerClient(
      jobId: 'job-1',
      statuses: [
        const ProjectCreationJobStatus(
          state: ProjectCreationJobState.running,
          phase: ProjectCreationJobPhase.catalogingTypes,
          typeCatalogProgress: ExtractionProgress(completed: 5, total: 10),
          sourceCatalogProgress: ExtractionProgress(completed: 0, total: 0),
        ),
      ],
    );

    await tester.pumpWidget(
      _host(
        client: client,
        input: input,
        pollInterval: const Duration(milliseconds: 10),
      ),
    );
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 10));

    final bar = tester.widget<LinearProgressIndicator>(
      find.byType(LinearProgressIndicator),
    );
    expect(bar.value, isNotNull);
    expect(bar.value, greaterThan(0));
    expect(find.text('Cataloging types'), findsOneWidget);
  });

  testWidgets('hands off the created project once the job succeeds', (
    tester,
  ) async {
    const project = CreatedProject(
      name: 'counter',
      projectDir: '/tmp/projects/counter',
      inputSourceDir: '/tmp/projects/counter/input-source',
      compilationUnits: [],
    );
    final client = _ScriptedServerClient(
      jobId: 'job-1',
      statuses: [
        const ProjectCreationJobStatus(
          state: ProjectCreationJobState.succeeded,
          project: project,
        ),
      ],
    );

    CreatedProject? created;
    await tester.pumpWidget(
      _host(
        client: client,
        input: input,
        pollInterval: const Duration(milliseconds: 10),
        onProjectCreated: (project) => created = project,
      ),
    );
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 10));

    expect(created?.name, 'counter');
    expect(find.text('Project created'), findsOneWidget);
  });

  testWidgets('reports a failed job and offers a way back', (tester) async {
    final client = _ScriptedServerClient(
      jobId: 'job-1',
      statuses: [
        const ProjectCreationJobStatus(
          state: ProjectCreationJobState.failed,
          errorMessage: 'no CMakeLists.txt found under input-source',
          isClientError: true,
        ),
      ],
    );

    var cancelled = false;
    await tester.pumpWidget(
      _host(
        client: client,
        input: input,
        pollInterval: const Duration(milliseconds: 10),
        onCancel: () => cancelled = true,
      ),
    );
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 10));

    expect(find.textContaining('no CMakeLists.txt found'), findsOneWidget);

    await tester.tap(find.byTooltip('Back'));
    expect(cancelled, isTrue);
  });
}

Widget _host({
  required ServerClient client,
  required CreateProjectInput input,
  Duration pollInterval = const Duration(milliseconds: 400),
  ValueChanged<CreatedProject>? onProjectCreated,
  VoidCallback? onCancel,
}) {
  return MaterialApp(
    home: CreatingProjectPage(
      serverClient: client,
      input: input,
      pollInterval: pollInterval,
      onProjectCreated: onProjectCreated ?? (_) {},
      onCancel: onCancel ?? () {},
    ),
  );
}

/// A [ServerClient] whose `startCreateProject` always succeeds with [jobId],
/// and whose `pollCreateProjectJob` returns [statuses] in order (one per
/// call), holding on the last entry once exhausted — good enough to script
/// "N running snapshots then a terminal one" without needing real timers.
class _ScriptedServerClient implements ServerClient {
  _ScriptedServerClient({required this.jobId, required this.statuses});

  final String jobId;
  final List<ProjectCreationJobStatus> statuses;
  int _pollCount = 0;

  @override
  Future<String> startCreateProject(CreateProjectInput input) async => jobId;

  @override
  Future<ProjectCreationJobStatus> pollCreateProjectJob(String jobId) async {
    if (statuses.isEmpty) {
      return const ProjectCreationJobStatus(
        state: ProjectCreationJobState.running,
        phase: ProjectCreationJobPhase.ingesting,
      );
    }

    final index = _pollCount < statuses.length
        ? _pollCount
        : statuses.length - 1;
    _pollCount++;
    return statuses[index];
  }

  @override
  Future<ServerStatus> health() =>
      throw UnimplementedError('not used by this screen');

  @override
  Future<List<RecentProject>> listRecentProjects() =>
      throw UnimplementedError('not used by this screen');

  @override
  Future<void> forgetProject(String projectDir) =>
      throw UnimplementedError('not used by this screen');

  @override
  Future<CreatedProject> openProject(String projectDir) =>
      throw UnimplementedError('not used by this screen');

  @override
  Future<String> readSourceFile({
    required String projectDir,
    required String path,
  }) => throw UnimplementedError('not used by this screen');

  @override
  Future<List<TypeDeclaration>> listTypes(String projectDir) =>
      throw UnimplementedError('not used by this screen');
}
