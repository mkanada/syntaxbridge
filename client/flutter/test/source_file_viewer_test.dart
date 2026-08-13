import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/src/project/project_models.dart';
import 'package:syntax_bridge/src/ui/ide_theme.dart';
import 'package:syntax_bridge/src/ui/source_file_viewer.dart';

/// Covers the click-to-open-with-highlight flow from the type catalog: a
/// [SourceFileViewer] told to highlight a line range scrolls it into view
/// and paints it with [IdePalette.selection]; without a range it behaves as
/// before (starts at the top, nothing highlighted).
void main() {
  String contentWithLines(int count) {
    return List.generate(count, (index) => 'content ${index + 1}').join('\n');
  }

  testWidgets('highlights every line within the requested range', (
    tester,
  ) async {
    await tester.pumpWidget(
      _host(
        content: Future.value(contentWithLines(10)),
        highlightStartLine: 3,
        highlightEndLine: 5,
      ),
    );
    await tester.pumpAndSettle();

    Color? colorBehind(String text) {
      final coloredBox = tester.widget<ColoredBox>(
        find
            .ancestor(of: find.text(text), matching: find.byType(ColoredBox))
            .first,
      );
      return coloredBox.color;
    }

    expect(colorBehind('content 3'), IdePalette.selection);
    expect(colorBehind('content 4'), IdePalette.selection);
    expect(colorBehind('content 5'), IdePalette.selection);
    expect(colorBehind('content 2'), isNot(IdePalette.selection));
    expect(colorBehind('content 6'), isNot(IdePalette.selection));
  });

  testWidgets('scrolls a far-away highlighted line into view', (tester) async {
    await tester.pumpWidget(
      _host(
        content: Future.value(contentWithLines(300)),
        highlightStartLine: 250,
        highlightEndLine: 250,
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('content 250'), findsOneWidget);
  });

  testWidgets('stays at the top with nothing highlighted by default', (
    tester,
  ) async {
    await tester.pumpWidget(
      _host(content: Future.value(contentWithLines(300))),
    );
    await tester.pumpAndSettle();

    expect(find.text('content 1'), findsOneWidget);
    expect(find.text('content 250'), findsNothing);
  });

  testWidgets('invokes onCallSelected when tapping a line with a recorded call '
      '(US-5 criterion 5)', (tester) async {
    const call = CallEdge(
      callerUsr: 'c:@F@describe#',
      resolution: CallResolution.resolved(
        calleeUsr: 'c:@N@geometry@S@Shape@F@area#1#',
        isDynamicDispatch: true,
      ),
      file: '/workspace/src/file.cpp',
      line: 2,
      column: 19,
    );

    CallEdge? selected;
    await tester.pumpWidget(
      _host(
        content: Future.value(contentWithLines(3)),
        calls: Future.value(const [call]),
        onCallSelected: (call) => selected = call,
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.text('content 2'));
    await tester.pumpAndSettle();

    expect(selected, call);
  });

  testWidgets('a line without a recorded call is not clickable', (
    tester,
  ) async {
    var called = false;
    await tester.pumpWidget(
      _host(
        content: Future.value(contentWithLines(3)),
        calls: Future.value(const []),
        onCallSelected: (_) => called = true,
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.text('content 2'));
    await tester.pumpAndSettle();

    expect(called, isFalse);
  });
}

Widget _host({
  required Future<String> content,
  int? highlightStartLine,
  int? highlightEndLine,
  Future<List<CallEdge>>? calls,
  ValueChanged<CallEdge>? onCallSelected,
}) {
  return MaterialApp(
    home: Scaffold(
      body: SizedBox(
        height: 300,
        child: SourceFileViewer(
          path: '/workspace/src/file.cpp',
          content: content,
          highlightStartLine: highlightStartLine,
          highlightEndLine: highlightEndLine,
          calls: calls,
          onCallSelected: onCallSelected,
        ),
      ),
    ),
  );
}
