# Organização das listas na interface

Este documento é o complemento de interface do roadmap `docs/plans/User Steps.md`.
Enquanto aquele documento diz *o que* o usuário consegue fazer em cada passo,
este diz *onde* cada lista aparece e o que a interface atual precisa mudar para
sustentá-las.

Ele existe porque o roadmap introduz cerca de doze listas novas, e a UI atual
tem um único mecanismo de lista e dois lugares para pô-las. Sem uma decisão de
organização escrita antes, cada passo inventaria seu próprio painel.

Vale a mesma regra de ouro do `AGENTS.md`: os critérios abaixo estão redigidos
como asserções verificáveis, para poderem virar teste antes da implementação.

## Interface hoje

- `ProjectLandingPage` — tela anterior ao workspace, com a lista de projetos
  recentes (US-1).
- `ServerStatusPage` — o workspace: barra de título, `_DockedWorkspace` com
  painéis dockáveis (esquerda/direita/topo/base + barra de painéis fechados) e
  `_WorkspaceCenter` com faixa de aba e visualizador de código.
- `DockablePanel` — moldura de painel com título, ícone, menu de doca e fechar.
- `SourceFilesView` — a única lista do workspace (US-2).
- `SourceFileViewer` — código com numeração de linha, somente leitura.
- `ExecutionLogView` — log de execução, com nível por entrada.

O esqueleto dockável é a peça certa e não precisa ser trocado. O que precisa
mudar são três decisões internas dele, descritas na seção "Bloqueios
estruturais".

## As quatro famílias de lista

A organização é por *papel da lista*, não pelo passo que a criou. Listas do
mesmo papel compartilham lugar, moldura e comportamento.

| Família | Papel | Lugar | Listas do roadmap |
| --- | --- | --- | --- |
| Navegador | escolher uma coisa; o centro reage | painel esquerdo, tabulado | arquivos (US-2), tipos (US-3), funções e macros (US-5), saída Dart (US-8) |
| Inspetor | detalhar o que está selecionado | painel direito, segue a seleção | usos (US-4), chamadores e chamados (US-5), dependências (US-3), traces (US-6) |
| Resultado | saída de uma execução inteira; cada linha navega para a origem | painel inferior, tabulado com o log | diagnósticos (US-9), divergências (US-10), pendências (US-11), diff (US-12) |
| Decisão | o input do usuário é o produto | documento no centro | mapeamentos C++ → Dart (US-7) |

### Por que US-7 não é um painel

US-7 não é uma lista de consulta. Cada linha carrega opções, consequências
propagadas pelo grafo de tipos e uma escolha persistida — e o próprio roadmap
diz que as decisões do usuário são o ativo mais valioso do projeto. Espremer
isso em um painel lateral de 360 px subdimensiona o passo.

Ela vai no centro, como documento, ao lado do código. O navegador esquerdo
recebe apenas a *fila* de decisões, ordenada topologicamente pelo grafo de US-3
— decidir tipos folha antes dos que dependem deles reduz retrabalho, e essa
ordem já existe no dado.

## Bloqueios estruturais

Os três itens abaixo impedem qualquer lista de US-3 em diante. Nenhum deles é
visível ao usuário, e todos ficam mais caros depois que houver seis listas
construídas por cima.

### 1. `DockablePanel` é dono da rolagem

`dockable_panel.dart` envolve o filho em `SingleChildScrollView`. Por isso o
conteúdo vive em altura ilimitada, e `SourceFilesView` precisa de
`shrinkWrap: true` com `NeverScrollableScrollPhysics` — ou seja, **constrói
todas as linhas em todo frame**.

Com os treze arquivos do fixture isso é invisível. Com o catálogo de tipos de um
projeto do porte do Verovio, são milhares de linhas por frame.

A posse da rolagem precisa inverter: o painel dá ao filho uma caixa de altura
limitada, e o filho rola com `ListView.builder` virtualizado.

Critérios de aceitação:

1. Um painel com 5.000 itens constrói apenas as linhas visíveis, e o teste
   assere sobre a contagem de linhas construídas, não sobre tempo.
2. O painel continua funcionando em modo compacto (largura < 980), onde a
   altura disponível não é limitada pelo pai.

### 2. Painéis do mesmo lado dividem espaço igualmente

`_PanelColumn` dá `Expanded` igual a cada painel empilhado. Com dois painéis em
lados opostos funciona. Com quatro navegadores à esquerda, cada um fica com 25%
da altura e nenhum é usável.

O lado esquerdo precisa ser tabulado — um navegador visível por vez — em vez de
empilhado.

Junto disso, `_IdePanel` é um enum com um `_buildPanels()` fixo dentro de um
`State`. Para dois painéis está correto; para dez, não. Deve virar um registro
de descritores (`id`, título, ícone, lado padrão, builder) que a página itera.

O ganho colateral é o que justifica a mudança: com um registro, o conjunto de
painéis abertos e seus lados viram um objeto de layout serializável. Como o
`project.db` já existe, **persistir o layout por projeto** fica quase de graça,
e é o que faz uma IDE dockável parecer real em vez de decorativa.

Critérios de aceitação:

1. Registrar um painel novo não exige tocar em `_DockedWorkspace` nem em
   `_panelsFor`.
2. Com dois ou mais navegadores no mesmo lado, apenas um está visível e há como
   alternar entre eles.
3. Fechar, mover e reabrir painéis, reabrir o projeto, e o layout volta como
   estava.

### 3. O centro não tem modelo de abas nem alvo de linha

Hoje o centro guarda dois campos (`_selectedSourceFile`,
`_selectedSourceContent`) e a faixa de aba é um rótulo, não uma barra de abas.

Mas US-4 exige clicar em um uso e abrir o arquivo *na linha certa* (critério 5),
US-7 quer o editor de mapeamento, US-8 quer o Dart gerado ao lado do C++ de
origem, e US-12 quer um diff. Isso é mais de um documento aberto e mais de um
tipo de documento.

Proposta: um `WorkspaceDocument` selado — `SourceDocument(path, line?)`,
`TypeMappingDocument(typeId)`, `GeneratedDartDocument(path)`,
`DiffDocument(...)` — com o centro guardando a lista de documentos abertos e o
índice ativo. A faixa de aba que já existe passa a ser a barra de abas de fato.

`SourceFileViewer` precisa de `ScrollController`, `initialLine` e realce da
linha alvo. Sem isso o critério 5 de US-4 não é satisfazível. A cor de realce já
existe na paleta (`IdePalette.selection`) e hoje só serve de fundo de botão
desabilitado.

Critérios de aceitação:

1. Abrir dois arquivos deixa duas abas, e alternar entre elas preserva a
   posição de rolagem de cada uma.
2. Abrir um arquivo com linha alvo rola até ela e a realça.
3. Fechar a última aba devolve o centro ao estado sem documento, sem faixa de
   aba (comportamento atual, que deve continuar coberto por teste).

## Peças compartilhadas

### `CatalogList<T>`

Toda lista futura precisa da mesma moldura: título com contagem, busca, filtros
por espécie ou status, ordenação, estado vazio, e linhas no formato
`ícone + primário + secundário + trailing`.

Hoje `SourceFilesView` e `_RecentProjectTile` já divergem no estilo sem motivo.
Um widget único faz US-3 a US-12 herdarem busca, ordenação e estado vazio, e —
no espírito do TDD exigido pelo `AGENTS.md` — faz esse comportamento ser provado
**uma vez** em vez de doze.

Critérios de aceitação:

1. Digitar na busca reduz as linhas exibidas às que casam, e a contagem do
   título reflete o filtro.
2. A ordenação é estável para empates, nos dois sentidos.
3. Uma lista vazia por filtro e uma lista vazia por ausência de dado mostram
   mensagens diferentes.

### `KindBadge`

"Espécie" reaparece em toda parte: `SourceFileKind` (2 valores),
`TypeDeclarationKind` (7), espécies de função em US-5, taxonomia de uso em US-4,
status de decisão em US-7, status de caracterização em US-6.

Um mapa único de espécie para (ícone, cor, rótulo curto) faz o usuário aprender
uma linguagem visual só. Também dá uso às cores da paleta que hoje estão
declaradas e mortas (`blue`, `violet`, `preview`, `dividerHover`).

## Seleção

A seleção hoje é uma `String` de caminho dentro do `State` da página. De US-4 em
diante ela é polimórfica: um arquivo, um tipo, uma função, um uso.

Precisa virar um `WorkspaceSelection` único ao qual o painel inspetor se liga,
para que o inspetor não dependa de qual navegador produziu a seleção. Um
`ValueNotifier` do próprio Flutter basta; não há motivo para dependência nova
(`AGENTS.md`).

A identidade usada na seleção é a mesma identidade estável de tipo discutida em
US-3 (USR do libclang). A UI é mais um consumidor que a exige: a seleção do
navegador precisa sobreviver a reabrir o projeto.

## Consequências para o contrato de API

A forma da lista decide a forma da rota, e vale decidir agora porque o navegador
de tipos (US-3) e o de funções (US-5) são o **mesmo** navegador.

- Uma linha de navegador é `(id, nome, espécie, contagem_de_usos)`.
- A contagem vem agregada do servidor. US-4 exige ordenar por ela, e o cliente
  não pode ordenar o que não recebe.
- Portanto `sort`, `filter` e `offset`/`limit` como parâmetros de consulta, em
  uma forma única reaproveitada por `GET /projects/types` e
  `GET /projects/functions`.

Isso ataca diretamente a observação transversal "contrato cliente/servidor" do
roadmap: uma forma de lista, um modelo dos dois lados, em vez de dois modelos
escritos à mão em linguagens diferentes.

## Mapa passo a passo

| Passo | Lista | Família | Lugar |
| --- | --- | --- | --- |
| US-2 | arquivos fonte | navegador | painel esquerdo (existe) |
| US-3 | catálogo de tipos | navegador | painel esquerdo, aba irmã do explorer |
| US-3 | dependências do tipo | inspetor | painel direito, seção do selecionado |
| US-4 | usos do tipo | inspetor | painel direito; contagem no trailing da linha do navegador |
| US-5 | funções, métodos, macros | navegador | painel esquerdo |
| US-5 | chamadores e chamados | inspetor | painel direito |
| US-6 | caracterização por função | inspetor | painel direito; relatório da execução no painel inferior |
| US-7 | mapeamentos e opções | decisão | documento no centro; fila topológica no navegador |
| US-8 | arquivos Dart gerados | navegador | painel esquerdo; documento no centro, ao lado da origem C++ |
| US-9 | diagnósticos do analisador | resultado | painel inferior; linha navega para a origem C++ |
| US-10 | divergências e cobertura | resultado | painel inferior |
| US-11 | itens não convertidos | resultado | painel inferior e relatório de exportação |
| US-12 | diff de re-ingestão | resultado | painel inferior; documento de diff no centro |

## Ordem sugerida

1. ✅ Inverter a posse da rolagem em `DockablePanel` —
   `client/flutter/lib/src/ui/dockable_panel.dart`,
   `source_files_view.dart`, `execution_log_view.dart`; teste em
   `test/dockable_panel_test.dart`.
2. ✅ Registro de painéis e lado esquerdo tabulado —
   `panel_descriptor.dart`, `panel_group.dart` (`TabbedPanelGroup`),
   integrados em `server_status_page.dart`; teste em
   `test/panel_group_test.dart`. A persistência do layout no `project.db`
   (critério 3 do bloqueio 2) **não** foi feita — fica para quando um passo
   futuro justificar o custo de schema.
3. ✅ `GET /projects/types` e painel navegador de tipos — fecha o critério 7 de
   US-3. `types_view.dart`, rota em `crates/server/src/server.rs`.
4. 🟡 Rolar-até-a-linha e realce, sem o modelo de abas ainda —
   `source_file_viewer.dart` (`SourceFileViewer` virou `StatefulWidget`, ganhou
   `ScrollController`, `highlightStartLine`/`highlightEndLine`); teste em
   `test/source_file_viewer_test.dart`. Disparado hoje só pelo clique em um
   tipo do navegador Types (`server_status_page.dart`), que abre o arquivo de
   origem com o corpo do tipo destacado — não pelo modelo `WorkspaceDocument`
   descrito abaixo, que continua sem existir: ainda há só um documento aberto
   por vez no centro (`_selectedSourceFile` continua sendo um campo único, não
   uma lista de abas). Falta o próprio modelo de abas para o critério 5 de
   US-4 (clicar em um *uso*, não numa declaração).
5. Painel inspetor ligado à seleção — o hospedeiro de US-4 e US-5.

Os passos 1, 2 e 4 são refatorações com teste próprio e nenhuma mudança de
comportamento visível.

## Fora do escopo deste documento

A partir de US-4 as rotas síncronas deixam de funcionar, e hoje a única
afordância global do workspace é o `Refresh` da barra de título. O
`ExecutionLogView` é o lugar natural para virar histórico de jobs, com entradas
carregando localização de origem e fração de progresso.

Mas isso é a decisão transversal de job, progresso e cancelamento do roadmap —
não uma decisão de interface. Ela não deve ser resolvida por acidente ao
construir a primeira lista.

## Barra lateral de ícones

A `_ActivityRail` foi removida por ser falsa: ícones sem função. Quando houver
seis ou mais painéis, a barra de painéis fechados (`_ClosedPanelsBar`) vira
poluição e uma barra lateral de ícones passa a ter justificativa funcional.

Ela só deve voltar dirigida pelo registro real de painéis descrito acima, nunca
antes — caso contrário é a mesma decoração vazia de novo.
