import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/src/project/project_models.dart';
import 'package:syntax_bridge/src/ui/types_view.dart';

/// US-3's only visible surface today (its route and backend already exist):
/// a table of the project's named types, showing name and kind, with no UI
/// path to functions or macros — those belong to US-5.
void main() {
  const point = TypeDeclaration(
    name: 'Point',
    kind: TypeDeclarationKind.struct,
    file: '/workspace/src/types.h',
    line: 3,
    column: 8,
  );
  const widget = TypeDeclaration(
    name: 'Widget',
    kind: TypeDeclarationKind.class_,
    file: '/workspace/src/widget.h',
    line: 5,
    column: 1,
  );
  const answer = TypeDeclaration(
    name: 'ANSWER',
    kind: TypeDeclarationKind.macro,
    file: '/workspace/src/consts.h',
    line: 1,
    column: 9,
  );
  const namespacedShape = TypeDeclaration(
    name: 'Shape',
    kind: TypeDeclarationKind.class_,
    namespace: 'geometry',
    file: '/workspace/src/shape.h',
    line: 4,
    column: 7,
  );
  const topLevelPoint = TypeDeclaration(
    name: 'Point',
    kind: TypeDeclarationKind.struct,
    file: '/workspace/src/types.h',
    line: 2,
    column: 8,
  );
  const nestedPoint = TypeDeclaration(
    name: 'Point',
    kind: TypeDeclarationKind.struct,
    namespace: 'geometry',
    file: '/workspace/src/shape.h',
    line: 9,
    column: 8,
  );

  testWidgets('lists each type with its name and kind', (tester) async {
    await tester.pumpWidget(_host(const [point, widget, answer]));

    expect(find.text('Point'), findsOneWidget);
    expect(find.text('struct'), findsOneWidget);
    expect(find.text('Widget'), findsOneWidget);
    expect(find.text('class'), findsOneWidget);
    expect(find.text('ANSWER'), findsOneWidget);
    expect(find.text('macro'), findsOneWidget);
  });

  testWidgets('shows an empty state when there are no types', (tester) async {
    await tester.pumpWidget(_host(const []));

    expect(find.text('No types found'), findsOneWidget);
  });

  testWidgets('qualifies a namespaced type with its namespace', (tester) async {
    await tester.pumpWidget(_host(const [namespacedShape]));

    expect(find.text('geometry::Shape'), findsOneWidget);
    expect(find.text('Shape'), findsNothing);
  });

  testWidgets('distinguishes homonym types declared in different namespaces', (
    tester,
  ) async {
    await tester.pumpWidget(_host(const [topLevelPoint, nestedPoint]));

    expect(find.text('Point'), findsOneWidget);
    expect(find.text('geometry::Point'), findsOneWidget);
  });

  testWidgets('reports a click on a type', (tester) async {
    TypeDeclaration? selected;

    await tester.pumpWidget(
      _host(const [point, widget], onTypeSelected: (type) => selected = type),
    );
    await tester.tap(find.text('Widget'));

    expect(selected, widget);
  });
}

Widget _host(
  List<TypeDeclaration> types, {
  ValueChanged<TypeDeclaration>? onTypeSelected,
}) {
  return MaterialApp(
    home: Scaffold(
      body: TypesView(types: types, onTypeSelected: onTypeSelected ?? (_) {}),
    ),
  );
}
