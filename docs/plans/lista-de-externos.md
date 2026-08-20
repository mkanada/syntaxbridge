# Lista de "extern" — código que o usuário decide não transpilar

Este documento registra um recurso negociado fora do roteiro original de
`docs/plans/User Steps.md` — não é um US-N novo porque não veio do roadmap,
veio de uma decisão de produto direta do usuário. Segue a mesma disciplina de
registrar decisão + justificativa que o resto de `docs/plans/` usa, para que
uma sessão futura não precise reconstruir o raciocínio.

## Problema

O produto só tinha um caminho para código que não sabe traduzir:
`Type::Unsupported`/`Stmt::Unsupported`/`Expr::Unsupported`
(`crates/server/src/emit/dart.rs`), renderizado como
`// TODO(syntax-bridge): ...` + `throw UnimplementedError(...)`. Isso é
correto para "o produto não sabe converter isto" (a regra "silêncio é
proibido" do `AGENTS.md`), mas não serve para "o usuário *decidiu* que isto
não deve ser transpilado" — código de terceiros vendorizado dentro do
projeto (`humlib`/`miniz`/`pugixml`, ver `docs/plans/diagnostico-verovio-6.2.0.md`),
uma função que vai continuar nativa, uma API sem equivalente que o usuário
prefere mockar em vez de esperar o produto resolver.

Pior: hoje esse segundo caso nem é honesto. Uma chamada para um símbolo
declarado mas nunca definido em nenhuma unidade de compilação do projeto
emite uma chamada Dart normal, sem import da função (porque nenhum
`ir::Function` nunca chegou a existir para ela — `function_catalog.rs`'s
`visit_cursor` só cataloga cursores que são definição), sem erro visível —
quebra silenciosamente só quando `dart analyze` roda sobre o pacote inteiro.

## Decisões (negociadas em duas rodadas de perguntas)

1. **Mock = valor plausível, execução segue.** Quando código transpilado
   chama algo marcado como externo, o resultado é um valor default coerente
   com o tipo — nunca `throw`, nunca trava. Rejeita explicitamente o padrão
   `Unsupported` (que é para "o produto não sabe", não para "o usuário
   decidiu que não quer").
2. **Cinco fontes alimentam um único conjunto de "externo", nunca
   persistido como lista materializada — sempre recomputado:**
   - Marcação manual em Tipos e em Funções (mesma mecânica nas duas: marca
     por usr).
   - Marca de arquivo inteiro, em Source Files — mecânica própria desde a
     revisão de 2026-08-19 (decisão 3 abaixo), não mais uma marcação manual
     por usr expandida.
   - Regexp sobre nome C++ qualificado (`namespace::Classe::metodo`).
   - Regexp sobre caminho de arquivo.
   - Auto-detecção: símbolo chamado mas nunca definido em nenhuma unidade de
     compilação do projeto.
3. **Cascata é foto, não vínculo — só para tipos.** Marcar um tipo inteiro
   expande, no momento da marcação, para os usrs que ele contém *agora* (o
   próprio tipo + todos os seus métodos). Depois da expansão, cada usr é uma
   marca solta — removível individualmente, sem relação retroativa com o
   tipo de origem. Marcar uma função é sempre direto (ela já é atômica, sem
   cascata).

   **Revisto em 2026-08-19** (`docs/prompts/2026-08-19-mudanca-interacao.md`
   item 3) para **arquivo**, por decisão explícita do usuário: desfazer uma
   marcação de arquivo item por item era incômodo demais na prática, e um
   arquivo inteiro (ao contrário de um tipo) já é uma unidade natural do
   produto (aparece como uma linha só em Source Files). Marcar um arquivo
   agora cria um vínculo persistente (`externals::FileMark`, item 1 do mesmo
   prompt) — toda declaração atualmente naquele arquivo, e qualquer uma
   declarada nele depois, é externa enquanto a marca existir; desmarcar o
   arquivo desmarca tudo de uma vez. Tipo continua "foto": a assimetria é
   intencional, não uma inconsistência a corrigir depois.
4. **Regexp edita-se na regexp, não no item — exceto override manual.** Os
   itens que uma regexp casa não são editáveis um a um; para tirar um falso
   positivo, reescreve-se o padrão. A única exceção é uma marcação manual
   individual, que sempre sobrepõe o que qualquer regexp (ou a
   auto-detecção) diria para aquele usr específico, nos dois sentidos
   (forçar externo, ou forçar não-externo).
5. **Auto-detecção é ativa por padrão.** Um símbolo indefinido detectado
   automaticamente já entra mockado, com sua origem visível como "detectado
   automaticamente" na lista — o usuário pode excluí-lo manualmente (regra 4)
   se preferir o comportamento antigo (chamada quebrada) por algum motivo.
6. **Auto-detecção, nesta entrega, cobre só função indefinida — não tipo
   indefinido.** Uma função declarada-mas-nunca-definida ganha uma
   assinatura completa catalogada (retorno/parâmetros), o suficiente para um
   mock fiel. Um *tipo* nunca declarado no projeto (STL sem adaptador,
   classe de terceiros nunca parseada — achado 4 de
   `docs/plans/diagnostico-verovio-6.2.0.md`) exigiria sintetizar uma classe
   vazia do zero, sem nenhuma informação de forma — mecanismo
   qualitativamente diferente, não cabe nesta entrega.

## Fórmula do conjunto efetivo

```
efetivo(usr) =
    ( nome_regexp_casa(usr) OU caminho_regexp_casa(usr) OU marca_de_arquivo_casa(usr)
      OU auto_detectado(usr) OU marca_manual(usr) == true )
    E NÃO marca_manual(usr) == false
```

Quatro coisas são persistidas por projeto: a marca manual (upsert por usr),
as duas listas de padrão regex, e a marca de arquivo (item 3). Tudo mais —
inclusive a auto-detecção — é derivado a cada leitura dos catálogos já
existentes (`type_declarations`, `function_declarations`, `call_edges`).
Mesmo padrão que `type_mappings` (US-7) já usa para "decisão persistida,
efeito computado": `usr` (ou, para a marca de arquivo, o caminho do arquivo)
como chave, upsert em vez de substituição, nunca apagado por inteiro num
re-ingest — só podado quando o próprio usr/arquivo some (mesma limitação
conhecida de `type_mappings` frente a US-12, ainda `planejado`: a poda hoje é
um `DELETE`/remoção silenciosa, não uma "decisão órfã" sinalizada; replicar
esse padrão aqui não é uma regressão nova, é o mesmo gap já existente em
produção).

**Revisto em 2026-08-19** (`docs/prompts/2026-08-19-mudanca-interacao.md`
item 1): as quatro coisas acima não vivem mais em `project.db` (tabelas
`external_marks`/`external_name_regexes`/`external_path_regexes`). Vivem em
`externals.txt`, um arquivo texto dentro do diretório do projeto
(`externals_store::ExternalsStore`), editável fora do syntax-bridge —
diferente de `type_mappings` e do resto do `project.db`, que continuam em
SQLite. Motivo: o usuário quer poder versionar/inspecionar/editar essa lista
por fora, algo que um arquivo SQLite não oferece de forma prática. Um
`project.db` de uma versão anterior é migrado automaticamente na primeira
abertura (`ProjectStore::open`, `SCHEMA_VERSION` 2): as três tabelas são lidas
uma vez, viram o `externals.txt` inicial, e são descartadas.

## Onde isso mora na UI, frente a `docs/plans/ui-lists.md`

Por papel, "extern" é uma lista de **Decisão** (`ui-lists.md`'s família
"o input do usuário é o produto") — as marcações e os padrões regex *são* o
produto dessa tela, exatamente como US-7 (mapeamentos). Pela classificação
documentada, isso a colocaria como documento central, não painel dockável.

Na prática, essa entrega implementa como **painel dockável** (família
Navegador), mesmo lugar de Tipos/Funções/Ponteiros. Motivo: o mecanismo de
documento central (`WorkspaceDocument` selado, item 3 dos "Bloqueios
estruturais" de `ui-lists.md`) nunca foi construído para nada, nem para o
próprio US-7 que o motivou — não há precedente algo funcionando a copiar, e
construí-lo do zero só para esta entrega seria adiantar uma peça de
arquitetura maior que ninguém pediu ainda. Painel dockável é o que já existe,
já funciona, e é onde o usuário já vai procurar uma lista nova (Tipos e
Funções, as duas telas de onde a marcação manual acontece, já vivem lá do
lado). Quando o documento central for construído (para US-7 ou para esta
lista), mover "Extern" para lá é uma migração isolada, não uma reescrita.

## Não-metas desta entrega

- Auto-detecção de **tipo** indefinido (STL sem adaptador, classe de
  terceiros nunca parseada) — decisão 6 acima.
- `WorkspaceDocument`/documento central — decisão de UI acima.
- `CatalogList<T>`/`KindBadge` (`ui-lists.md`, "Peças compartilhadas") —
  continuam propostos, não construídos; esta entrega segue o padrão atual
  (cada view rola sua própria `ListTile`+`Divider`), não introduz a
  abstração compartilhada por conta própria.
- Resolver a poda-silenciosa-de-órfão de US-12 — replicada aqui igual a
  `type_mappings`, não corrigida aqui.
