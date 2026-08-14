# E12 — Exceções e RAII

Décimo segundo degrau. Primeiro a lowering um construto C++ que não tem
representação direta em Dart: um destrutor determinístico executado na saída
de escopo. `throw`/`try`/`catch`, em contraste, mapeiam quase palavra por
palavra.

## O que ele forçou a existir

- `ir::Stmt::Throw`/`TryCatch` — lowering direto de `CXXThrowExpr`/
  `CXXTryStmt`/`CXXCatchStmt`, emitidos como `throw`/`try { } on T catch
  (nome) { }`.
- `ir::Stmt::TryFinally` — nunca produzido a partir de um cursor C++
  (C++ não tem `finally`); só existe como saída *sintetizada* da nova
  passagem `apply_raii_scope_guards`.
- `ir::Record.destructor: Option<Vec<Stmt>>` — o corpo (já lowered) do
  destrutor de um record, quando ele faz trabalho real. Nunca emitido como
  membro da classe Dart; só consumido pela passagem de RAII.
- `function_catalog::apply_raii_scope_guards` — passagem de pós-processamento
  (roda depois que todo record/função já foi lowered, no mesmo ponto onde
  `apply_overload_renames` do E07 já rodava): para cada função, procura o
  *primeiro* `VarDecl` de nível superior cujo tipo tem destrutor com corpo
  real, e envolve tudo que vem depois dele num `TryFinally` cujo `finally`
  é o corpo do destrutor, com `Expr::This` substituído pela referência ao
  próprio local (`replace_this_with_ref_in_stmts`/`_stmt`/`_expr` — terceiro
  caminhador mecânico da sessão, mesmo padrão de `rename_calls_in_*` do E07
  e `collect_referenced_usrs_in_*` do E11).

## Armadilhas

- **RAII não tem construto Dart equivalente — é sintetizado, não traduzido.**
  C++ garante que o destrutor de `Guarda g` roda no fim do escopo de `g`,
  determinística e incondicionalmente (inclusive se uma exceção for lançada
  no meio). O único jeito de expressar "roda incondicionalmente ao sair do
  bloco" em Dart é `try`/`finally` — não existe destrutor, `Finalizer` do
  Dart é não determinístico (amarrado ao GC) e não serve. A saída correta
  não é uma tradução 1:1 de `~Guarda()`; é uma reestruturação do *fluxo de
  controle* ao redor da declaração.

- **A passagem de RAII só olha o primeiro guard, só no nível superior, só em
  funções livres — decisão de escopo, não lacuna silenciosa.** Duas locais
  RAII na mesma função exigiriam `try`/`finally` *aninhados* (o guard de
  cada uma ativo só a partir da própria declaração); nenhum fixture força
  isso ainda, e uma segunda local desse tipo hoje fica como `VarDecl` comum,
  sem destrutor chamado — um buraco real, mas documentado, não escondido.
  Da mesma forma, um guard dentro de um `if`/`while`/`for` ou dentro de um
  método/construtor não é detectado (a passagem não recursa em blocos
  aninhados nem processa `Method`/construtor, só `Function.body` de nível
  superior) — nenhum fixture ainda cria essa forma.

- **Campo estático acessado de fora da classe precisava de qualificação que
  nunca tinha sido testada.** `usarGuarda()` (função livre) lê/escreve
  `Guarda::contadorAberto` — um `DeclRefExpr` para um `VarDecl` estático cujo
  pai semântico é a classe. C++ aceita `contadorAberto` bare *dentro* da
  classe e exige `Guarda::` fora; Dart exige `Guarda.contadorAberto` nos dois
  lugares (o qualificador é sempre válido, nunca é exigido só de fora).
  `lower_expr` não tem noção de "classe atual" para decidir condicionalmente,
  então a correção (`qualified_static_member_name`) qualifica sempre,
  incondicionalmente — inclusive dentro dos próprios métodos da classe. Isso
  reescreveu a golden do E04 (`totalContas` → `ContaBancaria._totalContas`
  nos próprios construtores/método da classe) — reverificado depois: E04
  segue passando golden + `dart analyze`/`format` + oráculo sem nenhuma outra
  mudança.

- **O destrutor deste fixture só toca estado estático — o guard local nunca
  aparece no Dart emitido.** `~Guarda()` é `contadorAberto = contadorAberto -
  1;`, que já lowera (via `qualified_static_member_name`, acima) para
  `Guarda.contadorAberto = ...` sem nunca passar por `Expr::This` — não há
  `this` implícito para substituir, porque acesso a campo estático nunca usa
  receptor. Resultado: `Guarda g = Guarda();` sobrevivia à emissão com `g`
  jamais referenciado em lugar nenhum do corpo emitido — `unused_local_
  variable` do `dart analyze` (que o harness trata como falha, não só
  erros). Corrigido de forma geral em `apply_raii_scope_guard_to_stmts`: se
  nem o `try_body` nem o `finally_body` (já com a substituição `This`→`Ref`
  aplicada) referenciam o nome do guard, a declaração vira uma expressão
  solta (`Guarda();`, só para disparar o construtor) em vez de um `VarDecl`
  nomeado — mesmo efeito colateral, sem o aviso. Um destrutor que *usa*
  estado de instância continuaria gerando um `VarDecl` nomeado normalmente,
  porque a substituição teria algo real para trocar.

- **`return -1;` sobrando depois de um `try`/`catch` exaustivo é `dead_code`
  em Dart, mesmo sendo código C++ válido.** A primeira versão do fixture
  tinha um `return -1;` de guarda depois do `try { throw 42; } catch (int) {
  return ...; }` em `testarExcecaoCapturada()` — nunca alcançável nem em
  C++ (o `try` só sai por exceção ou pelo `catch`, que sempre retorna), mas
  C++ não exige isso do compilador. O analisador do Dart, sim. Removido do
  `.cpp` em vez de suprimido na emissão — o código realmente era morto nas
  duas linguagens.

## Decisão de projeto tomada aqui

- **`throw`/`rethrow` sem operando (`throw;`) e `catch (...)` catch-all não
  são suportados** — ambos retornam `Unsupported` com motivo explícito
  (`lower_stmt`, nos ramos de `CXXThrowExpr`/`CXXTryStmt`/`CXXCatchStmt`).
  Nenhum fixture usa nenhum dos dois; "silêncio é proibido" exige que a
  ausência apareça como razão, não como tradução errada.
- **Múltiplos `catch` para o mesmo `try` não são suportados** — `CXXTryStmt`
  com mais de dois filhos (`[try, catch]`) vira `Unsupported`. Mapear para a
  cadeia `on T1 catch (...) { } on T2 catch (...) { }` do Dart é mecânico,
  mas nenhum fixture força mais de uma cláusula ainda.
