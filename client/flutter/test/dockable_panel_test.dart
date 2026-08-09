import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/src/ui/dockable_panel.dart';

/// The panel is the host of every list in the workspace, and from the type
/// catalog onwards those lists are large enough that building every row on
/// every frame stops being viable. These tests pin the contract that makes
/// that possible: the panel bounds the body, and the child owns the scrolling.
void main() {
  testWidgets('builds only the visible rows of a long list', (tester) async {
    final builtRows = <int>{};

    await tester.pumpWidget(
      _panelHost(height: 400, child: _countingList(builtRows)),
    );

    expect(find.text('row 0'), findsOneWidget);
    expect(find.text('row 4999'), findsNothing);
    expect(builtRows.length, lessThan(60));
  });

  testWidgets('scrolls a long list inside the panel', (tester) async {
    await tester.pumpWidget(
      _panelHost(height: 400, child: _countingList(<int>{})),
    );

    await tester.drag(find.byType(ListView), const Offset(0, -2000));
    await tester.pumpAndSettle();

    expect(find.text('row 0'), findsNothing);
    expect(find.text('row 50'), findsOneWidget);
  });

  testWidgets('bounds the body when the parent gives unbounded height', (
    tester,
  ) async {
    final builtRows = <int>{};

    await tester.pumpWidget(_panelHost(child: _countingList(builtRows)));

    expect(find.text('row 0'), findsOneWidget);
    expect(builtRows.length, lessThan(60));
  });

  testWidgets('keeps the header reachable while the body scrolls', (
    tester,
  ) async {
    await tester.pumpWidget(
      _panelHost(height: 400, child: _countingList(<int>{})),
    );

    final headerTop = tester.getTopLeft(find.text('Catalog')).dy;

    await tester.drag(find.byType(ListView), const Offset(0, -2000));
    await tester.pumpAndSettle();

    expect(find.text('Catalog'), findsOneWidget);
    expect(tester.getTopLeft(find.text('Catalog')).dy, headerTop);
  });
}

/// A list long enough that building it whole would be the bug under test.
/// [builtRows] records which rows the builder was actually asked for.
Widget _countingList(Set<int> builtRows) {
  return ListView.builder(
    itemCount: 5000,
    itemBuilder: (context, index) {
      builtRows.add(index);
      return SizedBox(height: 40, child: Text('row $index'));
    },
  );
}

/// Hosts a panel the way the workspace does. With [height] the parent bounds
/// the panel, as the desktop layout does; without it the panel sits in a
/// scroll view with unbounded height, as the compact layout does.
Widget _panelHost({required Widget child, double? height}) {
  final panel = DockablePanel(
    title: 'Catalog',
    icon: Icons.list_alt_outlined,
    side: DockSide.left,
    onClose: () {},
    onDockSide: (_) {},
    child: child,
  );

  return MaterialApp(
    home: Scaffold(
      body: height == null
          ? SingleChildScrollView(child: Column(children: [panel]))
          : SizedBox(height: height, child: panel),
    ),
  );
}
