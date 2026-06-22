import 'package:flutter/material.dart';

import '../project/project_models.dart';

class BuildDependenciesView extends StatelessWidget {
  const BuildDependenciesView({super.key, required this.layers});

  final List<BuildDependencyLayer> layers;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          'Build dependencies',
          style: Theme.of(context).textTheme.titleLarge,
        ),
        const SizedBox(height: 8),
        for (final layer in layers) BuildDependencyLayerView(layer: layer),
      ],
    );
  }
}

class BuildDependencyLayerView extends StatelessWidget {
  const BuildDependencyLayerView({super.key, required this.layer});

  final BuildDependencyLayer layer;

  @override
  Widget build(BuildContext context) {
    return ExpansionTile(
      initiallyExpanded: true,
      tilePadding: EdgeInsets.zero,
      childrenPadding: EdgeInsets.zero,
      leading: const Icon(Icons.schema_outlined),
      title: Text('Dependency layer ${layer.index}'),
      subtitle: Text('${layer.items.length} item(s)'),
      children: [
        SingleChildScrollView(
          scrollDirection: Axis.horizontal,
          child: DataTable(
            columns: const [
              DataColumn(label: Text('Item')),
              DataColumn(label: Text('Dependencies')),
              DataColumn(label: Text('Kind')),
            ],
            rows: [
              for (final item in layer.items)
                DataRow(
                  cells: [
                    DataCell(Text(item.name)),
                    DataCell(Text(_dependencyText(item.dependencies))),
                    DataCell(Text(item.kind)),
                  ],
                ),
            ],
          ),
        ),
      ],
    );
  }

  String _dependencyText(List<String> dependencies) {
    if (dependencies.isEmpty) {
      return 'No dependencies';
    }
    return dependencies.join(', ');
  }
}
