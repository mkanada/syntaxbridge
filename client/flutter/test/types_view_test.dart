import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/src/project/project_models.dart';
import 'package:syntax_bridge/src/ui/types_view.dart';

/// US-3's only visible surface today (its route and backend already exist):
/// a table of the project's named types and macro declarations, showing name
/// and kind, grouped into a "Types" and a "Constants & macros" section. No UI
/// path to functions or macro call sites here — those belong to US-5.
/// Header guards and other valueless annotation macros are pure preprocessor
/// plumbing and never shown at all.
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
    kind: TypeDeclarationKind.constantMacro,
    file: '/workspace/src/consts.h',
    line: 1,
    column: 9,
  );
  const square = TypeDeclaration(
    name: 'SQUARE',
    kind: TypeDeclarationKind.functionMacro,
    file: '/workspace/src/consts.h',
    line: 2,
    column: 9,
  );
  const includeGuard = TypeDeclaration(
    name: 'CONSTS_H',
    kind: TypeDeclarationKind.headerGuard,
    file: '/workspace/src/consts.h',
    line: 1,
    column: 9,
  );
  const exportAnnotation = TypeDeclaration(
    name: 'MYLIB_API',
    kind: TypeDeclarationKind.annotationMacro,
    file: '/workspace/src/consts.h',
    line: 3,
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
  const rectangle = TypeDeclaration(
    name: 'Rectangle',
    kind: TypeDeclarationKind.struct,
    file: '/workspace/src/types.h',
    line: 10,
    column: 8,
  );

  testWidgets('lists each type with its name and kind', (tester) async {
    await tester.pumpWidget(_host(const [point, widget, answer]));

    expect(find.text('Point'), findsOneWidget);
    expect(_rowLabel('struct'), findsOneWidget);
    expect(find.text('Widget'), findsOneWidget);
    expect(_rowLabel('class'), findsOneWidget);
    expect(find.text('ANSWER'), findsOneWidget);
    expect(_rowLabel('constant'), findsOneWidget);
  });

  testWidgets('groups constant and function macros in their own section', (
    tester,
  ) async {
    await tester.pumpWidget(_host(const [point, answer, square]));

    expect(find.text('Constants & macros'), findsOneWidget);
    expect(find.text('ANSWER'), findsOneWidget);
    expect(_rowLabel('constant'), findsOneWidget);
    expect(find.text('SQUARE'), findsOneWidget);
    expect(_rowLabel('function macro'), findsOneWidget);
  });

  testWidgets('hides header guards and other valueless annotation macros', (
    tester,
  ) async {
    await tester.pumpWidget(
      _host(const [point, includeGuard, exportAnnotation]),
    );

    expect(find.text('3 declared'), findsNothing);
    expect(find.text('1 declared'), findsOneWidget);
    expect(find.text('CONSTS_H'), findsNothing);
    expect(find.text('MYLIB_API'), findsNothing);
    expect(find.text('Constants & macros'), findsNothing);
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

  testWidgets('sorts declarations by name ascending by default', (
    tester,
  ) async {
    await tester.pumpWidget(_host(const [widget, point]));

    final pointY = tester.getTopLeft(find.text('Point')).dy;
    final widgetY = tester.getTopLeft(find.text('Widget')).dy;
    expect(pointY, lessThan(widgetY));
  });

  testWidgets(
    'reverses direction when the active sort control is tapped again',
    (tester) async {
      await tester.pumpWidget(_host(const [widget, point]));

      await tester.tap(find.text('Name'));
      await tester.pump();

      final pointY = tester.getTopLeft(find.text('Point')).dy;
      final widgetY = tester.getTopLeft(find.text('Widget')).dy;
      expect(widgetY, lessThan(pointY));
    },
  );

  testWidgets(
    'sorts by kind, grouping same-kind declarations and breaking ties by name',
    (tester) async {
      await tester.pumpWidget(_host(const [rectangle, point, widget]));

      await tester.tap(find.text('Kind'));
      await tester.pump();

      final widgetY = tester.getTopLeft(find.text('Widget')).dy;
      final pointY = tester.getTopLeft(find.text('Point')).dy;
      final rectangleY = tester.getTopLeft(find.text('Rectangle')).dy;
      expect(widgetY, lessThan(pointY), reason: 'class sorts before struct');
      expect(
        pointY,
        lessThan(rectangleY),
        reason: 'Point sorts before Rectangle',
      );
    },
  );

  testWidgets(
    'hides every declaration of a kind when its filter chip is deselected',
    (tester) async {
      await tester.pumpWidget(_host(const [point, widget]));

      expect(find.text('Widget'), findsOneWidget);
      expect(find.text('2 declared'), findsOneWidget);

      await tester.tap(find.widgetWithText(FilterChip, 'class'));
      await tester.pump();

      expect(find.text('Widget'), findsNothing);
      expect(find.text('Point'), findsOneWidget);
      expect(find.text('1 declared'), findsOneWidget);

      await tester.tap(find.widgetWithText(FilterChip, 'class'));
      await tester.pump();

      expect(find.text('Widget'), findsOneWidget);
      expect(find.text('2 declared'), findsOneWidget);
    },
  );

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

/// A kind label as shown in a row's trailing text — scoped to the list so it
/// doesn't collide with the same word used as a filter chip's label.
Finder _rowLabel(String label) {
  return find.descendant(of: find.byType(ListView), matching: find.text(label));
}
