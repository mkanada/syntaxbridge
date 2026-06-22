# Primeira etapa - Project creation

Passos do ponto de vista do usuário:
  - Especificar nome do projeto e diretório de trabalho.
  - Usuário escolhe um arquivo 'tar.gz' ou '.zip' que tem que ser descompactado no diretório do projeto, dentro do subdiretório 'input-source'.
  - Após ter descompactado o arquivo de input, o sistema identifica os arquivos do projeto CMake, roda-o com a variável CMAKE_EXPORT_COMPILE_COMMANDS habilitada, e obtém a lista de 'compilation units' pelo arquivo 'compile_commands.json'. Esta lista tem que ser apresentada ao usuário.
