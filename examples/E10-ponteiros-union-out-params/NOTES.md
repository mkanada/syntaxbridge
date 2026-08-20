# E10 — Ponteiros, `union`, out params

Décimo degrau. O primeiro em que a resposta certa, na prática, é recusar —
e onde recusar de forma **honesta** (não silenciosamente errada) é o que
realmente precisava de correção.

## O que ele forçou a existir

- Nada de novo na IR. O catch-all `Type::Unsupported(spelling)` de
  `lower_type` já existia desde o E01 — este degrau prova que ele cobre
  ponteiro cru corretamente, e corrige um caso em que **não** cobria.

## Armadilhas

- **A armadilha documentada — talvez a resposta certa seja recusar — já
  funcionava de graça para ponteiro cru (`int*`), e revelou um bug real
  para `union`.** `int* a` cai no braço `_ =>` genérico de `lower_type`
  (nenhum caso trata `CXType_Pointer`), vira
  `Type::Unsupported("int *")`, e o mecanismo já existente desde o E01
  ("um `Stmt`/`Type::Unsupported` em qualquer profundidade derruba a
  função inteira") faz `trocar` compilar e lançar
  `UnimplementedError` — honesto, sem tentar adivinhar `dart:ffi`.

  `union ValorBruto`, porém, **não** caía nesse braço: `CXType_Record` é o
  mesmo *type kind* que Clang usa para `struct`/`class`/`union` (não há um
  `CXType_Union` separado), então `lower_type` tentava resolver
  `ValorBruto` como se fosse um `Record` normal — usr/nome resolvem
  perfeitamente (é uma declaração real) — devolvendo
  `Type::Record { usr, name: "ValorBruto" }`. O problema:
  `function_catalog::visit_cursor` só despacha `CXCursor_StructDecl`/
  `CXCursor_ClassDecl` para `lower_record` — nunca `CXCursor_UnionDecl` —
  então nenhum `ir::Record` para `ValorBruto` chega a existir. Resultado:
  um parâmetro Dart do tipo `ValorBruto`, apontando para uma classe que
  nunca é gerada — `dart analyze` reclamando `undefined_class`, pego na
  primeira tentativa (não um "quase bug" teórico: apareceu no golden
  abençoado antes de qualquer correção). Nenhum degrau anterior tinha um
  `union` para expor isso. Corrigido checando `clang_getCursorKind(decl) ==
  CXCursor_UnionDecl` dentro do próprio ramo `CXType_Record`, devolvendo
  `Unsupported` explicitamente antes de chegar à resolução de usr/nome que
  produziria a referência pendurada.

## Decisão de projeto tomada aqui

- **Nenhuma tentativa de gerar ponte real via `dart:ffi`.** Um `Struct`
  do `dart:ffi` de verdade — layout de memória, offset por campo,
  convenção de chamada — é uma categoria de trabalho bem maior do que
  qualquer degrau anterior, e a própria descrição deste degrau já aponta
  a resposta honesta ("talvez a resposta certa seja recusar"). Fica em
  aberto para quando um caso de uso real (não sintético) justificar o
  custo.
- **Atualização (round 19 do loop de bailouts, `docs/prompts/2026-08-20-loop-bailout.md`):
  o idioma de "out param" por ponteiro passou a ser reconhecido.** `int*
  a` (e `int&`/`double&`/`bool&` na forma de referência, já suportada
  desde E13) agora vira um parâmetro Dart comum do tipo do próprio
  ponteiro (`int a`, não `int? a`) mais um slot no record de retorno
  (`(int, int) trocar(int a, int b)`), com o call site desconstruindo o
  record de volta nas variáveis do chamador (`(a, b) = trocar(a, b);`).
  `trocar` neste fixture — cujo único propósito é justamente esse idioma
  — deixou de ser um bailout honesto para virar Dart real, exercitando o
  próprio golden deste degrau. Escopo continua restrito a ponteiro para
  escalar puro (`int`/`double`/`bool`; um buffer nomeado como
  `mz_uint8*`/`uint8_t*` continua `Bytes`, nunca reinterpretado como
  out-param) e a um argumento de chamada com a forma exata `&lvalue`
  (`nullptr` ou qualquer outra forma vira bailout explícito, nunca um
  `ExprStmt` que descartaria o record de retorno silenciosamente). `union
  ValorBruto` continua fora de escopo, sem mudança — a decisão de não
  gerar `dart:ffi` real para `struct`/`union` acima permanece.
- **`mapping::signature_options_for`** (o solver que já detecta ponteiro,
  inteiro de largura fixa, `float`, `setjmp`/`goto`/mutex por assinatura)
  **não é consultado pela geração aqui**, pela mesma razão do E08/E09: o
  catch-all de `lower_type` já produz o resultado certo sozinho — a
  consulta ao solver mudaria a *descrição* apresentada ao usuário (US-7,
  UI), não o Dart gerado.
