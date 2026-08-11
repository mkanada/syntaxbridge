import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/src/project/project_models.dart';
import 'package:syntax_bridge/src/ui/callers_view.dart';

/// US-5's callers panel: every recorded call to the function currently
/// selected in the Functions navigator, each clickable to jump straight to
/// that exact call site (criterion 5), flagging virtual dispatch (criterion
/// 3) and unresolved calls (criterion 6) rather than presenting every call
/// as a plain direct one.
void main() {
  const area = FunctionDeclaration(
    name: 'area',
    kind: FunctionDeclarationKind.method,
    namespace: 'geometry',
    signature: 'double geometry::Shape::area() const',
    file: '/workspace/src/shapes.h',
    line: 4,
    column: 19,
    usr: 'c:@N@geometry@S@Shape@F@area#1#',
  );

  const directCall = CallEdge(
    callerUsr: 'c:@F@compute#',
    resolution: CallResolution.resolved(
      calleeUsr: 'c:@N@geometry@S@Shape@F@area#1#',
      isDynamicDispatch: false,
    ),
    file: '/workspace/src/math.cpp',
    line: 20,
    column: 18,
  );
  const dynamicCall = CallEdge(
    callerUsr: 'c:@F@describe#',
    resolution: CallResolution.resolved(
      calleeUsr: 'c:@N@geometry@S@Shape@F@area#1#',
      isDynamicDispatch: true,
    ),
    file: '/workspace/src/shapes.cpp',
    line: 10,
    column: 19,
  );
  const unresolvedCall = CallEdge(
    callerUsr: 'c:@F@apply#',
    resolution: CallResolution.unresolved(
      unresolvedReason: 'call target is not statically a function',
    ),
    file: '/workspace/src/math.cpp',
    line: 25,
    column: 12,
  );

  testWidgets('shows a prompt when no function is selected', (tester) async {
    await tester.pumpWidget(_host(selectedFunction: null, callers: const []));

    expect(find.text('Select a function to see its callers'), findsOneWidget);
  });

  testWidgets('lists every caller of the selected function', (tester) async {
    await tester.pumpWidget(
      _host(selectedFunction: area, callers: const [directCall, dynamicCall]),
    );

    expect(find.text('2 callers of geometry::area'), findsOneWidget);
    expect(find.text('math.cpp:20'), findsOneWidget);
    expect(find.text('direct call'), findsOneWidget);
    expect(find.text('shapes.cpp:10'), findsOneWidget);
    expect(find.text('dynamic dispatch'), findsOneWidget);
  });

  testWidgets('flags an unresolved call instead of omitting it', (
    tester,
  ) async {
    await tester.pumpWidget(
      _host(selectedFunction: area, callers: const [unresolvedCall]),
    );

    expect(find.text('math.cpp:25'), findsOneWidget);
    expect(
      find.text('unresolved: call target is not statically a function'),
      findsOneWidget,
    );
  });

  testWidgets('shows an empty state when the function has no callers', (
    tester,
  ) async {
    await tester.pumpWidget(_host(selectedFunction: area, callers: const []));

    expect(find.text('No callers found'), findsOneWidget);
  });

  testWidgets('clicking the open-in-editor action jumps to the caller, without '
      'expanding the accordion', (tester) async {
    CallEdge? selected;
    var loadCalled = false;

    await tester.pumpWidget(
      _host(
        selectedFunction: area,
        callers: const [directCall, dynamicCall],
        onCallerSelected: (caller) => selected = caller,
        loadCallerSource: (caller) async {
          loadCalled = true;
          return '';
        },
      ),
    );
    await tester.tap(
      find.byKey(Key('caller-open-${dynamicCall.file}:${dynamicCall.line}')),
    );
    await tester.pumpAndSettle();

    expect(selected, dynamicCall);
    expect(loadCalled, isFalse);
  });

  testWidgets('caller items start collapsed, with no code visible', (
    tester,
  ) async {
    await tester.pumpWidget(
      _host(
        selectedFunction: area,
        callers: const [directCall],
        loadCallerSource: (caller) async => 'shape.area(); // direct call',
      ),
    );

    expect(find.text('shape.area(); // direct call'), findsNothing);
  });

  testWidgets(
    'clicking a caller expands it to show the code at that call site, '
    'without navigating the main viewer',
    (tester) async {
      CallEdge? selected;
      final lines = List.generate(30, (i) => 'line ${i + 1}');
      lines[19] = 'shape.area(); // direct call';
      final content = lines.join('\n');

      await tester.pumpWidget(
        _host(
          selectedFunction: area,
          callers: const [directCall],
          onCallerSelected: (caller) => selected = caller,
          loadCallerSource: (caller) async => content,
        ),
      );

      await tester.tap(find.text('math.cpp:20'));
      await tester.pumpAndSettle();

      expect(find.text('shape.area(); // direct call'), findsOneWidget);
      expect(selected, isNull);
    },
  );
}

Widget _host({
  required FunctionDeclaration? selectedFunction,
  required List<CallEdge> callers,
  ValueChanged<CallEdge>? onCallerSelected,
  Future<String> Function(CallEdge caller)? loadCallerSource,
}) {
  return MaterialApp(
    home: Scaffold(
      body: CallersView(
        selectedFunction: selectedFunction,
        callers: callers,
        onCallerSelected: onCallerSelected ?? (_) {},
        loadCallerSource: loadCallerSource ?? (_) async => '',
      ),
    ),
  );
}
