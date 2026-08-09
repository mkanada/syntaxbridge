import 'package:flutter/material.dart';

import '../project/project_models.dart';
import 'ide_theme.dart';

/// The type catalog navigator (US-3): every struct, class, union, enum,
/// typedef, type alias, and macro declared in the project, with its kind.
///
/// Functions, methods and macro-as-callable belong to US-5's own navigator,
/// not here — mixing declarations and callables in one list is exactly what
/// `docs/plans/User Steps.md` flags as a modeling mistake to avoid.
class TypesView extends StatelessWidget {
  const TypesView({
    super.key,
    required this.types,
    required this.onTypeSelected,
    this.selectedType,
  });

  final List<TypeDeclaration> types;
  final ValueChanged<TypeDeclaration> onTypeSelected;
  final TypeDeclaration? selectedType;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text('Types', style: Theme.of(context).textTheme.headlineSmall),
        const SizedBox(height: 4),
        Text(
          '${types.length} declared',
          style: Theme.of(context).textTheme.bodyMedium,
        ),
        const SizedBox(height: 16),
        Expanded(
          child: types.isEmpty
              ? const Padding(
                  padding: EdgeInsets.only(top: 8),
                  child: Text(
                    'No types found',
                    style: TextStyle(color: IdePalette.muted),
                  ),
                )
              : ListView.separated(
                  itemCount: types.length,
                  separatorBuilder: (context, index) =>
                      const Divider(height: 1),
                  itemBuilder: (context, index) {
                    final type = types[index];

                    return ListTile(
                      contentPadding: EdgeInsets.zero,
                      selected: type == selectedType,
                      leading: const Icon(Icons.data_object),
                      title: Text(_qualifiedName(type)),
                      trailing: Text(
                        type.kind.label,
                        style: const TextStyle(color: IdePalette.muted),
                      ),
                      onTap: () => onTypeSelected(type),
                    );
                  },
                ),
        ),
      ],
    );
  }
}

/// The type's name, qualified with its enclosing namespace when it has one,
/// so homonym types declared in different namespaces read as distinct rows.
String _qualifiedName(TypeDeclaration type) {
  return type.namespace.isEmpty ? type.name : '${type.namespace}::${type.name}';
}
