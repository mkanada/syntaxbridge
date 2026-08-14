# Estilo do código Dart gerado

Plano para um passo **opcional** do usuário: ajustar o estilo do Dart que o
Syntax Bridge gera, sem afetar quem nunca abrir essa tela. Mesma regra que já
vale para US-6 ("US-6 é opcional de ponta a ponta") e para US-7 sem decisão
gravada: ausência de preferência é um estado normal do produto, não um estado
incompleto, e nunca bloqueia a geração.

Este documento não substitui `docs/plans/User Steps.md` — é um complemento,
no mesmo espírito de `conversao-guiada-por-exemplos.md` e
`primeiro-corte-e01-e03.md`: descreve uma fatia vertical de US-8 (Geração do
código Dart), com critérios testáveis e um roteiro para um agente implementar.
Proposta de encaixe no roadmap: **US-8.1 — Preferências de estilo do código
gerado**, sub-passo de US-8 da mesma forma que US-6.1–US-6.5 são sub-passos de
US-6, opcional exatamente como aquele.

## Índice

1. [O que a documentação do Dart realmente permite configurar](#1--o-que-a-documentação-do-dart-realmente-permite-configurar)
2. [Decisão de arquitetura: reusar o tooling oficial, não reimplementar estilo](#2--decisão-de-arquitetura-reusar-o-tooling-oficial-não-reimplementar-estilo)
3. [Eixos de estilo expostos ao usuário](#3--eixos-de-estilo-expostos-ao-usuário)
4. [Onde isso entra no pipeline de `transpile.rs`](#4--onde-isso-entra-no-pipeline-de-transpilers)
5. [Persistência e rotas](#5--persistência-e-rotas)
6. [UI](#6--ui)
7. [Critérios de aceitação (testáveis)](#7--critérios-de-aceitação-testáveis)
8. [Condições de testabilidade](#8--condições-de-testabilidade)
9. [Roteiro de implementação (para um agente)](#9--roteiro-de-implementação-para-um-agente)
10. [Fora de escopo](#10--fora-de-escopo)

---

## 1 — O que a documentação do Dart realmente permite configurar

Lido em `dart.dev/effective-dart`, `dart.dev/effective-dart/style`,
`dart.dev/tools/dart-format`, `dart.dev/tools/dart-fix` e
`dart.dev/tools/linter-rules` (agosto de 2026). Três coisas distintas, com
graus de configurabilidade muito diferentes — confundi-las é o erro mais fácil
de cometer neste passo:

- **O layout que `dart format` produz é essencialmente fixo.** A doc é
  explícita: *"the official whitespace-handling rules for Dart are whatever
  `dart format` produces"*. Não existem "estilos alternativos" de indentação,
  quebra de chaves, espaçamento — é um formatador de opinião única (como
  `gofmt`), de propósito. As **únicas** duas coisas configuráveis do
  formatador em si, via seção `formatter:` de `analysis_options.yaml`
  (linguagem 3.7+; `trailing_commas` exige 3.8+), são:
  - `page_width: N` — largura de linha (padrão 80).
  - `trailing_commas: preserve` — se uma vírgula final foi digitada à mão,
    força a quebra em múltiplas linhas em vez de colapsar; sem essa chave, o
    formatador decide sozinho.
  Chega a existir um comentário de override por arquivo
  (`// dart format width=123` no topo do arquivo), mas não é o mecanismo certo
  aqui — o Syntax Bridge gera o pacote inteiro de uma vez, o lugar certo é o
  `analysis_options.yaml` do pacote gerado, não um comentário por arquivo.
- **Convenções de nomenclatura/aspas/`final`/chaves são *linter*, não
  formatador.** Regras como `prefer_single_quotes`/`prefer_double_quotes`
  (mutuamente exclusivas — a doc chama de *"incompatible rules"*),
  `require_trailing_commas`, `prefer_const_declarations`,
  `prefer_final_locals`, `curly_braces_in_flow_control_structures`,
  `unnecessary_this` só **acusam** o desvio quando habilitadas em
  `analysis_options.yaml`; não reescrevem nada sozinhas.
- **`dart fix --apply` é o que de fato reescreve o código**, e só para
  diagnósticos que têm "quick fix" associado — nem toda regra tem. As citadas
  acima têm ("Fix available"). O fluxo real é: habilitar a regra em
  `analysis_options.yaml` → `dart fix --apply` reescreve o que a regra sabe
  corrigir → `dart format` normaliza o layout por cima.

Conclusão prática: "deixar o usuário ajustar o estilo" não é um recurso do
emissor Dart do Syntax Bridge — é, na maior parte, **gerar o
`analysis_options.yaml` certo e rodar `dart fix --apply` antes do `dart
format`**, que já roda hoje (`transpile::format_dart_source`).

## 2 — Decisão de arquitetura: reusar o tooling oficial, não reimplementar estilo

`crates/server/src/emit/dart.rs` hoje monta o texto Dart na mão
(`push_str`/`format!`), com aspas simples hardcoded (linha 684) e um único
jeito de emitir cada construção. A tentação óbvia é dar a cada uma dessas
decisões um parâmetro (`quote_style`, `brace_style`, ...) e ramificar o
emissor inteiro por preferência.

**Decisão: não fazer isso.** Duas razões, uma técnica e uma de arquitetura:

- **Reinventaria, em Rust, exatamente o que `dart fix`/`dart format` já fazem
  — testados pelo time do Dart, não por nós.** AGENTS.md pede para não
  introduzir dependência externa sem justificar a necessidade; aqui o
  argumento é o oposto — `clang++`/`llvm-cov`/`dart` já são dependências
  aceitas e **já estão no manifesto Flatpak**, e usar mais do que já usamos
  delas custa zero módulo novo.
- **Multiplicaria o espaço de teste do emissor por combinação de
  preferência.** Hoje um teste de `emit::dart` prova uma construção. Se o
  emissor ramificasse por estilo, cada teste precisaria ser multiplicado (ou
  parametrizado) por eixo de estilo, para sempre — o oposto de
  "silêncio é proibido" e de geração determinística testável por comparação
  simples (critério 3 de US-8).

Em vez disso, o pipeline ganha uma etapa de **pós-processamento opcional**,
depois de `emit_module` e antes da validação com `dart analyze` que já existe:

```
IR → emit::dart (inalterado, sempre a mesma saída "canônica")
   → escreve pacote em disco
   → [se houver preferência] escreve/mescla analysis_options.yaml
   → [se houver preferência com regra fixável] dart fix --apply
   → dart format (já existe — passa a ler do disco em vez de stdin, ver §4)
   → dart analyze (já existe)
```

O emissor Rust continua emitindo **uma única forma canônica**, sempre a
mesma independente de preferência — exatamente o que os testes de
`emit_dart.rs` já verificam e continuam verificando sem mudança. A
preferência do usuário é aplicada por ferramenta externa, sobre texto já
válido, nunca dentro da lógica que decide *o que* o código diz.

## 3 — Eixos de estilo expostos ao usuário

Curados por dois critérios: (a) têm efeito visível e (b) ou são configuração
nativa do formatador, ou têm "Fix available" confirmado na documentação do
linter. Não é uma cobertura das ~200 regras do linter — ver §10.

| Eixo | Mecanismo Dart | Valores |
| --- | --- | --- |
| Largura de linha | `analysis_options.yaml`: `formatter.page_width` | inteiro, padrão 80 |
| Vírgula final em listas multi-linha | `formatter.trailing_commas: preserve` + lint `require_trailing_commas` | `auto` (padrão do `dart format`) \| `sempre` |
| Aspas em strings | lint `prefer_single_quotes` **ou** `prefer_double_quotes` (nunca as duas — são incompatíveis) + `dart fix --apply` | `simples` (padrão, já é o que o emissor produz hoje) \| `duplas` |
| `final` em variável local não reatribuída | lint `prefer_final_locals` + `dart fix --apply` | ligado \| desligado (padrão) |
| `const` onde aplicável | lint `prefer_const_declarations` + `dart fix --apply` | ligado \| desligado (padrão) |
| `this.` redundante | lint `unnecessary_this` + `dart fix --apply` | remover (padrão) \| manter |
| Conjunto de lints do pacote exportado | `analysis_options.yaml`: `include:` | nenhum (padrão) \| `package:lints/recommended.yaml` \| `package:flutter_lints/flutter.yaml` |

Sobre o item que **não** vira eixo: `curly_braces_in_flow_control_structures`
("DO use curly braces for all flow control statements") — `emit::dart` já
sempre emite chaves em `if`/`else`/`while`/`for` (não existe caminho de
emissão sem chaves no código atual). Não há o que alternar; citado aqui para
registrar que foi considerado e descartado, não esquecido.

O último item da tabela (conjunto de lints do pacote exportado) é diferente
dos demais: não muda a *forma* do código gerado, muda o que `dart analyze`
vai reclamar para quem herdar o pacote depois. Incluído porque é a mesma
mecânica (`analysis_options.yaml`) e é o que "estilo de código" costuma
significar para quem já usa `package:lints`/`flutter_lints` no resto do time.

## 4 — Onde isso entra no pipeline de `transpile.rs`

Mudança de arquitetura que este passo força, registrada explicitamente porque
não é óbvia lendo só a tabela acima: **`format_dart_source` hoje formata via
stdin/stdout** (`dart format --output=show`, sem tocar disco — ver o comentário
em `transpile.rs` sobre por que isso existe). `dart fix` **não tem modo
stdin/stdout** — só opera sobre arquivos de um diretório real. Assim que
qualquer preferência do usuário ligar uma regra fixável, o pipeline precisa
escrever o pacote em disco *antes* de formatar, não depois.

Duas opções:

1. **Dois caminhos**: sem preferência (ou preferência sem regra fixável),
   mantém o caminho atual (stdin/stdout, sem tocar disco); com preferência
   fixável, um caminho novo via diretório temporário.
2. **Um caminho só**: sempre escreve em diretório real (temporário durante
   teste/rota síncrona, ou o próprio diretório do projeto em produção), roda
   `dart fix --apply` condicionalmente, sempre termina com `dart format` sobre
   os arquivos em disco (lendo o resultado de volta para popular
   `TranspiledPackage.files`, que é o que a API/UI consomem hoje).

**Recomendação: opção 2.** Path único é menos superfície de teste, e o custo
de I/O extra é pequeno comparado ao `dart analyze` que já roda depois — este
plano prioriza "um caminho, sempre testado" sobre "caminho rápido, às vezes
não exercitado". Fica registrado como decisão em aberto para quem implementar
confirmar por medição, não por suposição, se o custo for maior do que parece.

Isso também responde à condição de determinismo (critério 3 de US-8, "gerar
duas vezes produz saída idêntica byte a byte"): `dart fix --apply` é uma
função pura de (código-fonte, regras habilitadas) — determinístico contanto
que o diretório temporário não vaze estado entre chamadas (limpar antes de
cada geração, mesmo padrão que `TempWorkspace` já usa nos testes existentes).

## 5 — Persistência e rotas

Mesmo padrão de `type_mappings` (US-7): dado do usuário, não catálogo
derivado — upsert, nunca apagado por uma reextração de catálogo.

- Tabela `style_preferences` — uma linha por projeto (não precisa de chave
  composta: é configuração de projeto inteiro, não por tipo/arquivo).
  Colunas espelham a tabela do §3: `page_width INTEGER`,
  `trailing_commas_always INTEGER` (booleano), `quote_style TEXT`
  (`'single'`/`'double'`), `prefer_final_locals INTEGER`,
  `prefer_const_declarations INTEGER`, `remove_unnecessary_this INTEGER`,
  `lint_preset TEXT` (`'none'`/`'lints_recommended'`/`'flutter_lints'`). Todas
  com `DEFAULT` igual ao comportamento atual, para que uma linha ausente e
  uma linha "tudo no padrão" sejam observacionalmente idênticas.
- `set_style_preferences`/`get_style_preferences` em
  `persistence/project_store.rs`, mesmo padrão de
  `set_type_mapping`/`list_type_mappings`.
- `GET`/`PUT /projects/style-preferences` — validação na rota, não só na UI:
  uma combinação inválida (`quote_style` fora do enum, `page_width` ≤ 0) é
  rejeitada com erro de cliente, nunca persistida — não existe o par
  `prefer_single_quotes`/`prefer_double_quotes` no modelo porque
  `quote_style` já é um enum de dois valores, não dois booleanos
  independentes (a incompatibilidade das duas regras deixa de ser algo a
  *validar* e vira algo *irrepresentável*, que é preferível).

## 6 — UI

Painel novo, provavelmente uma aba dentro da área de exportação/geração (ou
vizinho ao "Dart Output" existente, `client/flutter/lib/src/ui/
dart_output_view.dart`) — decisão de layout para quem implementar, não deste
plano. Controles diretos por linha da tabela do §3: campo numérico para
largura, toggle para vírgula final sempre, seletor simples/duplas para
aspas, três toggles para final/const/this, dropdown para o preset de lint.
Um botão "restaurar padrão" que apaga a linha de `style_preferences` (não
grava uma linha "tudo no padrão" — mantém a distinção limpa entre "sem
preferência" e "preferência explícita igual ao padrão", que não importa
para o resultado mas importa para depurar).

## 7 — Critérios de aceitação (testáveis)

1. Sem preferência configurada, a saída de `transpile` é **byte a byte
   idêntica** à saída atual (antes deste passo existir) — este passo é
   estritamente aditivo.
2. Escolher aspas duplas produz um pacote em que os literais de string do
   `Dart` gerado usam `"`, e o pacote continua passando em `dart analyze`.
3. Escolher `page_width` diferente de 80 muda de fato o ponto de quebra do
   `dart format` — provado com um fixture cuja única linha longa quebra em
   80 e não quebra (ou vice-versa) na largura escolhida.
4. Preferências persistem entre reaberturas do projeto (mesmo teste inline
   que `reopening_the_project_preserves_the_recorded_type_mapping` já faz
   para US-7).
5. Gerar duas vezes com a mesma preferência produz saída idêntica byte a
   byte (critério 3 de US-8 preservado com preferência ativa, não só sem
   ela).
6. Uma preferência inválida (largura ≤ 0, valor de enum desconhecido) é
   rejeitada pela rota com erro de cliente, sem gravar no banco.
7. Nenhuma combinação de preferências suportada produz Dart que falhe em
   `dart analyze` — testado com a combinação "todas ligadas ao mesmo tempo"
   além dos testes individuais.

## 8 — Condições de testabilidade

- Precisa de um fixture com pelo menos: uma `String` literal (para o
  critério 2), uma linha cujo comprimento é sensível à largura de página
  escolhida (para o critério 3), uma variável local não reatribuída elegível
  a `final` e um `this.` redundante em método de classe (para exercitar as
  regras fixáveis restantes) — provavelmente um novo exemplo dedicado em vez
  de reaproveitar um de `examples/`, para não acoplar este passo ao avanço da
  escada principal.
- O teste do critério 1 (byte a byte igual ao comportamento atual) só é
  honesto se rodar contra um exemplo que **já existe** hoje (ex.: E01) —
  gravar o golden atual antes de tocar no pipeline, comparar depois.
- `dart fix` precisa do Dart SDK, já no manifesto Flatpak (`dart-sdk`,
  3.12.2) — nenhuma ferramenta nova, mesma cobertura de ambiente que já vale
  para `dart format`/`dart analyze` hoje.

## 9 — Roteiro de implementação (para um agente)

1. **Modelo de preferências**, `crates/server/src/style.rs`:
   `StylePreferences` com um campo por linha da tabela do §3, `Default`
   igual ao comportamento atual de hoje (sem essa struct existir). Função
   pura `render_analysis_options(&StylePreferences) -> String`, testável sem
   tocar disco nem `dart`.
2. **Persistência**: tabela `style_preferences`,
   `set_style_preferences`/`get_style_preferences` em `project_store.rs`,
   teste de round-trip e de "reabrir preserva", mesmo padrão de
   `type_mappings`.
3. **Rotas**: `GET`/`PUT /projects/style-preferences`, com a validação do
   critério 6, testadas sem `libclang`/`dart` (popula banco direto, mesmo
   padrão de `function_catalog_route.rs`).
4. **Pipeline de `transpile.rs`** (a parte cara, critério 1 é o que prova
   que não regrediu):
   1. `emit_package` passa a sempre escrever em diretório real antes de
      formatar (opção 2 do §4) — critério 1 e 5 são os testes que provam
      que essa mudança de tubulação não muda o resultado observável quando
      não há preferência.
   2. Se houver `StylePreferences` não-vazia, escrever
      `analysis_options.yaml` no pacote (mesclando com um já existente
      gerado pelo próprio Syntax Bridge, se houver — não pisar em edição
      manual do usuário no pacote exportado sem ao menos avisar; decisão de
      merge fica para quem implementar, mas "sobrescrever silenciosamente"
      está fora de cogitação, mesma régua de "silêncio é proibido").
   3. Se houver regra fixável ligada, rodar `dart fix --apply` sobre o
      diretório antes do `dart format` já existente.
   4. `dart format`/`dart analyze` continuam exatamente como hoje, agora
      operando sobre arquivos que já passaram por `dart fix` quando
      aplicável.
5. **Testes de estilo**: um teste por linha da tabela do §3 (critérios 2 e
   3), mais o teste de "tudo ligado ao mesmo tempo" (critério 7) e o de
   equivalência byte a byte sem preferência (critério 1).
6. **UI**: painel de preferências (§6), consumindo as rotas de 3.

## 10 — Fora de escopo

- **Não cobrir as ~200 regras do linter do Dart.** Só as citadas no §3, que
  têm efeito visível e fix automático confirmado. Regras de "design" (nomes
  de API, forma de construtor, organização de biblioteca) não têm fix
  automático porque são decisões de projeto, não de forma — expor um toggle
  para elas exigiria mudar o que o emissor decide gerar, não só como
  formatar, e isso é trabalho de outro passo (se algum dia fizer sentido),
  não deste.
- **Não inventar um "segundo estilo de formatação".** `dart format` não tem
  configuração de estilo de indentação/quebra de bloco além de largura e
  vírgula — não há nada a expor aqui além do que o §1 já levantou.
- **Não aplicar preferências ao próprio código-fonte do Syntax Bridge**
  (client Flutter, servidor Rust) — este passo é sobre o Dart **gerado para
  o usuário final**, não sobre o produto em si.
