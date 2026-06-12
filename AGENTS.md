# Resumo do Projeto

Este projeto propõe uma ferramenta desktop interativa para apoiar a conversão incremental de código C++ para Dart. A ferramenta deve importar projetos ou arquivos C++, analisar sua estrutura, apresentar decisões ao usuário quando houver ambiguidade e gerar código Dart equivalente, sempre com validação por compilação e testes.

A arquitetura prevista combina um núcleo em Rust com uma interface gráfica em Flutter, integrados por `flutter_rust_bridge`. O núcleo Rust concentra parsing, análise, persistência, regras de conversão, geração de código e integração com ferramentas externas. A interface Flutter fica responsável por exibir o projeto, guiar as etapas de conversão, mostrar diagnósticos e coletar decisões do usuário.

O ID Flatpak oficial escolhido para a aplicação é `io.github.mkanada.syntaxbridge`, alinhado ao repositório `github.com/mkanada/syntaxbridge`. Os IDs anteriores `io.github.syntaxbridge.SyntaxBridge` e `com.syntaxbridge.SyntaxBridge` foram usados apenas localmente e não devem ser usados como referência principal.

A ferramenta não deve usar IA em tempo de execução. As conversões devem ser baseadas em parsing, regras explícitas, mapeamentos persistidos, testes, validações e interação humana. As decisões tomadas pelo usuário devem ser armazenadas em SQLite para reutilização posterior e para permitir retomar conversões interrompidas.

O desenvolvimento inicial será guiado pelo projeto C++ real `verovio`, priorizando casos práticos em vez de tentar oferecer suporte completo a todo C++ desde o início. Tentativas anteriores em `tmp/verovio-port2` e `tmp/legacy-bridge` podem servir como referência conceitual.

O fluxo geral envolve verificar o ambiente, importar o projeto C++, criar testes para capturar comportamento original, mapear tipos e símbolos, gerar uma estrutura Dart compilável e converter implementações de forma incremental, começando por funções e métodos mais simples ou isolados. Ferramentas previstas incluem Tree-sitter, CMake, Clang/libclang, Dart analysis server, gtest, KLEE e SQLite. Tree-sitter deve ser usado para preservar comentários e aspectos visuais do código; informações semânticas não visuais do C++ devem vir do libclang.

Como base do desenvolvimento deste projeto, usamos TDD (Test Driven Development). Iniciamos com um teste, executamos o teste que deve falhar, depois implementamos a tarefa e ao final executamos o teste novamente, agora esperando que ele passe.

Os principais princípios são conversão incremental, validação contínua, rastreabilidade entre C++ e Dart, persistência de decisões, intervenção humana em casos ambíguos e evitar conversões implícitas perigosas. O resultado esperado é um projeto Dart compilável, com testes equivalentes, histórico de validação e relatório de itens convertidos, parcialmente convertidos e não convertidos.
