# Mudanças na interação com o usuário

Decisões tomadas em 2026-08-19, a partir de esclarecimentos do usuário sobre a
versão original deste prompt.

## 1. Lista de externos em arquivo texto

Mover a fonte de verdade de "o que é externo" das tabelas SQLite do projeto
(`external_marks`, `external_name_regexes`, `external_path_regexes` em
`project.db`, ver `crates/server/src/persistence/project_store.rs`) para um
arquivo texto por projeto, editável externamente ao syntax-bridge.

- **Escopo**: o arquivo cobre tudo que hoje está nas três tabelas — marcas
  manuais por item (usr ligado/desligado), regras de regex por nome, regras de
  regex por caminho — e também as marcas de arquivo inteiro do item 3 abaixo.
- O usuário continua definindo o que é externo de dentro do syntax-bridge (a UI
  atual em `client/flutter/lib/src/ui/externals_view.dart` e os botões de
  toggle continuam existindo); a diferença é só onde esse estado é persistido.
- **Local**: dentro do diretório do projeto (mesmo nível de `project.db`), não
  no banco global (`~/.../projects`).
- **Formato**: texto simples, legível e diffável em controle de versão — é o
  ponto de ser "editável externamente". Definir um formato concreto (ex.: TOML
  com seções para marcas manuais / regex-por-nome / regex-por-caminho /
  marcas-de-arquivo, ou um formato linha-a-linha mais simples) na
  implementação.
- **Migração**: projetos existentes com dados nas três tabelas precisam ser
  migrados para o novo arquivo (sem perder marcas já feitas pelo usuário) na
  próxima vez que o projeto for aberto.
- Depois da migração, avaliar se as tabelas somem completamente (servidor lê o
  arquivo diretamente ou mantém um cache em memória recarregado dele) ou se
  sobra alguma como índice derivado — mas a fonte de verdade passa a ser o
  arquivo, não o banco.

## 2. Ingestão mínima + etapa "Analyse"

Hoje `project_service::create_project` (`crates/server/src/project_service.rs`)
faz tudo em um único passo em background: descompacta o arquivo, roda cmake,
extrai via libclang os 4 catálogos (tipos, fontes, funções, ponteiros) —
incluindo usos, dependências, grafo de chamadas e IR — e grava tudo em
`project.db` numa única transação (`ProjectStore::replace_all`). Não existe
hoje nenhuma etapa "Analyse" separada.

Passa a ser dividido em duas fases:

- **Ingestão** (roda automaticamente ao criar/abrir o projeto, como hoje):
  descompacta o arquivo, roda cmake, faz o parse mínimo necessário e persiste
  apenas `source_files`, `type_declarations` e `function_declarations`
  (existência + assinatura + usr — o suficiente para o usuário navegar a lista
  de arquivos/tipos/funções e decidir o que marcar como externo). **Não**
  grava `call_edges`, `type_usages`, `type_dependencies`,
  `ir_functions`/`ir_records`/`ir_enums` nem `pointer_declarations` nessa
  fase.
- **Análise** (novo botão "Analyse", disparado pelo usuário quando quiser
  prosseguir): roda o restante da extração e grava `call_edges`,
  `type_usages`, `type_dependencies`, IR e `pointer_declarations`. A detecção
  automática de função externa por estar indefinida
  (`ExternalSource::AutoUndefinedFunction` em
  `crates/server/src/externals.rs`, que depende do grafo de chamadas) só fica
  disponível depois dessa etapa — antes disso, essa fonte de marcação
  simplesmente não contribui nenhuma marca ainda, sem erro.
- Qualquer funcionalidade que dependa de dados pós-análise deve tratar
  "ingerido mas ainda não analisado" como um estado normal, não bloquear o
  fluxo nem exigir a análise como pré-requisito duro — mesmo princípio já
  adotado no AGENTS.md para a caracterização opcional (US-6).
- Definir onde esse estado fica registrado (provavelmente uma coluna de status
  equivalente a `last_ingest_status`, em
  `crates/server/src/persistence/global_store.rs`, para algo como
  `last_analysis_status`).

## 3. Arquivo como item externo de primeira classe (reversão de decisão)

**Reverte** a decisão "Cascata é foto, não vínculo" registrada em
`docs/plans/lista-de-externos.md` (decisão 3). Hoje, marcar um arquivo como
externo (`mark_file_external` / `expand_file_mark` em
`crates/server/src/externals.rs`) só expande, no momento do clique, para
marcas individuais por usr — o arquivo em si não tem estado próprio, e
desmarcar exige desmarcar item por item.

A partir de agora, o arquivo passa a ter estado próprio de "externo", com
vínculo persistente:

- Marcar um arquivo como externo cria uma marca no nível do arquivo (não mais
  uma expansão pontual em marcas por usr).
- Desmarcar o arquivo desmarca de uma vez todos os itens daquele arquivo, sem
  precisar desmarcar item por item.
- Itens novos que aparecerem no arquivo após uma reingestão/reanálise (ex.:
  uma função nova adicionada ao código-fonte) herdam automaticamente o status
  de externo do arquivo, sem ação manual do usuário.
- Essa marca de arquivo entra no mesmo arquivo texto do item 1, como um tipo
  de entrada adicional (ao lado de marcas manuais por item e regras de regex).
- Na UI (`client/flutter/lib/src/ui/source_files_view.dart`), o botão de
  "marcar arquivo como externo" deixa de ser uma ação de disparo único e passa
  a ser um toggle (mesmo padrão visual de
  `external_toggle_button.dart`, já usado em `TypesView`/`FunctionsView`),
  refletindo o estado persistente do arquivo.
- Atualizar `docs/plans/lista-de-externos.md` para registrar essa reversão
  explicitamente, no mesmo padrão usado no AGENTS.md para a reversão do
  GoogleTest (Q10): o que mudou, por quê, e a partir de quando.

## 4. Nome de exibição dos Source Files

Bug confirmado em `SourceFilesView._projectRelativeFile`
(`client/flutter/lib/src/ui/source_files_view.dart`): hoje corta apenas o
prefixo `project.projectDir`, não `project.inputSourceDir`. Como o arquivo é
descompactado em `project_dir/input-source/`, o prefixo
`input-source/<pasta-raiz-do-arquivo-compactado>/...` continua aparecendo na
lista (ex.: `input-source/verovio-version-6.2.0/include/json/jsonxx.h`).

Correção: cortar também o primeiro nível de diretório abaixo de
`input-source/` — a pasta-raiz que o `.zip`/`.tar` continha — exibindo o
caminho relativo à raiz do projeto C++ em si (ex.:
`include/json/jsonxx.h`).

- Tratar o caso de arquivos compactados sem uma única pasta-raiz (arquivos
  soltos na raiz do `.zip`): nesse caso não há segundo nível para cortar além
  de `input-source/`.
- `project.inputSourceDir` já existe end-to-end (servidor → JSON →
  `CreatedProject`/`LoadedProject` em
  `client/flutter/lib/src/project/project_models.dart`); falta só usá-lo.

## Notas gerais de implementação

- Seguir TDD (AGENTS.md): cada mudança de comportamento começa com um teste
  que falha.
- Mudanças na tela de Source Files (item 4) e no toggle de arquivo externo
  (item 3) são mudanças de UI e precisam de teste de screenshot novo em
  `client/flutter/test/screenshots/`, cobrindo os estados relevantes (nome
  limpo na lista; arquivo marcado/desmarcado como externo).
- O item 2 introduz um novo botão/etapa "Analyse" na UI — também precisa de
  teste de screenshot cobrindo o estado "ingerido, aguardando análise" e o
  estado "analisado".
