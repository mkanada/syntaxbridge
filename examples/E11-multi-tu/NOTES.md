# E11 — Multi-TU, namespaces, CMake real

Décimo primeiro degrau. Primeiro com mais de uma unidade de compilação — e
o primeiro em que "para qual arquivo Dart isto vai" deixa de ser a mesma
pergunta que "onde isto foi declarado".

## O que ele forçou a existir

- `emit::dart::emit_module` passa a montar `usr_to_stem` — todo `usr` de
  `Record`/`Function` de nível superior mapeado para o stem do arquivo que
  o declara — antes de emitir qualquer arquivo individual.
- `collect_referenced_usrs_in_record`/`_type`/`_stmt`/`_expr` — um
  caminhamento novo (mecânico, espelhando o `rename_calls_in_*` do E07 em
  `function_catalog.rs`, mas coletando em vez de renomear) sobre toda a
  árvore de um `Record`/`Function`: tipos de campo, base/mixins,
  parâmetros, tipo de retorno, e todo `usr` referenciado dentro do corpo
  (`Expr::Call.callee_usr`, `Expr::ConstructorCall`/`RecordConstruct.type_usr`,
  todo `Type::Record` encontrado em qualquer posição).
- `emit_file` usa os dois para decidir seus próprios `import '<outro>.dart';`
  — todo `usr` referenciado cujo stem (via `usr_to_stem`) difere do stem do
  próprio arquivo.

## Armadilhas

- **A armadilha documentada — header incluído em N TUs duplica declaração
  nos catálogos — já não acontecia na IR usada para geração.** A
  deduplicação por `usr` na junção final entre workers
  (`extract_function_catalog_cancellable`, `ir_seen`/`ir_record_seen`) já
  existia desde o E01 e já cobria multi-TU de graça: `Ponto3D`/
  `normaAoQuadrado`, declarados em `comum.hpp` e incluídos por
  `distancia.cpp` e `escala.cpp`, geram exatamente uma entrada em
  `ir_records`/`ir_functions`, não duas — confirmado, não assumido
  (`"1 ir records"` no log da extração, apesar de duas unidades de
  compilação analisando o mesmo header). US-3/US-5 convivem com duplicação
  em seus próprios catálogos (é só uma lista para a UI, mostrar a mesma
  declaração duas vezes é cosmético); a geração de código nunca teve esse
  problema porque a chave de dedup (`usr`, estável entre TUs) já era usada
  desde o primeiro commit.

- **A armadilha real não documentada — nenhum arquivo Dart gerado jamais
  precisou importar outro, porque nenhum fixture anterior teve mais de um
  arquivo de saída com uma referência cruzada.** Cada exemplo E01–E10
  vive num único `.cpp`, então cada um sempre virou exatamente um arquivo
  `.dart` autossuficiente — a ausência de `import` nunca foi testada.
  Rodar E11 sem a correção mostrou exatamente o buraco: `distancia.dart`/
  `escala.dart` referenciando `Ponto3D`/`normaAoQuadrado` (declarados em
  `comum.dart`) sem importar nada — `dart analyze` acusando
  `undefined_class`/`undefined_function` em cada uso. A dedução de
  dependência via `usr_to_stem` + o caminhamento de coleta resolve isso de
  forma geral, não só para os dois casos deste fixture.

## Decisão de projeto tomada aqui

- **Chamada de método entre arquivos não é detectada.** `usr_to_stem` só
  mapeia `Record`/`Function` de nível superior — o `usr` de um `Method`
  não está nele, então uma chamada `objeto.metodo()` cujo tipo estático
  mora em outro arquivo não geraria o `import` necessário. Nenhum fixture
  força isso ainda (os métodos usados em E04–E09 sempre operam dentro do
  próprio arquivo da classe). Não é uma lacuna silenciosa: o resultado é
  `undefined_method` do `dart analyze`, alto o bastante para aparecer na
  hora certa quando um fixture futuro precisar disso.
- **Nenhum uso de `namespace` C++ neste fixture** — apesar do nome do
  degrau incluir "namespaces". `type_catalog`/`function_catalog` já
  capturam `namespace` desde o US-3/US-5 (`TypeDeclaration.namespace`/
  `FunctionDeclaration.namespace`), mas nada na geração de Dart ainda usa
  esse campo (não há `library`/prefixo de biblioteca Dart correspondente a
  namespace C++). Fica em aberto para quando um fixture com dois tipos de
  mesmo nome em namespaces diferentes forçar essa decisão — este degrau já
  tinha o bastante para provar sozinho (dedup + import), sem também abrir
  a questão de nomes de biblioteca Dart.
