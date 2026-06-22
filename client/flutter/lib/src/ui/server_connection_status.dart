import 'package:flutter/material.dart';

import '../project/project_models.dart';

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
              style: Theme.of(context).textTheme.headlineSmall,
            ),
            const SizedBox(height: 20),
            Row(
              children: [
                Icon(
                  connected
                      ? Icons.check_circle
                      : failed
                      ? Icons.error
                      : Icons.sync,
                  color: connected
                      ? const Color(0xFF237A57)
                      : failed
                      ? const Color(0xFFB3261E)
                      : const Color(0xFF7B6324),
                ),
                const SizedBox(width: 10),
                Text(
                  connected
                      ? 'Connected'
                      : failed
                      ? 'Disconnected'
                      : 'Connecting',
                  style: Theme.of(context).textTheme.titleMedium,
                ),
                const Spacer(),
                IconButton(
                  tooltip: 'Refresh',
                  onPressed: onRefresh,
                  icon: const Icon(Icons.refresh),
                ),
              ],
            ),
            const Divider(height: 32),
            Text(
              snapshot.data?.service ?? 'syntax-bridge-server',
              style: Theme.of(context).textTheme.bodyLarge,
            ),
          ],
        );
      },
    );
  }
}
