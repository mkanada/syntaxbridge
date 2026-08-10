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

  testWidgets('reports a click on a usage', (tester) async {
    TypeUsage? selected;

    await tester.pumpWidget(
      _host(
        selectedType: point,
        usages: const [fieldUsage, parameterUsage],
        onUsageSelected: (usage) => selected = usage,
      ),
    );
    await tester.tap(find.text('main.cpp:4'));

    expect(selected, parameterUsage);
  });
}

Widget _host({
  required TypeDeclaration? selectedType,
  required List<TypeUsage> usages,
  ValueChanged<TypeUsage>? onUsageSelected,
}) {
  return MaterialApp(
    home: Scaffold(
      body: UsagesView(
        selectedType: selectedType,
        usages: usages,
        onUsageSelected: onUsageSelected ?? (_) {},
      ),
    ),
  );
}
