import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/src/ui/dockable_panel.dart';
import 'package:syntax_bridge/src/ui/panel_group.dart';

/// From the type catalog (US-3) onward, the left side of the workspace hosts
/// more than one navigator. Stacking full [DockablePanel]s there gives each
/// one a fraction of the height, which stops being usable past two. These
/// tests pin the alternative: one navigator visible at a time, switched by a
/// tab strip, sharing a single frame.
void main() {
  testWidgets('shows only the active tab content', (tester) async {
    await tester.pumpWidget(
      _groupHost(
        height: 400,
        tabs: [
          _tab('Types', const Text('types content')),
          _tab('Functions', const Text('functions content')),
        ],
      ),
    );

    expect(find.text('types content'), findsOneWidget);
    expect(find.text('functions content'), findsNothing);
  });

  testWidgets('switches the visible content on tab tap', (tester) async {
    await tester.pumpWidget(
      _groupHost(
        height: 400,
        tabs: [
          _tab('Types', const Text('types content')),
          _tab('Functions', const Text('functions content')),
        ],
      ),
    );

    await tester.tap(find.text('Functions'));
    await tester.pump();

    expect(find.text('types content'), findsNothing);
    expect(find.text('functions content'), findsOneWidget);
  });

  testWidgets('close and dock act on the active tab', (tester) async {
    var closedTypes = false;
    var closedFunctions = false;
    DockSide? dockedTypesTo;

    await tester.pumpWidget(
      _groupHost(
        height: 400,
        tabs: [
          _tab(
            'Types',
            const Text('types content'),
            onClose: () => closedTypes = true,
            onDockSide: (side) => dockedTypesTo = side,
          ),
          _tab(
            'Functions',
            const Text('functions content'),
            onClose: () => closedFunctions = true,
          ),
        ],
      ),
    );

    await tester.tap(find.byTooltip('Dock Types panel'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Dock bottom').last);
    await tester.pumpAndSettle();

    expect(dockedTypesTo, DockSide.bottom);

    await tester.tap(find.byTooltip('Close Types panel'));
    await tester.pump();

    expect(closedTypes, isTrue);
    expect(closedFunctions, isFalse);
  });

  testWidgets('bounds the active body when the parent gives unbounded height', (
    tester,
  ) async {
    final builtRows = <int>{};

    await tester.pumpWidget(
      _groupHost(
        tabs: [
          _tab('Types', _countingList(builtRows)),
          _tab('Functions', const Text('functions content')),
        ],
      ),
    );

    expect(find.text('row 0'), findsOneWidget);
    expect(builtRows.length, lessThan(60));
  });
}

Widget _countingList(Set<int> builtRows) {
  return ListView.builder(
    itemCount: 5000,
    itemBuilder: (context, index) {
      builtRows.add(index);
      return SizedBox(height: 40, child: Text('row $index'));
    },
  );
}

PanelTab _tab(
  String title,
  Widget child, {
  VoidCallback? onClose,
  ValueChanged<DockSide>? onDockSide,
}) {
  return PanelTab(
    title: title,
    icon: Icons.list_alt_outlined,
    side: DockSide.left,
    onClose: onClose ?? () {},
    onDockSide: onDockSide ?? (_) {},
    child: child,
  );
}

Widget _groupHost({required List<PanelTab> tabs, double? height}) {
  final group = TabbedPanelGroup(tabs: tabs);

  return MaterialApp(
    home: Scaffold(
      body: height == null
          ? SingleChildScrollView(child: Column(children: [group]))
          : SizedBox(height: height, child: group),
    ),
  );
}
