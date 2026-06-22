enum ExecutionLogLevel { info, success, warning, error }

class ExecutionLogEntry {
  const ExecutionLogEntry({
    required this.timestamp,
    required this.level,
    required this.message,
  });

  final DateTime timestamp;
  final ExecutionLogLevel level;
  final String message;
}
