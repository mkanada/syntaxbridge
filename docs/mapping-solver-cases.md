# Casos de teste do solver de mapeamento de tipos (US-7)

Este documento descreve o corpus de projetos C++ em `mapping-solver-fixtures/`,
na raiz do repositório. Cada um existe para exercitar o solver de US-7 em
`crates/server/src/mapping.rs` — a peça que decide, para cada tipo/sobrecarga
C++, quais mapeamentos para Dart são oferecidos ao usuário e quais são
recusados por inviabilizarem outro tipo do projeto (Q9 em
`docs/plans/User Steps.md`).

**Status (2026-08-13): implementado e testado contra os 22 casos.** Cada caso
tem um teste próprio em `crates/server/tests/mapping_solver_cases.rs`, rodando
sobre catálogos extraídos de verdade (`type_catalog`/`function_catalog` via
`libclang`), não `TypeDeclaration`s escritos à mão. Cinco pontos de entrada,
não um único `options_for`, porque nem toda decisão de US-7 é sobre um
`TypeDeclaration`:

- `mapping::options_for` — decisões por tipo (`struct`/`class`/`union`):
  herança múltipla (com e sem conflito), Regra dos Três/RAII, interface vs.
  mixin, compilação condicional, e o caso B01 (restrição descoberta por
  referência não-const em outro arquivo).
- `mapping::overload_options_for` — decisões por grupo de sobrecarga (mesmo
  nome, mesma classe ou função livre): aridade, tipo, `const`/não-`const`,
  operadores, com propagação de consequência para call sites em outros
  arquivos.
- `mapping::template_options_for` — monomorfização local vs. decisão global,
  pelo número de arquivos de onde vêm as instanciações visíveis.
- `mapping::signature_options_for` — construções sem tipo de projeto próprio
  (ponteiro, inteiro de largura fixa, `float`, `setjmp`/`goto`,
  `std::thread`/`std::mutex`), detectadas na assinatura ou no texto fonte da
  função.
- `mapping::string_usage_conflict` — varredura de projeto inteiro para o caso
  B05 (`std::string` como texto em um lugar, binário em outro).

Não é o solver de satisfação de restrições completo que o roteiro de US-7
chama de "o item mais caro" — é um primeiro corte por regras sobre fatos já
extraídos, mais varredura textual para o que nenhum catálogo expõe hoje
(ponteiros, `#ifdef`, `std::thread`/`std::mutex`, `setjmp`/`goto`). Limitações
conhecidas, deixadas explícitas no código e nos testes em vez de escondidas:

- **B06** (herança virtual): a regra reconhece corretamente que
  `Anfibio` precisa de composição via mixins (a resposta certa em alto
  nível), mas `type_catalog` não registra se uma base é `virtual` — a razão
  mais profunda do caso (um único subobjeto `Motor` compartilhado) não é
  modelada ainda.
- **C03** (compilação condicional): por definição, não é *detectável* por uma
  regra melhor — uma única passada de `libclang` só enxerga o ramo compilado.
  A regra existe para reconhecer o padrão (arquivo com `#ifdef`/`#ifndef`) e
  devolver uma decisão de produto em vez de fingir que resolveu, não para
  "ver" o ramo ausente.
- Toda detecção via varredura textual (ponteiro, `setjmp`, `goto`,
  thread/mutex, `#ifdef`) é heurística sobre texto, não sobre AST — está
  documentada como tal em `mapping.rs`.

Consertar essas limitações, e crescer daqui para um solver de satisfação de
restrições de verdade (Q9 completo), é trabalho do E09 em diante — este
corpus continua sendo o que dirige esse trabalho, agora com uma primeira
implementação para comparar contra, não mais um espaço de problema vazio.

## Por que isto não vive em `examples/`

`examples/` é a escada de `docs/plans/conversao-guiada-por-exemplos.md`: cada
subdiretório é varrido por `crates/server/tests/conversion_examples.rs`, que
exige `example.toml`, roda o pipeline inteiro (`cmake` → transpilar → `dart
analyze` → oráculo comportamental) e compara com um golden em `expected/`.
Isso é certo para provar que uma fatia *do produto inteiro* funciona — mas é
prematuro aqui: nada neste corpus tem golden ou oráculo, porque o que ele
testa é uma decisão (quais opções `options_for`/`feasible_options` devolvem,
e com quais consequências), não um resultado de código gerado. Forçar esses
casos por `conversion_examples.rs` quebraria a suíte hoje (todo subdiretório
sem `example.toml` é erro de descoberta) e teria que fingir golden/oráculo
para construções que a IR ainda nem representa.

Por isso o corpus mora em `mapping-solver-fixtures/`, um diretório irmão de
`examples/`, fora do alcance desse harness. Cada caso é um projeto C++ real —
compila com `cmake` + `clang++` (checado com `-Wall -Wextra`, zero avisos, ao
escrever este documento) — para que quem implementar o solver possa apontar
`ingest`/`type_catalog`/`function_catalog` direto para `case/input` (aqui,
cada caso não usa `input/`; a raiz do próprio diretório do caso já é o
projeto) e obter um catálogo de tipos de verdade para alimentar
`options_for`/`feasible_options`, em vez de construir `TypeDeclaration` à mão
como os testes atuais de `mapping.rs` fazem.

## As três categorias

O pedido que originou este corpus foi por três formatos de caso, e a divisão
abaixo é literal:

- **A — local.** A resposta certa está inteira em um arquivo (ou par
  header/`.cpp`) autocontido. Um agente ou pessoa lendo só aquele arquivo
  já vê por que a decisão de mapeamento é o que é.
- **B — global.** A resposta certa só aparece combinando peças de arquivos
  diferentes do mesmo projeto. Ler qualquer arquivo isolado sugere (ou não
  contradiz) uma opção que outro arquivo, em outro lugar do projeto, torna
  inviável — exatamente o problema de satisfação de restrições que Q9
  decidiu resolver de fato, não só alertar depois (critério 3 de US-7: "uma
  opção que tornaria outro tipo não convertível não é oferecida").
- **C — código ponte obrigatório.** Não existe opção de mapeamento de tipo
  que resolva o caso — só existe conversão possível se o produto gerar
  código intermediário (adaptador manual, `dart:ffi`, ou reescrita da forma
  do código). É o papel que Q9 atribuiu ao código ponte: garantir que a
  lista de opções nunca fique vazia (critério 5 de US-7).

As categorias não são estanques — um caso B quase sempre também precisa de
código ponte uma vez resolvido, e um caso C às vezes só se revela ao combinar
arquivos. A letra em cada ID marca a *lição principal* do caso, não a única.

## Índice

| ID | Nome | Categoria | Item(ns) da checklist de US-7 |
| --- | --- | --- | --- |
| A01 | Herança múltipla sem conflito | Local | 1 (herança múltipla) |
| A02 | Sobrecarga por aridade e por tipo | Local | 6 (sobrecarga) |
| A03 | União simples | Local | 9 (`union`) |
| A04 | Inteiros de largura fixa com overflow | Local | 10 (largura fixa/overflow) |
| A05 | Sobrecarga const vs. não-const | Local | 6, 8 (sobrecarga, const-correctness) |
| A06 | Monomorfização de template local | Local | 5 (templates) |
| A07 | Operador sobrecarregado direto | Local | 7 (sobrecarga de operadores) |
| A08 | `float` vs. `double` | Local | 11 (ponto flutuante) |
| A09 | `std::vector` trivial | Local | 13 (contêineres STL) |
| B01 | Duas classes com restrição cruzada | Global | 3 (semântica de valor/referência) |
| B02 | Diamante com métodos conflitantes | Global | 1 (herança múltipla) |
| B03 | Interface implementada em vários locais | Global | 1 (herança múltipla) |
| B04 | Sobrecarga entre unidades de compilação | Global | 6 (sobrecarga) |
| B05 | Texto em um lugar, binário em outro | Global | 12 (`char*`/`std::string`) |
| B06 | Herança virtual com estado compartilhado | Global | 1, 3 (herança múltipla, semântica de valor/referência) |
| C01 | Aritmética de ponteiros | Ponte | 2 (ponteiros/aritmética de ponteiros) |
| C02 | `setjmp`/`longjmp` | Ponte | 14 (exceções/`goto`/`setjmp`) |
| C03 | Compilação condicional | Ponte | 15 (pré-processador) |
| C04 | Threads e mutex | Ponte | 16 (concorrência) |
| C05 | Semântica de valor com ponteiro próprio | Ponte | 3, 17 (semântica de valor, `dart:ffi`) |
| C06 | RAII sobre recurso externo | Ponte | 4 (RAII/destrutores) |
| C07 | `goto` para limpeza compartilhada | Ponte | 14 (exceções/`goto`/`setjmp`) |

Os itens 1–18 são os da lista "Herança múltipla é um item de uma lista maior"
em `docs/plans/User Steps.md` §US-7. Todos os 18 aparecem em pelo menos um
caso; a tabela acima só nomeia o item central de cada um.

## Categoria A — local

### A01 — Herança múltipla sem conflito

`mapping-solver-fixtures/A01-heranca-multipla-sem-conflito/`

`Pato` herda de `Voador` e `Nadador`, duas interfaces com métodos de nomes
diferentes (`altitudeMaxima`/`profundidadeMaxima`). Não há nada para o solver
resolver: é o caso trivial do critério 1 de US-7 ("uma classe C++ sem herança
múltipla recebe um mapeamento direto... sem apresentar alternativas") —
exceto que aqui a classe *tem* herança múltipla, e mesmo assim só existe uma
composição sensata (`class Pato with Voador, Nadador` ou equivalente). Serve
de controle: se o solver um dia recusar isto ou oferecer alternativas
espúrias, o bug está nele, não no caso.

### A02 — Sobrecarga por aridade e por tipo

`mapping-solver-fixtures/A02-sobrecarga-aridade-e-tipo/`

Duas sobrecargas de `area` (arte por número de parâmetros: `area(lado)` vs.
`area(largura, altura)`) e duas de `paraTexto` (mesma aridade, tipos
diferentes: `int` vs. `double`) no mesmo arquivo. A primeira dupla mapeia
direto para um parâmetro opcional Dart (`area(double largura, [double?
altura])`); a segunda não tem equivalente de despacho por tipo em Dart e
precisa de renomeação (`paraTextoInt`/`paraTextoDouble`). As duas decisões —
"quando basta parâmetro opcional" e "quando é preciso renomear" — são
observáveis lendo só este arquivo, o que faz dele o caso de referência para o
armadilha do E07 antes de qualquer propagação entre arquivos (ver B04).

### A03 — União simples

`mapping-solver-fixtures/A03-uniao-simples/`

`union ValorNumerico { int32_t comoInteiro; float comoPontoFlutuante; }` não
tem equivalente em Dart — os dois campos nunca compartilham memória lá. A
única opção viável é código ponte: uma classe com uma tag (`TagValorNumerico`,
já presente no arquivo) mais um campo por alternativa, ou um wrapper sobre
bytes crus se a sobreposição binária em si for o que importa. Caso simples
porque a tag já existe explícita no C++ original — não precisa ser inferida.

### A04 — Inteiros de largura fixa com overflow

`mapping-solver-fixtures/A04-inteiros-largura-fixa-overflow/`

`checksum` soma em `uint8_t`, com overflow modular (`% 256`) intencional. O
`int` de Dart (64 bits na VM) nunca estoura no mesmo ponto — um mapeamento
ingênuo de tipo (`uint8_t` → `int`) muda o resultado observável da função.
Precisa de mascaramento explícito (`& 0xFF`) emitido junto com a soma, não só
troca de tipo — é uma decisão sobre *como emitir a expressão*, amarrada à
decisão de mapeamento do tipo do parâmetro.

### A05 — Sobrecarga const vs. não-const

`mapping-solver-fixtures/A05-const-e-nao-const-overload/`

`Contador::valor() const` só lê; `Contador::valor()` sem `const` também
avança o contador — duas operações diferentes que só existem como
"sobrecarga" porque C++ despacha por const-ness do objeto receptor. Dart não
tem esse eixo: as duas precisam de nomes distintos (`valorAtual()` e
`proximoValor()`, por exemplo). Item 8 (const-correctness) da checklist,
observável só com o par de assinaturas lado a lado no mesmo arquivo.

### A06 — Monomorfização de template local

`mapping-solver-fixtures/A06-template-monomorfizacao-local/`

`template <typename T> T maior(T a, T b)` instanciado só para `int` e
`double`, ambos no mesmo arquivo. Com duas instanciações concretas e
conhecidas, dá para decidir localmente entre genéricos de Dart (`T
maior<T extends Comparable>(T a, T b)`) e monomorfização (`maiorInt`,
`maiorDouble`) sem precisar varrer o resto do projeto — contraste direto com
B06 (a versão do mesmo problema espalhada entre arquivos).

### A07 — Operador sobrecarregado direto

`mapping-solver-fixtures/A07-operador-sobrecarregado-direto/`

`Vetor2::operator+` e `Vetor2::operator==`, binários, sem estado externo
envolvido, caem direto no subconjunto de operadores que Dart também
sobrecarrega (`operator +`, `operator ==`). Mapeamento óbvio — o "caso fácil"
que justifica por que B07 (cadeia de operadores entre arquivos) é o que
realmente testa o solver.

### A08 — `float` vs. `double`

`mapping-solver-fixtures/A08-float-vs-double-precisao/`

`dividirFloat` (32 bits) e `dividirDouble` (64 bits) fazem a mesma divisão.
Dart só tem um tipo de ponto flutuante (`double`, 64 bits) — mapear `float`
direto para `double` muda o arredondamento observável. O caso é local porque
a comparação (`1.0f / 3.0f` vs. `1.0 / 3.0`) já está lado a lado no mesmo
arquivo.

### A09 — `std::vector` trivial

`mapping-solver-fixtures/A09-vetor-stl-trivial/`

`std::vector<int>` usado só com `push_back`/indexação/tamanho mapeia direto
para `List<int>`, sem decisão nenhuma a fazer. Incluído de propósito como
linha de base "fácil" de contêiner STL, para contrastar com B05 (o caso
difícil de STL/`std::string`, que decide `String` vs. `Uint8List`).

## Categoria B — global

### B01 — Duas classes com restrição cruzada

`mapping-solver-fixtures/B01-duas-classes-com-restricao-cruzada/`

O caso canônico de Q9, e o que a "condição de testabilidade" de US-7 pede
explicitamente ("um par de tipos em que a escolha de um restringe o outro").
`Ponto3D` (`ponto3d.hpp`), lido sozinho, parece um candidato perfeito para
classe imutável Dart (campos `final`, nenhum método que o mute). Só em
`atualizador_de_posicao.hpp`, arquivo separado, `AtualizadorDePosicao::
empurrar(Ponto3D&, double)` escreve em `alvo.x` através de uma referência —
o que obriga `Ponto3D` a permanecer uma classe Dart com campos mutáveis.
`options_for(Ponto3D, catalog, decisions)` só pode excluir corretamente
"classe imutável" da lista se `catalog` incluir `AtualizadorDePosicao`; olhar
só a declaração de `Ponto3D` não basta — é exatamente por isso que a
assinatura de `options_for` já recebe `catalog` inteiro, não só a
declaração (ver o comentário em `mapping.rs`).

### B02 — Diamante com métodos conflitantes

`mapping-solver-fixtures/B02-diamante-com-metodos-conflitantes/`

`BaseA::nome()` e `BaseB::nome()`, cada uma com corpo próprio e diferente,
lidas isoladamente parecem mixins perfeitos. `Combinado : public BaseA,
public BaseB` é obrigado, em C++, a resolver a ambiguidade sobrescrevendo
`nome()`. Em Dart, `with BaseA, BaseB` não gera esse erro de compilação — só
usa o último mixin da lista, silenciosamente — então "os dois como mixins,
sem sobrescrita" *parece* uma opção válida para um solver que não olhe o
conteúdo dos dois mixins ao mesmo tempo, mas devolveria um resultado
diferente do C++ original. A opção correta (mixins mais um `Combinado` que
sobrescreve `nome()` explicitamente, replicando a combinação dos dois corpos)
só é visível comparando os três arquivos.

### B03 — Interface implementada em vários locais

`mapping-solver-fixtures/B03-interface-implementada-em-varios-locais/`

`Desenhavel` declara `desenhar()` puro (candidato a `abstract interface
class`) e `descricaoPadrao()` com corpo próprio, não-puro. `Circulo` e
`Quadrado` (arquivos separados) nunca chamam `descricaoPadrao()` — olhar
qualquer um dos dois sozinho sugere que o corpo default é morto e pode ser
ignorado, e que `Desenhavel` é seguro como interface pura. Só `Triangulo`
(terceiro arquivo) chama `descricaoPadrao()` de verdade, o que torna
"interface pura" inviável para `Desenhavel` como um todo — uma interface
Dart não carrega implementação herdável. A decisão certa (mixin, não
interface) só aparece depois de ler os três implementadores.

### B04 — Sobrecarga entre unidades de compilação

`mapping-solver-fixtures/B04-sobrecarga-entre-unidades-de-compilacao/`

`formatar(int)` e `formatar(double)` são declaradas juntas em
`formatador.hpp`, mas definidas em unidades de compilação diferentes
(`formatador_int.cpp`, `formatador_double.cpp`) e chamadas de um quarto
arquivo (`relatorio.cpp`). É a armadilha do E07 documentada em
`conversao-guiada-por-exemplos.md` ("renomear... obriga a reescrever todos os
call sites"), levada ao limite entre arquivos: a decisão de renomear
`formatar(int)` para `formatarInt` não termina no `.cpp` que a declara —
precisa alcançar `relatorio.cpp`, que nem faz parte da declaração original.
Exercita o grafo de chamadas de US-5 como dado consumido pelo gerador, não só
exibido na UI (ver a armadilha do E07 no plano da escada).

### B05 — Texto em um lugar, binário em outro

`mapping-solver-fixtures/B05-string-texto-em-um-lugar-binario-em-outro/`

`codificarCabecalho` (`saudacao.cpp`) usa `std::string` como texto puro —
concatenação, nunca indexação por byte. `enviarBytes` (`transporte.cpp`, outro
arquivo) trata o mesmo tipo `std::string` como buffer binário opaco
(`.data()`/`.size()` para `memcpy`). Item 12 da checklist de US-7 (`char*` e
`std::string` → `String` ou `Uint8List`) não é uma decisão por tipo
declarado — é por *uso* — e os dois usos vivem em arquivos diferentes.
Contraste com A09 (STL "fácil"): aqui não basta olhar a declaração do tipo,
é preciso olhar todos os call sites do projeto.

### B06 — Herança virtual com estado compartilhado

`mapping-solver-fixtures/B06-heranca-virtual-estado-compartilhado/`

`VeiculoTerrestre` e `Barco` herdam `Motor` por herança virtual
(`: public virtual Motor`); `Anfibio` herda dos dois, e por causa da herança
virtual existe **um único** subobjeto `Motor` compartilhado — `andar()` e
`remar()` giram o mesmo contador. Isso já é visível lendo `anfibio.hpp`
sozinho. O que não é visível ali é que `monitor.hpp` (arquivo separado)
depende dessa identidade compartilhada: `mesmoMotor` compara os dois
subobjetos por endereço e só retorna `true` por causa da deduplicação da
herança virtual. Dart não tem herança virtual nem identidade de subobjeto —
reproduzir "um único `Motor` compartilhado entre duas superclasses" exige
composição explícita (um campo `Motor` só, referenciado pelas duas partes),
e só combinando `anfibio.hpp` com `monitor.hpp` fica claro que a composição
*precisa* preservar identidade, não só equivalência de valor.

## Categoria C — código ponte obrigatório

### C01 — Aritmética de ponteiros

`mapping-solver-fixtures/C01-aritmetica-de-ponteiros/`

`somaJanela` desloca um ponteiro (`dados + inicio`) e itera com
`*(ponteiro + i)`. Não existe opção de mapeamento de tipo aqui — Dart não tem
ponteiro nem aritmética de endereço sobre `List`. Só `dart:ffi`
(`Pointer<Int32>`, `.elementAt`) mantém a conversão possível.

### C02 — `setjmp`/`longjmp`

`mapping-solver-fixtures/C02-setjmp-longjmp/`

Desvio de controle não local, sem pilha de exceção. Dart não tem
equivalente — nem `goto` cruza função. Não é uma decisão de tipo: é uma
decisão sobre a *forma* do código. A armadilha documentada para o E10 em
`conversao-guiada-por-exemplos.md` se aplica ao pé da letra: "talvez a
resposta certa seja recusar" — código ponte aqui pode ser uma reescrita como
máquina de estados, ou uma recusa explícita com motivo, nunca um mapeamento
de tipo.

### C03 — Compilação condicional

`mapping-solver-fixtures/C03-compilacao-condicional/`

`Config` tem dois layouts incompatíveis por trás de `#ifdef
SYNTAX_BRIDGE_PLATAFORMA_A`. `libclang` só enxerga o ramo ativo na unidade de
compilação que foi de fato compilada (este fixture fixa
`SYNTAX_BRIDGE_PLATAFORMA_A` no `CMakeLists.txt` para ser honesto sobre
compilar); o ramo `#else` é texto morto do ponto de vista da análise, mas não
do ponto de vista do produto real, que pode ter que converter para as duas
plataformas. Dart não tem pré-processador — não há mapeamento de tipo que
resolva isto, só uma decisão de produto (gerar as duas variantes atrás de uma
flag, ou perguntar qual configuração converter).

### C04 — Threads e mutex

`mapping-solver-fixtures/C04-threads-e-mutex/`

`ContadorCompartilhado::incrementarEmParalelo` sobe várias `std::thread`
incrementando o mesmo `int` sob `std::mutex`. Isolates de Dart não
compartilham memória — cada um tem heap próprio, comunicação é por
mensagem. Não há opção de mapeamento de tipo que preserve "duas threads
incrementam a mesma variável sob lock"; só código ponte que reestrutura o
algoritmo em torno de troca de mensagens entre isolates (ou documenta
explicitamente que o paralelismo real não é preservado) mantém a conversão
honesta.

### C05 — Semântica de valor com ponteiro próprio

`mapping-solver-fixtures/C05-semantica-de-valor-com-ponteiro-proprio/`

`BufferProprio` segue a Regra dos Três: construtor de cópia e `operator=`
fazem cópia profunda do buffer que possui; o destrutor libera a memória.
`BufferProprio a = b;` produz dois buffers independentes em C++. Em Dart,
atribuição é sempre referência, e não existe construtor de cópia para
interceptar `=` — `var a = b;` faria `a` e `b` apontarem para o mesmo objeto.
Só código ponte (um método `clonar()` explícito, chamado em todo call site do
C++ original que copiava implicitamente) preserva a semântica observável —
`dart:ffi` entra se o buffer também precisar viver fora do heap do Dart.

### C06 — RAII sobre recurso externo

`mapping-solver-fixtures/C06-raii-recurso-externo/`

`ArquivoTexto` fecha um `FILE*` no destrutor, de forma determinística, no
ponto exato em que o objeto sai de escopo. Dart não tem destrutor
determinístico — `Finalizer` roda depois do GC, em tempo não prevísivel,
tarde demais para um descritor de arquivo que outro processo pode precisar.
Código ponte aqui precisa expor `dispose()`/`close()` explícito e reescrever
cada escopo que dependia do RAII para chamá-lo — muda a forma do código do
usuário, não só o tipo do objeto.

### C07 — `goto` para limpeza compartilhada

`mapping-solver-fixtures/C07-goto-limpeza-compartilhada/`

`processarComDoisRecursos` usa `goto limpar_a` a partir de dois pontos de
saída diferentes para alcançar um rótulo de limpeza compartilhado — o
padrão de RAII manual mais comum em C. Dart não tem `goto` entre blocos
assim (o `label: while` de Dart só serve `break`/`continue` de laço, não
saída antecipada de função). A tradução mecânica não existe; só código ponte
que reestrutura isto como `try`/`finally` (ou uma cascata de `if` aninhados)
resolve — mesma lição de C06, formato de controle de fluxo diferente.

## Como usar este corpus

Nenhum destes casos tem `example.toml`, `expected/` ou `oracle/`: eles não
testam geração de Dart, testam **decisão** de mapeamento. Cada um já está
consumido por um teste em `crates/server/tests/mapping_solver_cases.rs`, que:

1. Roda `cmake -S mapping-solver-fixtures/<caso> -B <build>
   -DCMAKE_EXPORT_COMPILE_COMMANDS=ON` para obter um `compile_commands.json`
   de verdade.
2. Extrai o catálogo real com `ingest`/`type_catalog`/`function_catalog`, do
   mesmo jeito que `crates/server/tests/conversion_examples.rs` já faz para os
   exemplos de `examples/`.
3. Alimenta `mapping::options_for`/`overload_options_for`/
   `template_options_for`/`signature_options_for`/`string_usage_conflict` com
   esse catálogo e afirma exatamente o que este documento descreve como a
   decisão certa — quais opções aparecem, quais consequências carregam, e por
   quê.

Rodar só este corpus: `cargo test -p syntax-bridge-server --test
mapping_solver_cases`. Um caso novo (se o checklist de 18 itens ganhar mais
alguma nuance, ou um degrau futuro — E09 em diante — expuser uma restrição que
nenhum destes 22 cobre) segue o mesmo padrão: fixture compilável em
`mapping-solver-fixtures/`, entrada aqui neste documento, teste em
`mapping_solver_cases.rs`.
