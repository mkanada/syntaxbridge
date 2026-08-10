import 'package:flutter/material.dart';

import '../project/project_models.dart';
import 'ide_theme.dart';

/// Every recorded usage of the type currently selected in [TypesView]
/// (US-4): clicking a row opens the file at that exact location, via
/// [onUsageSelected]. Clicking a type in the catalog navigates to its own
/// declaration (US-3) *and* populates this panel — the two stay in sync
/// through the shared [selectedType], without this widget owning any
/// fetching itself.
class UsagesView extends StatelessWidget {
  const UsagesView({
    super.key,
    required this.selectedType,
    required this.usages,
    required this.onUsageSelected,
  });

  final TypeDeclaration? selectedType;
  final List<TypeUsage> usages;
  final ValueChanged<TypeUsage> onUsageSelected;

  @override
  Widget build(BuildContext context) {
    final selectedType = this.selectedType;
    if (selectedType == null) {
      return const Padding(
        padding: EdgeInsets.all(16),
        child: Text(
          'Select a type to see where it is used',
          style: TextStyle(color: IdePalette.muted),
        ),
      );
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text('Usages', style: Theme.of(context).textTheme.headlineSmall),
        const SizedBox(height: 4),
        Text(
          '${usages.length} ${usages.length == 1 ? 'use' : 'uses'} of '
          '${_qualifiedName(selectedType)}',
          style: Theme.of(context).textTheme.bodyMedium,
        ),
        const SizedBox(height: 16),
        Expanded(
          child: usages.isEmpty
              ? const Padding(
                  padding: EdgeInsets.only(top: 8),
                  child: Text(
                    'No usages found',
                    style: TextStyle(color: IdePalette.muted),
                  ),
                )
              : ListView.separated(
                  itemCount: usages.length,
                  separatorBuilder: (context, index) =>
                      const Divider(height: 1),
                  itemBuilder: (context, index) {
                    final usage = usages[index];
                    return ListTile(
                      contentPadding: EdgeInsets.zero,
                      leading: const Icon(Icons.place_outlined),
                      title: Text('${_fileName(usage.file)}:${usage.line}'),
                      trailing: Text(
                        usage.kind.label,
                        style: const TextStyle(color: IdePalette.muted),
                      ),
                      onTap: () => onUsageSelected(usage),
                    );
                  },
                ),
        ),
      ],
    );
  }
}

String _qualifiedName(TypeDeclaration type) {
  return type.namespace.isEmpty ? type.name : '${type.namespace}::${type.name}';
}

String _fileName(String path) => path.split('/').last;
