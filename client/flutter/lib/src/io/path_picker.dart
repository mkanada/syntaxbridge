import 'package:file_picker/file_picker.dart';

import '../logging/cli_log.dart';

abstract class PathPicker {
  Future<String?> pickWorkspaceDirectory();

  Future<String?> pickSourceArchive();
}

class FilePickerPathPicker implements PathPicker {
  const FilePickerPathPicker();

  @override
  Future<String?> pickWorkspaceDirectory() async {
    cliLog('opening workspace directory picker');
    final path = await FilePicker.getDirectoryPath(
      dialogTitle: 'Choose workspace',
    );
    cliLog('workspace directory picker returned: ${path ?? '<cancelled>'}');
    return path;
  }

  @override
  Future<String?> pickSourceArchive() async {
    cliLog('opening source archive picker');
    final result = await FilePicker.pickFiles(
      dialogTitle: 'Choose source archive',
      type: FileType.custom,
      allowedExtensions: const ['zip', 'tgz', 'gz'],
      allowMultiple: false,
    );

    final path = result?.files.single.path;
    cliLog('source archive picker returned: ${path ?? '<cancelled>'}');
    return path;
  }
}
