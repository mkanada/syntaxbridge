# E09 — Herança múltipla

Nono degrau. Primeiro em que uma classe deriva de mais de uma base, e em
que uma classe do próprio C++ vira um `mixin` do Dart, não um `class` —
uma segunda forma de declaração inteiramente nova, não só um novo campo na
IR.

## O que ele forçou a existir

- `ir::Record.mixins: Vec<BaseClass>` — toda base além de um único
  `extends` (E06 continua cobrindo esse caso, populando `base_class`).
  `lower::cpp::base_classes_of` (generalização de `base_class_of` do E06)
  devolve todas as bases; `lower_record` decide qual campo preencher pela
  contagem.
- `emit::dart::emit_module` passa a varrer o `Module` inteiro (todos os
  arquivos, não só o corrente) coletando `usr` de toda base referenciada
  em algum `mixins` — o único jeito de `emit_record` saber, ao processar
  `Voador` isoladamente, que ele vai ser usado como mixin em outro lugar.
- `emit_record` ganha um parâmetro `is_mixin: bool`: muda a palavra-chave
  (`mixin` em vez de `class`), suprime `abstract`, força todo campo a ter
  valor-zero direto na declaração (nunca `campo;` sem inicializador) e
  **suprime completamente a emissão de construtor** — Dart proíbe qualquer
  construtor em `mixin`.

## Armadilhas

- **A armadilha documentada — estado em mixin, ordem de linearização —
  apareceu de duas formas, e as duas já eram resolvidas por decisões de
  degraus anteriores, sem código novo.** "Estado em mixin": `Voador`
  carrega `altitude`, `Nadador` carrega `profundidade`; `PatoDaguaVoador`
  herda os dois campos via `with`, e `pato.altitude`/`pato.subir()` (que
  lê e escreve `altitude`) funcionam sem nenhuma mudança na resolução de
  campo — `dart_member_name`/`member_ref_receiver` (E04) já operam no
  nível do cursor que *declara* o campo, nunca precisando saber em qual
  `Record` ele fisicamente mora no Dart gerado. "Ordem de linearização":
  `Voador` e `Nadador` declaram `mover()` com corpos diferentes;
  `PatoDaguaVoador` declara o seu próprio, que é o que de fato roda —
  tanto em C++ (ocultação de nome comum, nem precisa de `virtual`) quanto
  em Dart (o método da própria classe sempre vence os dos mixins,
  independente da ordem de `with`). O oráculo (`testarMovimento` →
  `"voa e nada"`, nem `"voa"` nem `"nada"`) prova que a fonte certa venceu.
  Se `PatoDaguaVoador` **não** sobrescrevesse `mover()`, `with Voador,
  Nadador` silenciosamente devolveria a versão de `Nadador` (o último da
  lista) — a pegadinha real que o nome do degrau aponta — mas nenhum
  fixture aqui exercita esse caminho, porque não há código de produção
  para gerar a partir dele: sobrescrever explicitamente já é a prática
  correta dos dois lados, e o produto não tem uma resposta melhor que
  "sobrescreva".

- **Um `mixin` do Dart não pode ter construtor nenhum — nem o
  posicional sintético que todo `Record` com campos ganha desde o E03.**
  `Voador`/`Nadador` têm campos (`altitude`, `profundidade`) mas nenhum
  construtor C++ próprio, então caíam automaticamente no caminho
  sintético existente (`Voador(this.altitude);`), que quebra a regra do
  Dart sobre construtor em mixin assim que outra classe tenta `with
  Voador`. Descoberto por raciocínio sobre a regra do Dart antes de rodar
  (não por erro do `dart analyze`) — `is_mixin` desvia tanto a emissão do
  campo (sempre valor-zero direto, nunca "campo sem inicializador,
  construtor inicializa depois") quanto suprime o bloco de construtor
  inteiro.

## Decisão de projeto tomada aqui

- **Toda base de herança múltipla vira mixin — nunca uma `extends` "real"
  entre as várias.** Mesma decisão que `mapping::options_for`'s já
  documenta (`classe-com-mixins`/`mixins-com-sobrescrita-explicita`) — não
  há tentativa de escolher "qual base é a mais importante" para virar
  `extends`; escolher arbitrariamente seria decidir por adivinhação
  exatamente o tipo de coisa que este projeto evita.
- **`mapping::options_for` não é consultado pela geração neste degrau**,
  pela mesma razão do `template_options_for` do E08: o solver já resolve
  isso (inclusive detectando conflito de diamante), mas sempre devolve
  exatamente uma opção — nenhuma decisão de usuário de verdade depende
  dela ainda, e a estratégia de geração (todas as bases viram mixin) não
  muda com o resultado. Fica para quando um fixture realmente expuser uma
  escolha entre alternativas viáveis.
- **`mover()` é declarado sem `virtual`/`override` nas três classes** —
  ocultação de nome comum, não despacho polimórfico. Evita depender de
  `clang_getOverriddenCursors` detectar conflito de diamante (o solver já
  sabe fazer isso, mas nenhum código novo de emissão dependeria desse
  sinal mesmo se ele disparasse — ver acima), mantendo o fixture no
  mecanismo mais simples que já prova a armadilha.
