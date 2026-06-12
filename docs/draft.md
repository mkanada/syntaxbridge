# Transpiler - rascunho original

Este documento é o rascunho original do projeto e deve ser tratado como histórico. A versão consolidada atual está em `docs/draft-1.md`; em caso de divergência, use `README.md`, `AGENTS.md`, `docs/steps.md` e `docs/t2-planejamento.md` como fontes mais atuais.

Objetivo. Criar uma ferramenta interativa que importa um código fonte em C++, apresenta opções em várias etapas ao usuário, e, através destas opções apresentadas, vai convertendo o código C++ para Dart (inicialmente, outras linguagens depois). A ferramenta em si NÃO usará modelos de IA em nenhuma fase de execução dela.

A linguagem de implementação do sistema será o Rust e a interface gráfica será em Flutter. A comunicação entre as partes deve usar o `flutter_rust_bridge`.

Para desenvolver a ferramenta atual, o projeto que será usado será o 'verovio'. Outras tentativas já foram feitas, que estão dentro do diretório 'tmp'. A primeira, 'verovio-port2' foi um conjunto de scripts Python com persistência em JSON. A segunda, projeto inicial em Rust chamada 'legacy-bridge', ainda muito inicial, mas que pode ajudar a direcionar a implementação atual.


### Ferramentas que serão usadas:

- Tree-sitter
- CMake
- Clang/libclang
- Dart analysis server
- klee e gtest
- SQLite

### Passos para a criação:

- Verificar se as ferramentas estão funcionando.
- Criar estrutura e procedimentos de testes unitários e ‘mock’ na ‘unha’.
    - Copiar o código fonte daquilo que será testado para um diretório separado.
    - Criar arquivos header separados, com implementação ‘mock’ daquilo que o código que será testado chama.
- Identificar os limites da conversão
    - Tipos internos
    - Tipos das bibliotecas padrão do C++ como std::*
    - Bibliotecas que já existam por padrão no Dart como XML, Json, etc
- Criar mapa de chamador-chamado e identificar os tipos menos chamados.
- Começando pelos menos chamados, criar os testes unitários ‘mockados’ e gravar o comportamento. ‘Função X, com valor Y, chama A, B e C com valores ‘a1’, ‘b1’, ‘c1’ e retorna ‘X1’.
- Varrer cada código fonte, coletando com Tree-sitter os comentários e as informações ligadas ao aspecto visual do código.
- Coletar com libclang as informações semânticas não visuais, como tipos, símbolos e relações chamador-chamado.
- Realizar o mapeamento dos tipos, nomes de variáveis e métodos/funções.
- Gerar o código sem implementação para validar a estrutura.
- Pelo que está gravado no SQLite, procurar as funções/métodos mais simples. Realizar conversão em ordem crescente, usando os mapeamentos de nomes gravados anteriormente.
    - Para cada função/método convertido, gerar, compilar e executar o teste unitário equivalentes aos já gerados nos passos anteriores.
