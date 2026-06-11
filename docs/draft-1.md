# Transpiler C++ para Dart

## Objetivo

Criar uma ferramenta interativa para apoiar a conversão de código fonte C++ para Dart.

A ferramenta deve importar um projeto ou conjunto de arquivos C++, analisar sua estrutura, apresentar opções em etapas ao usuário e, a partir das escolhas feitas, gerar código Dart equivalente. A conversão inicial terá como alvo Dart, mas a arquitetura deve permitir suporte futuro a outras linguagens.

A ferramenta em si não deve usar modelos de IA em nenhuma fase de execução. Toda análise, decisão, transformação e validação deve ser baseada em parsing, regras explícitas, mapeamentos persistidos, testes e interação humana.

## Arquitetura prevista

O sistema será dividido em duas partes principais:

- Núcleo em Rust.
- Interface gráfica em Flutter.

A comunicação entre o núcleo Rust e a interface Flutter deve ser feita usando `flutter_rust_bridge`.

O aplicativo sendo criado será uma aplicação desktop. A versão inicial deve ter como alvo Linux, com possibilidade de suporte futuro a Windows.

A parte Rust deve ser integrada ao aplicativo Flutter como biblioteca nativa via `flutter_rust_bridge` e empacotada junto com a aplicação desktop, sem depender de um serviço ou processo separado.

No Linux, a distribuição inicial deve seguir o formato Flatpak com empacotamento autossuficiente. O usuário deve precisar baixar e instalar apenas a ferramenta para usá-la, sem instalar manualmente CMake, Clang, LSPs, Dart SDK, gtest, KLEE, SQLite ou outros componentes previstos pelo fluxo principal.

O Flatpak deve incluir o aplicativo Flutter, a biblioteca nativa Rust e as ferramentas necessárias para análise, build, validação, testes e geração de código. O objetivo é reduzir ao máximo dependências externas, controlar as versões usadas pela ferramenta e tornar a execução reproduzível entre distribuições Linux compatíveis com Flatpak.

As chamadas feitas pelo núcleo Rust devem priorizar os binários, bibliotecas e recursos empacotados dentro do próprio Flatpak. O ambiente do host não deve ser requisito para o fluxo principal. Dependências específicas do projeto C++ importado podem continuar sendo um risco técnico, mas a estratégia inicial do produto é oferecer uma ferramenta autocontida.

O uso de `musl` pode ser avaliado para componentes Rust isolados, mas não deve ser assumido como requisito obrigatório para o executável Flutter desktop, já que o Flutter no Linux normalmente depende do runtime gráfico e das bibliotecas do sistema usadas pelo embedder Linux.

### Núcleo Rust

Responsabilidades esperadas:

- Importar e indexar projetos C++.
- Coordenar chamadas às ferramentas empacotadas com a aplicação, como Tree-sitter, CMake, Clang, LSPs, KLEE e gtest.
- Persistir informações no SQLite.
- Construir modelos intermediários do código C++.
- Executar regras de mapeamento e conversão.
- Gerar código Dart e testes equivalentes.
- Expor APIs para a interface Flutter via `flutter_rust_bridge`.

### Interface Flutter

Responsabilidades esperadas:

- Apresentar o projeto importado ao usuário.
- Exibir etapas da conversão.
- Solicitar decisões quando houver ambiguidade.
- Exibir diagnósticos de compilação, LSP e testes.
- Permitir revisar mapeamentos, nomes, tipos e estrutura gerada.
- Mostrar progresso da conversão incremental.

### Fronteira Rust/Flutter

O `flutter_rust_bridge` deve ser usado para manter a lógica pesada no Rust e deixar o Flutter focado na experiência interativa.

Dados que provavelmente atravessarão essa fronteira:

- Configuração do projeto importado.
- Lista de arquivos, símbolos e dependências.
- Opções apresentadas ao usuário.
- Decisões tomadas pelo usuário.
- Status de etapas longas.
- Diagnósticos e resultados de testes.
- Relatórios de conversão.

Como regra geral, a interface não deve implementar regras centrais de conversão. Essas regras devem permanecer no núcleo Rust para serem testáveis e reutilizáveis.

## Projeto inicial de referência

O primeiro projeto usado para desenvolver e validar a ferramenta será o `verovio`.

Objetivos do uso do `verovio`:

- Trabalhar com um projeto C++ real.
- Descobrir dificuldades práticas de parsing, build, dependências e conversão.
- Validar o fluxo de testes mockados antes da conversão.
- Criar critérios reais para priorização de funções, métodos e tipos.
- Medir quais recursos de C++ precisam ser suportados primeiro.

O desenvolvimento inicial deve ser guiado pelas necessidades encontradas no `verovio`, sem assumir suporte genérico completo a C++ desde o início.

## Referências internas anteriores

Existem tentativas anteriores dentro do diretório `tmp` que podem orientar a implementação atual.

### `tmp/verovio-port2`

Tentativa anterior baseada em scripts Python com persistência em JSON.

Possíveis aprendizados a reaproveitar:

- Estratégias de extração de informações do projeto `verovio`.
- Estrutura de dados usada para persistir informações intermediárias.
- Problemas encontrados ao tentar converter trechos de código.
- Decisões que já foram experimentadas e podem ser avaliadas novamente.

### `tmp/legacy-bridge`

Projeto inicial em Rust, ainda em fase muito inicial.

Possíveis aprendizados a reaproveitar:

- Organização inicial do núcleo Rust.
- Ideias de ponte entre análise, persistência e geração.
- Tipos, módulos ou modelos que possam direcionar a implementação atual.

Essas tentativas devem ser usadas como referência, não necessariamente como base obrigatória. Qualquer reaproveitamento deve ser avaliado caso a caso.

## Princípios

- A conversão deve ser incremental.
- O usuário deve poder tomar decisões quando houver ambiguidade.
- As decisões tomadas devem ser registradas para reutilização posterior.
- O comportamento do código C++ original deve ser capturado por testes antes da conversão sempre que possível.
- O código Dart gerado deve ser validado continuamente por compilação e execução de testes equivalentes.
- A ferramenta deve priorizar funções, métodos e tipos mais simples antes dos elementos mais complexos.
- A ferramenta deve evitar conversões implícitas perigosas quando não houver confiança suficiente.

## Fora de escopo inicial

Os itens abaixo não aparecem definidos no rascunho original e, portanto, devem ser tratados como pendentes até decisão explícita:

- Suporte completo a todo o padrão C++.
- Conversão automática perfeita sem intervenção humana.
- Uso de IA para inferir comportamento ou gerar código.
- Suporte inicial a múltiplas linguagens de destino além de Dart.
- Garantia inicial de equivalência formal completa entre C++ e Dart.

## Ferramentas previstas

As ferramentas previstas devem ser distribuídas junto com o Flatpak sempre que fizerem parte do fluxo principal da aplicação. A ferramenta não deve assumir que o usuário terá esses componentes instalados no sistema host.

Estratégia inicial de empacotamento:

- Tree-sitter e grammar C++ empacotados ou integrados ao núcleo Rust.
- CMake empacotado para configurar e reproduzir builds C++ dentro do ambiente controlado.
- Clang/LLVM empacotado para compilação, análise semântica e integração com `compile_commands.json`.
- LSP C++ empacotado, preferencialmente via `clangd` da mesma família LLVM incluída.
- Dart SDK empacotado para compilação, testes e execução das ferramentas Dart.
- LSP Dart empacotado via analysis server incluído no Dart SDK.
- gtest empacotado ou disponibilizado como recurso interno para a geração e execução de testes C++.
- KLEE empacotado desde a primeira versão, junto com as dependências necessárias para sua execução dentro do Flatpak.
- SQLite integrado ao núcleo Rust ou empacotado como biblioteca, sem exigir instalação externa.

### Tree-sitter

Responsável por analisar sintaticamente os arquivos C++ e extrair a árvore de sintaxe.

Usos esperados:

- Identificar declarações de classes, structs, enums, funções e métodos.
- Identificar variáveis, parâmetros, tipos, namespaces e chamadas.
- Extrair relações entre trechos de código.
- Apoiar a criação de um modelo intermediário do código fonte.

### CMake

Responsável por compreender e reproduzir o processo de build do projeto C++.

Usos esperados:

- Detectar alvos, includes, flags e dependências.
- Construir o código original.
- Construir versões isoladas ou instrumentadas para testes.
- Possivelmente gerar informações auxiliares para Clang.

### Clang

Responsável por fornecer análise semântica mais profunda do código C++.

Usos esperados:

- Resolver tipos reais após includes, templates e overloads.
- Validar compilação do código original.
- Apoiar extração de informações que o Tree-sitter sozinho não garante.
- Possivelmente gerar ou consumir `compile_commands.json`.

### LSP C++ e Dart

Responsáveis por validações, diagnósticos e informações estruturais das linguagens.

Usos esperados para C++:

- Diagnósticos do código original.
- Resolução de símbolos.
- Navegação entre definições e usos.

Usos esperados para Dart:

- Diagnósticos do código gerado.
- Formatação e organização de imports.
- Validação progressiva durante a geração.

### KLEE e gtest

Responsáveis por apoiar a captura e validação de comportamento.

Usos esperados:

- Criar testes unitários para funções e métodos C++.
- Gerar ou auxiliar na descoberta de entradas relevantes.
- Registrar chamadas feitas por uma função, seus argumentos e retornos.
- Reexecutar comportamento esperado contra a versão convertida.

### SQLite

Responsável por persistir informações extraídas, decisões de usuário, mapeamentos e resultados de testes.

Usos esperados:

- Armazenar símbolos extraídos do código C++.
- Armazenar relações chamador-chamado.
- Armazenar mapeamentos entre tipos C++ e Dart.
- Armazenar mapeamentos entre nomes originais e nomes convertidos.
- Armazenar decisões tomadas pelo usuário.
- Armazenar resultados de testes e validações.
- Permitir retomar uma conversão interrompida.

## Fluxo geral da ferramenta

### 1. Verificação do ambiente

A ferramenta deve verificar se todas as ferramentas empacotadas necessárias estão presentes, acessíveis e funcionando dentro do Flatpak. Essa etapa não deve exigir instalação manual de dependências pelo usuário no sistema host.

Verificações esperadas:

- Tree-sitter disponível e com grammar C++ configurada.
- CMake disponível.
- Clang disponível.
- LSP C++ disponível.
- LSP Dart disponível.
- SDK Dart disponível.
- gtest disponível ou configurável.
- KLEE disponível.
- SQLite disponível.

Resultado esperado:

- Relatório de ferramentas empacotadas encontradas.
- Relatório de ferramentas empacotadas ausentes ou inacessíveis.
- Versões detectadas.
- Bloqueios que impedem iniciar a conversão.
- Indicação clara de que falhas nessa etapa representam problema no pacote Flatpak ou em permissões do sandbox, não ausência de dependências instaladas pelo usuário.

### 2. Importação do código C++

A ferramenta deve receber um projeto ou conjunto de arquivos C++.

Informações a coletar:

- Diretório raiz do projeto.
- Arquivos fonte e headers.
- Arquivos CMake.
- Alvos de build.
- Caminhos de include.
- Dependências externas.
- Flags de compilação.

Resultado esperado:

- Inventário inicial do projeto.
- Configuração necessária para compilar e analisar o código.
- Registro das informações no SQLite.

### 3. Criação da estrutura de testes

Antes de converter código, a ferramenta deve criar uma estrutura de testes para capturar comportamento do C++ original.

O rascunho original indica uma abordagem manual ou semi-manual de mocks "na unha".

Procedimento esperado:

- Copiar o código fonte da unidade que será testada para um diretório separado.
- Criar headers separados para substituir dependências chamadas por essa unidade.
- Implementar mocks dessas dependências.
- Compilar a unidade isolada.
- Executar testes para registrar comportamento.

Artefatos esperados:

- Diretório isolado de teste.
- Arquivos fonte copiados.
- Headers mockados.
- Implementações mockadas.
- Testes gtest.
- Registro dos resultados no SQLite.

### 4. Identificação dos limites da conversão

A ferramenta deve identificar quais elementos do código podem ser convertidos diretamente, quais exigem mapeamento e quais precisam de decisão humana.

Categorias iniciais:

- Tipos internos do C++.
- Tipos da biblioteca padrão C++, como `std::*`.
- Bibliotecas com equivalentes padrão ou comuns em Dart, como JSON e XML.
- Tipos definidos pelo usuário.
- APIs externas ou bibliotecas de terceiros.
- Recursos de C++ sem equivalente direto em Dart.

Exemplos de decisões necessárias:

- Como mapear `int`, `long`, `size_t`, `double`, `bool`, `char` e ponteiros.
- Como mapear `std::string`, `std::vector`, `std::map`, `std::optional` e similares.
- Como tratar referências, ponteiros, ownership e mutabilidade.
- Como tratar overloads, templates, macros e namespaces.
- Como tratar exceções C++ em Dart.
- Como tratar código dependente de plataforma.

Resultado esperado:

- Tabela de mapeamentos automáticos confiáveis.
- Lista de mapeamentos que exigem confirmação do usuário.
- Lista de elementos inicialmente não suportados.

### 5. Mapa chamador-chamado

A ferramenta deve construir um grafo de chamadas entre funções e métodos.

Objetivos:

- Identificar quais funções chamam quais outras funções.
- Identificar funções menos chamadas ou com menor dependência.
- Definir uma ordem inicial de conversão.
- Reduzir risco ao converter primeiro unidades menores e mais isoladas.

Dados a registrar:

- Símbolo chamador.
- Símbolo chamado.
- Local da chamada.
- Tipos dos argumentos.
- Tipo de retorno esperado.
- Frequência de chamadas.
- Dependências externas envolvidas.

Resultado esperado:

- Grafo persistido no SQLite.
- Ranking de funções/métodos por simplicidade ou isolamento.

### 6. Captura de comportamento com mocks

Começando pelos elementos menos chamados ou mais isolados, a ferramenta deve criar testes unitários mockados para registrar comportamento observável.

Formato conceitual do registro:

`Função X`, chamada com entrada `Y`, chama `A`, `B` e `C` com valores `a1`, `b1` e `c1`, e retorna `X1`.

Dados a capturar:

- Função ou método sob teste.
- Entradas utilizadas.
- Chamadas feitas a dependências mockadas.
- Argumentos enviados a cada dependência.
- Ordem das chamadas, se relevante.
- Valor retornado.
- Alterações em parâmetros por referência ou ponteiro.
- Alterações em estado interno, quando aplicável.
- Exceções lançadas, quando aplicável.

Resultado esperado:

- Testes C++ que caracterizam o comportamento original.
- Dados de comportamento persistidos no SQLite.
- Base para gerar testes equivalentes em Dart.

### 7. Extração estrutural com Tree-sitter

A ferramenta deve varrer cada arquivo fonte e coletar informações sintáticas.

Informações esperadas:

- Declarações de tipos.
- Declarações de funções e métodos.
- Parâmetros e retornos.
- Corpos de funções.
- Variáveis locais.
- Atribuições.
- Condicionais.
- Loops.
- Chamadas de função.
- Acessos a membros.
- Includes e namespaces.

Resultado esperado:

- Modelo sintático persistido.
- Relação entre código fonte original e elementos internos da ferramenta.

### 8. Mapeamento de tipos, nomes e símbolos

A ferramenta deve mapear elementos C++ para seus equivalentes Dart.

Mapeamentos esperados:

- Tipos C++ para tipos Dart.
- Nomes de classes, structs e enums.
- Nomes de métodos e funções.
- Nomes de variáveis e parâmetros.
- Namespaces C++ para organização Dart.
- Headers/includes para imports Dart.

Regras desejadas:

- Evitar nomes inválidos em Dart.
- Resolver conflitos de nomes.
- Preservar rastreabilidade com o código original.
- Registrar decisões no SQLite.
- Reutilizar decisões já tomadas em conversões posteriores.

Resultado esperado:

- Mapa persistido de símbolos C++ para Dart.
- Lista de conflitos resolvidos automaticamente.
- Lista de conflitos pendentes de decisão humana.

### 9. Geração de código sem implementação

Antes de converter os corpos das funções, a ferramenta deve gerar uma versão estrutural do código Dart.

Conteúdo esperado:

- Classes.
- Enums.
- Assinaturas de métodos.
- Assinaturas de funções.
- Campos.
- Imports.
- Stubs de implementação.

Objetivos:

- Validar se a estrutura geral do código Dart é compilável.
- Detectar problemas de nomes, tipos e organização antes da conversão lógica.
- Permitir revisão humana da estrutura gerada.

Resultado esperado:

- Projeto Dart inicial.
- Código Dart estruturalmente válido.
- Diagnósticos do LSP Dart e/ou compilador Dart.

### 10. Conversão incremental das implementações

A ferramenta deve procurar no SQLite as funções e métodos mais simples e convertê-los em ordem crescente de complexidade.

Critérios possíveis de simplicidade:

- Poucas linhas de código.
- Poucas chamadas externas.
- Poucos branches.
- Ausência de ponteiros ou referências complexas.
- Ausência de templates.
- Tipos já mapeados.
- Testes já existentes.
- Baixa centralidade no grafo de chamadas.

Processo esperado para cada função/método:

- Recuperar mapeamentos de nomes e tipos.
- Converter expressões e comandos C++ para Dart.
- Gerar ou atualizar a implementação Dart.
- Gerar teste Dart equivalente ao teste C++ existente.
- Compilar o código Dart.
- Executar o teste Dart.
- Registrar sucesso ou falha no SQLite.
- Em caso de falha, apresentar diagnóstico e opções ao usuário.

Resultado esperado:

- Código Dart convertido incrementalmente.
- Testes equivalentes passando.
- Histórico de conversão e validação.

## Interação com o usuário

A ferramenta deve ser interativa, apresentando escolhas em momentos onde a decisão automática não for segura.

Possíveis pontos de interação:

- Confirmar mapeamento de tipos.
- Resolver conflitos de nomes.
- Escolher como tratar APIs externas.
- Selecionar equivalentes Dart para bibliotecas C++.
- Aprovar estrutura Dart gerada.
- Decidir o que fazer quando um teste falha.
- Marcar trechos como fora de escopo temporariamente.

Cada decisão deve ser persistida para evitar perguntas repetidas.

## Dados persistidos no SQLite

Modelo conceitual inicial:

- Projetos importados.
- Arquivos analisados.
- Símbolos C++.
- Símbolos Dart gerados.
- Relações chamador-chamado.
- Mapeamentos de tipos.
- Mapeamentos de nomes.
- Decisões do usuário.
- Casos de teste C++.
- Casos de teste Dart.
- Resultados de compilação.
- Resultados de execução de testes.
- Falhas e diagnósticos.

## Resultado final esperado

Ao final de uma conversão bem-sucedida, a ferramenta deve produzir:

- Um projeto Dart compilável.
- Código Dart com estrutura equivalente ao código C++ original.
- Implementações convertidas de forma incremental.
- Testes Dart equivalentes aos testes gerados para o C++.
- Registro persistido das decisões tomadas.
- Relatório de itens convertidos, parcialmente convertidos e não convertidos.

## Riscos técnicos

- C++ possui recursos sem equivalente direto em Dart.
- Tree-sitter fornece sintaxe, mas não resolve toda a semântica necessária.
- Templates, macros, overloads e ponteiros podem exigir tratamento especial.
- Gerar mocks automaticamente pode ser difícil em código com muitas dependências.
- Capturar comportamento apenas por testes pode não cobrir todos os caminhos.
- KLEE pode exigir restrições ou adaptações relevantes para projetos reais.
- Empacotar KLEE desde a primeira versão aumenta a complexidade do Flatpak, especialmente pela dependência de versões compatíveis de LLVM e solver SMT.
- Equivalência entre C++ e Dart pode ser afetada por diferenças de tipos numéricos, memória, exceções e bibliotecas.
- O Flatpak autossuficiente tende a ser maior e mais complexo de construir, pois inclui toolchain, SDKs e ferramentas auxiliares.
- Projetos C++ importados podem depender de bibliotecas, headers ou ferramentas específicas não incluídas no Flatpak; esses casos precisarão ser detectados e reportados como limitações do projeto importado ou itens a empacotar futuramente.

## Perguntas em aberto

1. Qual subconjunto de C++ deve ser suportado na primeira versão?
2. A ferramenta deve receber um projeto CMake completo ou também arquivos C++ avulsos?
3. O alvo inicial gerado em Dart será um pacote Dart puro, um app Flutter, uma biblioteca ou outro formato?
4. A interface Flutter será a única interface da ferramenta ou também existirá uma CLI para automação e testes?
5. O núcleo Rust deve ser organizado como biblioteca reutilizável, binário, workspace com múltiplos crates ou outro formato?
6. O nome correto da ponte será `flutter_rust_bridge`? O rascunho original escreve `flutte-rust-bridge`, que parece ser apenas um erro de digitação.
7. O usuário deve interagir durante a conversão inteira ou a ferramenta deve executar em lote e perguntar apenas nos bloqueios?
8. Os mocks devem ser gerados automaticamente, manualmente ou por um fluxo semi-automático?
9. Como será definido que uma função é "simples" para priorização?
10. O critério "tipos menos chamados" significa tipos/funções com menos dependentes, menos dependências ou ambos?
11. Como tratar ponteiros, referências e ownership na primeira versão?
12. Como tratar `std::*` inicialmente? Deve haver uma tabela fechada de tipos suportados?
13. Como tratar templates na primeira versão?
14. Como tratar macros e código condicionado por pré-processador?
15. Como tratar overloads de funções e operadores?
16. Como tratar exceções C++ em Dart?
17. Como tratar comportamento indefinido ou dependente de plataforma no código C++?
18. Os testes C++ devem ser escritos em gtest obrigatoriamente?
19. Os testes Dart devem usar `package:test` ou outro framework?
20. O SQLite deve armazenar apenas metadados ou também snapshots de código gerado?
21. A ferramenta deve permitir retomar conversões interrompidas?
22. A ferramenta deve suportar edição manual do código Dart gerado sem sobrescrever alterações do usuário?
23. Qual deve ser o formato dos relatórios de conversão?
24. Como a ferramenta deve lidar com bibliotecas C++ externas sem equivalente em Dart?
25. A validação por LSP será obrigatória ou complementar a compilação/testes?
26. O conteúdo de `tmp/verovio-port2` e `tmp/legacy-bridge` deve ser apenas referência conceitual ou deve ser migrado parcialmente para a implementação atual?
27. O projeto `verovio` deve ser copiado para dentro deste repositório, referenciado externamente ou baixado automaticamente por algum script?
