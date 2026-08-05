import 'dart:io';

import 'project_creation_exception.dart';

String projectErrorMessage(Object error) {
  return switch (error) {
    ProjectCreationException(:final message) => message,
    HttpException(:final message) => message,
    _ => error.toString(),
  };
}
