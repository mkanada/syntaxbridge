# E13 — Fatia real do Verovio

Décimo terceiro degrau, e o primeiro cujo C++ não foi escrito para o produto:
`input/src/fraction.hpp`/`fraction.cpp` é uma fatia de
`include/vrv/fraction.h`/`src/fraction.cpp` do Verovio 6.2.0 (fixture já
usada por `crates/server/tests/project_ingest.rs` e
`crates/server/tests/fixtures/verovio/verovio-version-6.2.0.tar.gz`), não um
fixture inventado. `input/src/uso.cpp` é a única parte escrita para este
degrau — funções livres `testarX()` que exercitam a classe extraída, no
mesmo papel que os `testarX()` de todo fixture E01–E12.

**Resultado: `esperado-falhar`, e é o resultado certo.** Seis lacunas reais
apareceram, nenhuma delas hipotética — nenhuma tinha sido vista por um
fixture sintético em doze degraus. É exatamente o argumento do próprio plano
(`conversao-guiada-por-exemplos.md` §6): "um degrau de realidade que falha
vale mais do que três degraus sintéticos que passam".

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

## Por que nenhum destes seis foi corrigido aqui

Cada achado é, por si, do tamanho de um degrau próprio — não um ajuste de
fixture. "Não implemente em largura": a extensão pertence a graus futuros
dedicados (inicialização por construtor direto provavelmente generaliza o
mesmo VarDecl parsing usado desde o E01; `static_cast`/atribuição composta
são extensões pontuais e independentes de `lower_expr`/`lower_stmt`;
operador-fora-da-classe e estático-vs-instância-mesmo-nome tocam
`lower_method_call`/`apply_overload_renames`, ambos já delicados o
suficiente para merecer atenção isolada; e a assinatura de `operator==`
é uma decisão de *emissão*, não de *lowering*, que quando resolvida
provavelmente generaliza para qualquer operador binário definido pelo
usuário, não só `==`). Resolver os seis juntos, sem um fixture dedicado a
cada um, é exatamente o tipo de mudança em largura que o método deste
projeto pede para evitar.

## O que isso prova sobre o resto da escada

Nada do que já passa (E01–E12) foi contradito por este degrau — os doze
continuam verdes depois de rodar E13 (nenhuma mudança de código foi feita
para chegar a este resultado, só o fixture novo). O que E13 prova é mais
estreito e mais honesto: dentro do que os doze degraus decidiram cobrir
(aritmética, controle de fluxo, classes com encapsulamento, herança,
sobrecarga, templates de função, biblioteca padrão limitada, multi-TU,
exceções/RAII), a tradução funciona também fora do laboratório — a classe
inteira, exceto pelos seis pontos acima, teria traduzido corretamente. A
armadilha do degrau ("descobrir que valia só no laboratório") não se
confirmou por inteiro; confirmou-se parcialmente, e de forma mensurável:
seis lacunas específicas, cada uma nomeável, nenhuma delas um sinal de que
o que já foi construído esteja errado.
