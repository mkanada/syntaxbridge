# Erros do `dart analyze` na saída do Verovio 6.2.0 — agrupamento, diagnóstico e prompts de correção

Reescrito em 2026-08-21 a partir da versão original deste arquivo (anotação
bruta de tarefa), como prompt autocontido — pensado para uma sessão com um
modelo mais capaz (Opus), dado o volume de dados e o julgamento arquitetural
envolvido nas propostas. O loop de bailouts que existia em
`docs/prompts/2026-08-20-loop-bailout.md` e o backlog em
`docs/plans/bailouts-verovio-6.2.0.md` foram removidos pelo usuário nesta
mesma data; esta tarefa não os retoma nem depende deles — parte do zero,
direto do relatório do `dart analyze`.

## O que é esta tarefa (e o que não é)

Os entregáveis desta sessão são **documentos e prompts, não código
corrigido**:

1. Um documento de diagnóstico com o agrupamento de erros, a descrição de
   cada grupo e as propostas de solução (objetivos 1-3 abaixo).
2. Um arquivo de prompt por proposta de solução, pronto para outra LLM (mais
   simples, com harness genérico) executar em uma sessão separada (objetivo
   4).

Mesmo que, durante a análise, uma causa pareça trivial e isolada, não a
corrija nesta sessão — escreva o prompt de correção como os demais. Isso
mantém o processo revisável antes de tocar em código de produção
(`crates/server/src/**`, `client/**`).

## Entrada

`.diagnosis/verovio-6.2.0.analyze.json` — saída bruta de `dart analyze
--format=json` rodada sobre o pacote Dart inteiro emitido a partir do Verovio
6.2.0 (298 unidades de compilação reais, sem modificação, de
`test-resources/verovio-version-6.2.0.tar.gz`). Gerado por `just
verovio-diagnosis` (recipe do `justfile`, roda
`crates/server/tests/verovio_6_2_0_transpile_diagnosis.rs`, ~5-6 min; precisa
de `just package-build` antes se rodar dentro do Flatpak).

Formato: `{"version": ..., "diagnostics": [...]}`, lista plana (24.791
entradas na rodada de 2026-08-21T00:09) de objetos com:

- `code` — identificador estável da regra (ex.: `undefined_method`). **É o
  "tipo de erro" do objetivo 1** — agrupe por este campo, não por
  `problemMessage` (que varia por ocorrência).
- `severity` (`ERROR`/`WARNING`) e `type`
  (`COMPILE_TIME_ERROR`/`STATIC_WARNING`/`SYNTACTIC_ERROR`).
- `location.file` — **cuidado**: caminho absoluto dentro do diretório
  temporário daquela rodada específica de teste
  (`/tmp/syntax-bridge-verovio-...`), que não existe mais em disco. Use só o
  sufixo a partir de `lib/` (ex.: `lib/abbr.dart`) como identidade do
  arquivo. Uma cópia persistida do pacote emitido nessa mesma rodada está em
  `.diagnosis/dart-package/lib/` — é onde ler o Dart real gerado, para os
  exemplos do objetivo 2.
- `location.range.start/end.line/column`.
- `problemMessage`, `correctionMessage` (sugestão do próprio Dart, pode ser
  `null`), `documentation` (link `dart.dev/diagnostics/<code>`).

Se o arquivo estiver ausente ou desatualizado (compare o timestamp/commit no
topo de `.diagnosis/verovio-6.2.0.md` com `git log -1`), rode `just
verovio-diagnosis` antes de continuar — não analise um snapshot que não
corresponde ao código atual.

Resumo da rodada de referência (reconfira antes de publicar números — não
está congelado): 15.738 erros + 9.053 avisos, 52 `code`s distintos, 289 dos
301 arquivos `.dart` emitidos têm ao menos um diagnóstico. Os 5 `code`s mais
frequentes já dão uma primeira régua de prioridade por volume:
`undefined_method` (8.309), `unnecessary_non_null_assertion` (6.107),
`unused_field` (1.759), `argument_type_not_assignable` (1.569),
`undefined_identifier` (1.223) — juntos, ~76% de todas as ocorrências.

Contexto de arquitetura relevante: `AGENTS.md` — mapeamento de tipos é o
objetivo central do produto; `dynamic`/`Type::Unsupported` nunca é solução
aceitável; suporte a uma linguagem deve ser tratado como plugin/adaptador
quando possível.

## Objetivo 1 — Agrupar por tipo de erro, com a lista de arquivos

Para cada um dos 52 `code`s distintos, produza:

- contagem total de ocorrências;
- lista de arquivos distintos onde ocorre (caminho relativo a partir de
  `lib/`), com a contagem de ocorrências por arquivo dentro do grupo — importa
  para o objetivo 3, porque um `code` concentrado em poucos arquivos costuma
  ter causa raiz mais estreita do que um espalhado por centenas.

Processe programaticamente (script Python/`jq` ad-hoc sobre o JSON,
descartável depois de usar — não é parte do produto final) em vez de ler as
24.791 linhas à mão.

## Objetivo 2 — Descrever cada grupo para um programador júnior

Para cada grupo do objetivo 1, escreva uma descrição que:

- explique em português simples o que o Dart analyzer está reclamando, sem
  jargão de compilador não explicado — `problemMessage`/`documentation` do
  Dart é o ponto de partida, não a resposta pronta: adapte para o vocabulário
  do projeto;
- mostre pelo menos um exemplo real, lido de `.diagnosis/dart-package/lib/<arquivo>`.
  Quando fizer sentido, localize o arquivo/classe C++ de origem
  correspondente extraindo `test-resources/verovio-version-6.2.0.tar.gz` e
  correlacionando por nome de arquivo/classe (o Dart emitido **não** carrega
  comentário de proveniência linha a linha até o C++ — a correspondência é
  por nome, não automática);
- **não proponha a correção aqui** — isso é o objetivo 3. Este passo é só
  entendimento.

## Objetivo 3 — Propor soluções

Para cada grupo (ou família de grupos com a mesma causa raiz — é esperado que
vários `code`s compartilhem uma única causa; por exemplo, é plausível que boa
parte de `undefined_method`/`undefined_getter`/`override_on_non_overriding_member`
venha de uma mesma lacuna estrutural, como uma interface/mixin C++ cujos
métodos não estão sendo materializados na classe Dart), diagnostique a causa
raiz no pipeline de transpilação (`crates/server/src/lower/cpp.rs`,
`crates/server/src/emit/dart.rs`, extração via libclang) e proponha uma
solução. Diga explicitamente em qual destas categorias a proposta se encaixa:

1. **Correção local na fase de lowering/emissão** — o dado já existe no IR,
   só a tradução para Dart está errada.
2. **Mais informação coletada na fase de ingestão** — o dado não está sendo
   extraído do C++ (via libclang) ainda; precisa de um novo campo/consulta.
3. **Uma fase nova, anterior à transpilação, que olha o projeto inteiro** —
   quando a decisão de tradução de um símbolo depende de como ele é usado em
   outros pontos do código-fonte, não só da sua própria declaração. Exemplo
   do usuário: decidir se uma classe C++ vira `mixin` ou `class` em Dart
   exige olhar todos os pontos de herança/instanciação daquela classe no
   projeto inteiro. Ao propor uma fase desse tipo, diga que decisão ela
   resolve, que dado ela produz, e em que ponto do pipeline esse dado passa a
   ser consumido.

Nunca proponha `dynamic` ou deixar `Type::Unsupported`/bailout como resposta
final — mesma régua do `AGENTS.md`. Quando não houver equivalente direto em
Dart, a proposta é uma fronteira/adaptador nomeado e explícito, não um
apagamento do tipo.

Registre os objetivos 1-3 em um documento novo,
`docs/plans/dart-analyze-verovio-6.2.0.md`. Use `docs/plans/diagnostico-verovio-6.2.0.md`
como referência de tom/nível de detalhe (embora ele seja um log de achados já
corrigidos, e este documento novo seja um backlog de achados ainda a
corrigir).

## Objetivo 4 — Um prompt por proposta

Para cada proposta de solução do objetivo 3, escreva um arquivo novo em
`docs/prompts/2026-08-21-<slug-da-proposta>.md`, autocontido — uma LLM mais
simples com harness genérico precisa executá-lo sem esta conversa como
contexto. Cada prompt deve conter:

- a causa raiz e por que ela produz os erros observados (`code`s afetados,
  contagem, exemplo concreto — pode citar o documento do objetivo 3 em vez de
  repetir tudo, mas o prompt precisa ser executável sozinho);
- onde no código mexer (arquivo(s)/função(ões) aproximados, sem prescrever a
  implementação exata — a LLM executora decide o "como" dentro da fronteira
  definida);
- o método: TDD (teste mínimo que reproduz a causa antes da correção,
  seguindo `AGENTS.md`), rodar `just test` (ou `just test-host` se o Flatpak
  não estiver disponível, registrando isso no resumo);
- o critério de sucesso mensurável: quais `code`s de
  `.diagnosis/verovio-6.2.0.analyze.json` devem cair a zero (ou à contagem
  esperada, se a correção só resolve parte dos casos daquele `code`) depois
  de rodar `just verovio-diagnosis` de novo; nenhuma regressão (novo `code`
  surgindo, contagem de outro grupo subindo) sem registrar e justificar;
- quando parar e perguntar: só por decisão de produto (duas soluções
  tecnicamente válidas e mutuamente exclusivas que mudam comportamento
  observável do produto), nunca por dificuldade técnica.

## Ordem sugerida

Priorize propostas por alavancagem, não só pela contagem bruta do `code`
isolado: uma causa raiz que explica vários `code`s ao mesmo tempo vale mais
que uma que resolve um `code` menor e isolado. Reflita isso na ordem/nome dos
arquivos de prompt do objetivo 4, para que a ordem de execução fique óbvia
para quem for rodá-los depois.

## Método

Siga `AGENTS.md` (TDD, fronteiras claras entre análise de entrada, geração de
saída e validação; suporte a linguagem como plugin/adaptador quando fizer
sentido). Esta sessão em si não corrige código nem roda `just test` para
produção — só produz o documento dos objetivos 1-3 e os N prompts do
objetivo 4. Registre no resumo final: quantos grupos foram tratados, quantas
propostas viraram prompt, e o que ficou pendente se a sessão não couber em
uma rodada.
