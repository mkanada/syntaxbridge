# Syntax Bridge
IDE de transpilação de C/C++ para Dart. A arquitetura deve permitir expansão de outras linguagens de entrada e saída.

### Aspectos técnicos
Arquitetura:
  - Cliente servidor
    - Parte servidor: Rust
    - Parte cliente (UI): Flutter

Ferramentas que serão usadas:
  - Para análise dos artefatos de entrada:
    - libclang
    - clang
    - clang++
    - cmake
  
  - Para análise dos artefatos de saída:
    - dart sdk
      
  - Para geração de testes unitários de entrada:
    - klee
    - gunit
      
  - Para persistência:
    - SQLite

Ferramentas de empacotamento:
  - Linux: flatpak
