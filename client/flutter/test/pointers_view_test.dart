import 'dart:collection';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/src/project/project_models.dart';
import 'package:syntax_bridge/src/ui/pointers_view.dart';

/// Regression coverage for a crash reported while browsing the pointer
/// catalog of a large project (Verovio, ~16k pointers): [PointersView] built
/// every row eagerly via `ListView(children: ...)`, so opening the tab
/// allocated tens of thousands of widgets up front instead of only the ones
/// on screen.
void main() {
  const forma = PointerDeclaration(
    kind: PointerDeclarationKind.parameter,
    shape: PointerShape.scalar,
    name: 'forma',
    pointeeTypeName: 'Forma',
    file: '/workspace/src/desenha.cpp',
    line: 5,
    column: 20,
  );

  testWidgets(
    'only builds the rows that fit on screen for a large pointer catalog',
    (tester) async {
      final pointers = [
        for (var i = 0; i < 5000; i++)
          PointerDeclaration(
            kind: PointerDeclarationKind.local,
            shape: PointerShape.scalar,
            name: 'p$i',
            pointeeTypeName: 'Forma',
            file: '/workspace/src/desenha.cpp',
            line: i,
            column: 1,
          ),
      ];

      await tester.pumpWidget(_host(pointers));

      expect(
        find.byType(ListTile).evaluate().length,
        lessThan(pointers.length),
        reason:
            'ListView should build lazily; building every row up front is '
            'what crashed the app on the Verovio pointer catalog',
      );
    },
  );

  testWidgets('lists a pointer with its name, pointee and shape', (
    tester,
  ) async {
    await tester.pumpWidget(_host(const [forma]));

    expect(find.text('forma'), findsOneWidget);
    expect(find.text('Forma*'), findsOneWidget);
  });

  testWidgets('shows an empty state when there are no pointers', (
    tester,
  ) async {
    await tester.pumpWidget(_host(const []));

    expect(find.text('No pointers found'), findsOneWidget);
  });

  testWidgets(
    'does not re-filter/re-sort on a rebuild unrelated to its own state '
    '(e.g. a host panel resizing while this tab is open)',
    (tester) async {
      final pointers = _CountingPointerList([forma]);

      await tester.pumpWidget(_hostWithExternalRebuilds(pointers));
      expect(pointers.iterations, 1);

      // Simulate an ancestor (e.g. a resizable dock panel) rebuilding this
      // widget without any of its own state changing.
      await tester.tap(find.text('rebuild ancestor'));
      await tester.pump();
      expect(
        pointers.iterations,
        1,
        reason: 'an unrelated ancestor rebuild must not re-run the sort',
      );

      // A real change to sort order still must recompute.
      await tester.tap(find.text('Name'));
      await tester.pump();
      expect(pointers.iterations, 2);
    },
  );
}

/// Counts how many times its contents are actually filtered/sorted (i.e.
/// iterated), to observe whether [PointersView] recomputes on every
/// `build()` or only when something relevant changed.
class _CountingPointerList extends ListBase<PointerDeclaration> {
  _CountingPointerList(this._items);

  final List<PointerDeclaration> _items;
  int iterations = 0;

  @override
  int get length => _items.length;

  @override
  set length(int newLength) => throw UnsupportedError('read-only');

  @override
  PointerDeclaration operator [](int index) => _items[index];

  @override
  void operator []=(int index, PointerDeclaration value) =>
      throw UnsupportedError('read-only');

  @override
  Iterator<PointerDeclaration> get iterator {
    iterations++;
    return _items.iterator;
  }
}

/// Hosts [PointersView] under a sibling button that forces an ancestor
/// rebuild without ever replacing the `pointers` list instance.
Widget _hostWithExternalRebuilds(List<PointerDeclaration> pointers) {
  return MaterialApp(
    home: Scaffold(
      body: StatefulBuilder(
        builder: (context, setState) {
          return Column(
            children: [
              TextButton(
                onPressed: () => setState(() {}),
                child: const Text('rebuild ancestor'),
              ),
              Expanded(child: PointersView(pointers: pointers)),
            ],
          );
        },
      ),
    ),
  );
}

Widget _host(List<PointerDeclaration> pointers) {
  return MaterialApp(
    home: Scaffold(body: PointersView(pointers: pointers)),
  );
}
