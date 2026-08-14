# E06 — Herança simples e `virtual`

Sexto degrau. Primeiro que sai de uma única classe (E04) para uma hierarquia:
`extends`, método abstrato/`@override`, despacho dinâmico. Também o primeiro
degrau em que uma classe do usuário (`Animal`, `Cachorro`, `Gato`) é passada
por referência (`const Animal&`) — não só tipos de biblioteca (E05).

## O que ele forçou a existir

- `ir::Record.base_class: Option<BaseClass>` — herança simples só (E09 é
  "herança múltipla"; uma classe com mais de um `CXXBaseSpecifier` não é
  reconhecida aqui, ver `lower::cpp::base_class_of`).
- `ir::Method.body: Option<Vec<Stmt>>` (era `Vec<Stmt>` não-opcional) —
  `None` para método virtual puro (`= 0`), que vira assinatura sem corpo em
  Dart (`String falar();`), não `{}` vazio (`{}` significaria "não faz
  nada", não "não implementado").
- `ir::Method.is_override: bool`, resolvido por
  `clang_getOverriddenCursors` — não por casar nome de método contra a
  lista de membros da base, que um método não relacionado com o mesmo nome
  enganaria.
- `emit::dart`: `abstract class` (derivado da própria lista de métodos —
  `any(|m| m.body.is_none())` — não um campo separado que pudesse discordar
  dela), `extends BaseName`, `@override`.

## Armadilhas

- **A armadilha documentada — destrutor virtual não tem equivalente em
  Dart — resolvida por omissão deliberada, não por tentativa de
  tradução.** `function_catalog::visit_cursor` já cataloga destrutores
  desde o US-5 (para o grafo de chamadas), mas nunca os despacha para
  `lower::cpp::lower_method`/`Record::methods` — a mesma distinção que já
  existia entre `FunctionDeclarationKind::Method` e `::Destructor` no
  `match` que decide o que vira IR. Nenhum destrutor deste fixture tem
  corpo com lógica de limpeza real (RAII de verdade é explicitamente E12),
  então "não emitir nada" é uma tradução honesta — o GC do Dart já cobre o
  que o destrutor vazio faria.

- **Passar um tipo do usuário por referência (`const Animal&`) reabriu uma
  ambiguidade que a generalização do E05 introduziu sem que nenhum fixture
  até aqui a expusesse.** `lower_type` (desde o E05) desembrulha
  `LValueReference` transparentemente, então `const Animal&` e `Animal`
  por valor resolvem para o mesmíssimo `Type::Record` — o suficiente para
  tipos, mas `collect_params_with_clone_prelude` (a cópia-na-entrada do
  E03) decidia se clonava um parâmetro checando exatamente esse
  `Type::Record` resolvido, não a forma original do parâmetro. Resultado:
  todo parâmetro `const Animal&` ganhava um autoclone bogus
  (`animal = Animal();`, sobrescrevendo com um valor recém-construído,
  não relacionado ao argumento de verdade) — silencioso o bastante para
  `dart analyze` não reclamar (o `Animal()` sintético é um valor
  perfeitamente válido do tipo certo), só errado em tempo de execução.
  Corrigido checando `cx_type.kind` (o tipo cru, antes do desembrulho) em
  vez do `ir::Type` já resolvido — só uma referência genuína evita o
  clone; um `Animal` por valor de verdade (nenhum fixture usa ainda, mas a
  regra continua geral) ainda seria clonado.

- **`operator[]`/`.size()` do E05 tinham forma de AST própria; agora
  conversão-implícita-derivado-para-base também tem a dela.**
  `apresentarAnimal(c)` (passando `Cachorro` onde `Animal` é esperado) é um
  `ImplicitCastExpr` (`DerivedToBase`) — mesmo `CXCursor_UnexposedExpr` que
  já cobre promoção `int`→`double`, mas com `child_ty`/`outer_ty` sendo dois
  `Type::Record` *diferentes*, não cobertos pelo único caso especial
  existente. Sem tratamento, virava "unsupported implicit conversion from
  Record{Cachorro} to Record{Animal}" — o suficiente para derrubar a função
  inteira e, como efeito colateral, fazer `c`/`g` parecerem variáveis não
  utilizadas no Dart gerado (o único uso de `c` desaparecia junto com a
  conversão). Corrigido tratando *qualquer* par `Type::Record`/`Type::Record`
  como sugar transparente — sem verificar se `child_ty` realmente deriva de
  `outer_ty`, porque só o compilador C++ insere esse cast exatamente quando
  essa relação existe; o front-end já validou isso para aceitar o código-fonte
  original, então confiar nessa aceitação é tão seguro quanto reverificá-la
  aqui sem ter o `Module` inteiro em escopo.

- **`return "Au au";` (retorno `std::string` a partir de um literal C)
  passa por um `CXXConstructExpr` para o construtor conversor de
  `basic_string(const char*, allocator)`, não pelo `UnexposedExpr` que já
  cobria `"Ola, " + nome` no E05.** Caía no caminho de "construtor de
  verdade" (E04), tentando montar um `Expr::ConstructorCall` nomeando
  `basic_string` — uma classe que nunca foi `lower_record`'d (E05: `Str`
  não é `Record`, de propósito), virando `basic_string(...)` no Dart
  gerado, que não existe. Corrigido tratando todo construtor de
  `basic_string` com pelo menos 1 argumento como sugar transparente,
  igual ao construtor de cópia/movimentação já tratado desde o E03 —
  recursa direto no primeiro argumento (o conteúdo), ignorando qualquer
  argumento de alocador implícito depois dele. Descoberto que
  `clang_Cursor_getNumArguments` devolve **2**, não 1, para essa chamada
  (o compilador materializa o alocador default como argumento explícito) —
  `eprintln!` temporário, não assumido.

## Decisão de projeto tomada aqui

- **`Animal` (abstrata) ainda ganha um construtor sintético
  (`Animal();`)** pelo mesmo caminho E03 já usa para toda classe sem
  construtor próprio (zero campos → zero parâmetros). Dart permite
  construtor em classe abstrata — é o que as subclasses chamam
  implicitamente via `super()` — então nenhuma regra nova era necessária;
  o caminho existente já produz a forma certa sem saber que a classe é
  abstrata.
- **Nenhuma classe deste fixture declara construtor próprio nem tem
  campos**, de propósito — evita misturar a decisão de "como um construtor
  declarado numa subclasse chama `super(...)`" (não forçada por este
  degrau) com o que o degrau realmente força (abstração, herança,
  `@override`, despacho). Fica em aberto para quem primeiro precisar de
  uma subclasse com construtor e campos próprios.
