import 'package:flutter/material.dart';

import '../project/project_models.dart';
import 'ide_theme.dart';

class ServerConnectionStatus extends StatelessWidget {
  const ServerConnectionStatus({
    super.key,
    required this.status,
    required this.onRefresh,
  });

  final Future<ServerStatus> status;
  final VoidCallback onRefresh;

  @override
  Widget build(BuildContext context) {
    return FutureBuilder<ServerStatus>(
      future: status,
      builder: (context, snapshot) {
        final connected = snapshot.hasData && snapshot.data?.status == 'ok';
        final failed = snapshot.hasError;

        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'Server connection',
              style: Theme.of(context).textTheme.titleLarge?.copyWith(
                color: IdePalette.text,
                fontWeight: FontWeight.w700,
              ),
            ),
            const SizedBox(height: 14),
            Row(
              children: [
                Icon(
                  connected
                      ? Icons.check_circle
                      : failed
                      ? Icons.error
                      : Icons.sync,
                  color: connected
                      ? IdePalette.green
                      : failed
                      ? IdePalette.red
                      : IdePalette.amber,
                ),
                const SizedBox(width: 10),
                Flexible(
                  child: Text(
                    connected
                        ? 'Connected'
                        : failed
                        ? 'Disconnected'
                        : 'Connecting',
                    overflow: TextOverflow.ellipsis,
                    style: Theme.of(context).textTheme.titleMedium?.copyWith(
                      color: IdePalette.softText,
                    ),
                  ),
                ),
                const Spacer(),
                IconButton(
                  tooltip: 'Refresh',
                  onPressed: onRefresh,
                  icon: const Icon(Icons.refresh, color: IdePalette.muted),
                ),
              ],
            ),
            const Divider(height: 28, color: IdePalette.border),
            Text(
              snapshot.data?.service ?? 'syntax-bridge-server',
              style: Theme.of(
                context,
              ).textTheme.bodyLarge?.copyWith(color: IdePalette.codeText),
            ),
          ],
        );
      },
    );
  }
}
