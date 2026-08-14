# E08 — Templates

Oitavo degrau. Primeiro em que a mesma função C++ tem **corpos diferentes
por instanciação** — não só assinaturas diferentes (E07) — e onde a solução
real é gerar Dart concreto por instanciação, não tentar expressar
genéricos parametrizados por operador.

## O que ele forçou a existir

- `lower::cpp::monomorphized_template_name` — nome determinístico para
  *qualquer* instanciação de template de função (`dobro` + sufixo de tipo
  concreto de cada parâmetro → `dobroInt`/`dobroDouble`/`dobroString`),
  compartilhando `overload_type_suffix` com o esquema de renomeação de
  sobrecarga do E07 (`function_catalog::dart_overload_name`) — mesma regra,
  duas origens de decisão (US-7 sobrecarga vs. instanciação de template).
- `function_catalog::record_call` ganha um segundo papel: além de montar o
  grafo de chamadas (US-5), agora também **sintetiza** o `ir::Function`
  monomorfizado de uma instanciação implícita nunca visitada
  independentemente pela travessia de topo — usando o próprio cursor
  `referenced` (que já chega com tipos concretos substituídos, não o `T`
  abstrato do template primário) como fonte para `lower::cpp::lower_function`.
- O caminho de `FreeFunction` em `visit_cursor` (usado desde o E01) ganha
  uma checagem a mais: se o próprio cursor sendo lowered é uma
  especialização explícita de template
  (`clang_getSpecializedCursorTemplate` não-nulo), seu nome também passa
  pelo mesmo `monomorphized_template_name` — para nunca divergir do nome
  que os call sites vão usar.

## Armadilhas

- **A armadilha documentada — especialização explícita e SFINAE: recusar,
  não adivinhar — não precisou de um caminho de recusa separado, porque a
  arquitetura escolhida nunca chega a *adivinhar*.** A alternativa óbvia
  (monomorfizar sempre a partir do corpo do template primário,
  substituindo `T` mecanicamente) teria produzido `dobroString` errado —
  concatenação ingênua (`valor + valor`) em vez do corpo real da
  especialização (`valor + " (dobrado)"`) — silenciosamente, porque nada
  no C++ *impede* essa substituição de compilar. Em vez disso, toda
  instanciação (implícita ou especialização explícita) é lowered a partir
  do **próprio cursor resolvido** (`referenced`), que o `libclang` já
  entrega com os tipos concretos substituídos e, no caso de uma
  especialização explícita, com o corpo *realmente escrito* para aquele
  tipo — nunca o corpo do template primário reinterpretado. O caso em que
  isso ainda poderia dar errado (uma instanciação cujo corpo `libclang` não
  consegue expor de jeito nenhum) já cai no caminho `Unsupported` existente
  de `lower_function`/`lower_expr` — "recusar" continua sendo o
  comportamento de fallback, só que herdado, não reimplementado.

- **`clang_getSpecializedCursorTemplate` não distingue especialização
  explícita de instanciação implícita — mas não precisava.** As duas
  reportam a mesma coisa (cursor do template primário, kind
  `CXCursor_FunctionTemplate`). A distinção que realmente importa —
  "este `usr` já tem uma declaração de nível superior de verdade, ou
  preciso sintetizar uma?" — é feita indiretamente: uma especialização
  explícita É visitada no nível superior (é uma declaração real do
  arquivo, com o mesmo tratamento que qualquer `FreeFunction` já tinha
  desde o E01); uma instanciação implícita nunca é. `record_call` só
  sintetiza quando `ir_seen.insert(usr)` ainda não tinha visto aquele usr
  — se a especialização explícita já foi visitada primeiro (ordem de
  declaração-antes-do-uso, a mesma premissa que o E04 já assume para
  membro fora de linha), a síntese não faz nada; se não, o merge final
  entre workers (`extract_function_catalog_cancellable`) ainda dedupe por
  usr.

- **`operator+`/`operator==` de `std::string` também são templates de
  função na `libstdc++`** — `clang_getSpecializedCursorTemplate` não-nulo
  para eles também. Sem cuidado, a renomeação por monomorfização
  interceptava a chamada *antes* de `lower_stdlib_operator_call` (E05) ter
  a chance de reconhecer `callee_name == "operator+"`, produzindo nomes
  como `UnsupportedString`/`StringString` — regressão real em todo fixture
  E05, pega pela suíte de regressão do corpus antes de virar golden.
  Corrigido restringindo a renomeação por monomorfização a templates
  **fora de cabeçalho de sistema** (`clang_Location_isInSystemHeader`) — a
  mesma guarda que `lower_stdlib_operator_call` já usava para o problema
  inverso (reconhecer biblioteca padrão, não confundir com código do
  usuário).

- **`std::string("oi")` (construção por sintaxe funcional, usada como
  argumento) tem filhos `NamespaceRef`/`TypeRef` próprios antes do
  `CXXConstructExpr` de verdade** — a mesma armadilha do valor default de
  parâmetro (E07) e do inicializador de variável local (E03), num terceiro
  lugar: o desembrulho genérico de wrapper transparente
  (`is_transparent_wrapper`) filtra agora `TypeRef`/`NamespaceRef`/`TemplateRef`
  antes de checar "exatamente um filho restante", não só nos dois sites
  específicos anteriores.

## Decisão de projeto tomada aqui

- **Monomorfização é a única estratégia implementada — genéricos Dart
  parametrizados (`T dobro<T extends num>(T valor)`) não foram tentados.**
  O corpo do template (`valor + valor`) depende implicitamente de `T` ter
  `operator+` — em C++ isso é resolvido por SFINAE/sobrecarga no ponto de
  instanciação; expressar a mesma coisa como bound genérico do Dart
  (`T extends num`, e ainda assim precisando de `as T` no retorno) exigiria
  modelar essa dependência de operador na IR, um trabalho de escopo bem
  maior que nenhum fixture força a fazer ainda. Monomorfização sempre
  produz Dart correto porque nunca tenta generalizar — cada instanciação
  vira sua própria função concreta, do jeito que `lower_function` já sabe
  lowerar.
- **Escopo restrito a template de função livre** — método de template
  (`E04`+`E08` juntos) não é tratado; `record_call`'s síntese só age sobre
  `referenced_kind == CXCursor_FunctionDecl`.
- **`mapping::template_options_for` não é consultado pela geração neste
  degrau, ao contrário de `overload_options_for` desde o E07.** O solver já
  existe e já distingue "decisão local" de "decisão global" (por quantos
  arquivos diferentes instanciam o template), mas não devolve uma opção
  "genéricos" de verdade — só sinaliza o escopo da decisão. Como este
  fixture é de único arquivo (multi-TU é escopo do E11) e a estratégia
  escolhida (monomorfizar sempre) não muda com esse sinal, consultar o
  solver aqui não mudaria nenhum comportamento — fica para quando um
  fixture com instanciações em mais de um arquivo (E11) fizer a diferença
  entre "local" e "global" importar de verdade.
