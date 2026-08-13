import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/main.dart';

void main() {
  testWidgets('shows connected status when the Rust server is healthy', (
    tester,
  ) async {
    await tester.pumpWidget(
      SyntaxBridgeApp(
        serverClient: _FakeServerClient(
          const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
        ),
      ),
    );

    await tester.pumpAndSettle();
    await _skipToIde(tester);

    expect(find.text('Syntax Bridge'), findsOneWidget);
    expect(find.text('Connected'), findsOneWidget);
    expect(find.text('syntax-bridge-server'), findsOneWidget);
  });

  testWidgets('renders the project workflow inside IDE chrome', (tester) async {
    tester.view
      ..devicePixelRatio = 1
      ..physicalSize = const Size(1280, 900);
    addTearDown(tester.view.reset);

    await tester.pumpWidget(
      SyntaxBridgeApp(
        serverClient: _FakeServerClient(
          const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
        ),
      ),
    );
    await tester.pumpAndSettle();
    await _skipToIde(tester);

    expect(find.text('Syntax Bridge workspace'), findsOneWidget);
    expect(find.text('Source Files'), findsOneWidget);
    expect(find.text('Execution log'), findsOneWidget);
    expect(find.byTooltip('Refresh'), findsWidgets);
    expect(find.byTooltip('Capture screen'), findsOneWidget);
  });

  testWidgets('omits IDE chrome that syntax-bridge does not implement', (
    tester,
  ) async {
    tester.view
      ..devicePixelRatio = 1
      ..physicalSize = const Size(1280, 900);
    addTearDown(tester.view.reset);

    await tester.pumpWidget(
      SyntaxBridgeApp(
        serverClient: _FakeServerClient(
          const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
        ),
      ),
    );
    await tester.pumpAndSettle();
    await _skipToIde(tester);

    // Decorative VS Code menu bar: none of these open anything.
    for (final menu in ['File', 'Edit', 'Search', 'Run', 'Window']) {
      expect(find.text(menu), findsNothing, reason: 'dead "$menu" menu');
    }

    // Activity rail entries for features the app does not have.
    for (final rail in [
      'Search',
      'Source control',
      'Debug',
      'Extensions',
      'Account',
      'Settings',
    ]) {
      expect(find.byTooltip(rail), findsNothing, reason: 'dead "$rail" rail');
    }

    // Toolbar buttons wired to nothing.
    expect(find.byTooltip('Run pipeline'), findsNothing);
    expect(find.byTooltip('Settings'), findsNothing);

    // Search box that searches nothing.
    expect(find.text('Search workspace'), findsNothing);

    // Fabricated status bar readings.
    expect(find.text('main'), findsNothing);
    expect(find.text('0 errors, 0 warnings'), findsNothing);
    expect(find.text('UTF-8'), findsNothing);
    expect(find.text('Flatpak pending'), findsNothing);

    // Hardcoded pipeline snippet presented as project state.
    expect(find.text('workspace.syntax-bridge'), findsNothing);
    expect(find.text('  build_graph: ready'), findsNothing);
    expect(find.text('  compilation_units: pending'), findsNothing);
  });

  testWidgets('creates a project and shows returned source files', (
    tester,
  ) async {
    final fakeClient = _FakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      project: const CreatedProject(
        name: 'counter',
        projectDir: '/tmp/projects/counter',
        inputSourceDir: '/tmp/projects/counter/input-source',
        compilationUnits: [
          CompilationUnit(
            directory: '/tmp/projects/counter/build',
            file: '/tmp/projects/counter/input-source/fixture/main.cpp',
            command: 'clang++ -c main.cpp',
          ),
        ],
        sourceFiles: [
          SourceFile(
            path: '/tmp/projects/counter/input-source/fixture/main.cpp',
            kind: SourceFileKind.translationUnit,
          ),
          SourceFile(
            path: '/tmp/projects/counter/input-source/fixture/types.h',
            kind: SourceFileKind.header,
          ),
        ],
      ),
    );

    await tester.pumpWidget(SyntaxBridgeApp(serverClient: fakeClient));
    await tester.pumpAndSettle();
    await _openNewProjectPage(tester);

    await tester.enterText(find.bySemanticsLabel('Project name'), 'counter');
    await tester.enterText(
      find.bySemanticsLabel('Workspace directory'),
      '/tmp/projects',
    );
    await tester.enterText(
      find.bySemanticsLabel('Source archive'),
      '/tmp/source.tar.gz',
    );
    await tester.pump();
    await tester.tap(find.widgetWithText(FilledButton, 'Create project'));
    await tester.pumpAndSettle();

    expect(fakeClient.createdProjectName, 'counter');
    expect(find.text('Execution log'), findsOneWidget);
    expect(find.text('Source files'), findsOneWidget);
    expect(find.text('input-source/fixture/main.cpp'), findsOneWidget);
    expect(find.text('input-source/fixture/types.h'), findsOneWidget);
    expect(
      find.text('/tmp/projects/counter/input-source/fixture/main.cpp'),
      findsNothing,
    );
    expect(find.text('clang++ -c main.cpp'), findsNothing);
  });

  testWidgets('shows the type catalog for the project', (tester) async {
    final fakeClient = _FakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      types: const [
        TypeDeclaration(
          name: 'Point',
          kind: TypeDeclarationKind.struct,
          file: '/tmp/projects/counter/input-source/fixture/types.h',
          line: 3,
          column: 8,
        ),
      ],
    );

    await tester.pumpWidget(SyntaxBridgeApp(serverClient: fakeClient));
    await tester.pumpAndSettle();
    await _skipToIde(tester);

    expect(find.text('Types'), findsOneWidget);
    await tester.tap(find.text('Types'));
    await tester.pumpAndSettle();

    expect(find.text('Point'), findsOneWidget);
    expect(find.widgetWithText(ListTile, 'struct'), findsOneWidget);
  });

  testWidgets("opens a type's source file with its body highlighted on click", (
    tester,
  ) async {
    final fakeClient = _FakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      project: const CreatedProject(
        name: 'counter',
        projectDir: '/tmp/projects/counter',
        inputSourceDir: '/tmp/projects/counter/input-source',
        compilationUnits: [],
        sourceFiles: [
          SourceFile(
            path: '/tmp/projects/counter/input-source/fixture/types.h',
            kind: SourceFileKind.header,
          ),
        ],
      ),
      types: const [
        TypeDeclaration(
          name: 'Point',
          kind: TypeDeclarationKind.struct,
          namespace: 'geometry',
          file: '/tmp/projects/counter/input-source/fixture/types.h',
          line: 3,
          column: 8,
          endLine: 6,
          endColumn: 1,
        ),
      ],
      sourceFileContent:
          'namespace geometry {\n\nstruct Point {\n  int x;\n  int y;\n};\n\n}',
    );

    await tester.pumpWidget(SyntaxBridgeApp(serverClient: fakeClient));
    await tester.pumpAndSettle();
    await _skipToIde(tester);

    await tester.tap(find.text('Types'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('geometry::Point'));
    await tester.pumpAndSettle();

    expect(
      fakeClient.readSourceFilePath,
      '/tmp/projects/counter/input-source/fixture/types.h',
    );
    expect(find.text('types.h'), findsWidgets);
    expect(find.text('struct Point {'), findsOneWidget);

    final highlighted = tester.widget<ColoredBox>(
      find
          .ancestor(
            of: find.text('struct Point {'),
            matching: find.byType(ColoredBox),
          )
          .first,
    );
    final unhighlighted = tester.widget<ColoredBox>(
      find
          .ancestor(
            of: find.text('namespace geometry {'),
            matching: find.byType(ColoredBox),
          )
          .first,
    );
    expect(highlighted.color, isNot(unhighlighted.color));
  });

  testWidgets(
    'shows usage counts and navigates from the type list to a usage site '
    '(US-4)',
    (tester) async {
      const pointUsr = 'c:@N@geometry@S@Point';
      final fakeClient = _FakeServerClient(
        const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
        project: const CreatedProject(
          name: 'counter',
          projectDir: '/tmp/projects/counter',
          inputSourceDir: '/tmp/projects/counter/input-source',
          compilationUnits: [],
          sourceFiles: [
            SourceFile(
              path: '/tmp/projects/counter/input-source/fixture/types.h',
              kind: SourceFileKind.header,
            ),
            SourceFile(
              path: '/tmp/projects/counter/input-source/fixture/main.cpp',
              kind: SourceFileKind.translationUnit,
            ),
          ],
        ),
        types: const [
          TypeDeclaration(
            name: 'Point',
            kind: TypeDeclarationKind.struct,
            namespace: 'geometry',
            file: '/tmp/projects/counter/input-source/fixture/types.h',
            line: 3,
            column: 8,
            usr: pointUsr,
          ),
        ],
        usageCounts: const {pointUsr: 1},
        usagesByType: const {
          pointUsr: [
            TypeUsage(
              typeUsr: pointUsr,
              kind: TypeUsageKind.variableDeclaration,
              file: '/tmp/projects/counter/input-source/fixture/main.cpp',
              line: 4,
              column: 1,
            ),
          ],
        },
        sourceFileContent: 'Point origin;\n',
      );

      await tester.pumpWidget(SyntaxBridgeApp(serverClient: fakeClient));
      await tester.pumpAndSettle();
      await _skipToIde(tester);

      await tester.tap(find.text('Types'));
      await tester.pumpAndSettle();

      expect(find.text('1 use'), findsOneWidget);
      // The Usages panel only enters (docked to the center split, side by
      // side with the source viewer) once a type is actually selected.
      expect(find.text('Usages'), findsNothing);

      await tester.tap(find.text('geometry::Point'));
      await tester.pumpAndSettle();

      // Both the panel frame's title and the panel content's own heading
      // say "Usages".
      expect(find.text('Usages'), findsWidgets);
      expect(find.text('1 use of geometry::Point'), findsOneWidget);
      expect(find.text('main.cpp:4'), findsOneWidget);
      expect(find.text('variable'), findsOneWidget);

      final openInEditor = find.byKey(
        const Key(
          'usage-open-/tmp/projects/counter/input-source/fixture/main.cpp:4',
        ),
      );
      await tester.ensureVisible(openInEditor);
      await tester.pumpAndSettle();
      await tester.tap(openInEditor);
      await tester.pumpAndSettle();

      expect(
        fakeClient.readSourceFilePath,
        '/tmp/projects/counter/input-source/fixture/main.cpp',
      );
      expect(find.text('main.cpp'), findsWidgets);
    },
  );

  testWidgets('shows the function catalog for the project', (tester) async {
    final fakeClient = _FakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      functions: const [
        FunctionDeclaration(
          name: 'area',
          kind: FunctionDeclarationKind.method,
          namespace: 'geometry',
          signature: 'double geometry::Shape::area() const',
          file: '/tmp/projects/counter/input-source/fixture/shapes.h',
          line: 4,
          column: 19,
          isVirtual: true,
        ),
      ],
    );

    await tester.pumpWidget(SyntaxBridgeApp(serverClient: fakeClient));
    await tester.pumpAndSettle();
    await _skipToIde(tester);

    expect(find.text('Functions'), findsOneWidget);
    await tester.tap(find.text('Functions'));
    await tester.pumpAndSettle();

    expect(find.text('geometry::area'), findsOneWidget);
    expect(find.text('double geometry::Shape::area() const'), findsOneWidget);
    expect(find.text('virtual'), findsOneWidget);
    expect(find.widgetWithText(ListTile, 'method'), findsOneWidget);
  });

  testWidgets(
    'shows caller counts and navigates from the function list to a caller '
    'site (US-5)',
    (tester) async {
      const areaUsr = 'c:@N@geometry@S@Shape@F@area#1#';
      final fakeClient = _FakeServerClient(
        const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
        project: const CreatedProject(
          name: 'counter',
          projectDir: '/tmp/projects/counter',
          inputSourceDir: '/tmp/projects/counter/input-source',
          compilationUnits: [],
          sourceFiles: [
            SourceFile(
              path: '/tmp/projects/counter/input-source/fixture/shapes.h',
              kind: SourceFileKind.header,
            ),
            SourceFile(
              path: '/tmp/projects/counter/input-source/fixture/main.cpp',
              kind: SourceFileKind.translationUnit,
            ),
          ],
        ),
        functions: const [
          FunctionDeclaration(
            name: 'area',
            kind: FunctionDeclarationKind.method,
            namespace: 'geometry',
            signature: 'double geometry::Shape::area() const',
            file: '/tmp/projects/counter/input-source/fixture/shapes.h',
            line: 4,
            column: 19,
            usr: areaUsr,
            isVirtual: true,
          ),
        ],
        callerCounts: const {areaUsr: 1},
        callersByFunction: const {
          areaUsr: [
            CallEdge(
              callerUsr: 'c:@F@describe#',
              resolution: CallResolution.resolved(
                calleeUsr: areaUsr,
                isDynamicDispatch: true,
              ),
              file: '/tmp/projects/counter/input-source/fixture/main.cpp',
              line: 10,
              column: 19,
            ),
          ],
        },
        sourceFileContent: 'return shape.area();\n',
      );

      await tester.pumpWidget(SyntaxBridgeApp(serverClient: fakeClient));
      await tester.pumpAndSettle();
      await _skipToIde(tester);

      await tester.tap(find.text('Functions'));
      await tester.pumpAndSettle();

      expect(find.text('1 caller'), findsOneWidget);
      // The Callers panel only enters (docked to the center split) once a
      // function is actually selected, mirroring the Usages panel.
      expect(find.text('Callers'), findsNothing);

      await tester.tap(find.text('geometry::area'));
      await tester.pumpAndSettle();

      expect(find.text('Callers'), findsWidgets);
      expect(find.text('1 caller of geometry::area'), findsOneWidget);
      expect(find.text('main.cpp:10'), findsOneWidget);
      expect(find.text('dynamic dispatch'), findsOneWidget);

      final openInEditor = find.byKey(
        const Key(
          'caller-open-/tmp/projects/counter/input-source/fixture/main.cpp:10',
        ),
      );
      await tester.ensureVisible(openInEditor);
      await tester.pumpAndSettle();
      await tester.tap(openInEditor);
      await tester.pumpAndSettle();

      expect(
        fakeClient.readSourceFilePath,
        '/tmp/projects/counter/input-source/fixture/main.cpp',
      );
      expect(find.text('main.cpp'), findsWidgets);
    },
  );

  testWidgets('navigates from a call in an open source file to its definition '
      '(US-5 criterion 5)', (tester) async {
    const describeUsr = 'c:@F@describe#';
    const areaUsr = 'c:@N@geometry@S@Shape@F@area#1#';
    const mainCppPath = '/tmp/projects/counter/input-source/fixture/main.cpp';
    const shapesHPath = '/tmp/projects/counter/input-source/fixture/shapes.h';
    final callToArea = CallEdge(
      callerUsr: describeUsr,
      resolution: const CallResolution.resolved(
        calleeUsr: areaUsr,
        isDynamicDispatch: true,
      ),
      file: mainCppPath,
      line: 2,
      column: 19,
    );

    final fakeClient = _FakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      project: const CreatedProject(
        name: 'counter',
        projectDir: '/tmp/projects/counter',
        inputSourceDir: '/tmp/projects/counter/input-source',
        compilationUnits: [],
        sourceFiles: [
          SourceFile(path: mainCppPath, kind: SourceFileKind.translationUnit),
          SourceFile(path: shapesHPath, kind: SourceFileKind.header),
        ],
      ),
      functions: const [
        FunctionDeclaration(
          name: 'describe',
          kind: FunctionDeclarationKind.freeFunction,
          signature: 'double describe(const Shape &shape)',
          file: mainCppPath,
          line: 1,
          column: 8,
          usr: describeUsr,
        ),
        FunctionDeclaration(
          name: 'area',
          kind: FunctionDeclarationKind.method,
          namespace: 'geometry',
          signature: 'double geometry::Shape::area() const',
          file: shapesHPath,
          line: 4,
          column: 19,
          usr: areaUsr,
          isVirtual: true,
        ),
      ],
      callerCounts: const {areaUsr: 1},
      callersByFunction: {
        areaUsr: [callToArea],
      },
      callsByFile: {
        mainCppPath: [callToArea],
      },
      sourceFileContent:
          'double describe(const Shape& shape) {\n'
          '    return shape.area();\n'
          '}\n',
    );

    await tester.pumpWidget(SyntaxBridgeApp(serverClient: fakeClient));
    await tester.pumpAndSettle();
    await _skipToIde(tester);

    await tester.tap(find.text('Functions'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('describe'));
    await tester.pumpAndSettle();

    // Selecting `describe` opens its own (empty) Callers panel first —
    // the baseline this test's tap needs to move away from.
    expect(find.text('0 callers of describe'), findsOneWidget);
    expect(fakeClient.readSourceFilePath, mainCppPath);

    final callLine = find.text('    return shape.area();');
    await tester.ensureVisible(callLine);
    await tester.pumpAndSettle();
    await tester.tap(callLine);
    await tester.pumpAndSettle();

    expect(fakeClient.readSourceFilePath, shapesHPath);
    expect(find.text('1 caller of geometry::area'), findsOneWidget);
  });

  testWidgets('opens a source file and displays its content on click', (
    tester,
  ) async {
    final fakeClient = _FakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      project: const CreatedProject(
        name: 'counter',
        projectDir: '/tmp/projects/counter',
        inputSourceDir: '/tmp/projects/counter/input-source',
        compilationUnits: [],
        sourceFiles: [
          SourceFile(
            path: '/tmp/projects/counter/input-source/fixture/types.h',
            kind: SourceFileKind.header,
          ),
        ],
      ),
      sourceFileContent: 'struct Point {\n  int x;\n  int y;\n};',
    );

    await tester.pumpWidget(SyntaxBridgeApp(serverClient: fakeClient));
    await tester.pumpAndSettle();
    await _openNewProjectPage(tester);

    await tester.enterText(find.bySemanticsLabel('Project name'), 'counter');
    await tester.enterText(
      find.bySemanticsLabel('Workspace directory'),
      '/tmp/projects',
    );
    await tester.enterText(
      find.bySemanticsLabel('Source archive'),
      '/tmp/source.tar.gz',
    );
    await tester.pump();
    await tester.tap(find.widgetWithText(FilledButton, 'Create project'));
    await tester.pumpAndSettle();

    await tester.tap(find.text('input-source/fixture/types.h'));
    await tester.pumpAndSettle();

    expect(
      fakeClient.readSourceFilePath,
      '/tmp/projects/counter/input-source/fixture/types.h',
    );
    expect(find.text('struct Point {'), findsOneWidget);
    expect(find.text('  int x;'), findsOneWidget);
    expect(find.text('types.h'), findsWidgets);
  });

  testWidgets(
    'opens a C++ file, transpiles the project, and shows the matching Dart',
    (tester) async {
      const dartSource = 'int soma(int a, int b) {\n  return a + b;\n}\n';
      final fakeClient = _FakeServerClient(
        const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
        project: const CreatedProject(
          name: 'counter',
          projectDir: '/tmp/projects/counter',
          inputSourceDir: '/tmp/projects/counter/input-source',
          compilationUnits: [],
          sourceFiles: [
            SourceFile(
              path: '/tmp/projects/counter/input-source/src/aritmetica.cpp',
              kind: SourceFileKind.translationUnit,
            ),
          ],
        ),
        sourceFileContent: 'int soma(int a, int b) {\n    return a + b;\n}',
        transpiledPackage: const TranspiledPackage(
          packageName: 'e01_funcao_aritmetica',
          files: {
            'pubspec.yaml': 'name: e01_funcao_aritmetica\n',
            'lib/aritmetica.dart': dartSource,
          },
        ),
      );

      await tester.pumpWidget(SyntaxBridgeApp(serverClient: fakeClient));
      await tester.pumpAndSettle();
      await _skipToIde(tester);

      await tester.tap(find.text('input-source/src/aritmetica.cpp'));
      await tester.pumpAndSettle();

      await tester.tap(find.byTooltip('Transpile'));
      await tester.pumpAndSettle();

      expect(fakeClient.transpileProjectDir, '/tmp/projects/counter');
      // The Dart Output panel opened (its own title, distinct from the C++
      // viewer alongside it) and shows the generated body — checked via the
      // return line's indentation (2 spaces), since the signature line
      // itself is identical text in both the C++ source and the Dart output
      // for this trivial example, so it can't disambiguate which panel
      // rendered it.
      expect(find.text('Dart Output'), findsOneWidget);
      expect(find.text('  return a + b;'), findsOneWidget);
      expect(find.text('    return a + b;'), findsOneWidget);
    },
  );

  testWidgets('opens path pickers for workspace and source archive', (
    tester,
  ) async {
    final pathPicker = _FakePathPicker(
      workspaceDir: '/tmp/projects',
      sourceArchive: '/tmp/source.zip',
    );

    await tester.pumpWidget(
      SyntaxBridgeApp(
        serverClient: _FakeServerClient(
          const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
        ),
        pathPicker: pathPicker,
      ),
    );
    await tester.pumpAndSettle();
    await _openNewProjectPage(tester);

    await tester.tap(find.byTooltip('Choose workspace directory'));
    await tester.pump();
    await tester.tap(find.byTooltip('Choose source archive'));
    await tester.pump();

    expect(pathPicker.workspacePickCount, 1);
    expect(pathPicker.archivePickCount, 1);
    expect(find.widgetWithText(TextField, '/tmp/projects'), findsOneWidget);
    expect(find.widgetWithText(TextField, '/tmp/source.zip'), findsOneWidget);
  });

  testWidgets('closes, reopens, and docks IDE panels with one click', (
    tester,
  ) async {
    tester.view
      ..devicePixelRatio = 1
      ..physicalSize = const Size(1280, 900);
    addTearDown(tester.view.reset);

    await tester.pumpWidget(
      SyntaxBridgeApp(
        serverClient: _FakeServerClient(
          const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
        ),
      ),
    );
    await tester.pumpAndSettle();
    await _skipToIde(tester);

    expect(find.text('Source Files'), findsOneWidget);
    expect(find.text('Execution log'), findsOneWidget);
    expect(
      tester.getTopLeft(find.text('Source Files')).dx,
      lessThan(tester.getTopLeft(find.text('Execution log')).dx),
    );

    await tester.tap(find.byTooltip('Close Execution log panel'));
    await tester.pumpAndSettle();

    expect(find.byTooltip('Close Execution log panel'), findsNothing);
    expect(find.text('Execution log'), findsNothing);

    // Reopening a closed panel goes through the Panels menu now, not a
    // per-panel button cluttering the workspace.
    await tester.tap(find.byTooltip('Panels'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Execution log'));
    await tester.pumpAndSettle();

    expect(find.byTooltip('Close Execution log panel'), findsOneWidget);

    await tester.tap(find.byTooltip('Dock Execution log panel'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Dock bottom').last);
    await tester.pumpAndSettle();

    expect(
      tester.getTopLeft(find.text('Execution log')).dy,
      greaterThan(tester.getTopLeft(find.text('Source Files')).dy),
    );

    await tester.tap(find.byTooltip('Dock Source Files panel'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Dock right').last);
    await tester.pumpAndSettle();

    expect(
      tester.getTopLeft(find.text('Source Files')).dx,
      greaterThan(tester.getTopLeft(find.text('Syntax Bridge workspace')).dx),
    );
  });

  testWidgets(
    'docks panels to the center split, side by side rather than tabbed',
    (tester) async {
      tester.view
        ..devicePixelRatio = 1
        ..physicalSize = const Size(1280, 900);
      addTearDown(tester.view.reset);

      await tester.pumpWidget(
        SyntaxBridgeApp(
          serverClient: _FakeServerClient(
            const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await _skipToIde(tester);

      await tester.tap(find.byTooltip('Dock Source Files panel'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Dock center').last);
      await tester.pumpAndSettle();

      // Center-docked, split side by side with the main workspace content —
      // not overlaid or replaced.
      expect(find.text('Source Files'), findsOneWidget);
      expect(find.text('Syntax Bridge workspace'), findsOneWidget);
      expect(
        tester.getTopLeft(find.text('Source Files')).dx,
        greaterThan(tester.getTopLeft(find.text('Syntax Bridge workspace')).dx),
      );

      await tester.tap(find.byTooltip('Dock Execution log panel'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Dock center').last);
      await tester.pumpAndSettle();

      // Two panels sharing the center dock still both show at once: it's a
      // split, not a tab group hiding one behind the other.
      expect(find.text('Source Files'), findsOneWidget);
      expect(find.text('Execution log'), findsOneWidget);
      expect(find.text('Syntax Bridge workspace'), findsOneWidget);
      expect(
        tester.getTopLeft(find.text('Execution log')).dx,
        greaterThan(tester.getTopLeft(find.text('Source Files')).dx),
      );
    },
  );

  testWidgets('shows project creation failures with server details', (
    tester,
  ) async {
    final fakeClient = _FakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      createError: const ProjectCreationException(
        'no CMakeLists.txt found under input-source',
      ),
    );

    await tester.pumpWidget(SyntaxBridgeApp(serverClient: fakeClient));
    await tester.pumpAndSettle();
    await _openNewProjectPage(tester);

    await tester.enterText(find.bySemanticsLabel('Project name'), 'counter');
    await tester.enterText(
      find.bySemanticsLabel('Workspace directory'),
      '/tmp/projects',
    );
    await tester.enterText(
      find.bySemanticsLabel('Source archive'),
      '/tmp/source.tar.gz',
    );
    await tester.pump();
    await tester.tap(find.widgetWithText(FilledButton, 'Create project'));
    await tester.pumpAndSettle();

    expect(find.text('Project creation failed'), findsOneWidget);
    expect(
      find.text('no CMakeLists.txt found under input-source'),
      findsOneWidget,
    );
  });

  testWidgets('shows an empty state when there are no recent projects', (
    tester,
  ) async {
    await tester.pumpWidget(
      SyntaxBridgeApp(
        serverClient: _FakeServerClient(
          const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('No recent projects yet'), findsOneWidget);
    expect(find.widgetWithText(FilledButton, 'New project'), findsOneWidget);
    expect(
      find.widgetWithText(OutlinedButton, 'Import project'),
      findsOneWidget,
    );
  });

  testWidgets('lists recent projects and opens one without re-ingesting', (
    tester,
  ) async {
    final fakeClient = _FakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      recentProjects: const [
        RecentProject(
          name: 'counter',
          projectDir: '/tmp/projects/counter',
          sourceLanguage: 'cpp',
          targetLanguage: 'dart',
          lastIngestStatus: 'success',
        ),
      ],
      openProjectResult: const CreatedProject(
        name: 'counter',
        projectDir: '/tmp/projects/counter',
        inputSourceDir: '/tmp/projects/counter/input-source',
        compilationUnits: [],
        sourceFiles: [
          SourceFile(
            path: '/tmp/projects/counter/input-source/fixture/main.cpp',
            kind: SourceFileKind.translationUnit,
          ),
        ],
      ),
    );

    await tester.pumpWidget(SyntaxBridgeApp(serverClient: fakeClient));
    await tester.pumpAndSettle();

    expect(find.text('counter'), findsOneWidget);
    expect(find.text('/tmp/projects/counter'), findsOneWidget);

    await tester.tap(find.text('counter'));
    await tester.pumpAndSettle();

    expect(fakeClient.openedProjectDir, '/tmp/projects/counter');
    expect(find.text('Source Files'), findsOneWidget);
    expect(find.text('input-source/fixture/main.cpp'), findsOneWidget);
  });

  testWidgets('offers to drop a recent project whose folder is gone', (
    tester,
  ) async {
    final fakeClient = _FakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      recentProjects: const [
        RecentProject(
          name: 'gone',
          projectDir: '/tmp/projects/gone',
          sourceLanguage: 'cpp',
          targetLanguage: 'dart',
          lastIngestStatus: 'success',
          available: false,
        ),
        RecentProject(
          name: 'kept',
          projectDir: '/tmp/projects/kept',
          sourceLanguage: 'cpp',
          targetLanguage: 'dart',
          lastIngestStatus: 'success',
        ),
      ],
    );

    await tester.pumpWidget(SyntaxBridgeApp(serverClient: fakeClient));
    await tester.pumpAndSettle();

    expect(find.text('gone'), findsOneWidget);
    expect(find.text('Folder no longer exists'), findsOneWidget);

    // A project that cannot be opened must not pretend it can: tapping it
    // does nothing instead of failing with a server error.
    await tester.tap(find.text('gone'));
    await tester.pumpAndSettle();
    expect(fakeClient.openedProjectDir, isNull);

    await tester.tap(find.byTooltip('Remove gone from the list'));
    await tester.pumpAndSettle();

    expect(fakeClient.forgottenProjectDir, '/tmp/projects/gone');
    expect(find.text('gone'), findsNothing);
    expect(find.text('kept'), findsOneWidget);
  });

  testWidgets('imports an existing project through the modal dialog', (
    tester,
  ) async {
    final fakeClient = _FakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      openProjectResult: const CreatedProject(
        name: 'verovio',
        projectDir: '/home/user/syntax-bridge-projects/verovio',
        inputSourceDir:
            '/home/user/syntax-bridge-projects/verovio/input-source',
        compilationUnits: [],
        sourceFiles: [],
      ),
    );
    final pathPicker = _FakePathPicker(
      workspaceDir: '/tmp/projects',
      sourceArchive: '/tmp/source.zip',
      existingProjectDir: '/home/user/syntax-bridge-projects/verovio',
    );

    await tester.pumpWidget(
      SyntaxBridgeApp(serverClient: fakeClient, pathPicker: pathPicker),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.widgetWithText(OutlinedButton, 'Import project'));
    await tester.pumpAndSettle();

    expect(find.text(_importDialogPrompt), findsOneWidget);

    // Tapping outside the dialog must not dismiss it: the import modal is
    // intentionally non-dismissible until it succeeds or is cancelled.
    await tester.tapAt(const Offset(5, 5));
    await tester.pumpAndSettle();
    expect(find.text(_importDialogPrompt), findsOneWidget);

    await tester.tap(find.byTooltip('Choose project directory'));
    await tester.pump();
    await tester.tap(find.widgetWithText(FilledButton, 'Import'));
    await tester.pumpAndSettle();

    expect(
      fakeClient.openedProjectDir,
      '/home/user/syntax-bridge-projects/verovio',
    );
    expect(find.text(_importDialogPrompt), findsNothing);
    expect(find.text('Source Files'), findsOneWidget);
  });

  testWidgets('keeps the import dialog open and shows the error on failure', (
    tester,
  ) async {
    final fakeClient = _FakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      openProjectError: const ProjectCreationException(
        'no syntax-bridge project found at /tmp/not-a-project',
      ),
    );
    final pathPicker = _FakePathPicker(
      workspaceDir: '/tmp/projects',
      sourceArchive: '/tmp/source.zip',
      existingProjectDir: '/tmp/not-a-project',
    );

    await tester.pumpWidget(
      SyntaxBridgeApp(serverClient: fakeClient, pathPicker: pathPicker),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.widgetWithText(OutlinedButton, 'Import project'));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('Choose project directory'));
    await tester.pump();
    await tester.tap(find.widgetWithText(FilledButton, 'Import'));
    await tester.pumpAndSettle();

    expect(find.text(_importDialogPrompt), findsOneWidget);
    expect(
      find.text(
        'Import failed: no syntax-bridge project found at /tmp/not-a-project',
      ),
      findsOneWidget,
    );
  });

  testWidgets('captures the screen and logs where it was saved', (
    tester,
  ) async {
    final screenshotStorage = _FakeScreenshotStorage(
      savedPath: '/tmp/screenshots/syntax-bridge-fake.png',
    );

    await tester.pumpWidget(
      SyntaxBridgeApp(
        serverClient: _FakeServerClient(
          const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
        ),
        screenshotStorage: screenshotStorage,
        screenCapturer: _FakeScreenCapturer(),
      ),
    );
    await tester.pumpAndSettle();
    await _skipToIde(tester);

    await tester.tap(find.byTooltip('Capture screen'));
    await tester.pumpAndSettle();

    expect(screenshotStorage.savedBytes, isNotNull);
    expect(screenshotStorage.savedBytes, isNotEmpty);
    expect(
      find.text('Screenshot saved: /tmp/screenshots/syntax-bridge-fake.png'),
      findsOneWidget,
    );
  });
}

const _importDialogPrompt =
    'Point to a directory that already contains a syntax-bridge project.';

Future<void> _openNewProjectPage(WidgetTester tester) async {
  await tester.tap(find.widgetWithText(FilledButton, 'New project'));
  await tester.pumpAndSettle();
}

/// Navigates from the landing page through the new-project form using
/// placeholder values, landing in the IDE. Use this in tests that only care
/// about reaching the IDE, not about the creation form itself.
Future<void> _skipToIde(WidgetTester tester) async {
  await _openNewProjectPage(tester);
  await tester.enterText(find.bySemanticsLabel('Project name'), 'counter');
  await tester.enterText(
    find.bySemanticsLabel('Workspace directory'),
    '/tmp/projects',
  );
  await tester.enterText(
    find.bySemanticsLabel('Source archive'),
    '/tmp/source.tar.gz',
  );
  await tester.pump();
  await tester.tap(find.widgetWithText(FilledButton, 'Create project'));
  await tester.pumpAndSettle();
}

class _FakeScreenCapturer implements ScreenCapturer {
  @override
  Future<Uint8List> capture(
    GlobalKey boundaryKey, {
    required double pixelRatio,
  }) async {
    return Uint8List.fromList([1, 2, 3, 4]);
  }
}

class _FakeScreenshotStorage implements ScreenshotStorage {
  _FakeScreenshotStorage({required this.savedPath});

  final String savedPath;
  Uint8List? savedBytes;

  @override
  Future<String> save(Uint8List pngBytes) async {
    savedBytes = pngBytes;
    return savedPath;
  }
}

class _FakeServerClient implements ServerClient {
  _FakeServerClient(
    this.status, {
    this.project,
    this.createError,
    this.sourceFileContent = '',
    this.recentProjects = const <RecentProject>[],
    this.openProjectResult,
    this.openProjectError,
    this.types = const <TypeDeclaration>[],
    this.usageCounts = const <String, int>{},
    this.usagesByType = const <String, List<TypeUsage>>{},
    this.functions = const <FunctionDeclaration>[],
    this.callerCounts = const <String, int>{},
    this.callersByFunction = const <String, List<CallEdge>>{},
    this.callsByFile = const <String, List<CallEdge>>{},
    this.transpiledPackage,
  });

  final ServerStatus status;
  final CreatedProject? project;
  final Object? createError;
  final String sourceFileContent;
  final List<RecentProject> recentProjects;
  final CreatedProject? openProjectResult;
  final Object? openProjectError;
  final List<TypeDeclaration> types;
  final Map<String, int> usageCounts;
  final Map<String, List<TypeUsage>> usagesByType;
  final List<FunctionDeclaration> functions;
  final Map<String, int> callerCounts;
  final Map<String, List<CallEdge>> callersByFunction;
  final Map<String, List<CallEdge>> callsByFile;
  final TranspiledPackage? transpiledPackage;
  String? createdProjectName;
  String? readSourceFilePath;
  String? openedProjectDir;
  String? forgottenProjectDir;
  String? transpileProjectDir;
  late List<RecentProject> _remainingProjects = recentProjects;

  @override
  Future<ServerStatus> health() async => status;

  @override
  Future<void> forgetProject(String projectDir) async {
    forgottenProjectDir = projectDir;
    _remainingProjects = _remainingProjects
        .where((project) => project.projectDir != projectDir)
        .toList();
  }

  @override
  Future<String> startCreateProject(CreateProjectInput input) async {
    createdProjectName = input.name;
    return 'job-1';
  }

  @override
  Future<void> cancelCreateProject(String jobId) async {}

  @override
  Future<ProjectCreationJobStatus> pollCreateProjectJob(String jobId) async {
    final error = createError;
    if (error != null) {
      final message = error is ProjectCreationException
          ? error.message
          : error.toString();
      return ProjectCreationJobStatus(
        state: ProjectCreationJobState.failed,
        errorMessage: message,
        isClientError: true,
      );
    }

    return ProjectCreationJobStatus(
      state: ProjectCreationJobState.succeeded,
      project:
          project ??
          const CreatedProject(
            name: 'empty',
            projectDir: '',
            inputSourceDir: '',
            compilationUnits: [],
          ),
    );
  }

  @override
  Future<List<RecentProject>> listRecentProjects() async => _remainingProjects;

  @override
  Future<CreatedProject> openProject(String projectDir) async {
    openedProjectDir = projectDir;
    final error = openProjectError;
    if (error != null) {
      throw error;
    }

    return openProjectResult ??
        const CreatedProject(
          name: 'opened',
          projectDir: '',
          inputSourceDir: '',
          compilationUnits: [],
        );
  }

  @override
  Future<String> readSourceFile({
    required String projectDir,
    required String path,
  }) async {
    readSourceFilePath = path;
    return sourceFileContent;
  }

  @override
  Future<TypeCatalogListing> listTypes(String projectDir) async =>
      TypeCatalogListing(types: types, usageCounts: usageCounts);

  @override
  Future<List<TypeUsage>> listTypeUsages({
    required String projectDir,
    required String typeUsr,
  }) async => usagesByType[typeUsr] ?? const <TypeUsage>[];

  @override
  Future<FunctionCatalogListing> listFunctions(String projectDir) async =>
      FunctionCatalogListing(functions: functions, callerCounts: callerCounts);

  @override
  Future<List<CallEdge>> listCallers({
    required String projectDir,
    required String functionUsr,
  }) async => callersByFunction[functionUsr] ?? const <CallEdge>[];

  @override
  Future<List<CallEdge>> listCallsInFile({
    required String projectDir,
    required String file,
  }) async => callsByFile[file] ?? const <CallEdge>[];

  @override
  Future<TranspiledPackage> transpileProject(String projectDir) async {
    transpileProjectDir = projectDir;
    return transpiledPackage ??
        const TranspiledPackage(packageName: 'output', files: {});
  }
}

class _FakePathPicker implements PathPicker {
  _FakePathPicker({
    required this.workspaceDir,
    required this.sourceArchive,
    this.existingProjectDir,
  });

  final String workspaceDir;
  final String sourceArchive;
  final String? existingProjectDir;
  int workspacePickCount = 0;
  int archivePickCount = 0;
  int existingProjectPickCount = 0;

  @override
  Future<String?> pickSourceArchive() async {
    archivePickCount += 1;
    return sourceArchive;
  }

  @override
  Future<String?> pickWorkspaceDirectory() async {
    workspacePickCount += 1;
    return workspaceDir;
  }

  @override
  Future<String?> pickExistingProjectDirectory() async {
    existingProjectPickCount += 1;
    return existingProjectDir;
  }
}
