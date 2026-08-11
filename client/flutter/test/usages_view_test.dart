import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/src/project/project_models.dart';
import 'package:syntax_bridge/src/ui/usages_view.dart';

/// US-4's usages panel: every recorded usage of the type currently selected
/// in the Types navigator, each clickable to jump straight to that exact
/// location (criterion 5).
void main() {
  const point = TypeDeclaration(
    name: 'Point',
    kind: TypeDeclarationKind.struct,
    namespace: 'geometry',
    file: '/workspace/src/types.h',
    line: 3,
    column: 8,
    usr: 'c:@N@geometry@S@Point',
  );

  const fieldUsage = TypeUsage(
    typeUsr: 'c:@N@geometry@S@Point',
    kind: TypeUsageKind.field,
    file: '/workspace/src/panel.h',
    line: 9,
    column: 11,
  );
  const parameterUsage = TypeUsage(
    typeUsr: 'c:@N@geometry@S@Point',
    kind: TypeUsageKind.parameter,
    file: '/workspace/src/main.cpp',
    line: 4,
    column: 22,
  );

  testWidgets('shows a prompt when no type is selected', (tester) async {
    await tester.pumpWidget(_host(selectedType: null, usages: const []));

    expect(find.text('Select a type to see where it is used'), findsOneWidget);
  });

  testWidgets('lists every usage of the selected type', (tester) async {
    await tester.pumpWidget(
      _host(selectedType: point, usages: const [fieldUsage, parameterUsage]),
    );

    expect(find.text('2 uses of geometry::Point'), findsOneWidget);
    expect(find.text('panel.h:9'), findsOneWidget);
    expect(find.text('field'), findsOneWidget);
    expect(find.text('main.cpp:4'), findsOneWidget);
    expect(find.text('parameter'), findsOneWidget);
  });

  testWidgets('shows an empty state when the type has no usages', (
    tester,
  ) async {
    await tester.pumpWidget(_host(selectedType: point, usages: const []));

    expect(find.text('No usages found'), findsOneWidget);
  });

  testWidgets('clicking the open-in-editor action jumps to the usage, without '
      'expanding the accordion', (tester) async {
    TypeUsage? selected;
    var loadCalled = false;

    await tester.pumpWidget(
      _host(
        selectedType: point,
        usages: const [fieldUsage, parameterUsage],
        onUsageSelected: (usage) => selected = usage,
        loadUsageSource: (usage) async {
          loadCalled = true;
          return '';
        },
      ),
    );
    await tester.tap(
      find.byKey(
        Key('usage-open-${parameterUsage.file}:${parameterUsage.line}'),
      ),
    );
    await tester.pumpAndSettle();

    expect(selected, parameterUsage);
    expect(loadCalled, isFalse);
  });

  testWidgets('usage items start collapsed, with no code visible', (
    tester,
  ) async {
    await tester.pumpWidget(
      _host(
        selectedType: point,
        usages: const [fieldUsage],
        loadUsageSource: (usage) async => 'Point origin; // field usage',
      ),
    );

    expect(find.text('Point origin; // field usage'), findsNothing);
  });

  testWidgets('clicking a usage expands it to show the code where it is used, '
      'without navigating the main viewer', (tester) async {
    TypeUsage? selected;
    final lines = List.generate(20, (i) => 'line ${i + 1}');
    lines[8] = 'Point origin; // field usage';
    final content = lines.join('\n');

    await tester.pumpWidget(
      _host(
        selectedType: point,
        usages: const [fieldUsage],
        onUsageSelected: (usage) => selected = usage,
        loadUsageSource: (usage) async => content,
      ),
    );

    await tester.tap(find.text('panel.h:9'));
    await tester.pumpAndSettle();

    expect(find.text('Point origin; // field usage'), findsOneWidget);
    expect(selected, isNull);
  });
}

Widget _host({
  required TypeDeclaration? selectedType,
  required List<TypeUsage> usages,
  ValueChanged<TypeUsage>? onUsageSelected,
  Future<String> Function(TypeUsage usage)? loadUsageSource,
}) {
  return MaterialApp(
    home: Scaffold(
      body: UsagesView(
        selectedType: selectedType,
        usages: usages,
        onUsageSelected: onUsageSelected ?? (_) {},
        loadUsageSource: loadUsageSource ?? (_) async => '',
      ),
    ),
  );
}
