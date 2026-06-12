# Passos


### T1 [x] - Criar um executável flutter simples chamando um código Rust usando o 'flutter_rust_bridge'. 
      Critério de conclusão: arquivo 'bundle' 'flatpak' instalado e testado.

### T2 [~] - Definir como todas as bibliotecas e ferramentas serão embutidas no sistema e uma estrutura de testes de verificação que as mesmas estão instaladas. O teste tem que ser esperto o suficiente para identificar se as ferramentas e bibliotecas executadas são aquelas que este sistema embute e não aquelas que eventualmente já existem no computador do usuário.

Progresso atual:

- T2.1 [x] - Infraestrutura de diagnóstico na tela inicial.
- T2.2 [x] - SQLite embutido via Rust.
- T2.3 [x] - Tree-sitter C++ embutido via Rust.
- T2.4 [ ] - CMake empacotado.
- T2.5 [ ] - LLVM/Clang e libclang empacotados.
- T2.6 [ ] - Dart SDK empacotado.
- T2.7 [ ] - Dart analysis server empacotado.
- T2.8 [ ] - gtest empacotado.
- T2.9 [ ] - KLEE empacotado ou bloqueio técnico documentado.
- T2.10 [ ] - Validação Flatpak completa.

3 - Criar projeto CMake com um ou mais códigos fonte C++ para testar a integração de todas as ferramentas.
