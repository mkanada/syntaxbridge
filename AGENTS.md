# AGENTS.md

Orientacoes para agentes trabalhando neste repositorio.

## Visao do projeto

Syntax Bridge e uma IDE para transpilacao de C/C++ para Dart.

A arquitetura deve permitir expansao futura para outras linguagens de entrada e
saida. Evite acoplar decisoes de dominio diretamente a C/C++ ou Dart quando uma
abstracao simples puder preservar essa extensibilidade sem adicionar complexidade
prematura.

## Arquitetura prevista

- Servidor: Rust.
- Cliente/UI: Flutter.
- Persistencia: SQLite.
- Empacotamento Linux: Flatpak.

Ferramentas previstas para analise dos artefatos de entrada:

- `libclang`
- `clang`
- `clang++`
- `cmake`
- `tree-sitter`

Ferramentas previstas para analise dos artefatos de saida:

- Dart SDK.

Ferramentas previstas para geracao e execucao de testes unitarios de entrada:

- `klee`
- GoogleTest (`gtest`).

## Metodo de desenvolvimento

- Use TDD: toda mudanca comportamental deve comecar com um teste que falha e
  terminar com o teste passando.
- Rode os testes dentro do ambiente Flatpak quando esse ambiente estiver
  disponivel.
- O objetivo de executar os testes no Flatpak e isolar as ferramentas embutidas
  no sistema das ferramentas instaladas na maquina de desenvolvimento.
- Enquanto o ambiente Flatpak ainda nao existir, registre no resumo final quais
  testes foram executados fora dele e qual cobertura ficou pendente.

## Diretrizes de implementacao

- Prefira Rust para componentes de servidor, analise, orquestracao e
  persistencia.
- Prefira Flutter para experiencia de IDE e interface de usuario.
- Mantenha fronteiras claras entre:
  - analise de entrada;
  - modelo intermediario;
  - geracao de saida;
  - validacao/testes;
  - persistencia;
  - UI.
- Ao adicionar suporte a uma linguagem, trate-a como plugin/adaptador quando
  possivel, em vez de espalhar condicionais por todo o codigo.
- Nao introduza dependencias externas sem justificar a necessidade no contexto da
  arquitetura.

## Estado atual

Este repositorio ainda esta em fase de definicao inicial. Antes de assumir
comandos de build, teste ou execucao, verifique a estrutura existente.

Comandos de verificacao ainda pendentes de scaffold:

- Backend Rust: a definir.
- Cliente Flutter: a definir.
- Testes no Flatpak: a definir.
