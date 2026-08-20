import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/src/project/project_models.dart';
import 'package:syntax_bridge/src/ui/source_files_view.dart';

/// US-2's source file navigator, plus the persistent "mark this file
/// external" toggle item 3 (`docs/prompts/2026-08-19-mudanca-interacao.md`)
/// adds to it — a reversal of decision 3's cascade-snapshot behavior in
/// `docs/plans/lista-de-externos.md`.
const _project = CreatedProject(
  name: 'counter',
  projectDir: '/tmp/projects/counter',
  inputSourceDir: '/tmp/projects/counter/input-source',
  compilationUnits: [],
  sourceFiles: [
    SourceFile(
      path: '/tmp/projects/counter/input-source/verovio-1.0/src/main.cpp',
      kind: SourceFileKind.translationUnit,
    ),
    SourceFile(
      path: '/tmp/projects/counter/input-source/verovio-1.0/src/types.h',
      kind: SourceFileKind.header,
    ),
    SourceFile(
      path: '/tmp/projects/counter/input-source/loose.cpp',
      kind: SourceFileKind.translationUnit,
    ),
  ],
);

void main() {
  testWidgets('lists each source file relative to the extracted archive root, '
      'stripping both input-source/ and the archive-root folder', (
    tester,
  ) async {
    await tester.pumpWidget(_host());

    expect(find.text('src/main.cpp'), findsOneWidget);
    expect(find.text('src/types.h'), findsOneWidget);
  });

  testWidgets(
    'keeps a file at the input-source root as-is when there is no archive-root folder to strip',
    (tester) async {
      await tester.pumpWidget(_host());

      expect(find.text('loose.cpp'), findsOneWidget);
    },
  );

  testWidgets('reports a click on a file', (tester) async {
    SourceFile? selected;
    await tester.pumpWidget(_host(onFileSelected: (file) => selected = file));

    await tester.tap(find.text('src/main.cpp'));

    expect(selected?.path, _project.sourceFiles[0].path);
  });

  testWidgets('hides the mark-external toggle when no callback is provided', (
    tester,
  ) async {
    await tester.pumpWidget(_host());

    expect(find.byIcon(Icons.link), findsNothing);
    expect(find.byIcon(Icons.link_off), findsNothing);
  });

  testWidgets(
    'shows every row unmarked when externalFiles is empty, and reports a toggle',
    (tester) async {
      SourceFile? toggled;
      await tester.pumpWidget(
        _host(onToggleFileExternal: (file) => toggled = file),
      );

      final markButtons = find.byIcon(Icons.link);
      expect(markButtons, findsNWidgets(3));
      expect(find.byIcon(Icons.link_off), findsNothing);

      await tester.tap(markButtons.first);
      await tester.pump();

      expect(toggled?.path, _project.sourceFiles[0].path);
    },
  );

  testWidgets(
    'shows a row already in externalFiles as marked, with the off icon',
    (tester) async {
      await tester.pumpWidget(
        _host(
          externalFiles: {_project.sourceFiles[0].path},
          onToggleFileExternal: (_) {},
        ),
      );

      expect(find.byIcon(Icons.link_off), findsOneWidget);
      expect(find.byIcon(Icons.link), findsNWidgets(2));
    },
  );
}

Widget _host({
  ValueChanged<SourceFile>? onFileSelected,
  Set<String> externalFiles = const <String>{},
  ValueChanged<SourceFile>? onToggleFileExternal,
}) {
  return MaterialApp(
    home: Scaffold(
      body: SourceFilesView(
        project: _project,
        onFileSelected: onFileSelected ?? (_) {},
        externalFiles: externalFiles,
        onToggleFileExternal: onToggleFileExternal,
      ),
    ),
  );
}
