# Syntax Bridge
IDE de transpilação de C/C++ para Dart. A arquitetura deve permitir expansão de outras linguagens de entrada e saída.

### Aspectos técnicos
Arquitetura:
  - Cliente-servidor
    - Parte servidor: Rust
    - Parte cliente (UI): Flutter

Ferramentas que serão usadas:
  - Para análise dos artefatos de entrada:
    - libclang
    - clang
    - clang++
    - CMake
    - tree-sitter
  
  - Para análise dos artefatos de saída:
    - Dart SDK
      
  - Para geração de testes unitários de entrada:
    - KLEE
    - GoogleTest (gtest)
      
  - Para persistência:
    - SQLite

Ferramentas de empacotamento:
  - Linux: Flatpak

### Metodologia de desenvolvimento
- TDD. Tudo começa com um teste que falha e termina com um teste que passa. 
- Todos os testes devem ser rodados dentro do ambiente do Flatpak, para garantir que as ferramentas de desenvolvimento deste projeto não 
  interfiram nas ferramentas de compilação que deverão ser embutidas neste sistema.
-
