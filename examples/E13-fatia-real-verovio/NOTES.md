# E13 — Fatia real do Verovio

Décimo terceiro degrau, e o primeiro cujo C++ não foi escrito para o produto:
`input/src/fraction.hpp`/`fraction.cpp` é uma fatia de
`include/vrv/fraction.h`/`src/fraction.cpp` do Verovio 6.2.0 (fixture já
usada por `crates/server/tests/project_ingest.rs` e
`crates/server/tests/fixtures/verovio/verovio-version-6.2.0.tar.gz`), não um
fixture inventado. `input/src/uso.cpp` é a única parte escrita para este
degrau — funções livres `testarX()` que exercitam a classe extraída, no
mesmo papel que os `testarX()` de todo fixture E01–E12.

**Resultado original: `esperado-falhar`, e foi o resultado certo naquele
momento.** Seis lacunas reais apareceram, nenhuma delas hipotética —
nenhuma tinha sido vista por um fixture sintético em doze degraus. É
exatamente o argumento do próprio plano (`conversao-guiada-por-exemplos.md`
§6): "um degrau de realidade que falha vale mais do que três degraus
sintéticos que passam".

**Resultado atual: `passa`.** As seis lacunas foram corrigidas — e, ao
corrigi-las, mais três lacunas que elas mascaravam ficaram visíveis pela
primeira vez e também foram corrigidas. Ver "Resolução" no fim deste
arquivo para os nove achados e a correção de cada um.

## O que foi extraído, e o que foi deliberadamente cortado

`Fraction` (`vrv::Fraction`) representa uma fração `numerador/denominador`
usada internamente pelo Verovio para durações musicais. Mantidos **byte a
byte** do arquivo real: o construtor de dois `int`, `operator+`,
`operator-`, `operator*`, `operator==`, `GetNumerator`/`GetDenominator`
(inline no header, já no arquivo real), `ToDouble`, o `Reduce()` privado
(com `std::gcd`) e o `Reduce(int&, int&)` estático.

Cortados, e por quê:

- **O construtor a partir de `data_DURATION`, e `ToDur()`.** Ambos giram em
  torno de `data_DURATION`, o enum de durações musicais do próprio Verovio —
  um tipo de domínio da aplicação original, não um construto geral de C++.
  Incluí-lo exigiria trazer também `DurationMin`/`DurationMax` e o enum
  inteiro só para dar suporte a métodos que nada testam sobre o produto.
- **O construtor-template com `std::enable_if_t`/SFINAE, e `operator<=>`.**
  Ambos são recusa conhecida desde o E08 (`examples/E08-templates/NOTES.md`,
  "especialização e SFINAE: recusar, não adivinhar") e C++20 puro
  (`std::strong_ordering`) que a v1 do produto nunca alegou cobrir.
- **`operator/` e `operator%`.** Só diferem dos operadores mantidos por
  também chamarem `LogDebug` na guarda de divisão por zero — a mesma chamada
  que o construtor de dois `int` já força a existir; cortá-los evita
  duplicar o mesmo achado sem revelar nada novo.
- **`ToString()`.** É o único método que chama `StringFormat`, o helper
  `printf`-style variádico do Verovio (`vrv.h`) — C++ variádico nunca foi
  exercitado por nenhum degrau, e não é o que este degrau está testando.

`LogDebug` continua declarado com a assinatura real (`void LogDebug(const
char *fmt, ...)`), porque o construtor de dois `int` genuinamente a chama no
arquivo original — só o corpo é um stub (`(void)fmt;`), claramente marcado
no comentário do arquivo como não-extraído, para o arquivo compilar sozinho
sem trazer o resto do subsistema de log do Verovio.

## Os seis achados

1. **Inicialização por construtor direto (`Tipo var(args);`) não é
   reconhecida.** Todo fixture E01–E12 sempre inicializou um objeto por
   cópia (`Ponto p = Ponto(1, 2);`, sempre com `=`); `uso.cpp` escreve
   `Fraction a(1, 2);`, a forma mais comum em C++ real. O `libclang` entrega
   um `VarDecl` com **dois** filhos "em formato de inicializador" (em vez de
   um só, a forma que `lower::cpp` sabe interpretar) — o resultado é
   `Unsupported` para toda variável construída assim, o que por sua vez faz
   com que qualquer coisa computada a partir dela (`soma.GetNumerator()`
   etc.) ainda apareça no Dart gerado, mas partindo de um valor que nunca
   existiu (uma chamada a `_syntaxBridgeUnsupported` que lança em tempo de
   execução) — correto pelo critério de "silêncio proibido" (nada é
   mistraduzido silenciosamente), mas é a lacuna de maior impacto: sem ela,
   nada mais neste fixture chega a rodar.

2. **`static_cast<double>(...)` explícito não é lowered.** `ToDouble()` usa
   `static_cast<double>(m_numerator) / m_denominator` — `Unsupported`
   ("unsupported expression cursor kind 124", `CXCursor_CXXStaticCastExpr`).
   O produto só sabe promover `int` → `double` *implicitamente* (`Expr::
   Convert`, do E02, inserido pelo próprio `libclang` nas conversões
   aritméticas usuais) — nunca um cast explícito escrito pelo usuário, forma
   idiomática e comum em C++ real para deixar uma conversão de tipo
   inequívoca.

3. **Atribuição composta (`/=`, e presumivelmente `+=`/`-=`/`*=`) não é
   lowered.** `Reduce()` privado tem `m_numerator /= gcdVal;` —
   `Unsupported` ("unsupported statement cursor kind 115",
   `CXCursor_CompoundAssignOperator`). Todo fixture E01–E12 sempre escreveu
   `x = x + y`, nunca `x += y`; C++ real usa a forma composta com a mesma
   frequência (ou mais) que a forma expandida.

4. **Chamada de operador definido pelo usuário, de fora da própria classe,
   não é reconhecida.** `testarIgualdade()` escreve `a == b` (duas
   `Fraction`); dentro dos próprios métodos de `Fraction`, `operator+`/`-`/
   `*` nunca precisam chamar o operador de *outro* objeto (`return
   Fraction(num, denom)` é construtor, não `operator==`) — então essa forma
   nunca tinha sido exercitada, apesar de `Fraction::operator==` em si
   traduzir perfeitamente quando *definido* (é um método comum, com corpo,
   sem problema). O buraco é só no *call site*: `lower_method_call` espera
   que o primeiro filho do `CallExpr` seja um `MemberRefExpr` (a forma de
   `obj.metodo(args)`), mas uma chamada de operador (`CXXOperatorCallExpr`,
   que o `libclang` normaliza para o mesmo `CXCursor_CallExpr` de qualquer
   outra chamada) não tem esse filho — confirma, com um caso real, uma
   hipótese que o texto de `lower_method_call` já registrava como possível
   antes deste degrau existir.

5. **Método estático e método de instância com o mesmo nome — válido em
   C++, proibido em Dart.** `Fraction` tem `void Reduce()` (privado, membro)
   e `static void Reduce(int&, int&)` (público) — nomes iguais, resolvidos
   por sobrecarga/assinatura em C++, sem conflito algum. `emit::dart` traduz
   os dois para membros Dart com o mesmo nome — `dart analyze` recusa com
   `conflicting_static_and_instance` ("Class 'Fraction' can't define static
   member 'Reduce' and have instance member 'Fraction.Reduce' with the same
   name"). Nenhum fixture anterior tinha um método estático e um de
   instância compartilhando nome; o mecanismo de renomeação por sobrecarga
   do E07 (`function_catalog::apply_overload_renames`) resolve dois métodos
   de instância com o mesmo nome, mas não foi desenhado para considerar
   `static`/não-`static` como parte do que precisa desambiguação.

6. **A assinatura que o Dart exige para `operator==` não é a que C++
   escreve.** Mesmo quando `Fraction::operator==(const Fraction&)` é lowered
   e emitido sem erro nenhum (achado 4 é só sobre o *call site*; a própria
   *definição* do método sempre funcionou), o Dart gerado
   (`bool operator ==(Fraction other)`) viola o contrato de
   `Object.==` do Dart, que exige `bool operator ==(Object other)` — `dart
   analyze` recusa com `invalid_override`. C++ permite `operator==` com
   qualquer assinatura que o overload resolution aceite; Dart, ao contrário,
   trata `operator==` como uma sobrescrita obrigatória de `Object.==`, com
   assinatura fixa (o corpo então faz seu próprio `is`-check). Nenhum
   fixture anterior definiu `operator==` num tipo do próprio usuário — a
   única sobrecarga de `==` vista até aqui era a de `std::string` (E05,
   ponte hardcoded para `Expr::Binary`, nunca passando pela emissão de
   método de classe).

## O que isso provou sobre o resto da escada, na primeira rodada

Nada do que já passava (E01–E12) foi contradito por este degrau na sua
primeira rodada — os doze continuaram verdes depois de rodar E13 pela
primeira vez (nenhuma mudança de código foi feita para chegar àquele
resultado, só o fixture novo). O que E13 provou naquele momento foi mais
estreito e mais honesto: dentro do que os doze degraus decidiram cobrir
(aritmética, controle de fluxo, classes com encapsulamento, herança,
sobrecarga, templates de função, biblioteca padrão limitada, multi-TU,
exceções/RAII), a tradução funcionava também fora do laboratório — a classe
inteira, exceto pelos seis pontos acima, já traduzia corretamente. A
armadilha do degrau ("descobrir que valia só no laboratório") não se
confirmou por inteiro; confirmou-se parcialmente, e de forma mensurável:
seis lacunas específicas, cada uma nomeável, nenhuma delas um sinal de que
o que já tinha sido construído estivesse errado.

## Resolução

Os seis achados foram corrigidos numa sessão dedicada a fechar E13 (não
"em largura" por acidente — cada um já era, como a seção anterior registra,
do tamanho de um degrau próprio; foram tratados um a um, com os doze
degraus anteriores continuamente verdes a cada passo, exatamente a
disciplina que "não implemente em largura" pede, só que sem um PR por
achado):

1. **Inicialização por construtor direto.** Não era o `VarDecl` reportando
   dois filhos de construtor — era um `NamespaceRef` (o `vrv::` de
   `vrv::Fraction`) contado como candidato a inicializador junto do
   `CallExpr` real. `lower::cpp::lower_decl_stmt` já filtrava `TypeRef`;
   passou a filtrar `NamespaceRef` também, e o único filho restante (o
   `CallExpr`) já caía no caminho existente de um só filho. Achado bem mais
   simples do que a hipótese original ("`libclang` entrega dois filhos em
   formato de inicializador") sugeria — nenhum novo caminho de construção
   foi necessário.
2. **`static_cast` explícito.** `CXXStaticCastExpr`/`CStyleCastExpr`
   entraram no mesmo conjunto de "wrappers transparentes" que já tratava
   conversão implícita (`is_transparent_wrapper`) — a lógica de comparar
   tipo externo/interno e emitir `Expr::Convert` quando promovem `int` para
   `double` já existia; só faltava reconhecer os dois cursor kinds do cast
   explícito.
3. **Atribuição composta.** `CompoundAssignOperator` ganhou
   `lower::cpp::lower_compound_assign_stmt`, que desaçucara para
   `alvo = alvo op valor` (a própria definição de `x op= y` em C++),
   reaproveitando os dois formatos de alvo (`Stmt::Assign`/`FieldAssign`)
   que a atribuição simples já tinha.
4. **Chamada de operador fora da classe.** `a == b` é um
   `CXXOperatorCallExpr` cujo receptor não vem como `MemberRefExpr` — vem
   como o primeiro *argumento* da chamada (`clang_Cursor_getArgument(0)`),
   confirmando a hipótese que `lower_method_call` já registrava.
   `lower_method_call` passou a reconhecer as duas formas. Separadamente,
   `lower::cpp::lower_record_operator_call` passou a traduzir uma chamada
   desse tipo direto para `Expr::Binary` quando o operador está no
   subconjunto que Dart também sobrecarrega (`+`, `-`, `*`, `==`, ...) — sem
   isso, o resultado seria `a.operator+(b)`, sintaxe inválida em Dart
   (confirmado com `dart analyze`: `undefined_getter` em `operator`).
5. **Método estático e de instância com o mesmo nome.** `FunctionDeclaration`
   ganhou `is_static`; `mapping::overload_options_for` passou a checar,
   antes da regra de aridade (que tratava isso, errado, como "parâmetro
   opcional"), se um grupo mistura declarações estáticas e de instância —
   se sim, força renomeação (`"renomear-estatico-instancia"`).
   `function_catalog::apply_overload_renames` renomeia só a(s) declaração(ões)
   estática(s) (`NomeStatic`), já que o esquema de sufixo por tipo de
   `dart_overload_name` não distingue nada quando um dos dois lados não tem
   parâmetro nenhum.
6. **Assinatura de `operator==`.** `emit::dart::emit_method` passou a
   reconhecer `operator==` como caso especial
   (`emit_equality_operator`): parâmetro sempre `Object`, corpo envolto em
   `if (other is NomeDaClasse) { <corpo original> } return false;` — o
   `is`-check promove `other` para o corpo original usar exatamente como
   `lower::cpp` já tinha gerado.

Corrigir os seis revelou **três lacunas adicionais**, nenhuma delas visível
antes porque cada uma dependia de passar por um dos seis primeiros:

7. **Chamada de método estático de fora da classe.**
   `vrv::Fraction::Reduce(a, b)` sempre caiu em
   `lower_method_call`, que exigia um receptor — e um método estático não
   tem um. `lower_call_expr` passou a desviar uma chamada a método estático
   para `lower_static_method_call`, nova função que trata a chamada como a
   de uma função livre (mesma forma de argumentos que `libclang` já usa
   para essa chamada), com o `target` sintético sendo uma `Expr::Ref` cujo
   nome é a própria classe — que `emit::dart` já imprime como
   `NomeDaClasse.metodo(args)`, a sintaxe Dart de chamada estática, de
   graça.
8. **Parâmetros de saída (`int&`).** `Reduce(int &numerador, int
   &denominador)` só apareceu depois do achado 7 estar corrigido — antes,
   a chamada nem chegava a ser emitida. `lower_type` sempre descartou
   referência (correto para `const T&`, usado como otimização de
   passagem), mas fazia o mesmo para uma referência **não-const**, que em
   C++ é o idioma de "parâmetro de saída": a mutação dentro da função nunca
   voltava para quem chamou, silenciosamente. Era exatamente a lacuna que
   `examples/E10-ponteiros-union-out-params/NOTES.md` tinha identificado e
   decidido não construir ("nenhum fixture força essa complexidade
   ainda") — E13 força. Resolvido com uma ponte genuína: `ir::Type::Tuple`/
   `Expr::Tuple`/`Stmt::TupleAssign` (records nativos do Dart 3),
   `lower::cpp::apply_out_param_bridge` reescreve toda função/método `void`
   com parâmetro de referência não-const para devolver uma tupla dos
   valores finais, e o call site (`lower_stmt`) rescreve a chamada como
   `(numerador, denominador) = Fraction.ReduceStatic(numerador,
   denominador);` — a mesma forma de atribuição por padrão que Dart usa
   para desestruturar um record.
9. **`std::gcd`.** Só ficou visível depois do achado 3 (atribuição
   composta) parar de derrubar `Reduce()` inteiro. Dart não tem `gcd` em
   nenhuma biblioteca padrão top-level, mas `int` já tem o método nativo
   (`a.gcd(b)`, confirmado com `dart analyze`/`dart run`) — bastou
   reconhecer `std::gcd(a, b)` (`lower::cpp::lower_stdlib_free_function_call`,
   ao lado de `lower_stdlib_operator_call`) e traduzir para uma chamada de
   método no primeiro argumento, sem precisar de nenhum helper novo no
   pacote gerado.

Um décimo problema — não uma lacuna de tradução, mas do próprio fixture:
`uso.cpp` nunca teve um `uso.hpp` declarando seus `testarX()`, o único
arquivo do corpus sem esse header. Invisível enquanto o oráculo nunca era
alcançado (os nove achados acima bloqueavam antes); `uso.hpp` foi
adicionado seguindo a mesma convenção de todo outro degrau.

## O que isso prova sobre o resto da escada

Os doze degraus anteriores continuam verdes depois de todas as correções
acima — nenhuma delas exigiu tocar um fixture já fechado, só generalizar,
de forma honesta, uma regra que já existia (ou reconhecer uma forma de
cursor que `libclang` já produzia e que este módulo ainda não sabia ler).
A classe `Fraction` inteira, extraída sem modificação do Verovio 6.2.0,
agora traduz para Dart corretamente — golden, `dart analyze`/`format`, e
o oráculo comportamental (C++ real vs. Dart transpilado) concordam em
todos os seis casos de `oracle/cases.json`. A armadilha do degrau
("descobrir que valia só no laboratório") não se confirmou: o que doze
degraus sintéticos construíram generalizou para código real, com nove
extensões pontuais e nomeáveis — nenhuma reescrita, nenhum caso especial
por fixture.
