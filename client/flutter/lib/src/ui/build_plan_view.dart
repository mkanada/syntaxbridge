import 'package:flutter/material.dart';

import '../project/project_models.dart';

class BuildPlanView extends StatelessWidget {
  const BuildPlanView({super.key, required this.layers});

  final List<BuildLayer> layers;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text('Build plan', style: Theme.of(context).textTheme.titleLarge),
        const SizedBox(height: 8),
        for (final layer in layers) BuildLayerView(layer: layer),
      ],
    );
  }
}

class BuildLayerView extends StatelessWidget {
  const BuildLayerView({super.key, required this.layer});

  final BuildLayer layer;

  @override
  Widget build(BuildContext context) {
    return ExpansionTile(
      initiallyExpanded: true,
      tilePadding: EdgeInsets.zero,
      childrenPadding: const EdgeInsets.only(left: 16),
      leading: const Icon(Icons.account_tree_outlined),
      title: Text('Layer ${layer.index}'),
      subtitle: Text('${layer.targets.length} target(s)'),
      children: [
        for (final target in layer.targets)
          ListTile(
            dense: true,
            contentPadding: EdgeInsets.zero,
            leading: Icon(_iconForKind(target.kind)),
            title: Text(target.name),
            subtitle: Text(target.kind),
          ),
      ],
    );
  }

  IconData _iconForKind(String kind) {
    return switch (kind) {
      'EXECUTABLE' => Icons.terminal,
      'STATIC_LIBRARY' ||
      'SHARED_LIBRARY' ||
      'MODULE_LIBRARY' => Icons.inventory_2_outlined,
      _ => Icons.adjust,
    };
  }
}
