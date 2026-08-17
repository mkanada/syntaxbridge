import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/src/project/project_models.dart';
import 'package:syntax_bridge/src/ui/source_files_view.dart';

/// US-2's source file navigator, plus the "mark this file external" action
/// `docs/plans/lista-de-externos.md` adds to it (decision 3's cascade entry
/// point for a whole file).
const _project = CreatedProject(
  name: 'counter',
  projectDir: '/tmp/projects/counter',
  inputSourceDir: '/tmp/projects/counter/input-source',
  compilationUnits: [],
  sourceFiles: [
    SourceFile(
      path: '/tmp/projects/counter/input-source/src/main.cpp',
      kind: SourceFileKind.translationUnit,
    ),
    SourceFile(
      path: '/tmp/projects/counter/input-source/src/types.h',
      kind: SourceFileKind.header,
    ),
  ],
);

void main() {
  testWidgets('lists each source file with its project-relative path', (
    tester,
  ) async {
    await tester.pumpWidget(_host());

    expect(find.text('input-source/src/main.cpp'), findsOneWidget);
    expect(find.text('input-source/src/types.h'), findsOneWidget);
  });

  testWidgets('reports a click on a file', (tester) async {
    SourceFile? selected;
    await tester.pumpWidget(_host(onFileSelected: (file) => selected = file));

    await tester.tap(find.text('input-source/src/main.cpp'));

    expect(selected?.path, _project.sourceFiles[0].path);
  });

  testWidgets('hides the mark-external action when no callback is provided', (
    tester,
  ) async {
    await tester.pumpWidget(_host());

    expect(find.byIcon(Icons.link), findsNothing);
  });

  testWidgets('marks a file external when its row action is tapped', (
    tester,
  ) async {
    SourceFile? marked;
    await tester.pumpWidget(_host(onMarkFileExternal: (file) => marked = file));

    final markButtons = find.byIcon(Icons.link);
    expect(markButtons, findsNWidgets(2));
    await tester.tap(markButtons.first);
    await tester.pump();

    expect(marked?.path, _project.sourceFiles[0].path);
  });
}

Widget _host({
  ValueChanged<SourceFile>? onFileSelected,
  ValueChanged<SourceFile>? onMarkFileExternal,
}) {
  return MaterialApp(
    home: Scaffold(
      body: SourceFilesView(
        project: _project,
        onFileSelected: onFileSelected ?? (_) {},
        onMarkFileExternal: onMarkFileExternal,
      ),
    ),
  );
}
