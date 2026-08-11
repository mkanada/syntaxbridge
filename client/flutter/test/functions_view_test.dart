import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/src/project/project_models.dart';
import 'package:syntax_bridge/src/ui/functions_view.dart';

/// US-5's function catalog navigator: every free function, method,
/// constructor, destructor and function-like macro declared in the project,
/// with its full signature, sortable by name, kind, or caller count.
void main() {
  const addInt = FunctionDeclaration(
    name: 'add',
    kind: FunctionDeclarationKind.freeFunction,
    signature: 'int add(int a, int b)',
    file: '/workspace/src/math.cpp',
    line: 1,
    column: 5,
    usr: 'c:@F@add#I#I#',
  );
  const addDouble = FunctionDeclaration(
    name: 'add',
    kind: FunctionDeclarationKind.freeFunction,
    signature: 'double add(double a, double b)',
    file: '/workspace/src/math.cpp',
    line: 5,
    column: 8,
    usr: 'c:@F@add#d#d#',
  );
  const area = FunctionDeclaration(
    name: 'area',
    kind: FunctionDeclarationKind.method,
    namespace: 'geometry',
    signature: 'double geometry::Shape::area() const',
    file: '/workspace/src/shapes.h',
    line: 4,
    column: 19,
    usr: 'c:@N@geometry@S@Shape@F@area#1#',
    isVirtual: true,
  );
  const square = FunctionDeclaration(
    name: 'SQUARE',
    kind: FunctionDeclarationKind.functionMacro,
    signature: 'SQUARE(...)',
    file: '/workspace/src/math.cpp',
    line: 1,
    column: 9,
  );

  testWidgets('lists each function with its qualified name and signature', (
    tester,
  ) async {
    await tester.pumpWidget(_host(const [addInt, area]));

    expect(find.text('add'), findsOneWidget);
    expect(find.text('int add(int a, int b)'), findsOneWidget);
    expect(find.text('geometry::area'), findsOneWidget);
    expect(find.text('double geometry::Shape::area() const'), findsOneWidget);
    expect(find.widgetWithText(ListTile, 'method'), findsOneWidget);
    expect(find.widgetWithText(ListTile, 'function'), findsOneWidget);
  });

  testWidgets('shows two overloads of the same name as distinct entries', (
    tester,
  ) async {
    await tester.pumpWidget(_host(const [addInt, addDouble]));

    expect(find.text('add'), findsNWidgets(2));
    expect(find.text('int add(int a, int b)'), findsOneWidget);
    expect(find.text('double add(double a, double b)'), findsOneWidget);
  });

  testWidgets('marks virtual methods', (tester) async {
    await tester.pumpWidget(_host(const [addInt, area]));

    expect(find.text('virtual'), findsOneWidget);
  });

  testWidgets('shows a function-like macro alongside functions', (
    tester,
  ) async {
    await tester.pumpWidget(_host(const [addInt, square]));

    expect(find.text('SQUARE'), findsOneWidget);
    expect(find.widgetWithText(ListTile, 'function macro'), findsOneWidget);
  });

  testWidgets('shows an empty state when there are no functions', (
    tester,
  ) async {
    await tester.pumpWidget(_host(const []));

    expect(find.text('No functions found'), findsOneWidget);
  });

  testWidgets('sorts declarations by name ascending by default', (
    tester,
  ) async {
    await tester.pumpWidget(_host(const [area, addInt]));

    final addY = tester.getTopLeft(find.text('add')).dy;
    final areaY = tester.getTopLeft(find.text('geometry::area')).dy;
    expect(addY, lessThan(areaY));
  });

  testWidgets(
    'reverses direction when the active sort control is tapped again',
    (tester) async {
      await tester.pumpWidget(_host(const [area, addInt]));

      await tester.tap(find.text('Name'));
      await tester.pump();

      final addY = tester.getTopLeft(find.text('add')).dy;
      final areaY = tester.getTopLeft(find.text('geometry::area')).dy;
      expect(areaY, lessThan(addY));
    },
  );

  testWidgets('sorts by kind, grouping same-kind declarations', (tester) async {
    await tester.pumpWidget(_host(const [area, addInt]));

    await tester.tap(find.text('Kind'));
    await tester.pump();

    final addY = tester.getTopLeft(find.text('add')).dy;
    final areaY = tester.getTopLeft(find.text('geometry::area')).dy;
    expect(addY, lessThan(areaY), reason: 'function sorts before method');
  });

  testWidgets('reports a click on a function', (tester) async {
    FunctionDeclaration? selected;

    await tester.pumpWidget(
      _host(const [
        addInt,
        area,
      ], onFunctionSelected: (function) => selected = function),
    );
    await tester.tap(find.text('geometry::area'));

    expect(selected, area);
  });

  testWidgets("shows each function's caller count", (tester) async {
    await tester.pumpWidget(
      _host(
        const [addInt, area],
        callerCounts: const {
          'c:@F@add#I#I#': 3,
          'c:@N@geometry@S@Shape@F@area#1#': 0,
        },
      ),
    );

    expect(find.text('3 callers'), findsOneWidget);
    expect(find.text('0 callers'), findsOneWidget);
  });

  testWidgets('sorts by caller count, breaking ties by name', (tester) async {
    await tester.pumpWidget(
      _host(
        const [addInt, area],
        callerCounts: const {
          'c:@F@add#I#I#': 1,
          'c:@N@geometry@S@Shape@F@area#1#': 5,
        },
      ),
    );

    await tester.tap(find.text('Called'));
    await tester.pump();

    final addY = tester.getTopLeft(find.text('add')).dy;
    final areaY = tester.getTopLeft(find.text('geometry::area')).dy;
    expect(addY, lessThan(areaY), reason: '1 caller sorts before 5 callers');

    await tester.tap(find.text('Called'));
    await tester.pump();

    final addYDescending = tester.getTopLeft(find.text('add')).dy;
    final areaYDescending = tester.getTopLeft(find.text('geometry::area')).dy;
    expect(
      areaYDescending,
      lessThan(addYDescending),
      reason: 'reversed: 5 callers sorts before 1 caller',
    );
  });
}

Widget _host(
  List<FunctionDeclaration> functions, {
  ValueChanged<FunctionDeclaration>? onFunctionSelected,
  Map<String, int> callerCounts = const {},
}) {
  return MaterialApp(
    home: Scaffold(
      body: FunctionsView(
        functions: functions,
        onFunctionSelected: onFunctionSelected ?? (_) {},
        callerCounts: callerCounts,
      ),
    ),
  );
}
