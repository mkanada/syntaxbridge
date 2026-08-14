import 'package:flutter_test/flutter_test.dart';

import '../../tool/generate_screenshot_gallery.dart';

void main() {
  test(
    'groups screenshots by US, in order, with captions and embedded images',
    () {
      final markdown = buildGalleryMarkdown([
        'us3-types-view.png',
        'us1-new-project-form.png',
        'us1-landing-empty.png',
      ]);

      final us1Heading = markdown.indexOf(
        '## US-1 — Criação de projeto e ingestão do input',
      );
      final us3Heading = markdown.indexOf(
        '## US-3 — Catálogo de tipos do projeto',
      );
      expect(us1Heading, greaterThanOrEqualTo(0));
      expect(us3Heading, greaterThanOrEqualTo(0));
      expect(
        us1Heading,
        lessThan(us3Heading),
        reason: 'US-1 sorts before US-3',
      );

      expect(markdown, contains('![Landing empty](us1-landing-empty.png)'));
      expect(
        markdown,
        contains('![New project form](us1-new-project-form.png)'),
      );
      expect(markdown, contains('![Types view](us3-types-view.png)'));
    },
  );

  test('keeps screenshots within a group in their given order', () {
    final markdown = buildGalleryMarkdown([
      'us1-landing-empty.png',
      'us1-new-project-form.png',
    ]);

    expect(
      markdown.indexOf('us1-landing-empty.png'),
      lessThan(markdown.indexOf('us1-new-project-form.png')),
    );
  });

  test('falls back to a generic group for names without a US prefix', () {
    final markdown = buildGalleryMarkdown(['misc-screen.png']);

    expect(markdown, contains('## Other'));
    expect(markdown, contains('![Misc screen](misc-screen.png)'));
  });

  test(
    'returns a gallery with no US sections when there are no screenshots',
    () {
      final markdown = buildGalleryMarkdown([]);

      expect(markdown, isNot(contains('##')));
    },
  );
}
