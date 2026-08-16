# Interface de linha de comando (CLI)

**Status (2026-08-15): v1 implementada ponta a ponta.** Novo crate
`crates/cli` (binário `syntax-bridge`), sem dependências novas além de
`serde`/`serde_json` já vendorizadas — cliente HTTP e parsing de argumentos
escritos à mão, pelo mesmo motivo que os testes do servidor já fazem HTTP
cru (ver "Como a CLI chega ao servidor"). Cobre toda a tabela de comandos
da v1 (`init`, `open`, `projects`/`projects forget`, `files`, `cat`,
`types`/`types usages`, `pointers`, `functions`/`functions callers`
(árvore/`--format dot`)/`functions calls-in-file`, `transpile`), resolução
de projeto por diretório (`project::find_project_dir`, com `--project`/
`--server-url`/`--json` como flags globais), progresso por fase em `init`
(polling de `GET /projects/jobs/{id}`), e uma suíte com 68 testes (unitários
com corpos JSON de fixture + testes de integração do cliente HTTP contra um
`SyntaxBridgeServer` real). As quatro "Decisões em aberto" da proposta
original foram resolvidas ao implementar — ver o fim do documento.

**Achado incidental durante o smoke test manual, corrigido:**
`sb functions callers area` com dois métodos `area` de classes diferentes
(`Forma::area`, `Triangulo::area`) os mostrava como `"area"` idêntico em
ambos, porque `FunctionDeclaration::namespace` é o *namespace* C++
(frequentemente vazio), não a classe dona — essa vem separada em
`owning_class_usr`, como `usr`, não como nome. `commands::functions` agora
busca `GET /projects/types` uma vez por comando e resolve
`owning_class_usr → nome da classe` (`ClassNames`,
`class_names_from_types`) antes de montar o nome qualificado — sem isso, a
funcionalidade-vitrine da proposta (desambiguar `functions callers <name>`)
estava genuinamente quebrada para o caso mais comum (métodos com o mesmo
nome em classes diferentes).

## Índice

- [Contexto](#contexto)
- [Os dois objetivos, e a tensão entre eles](#os-dois-objetivos-e-a-tensão-entre-eles)
- [Modelo de resolução de projeto](#modelo-de-resolução-de-projeto)
- [Como a CLI chega ao servidor](#como-a-cli-chega-ao-servidor)
- [Superfície de comandos (v1)](#superfície-de-comandos-v1)
- [Formato de saída: texto por padrão, `--json` para agente](#formato-de-saída-texto-por-padrão---json-para-agente)
- [Grafo de chamadas em texto](#grafo-de-chamadas-em-texto)
- [Progresso de operações longas](#progresso-de-operações-longas)
- [Onde isso mora no código](#onde-isso-mora-no-código)
- [Fora de escopo desta proposta](#fora-de-escopo-desta-proposta)
- [Decisões em aberto](#decisões-em-aberto)

## Contexto

Discussão em três partes que motivou este documento:

1. O usuário quer uma interface para o Syntax Bridge que (a) possa ser
   chamada do celular e remotamente sem instalar nada, e (b) abra caminho
   para um agente de IA interagir diretamente com o produto — hoje só a UI
   Flutter faz isso, e UI gráfica não é algo que um agente opera.
2. Contraponto levantado pelo próprio usuário: a UI gráfica comporta um
   volume de informação (catálogo de tipos, grafo de chamadas) que texto
   é, em princípio, mais limitado para carregar.
3. Ideia de resolução de projeto no estilo git: chamada de fora de um
   diretório de projeto cria; chamada de dentro consulta/age sobre o
   projeto daquele diretório.

Isso foi verificado contra o código antes de virar proposta, não só
discutido em abstrato. Dois fatos do servidor atual (`crates/server/src/server.rs`,
`crates/server/src/project_service.rs`) mudam o que é barato construir:

- **O servidor já é uma API HTTP/JSON** (`axum`, `crates/server/src/server.rs`)
  e a UI Flutter já é só um cliente dela
  (`client/flutter/lib/src/server/http_server_client.dart`, apontando para
  `http://127.0.0.1:37651`). Uma CLI não duplicaria lógica — seria mais um
  cliente fino sobre a mesma API.
- **Cada rota é stateless e recebe `project_dir` explícito** — não existe
  noção de "projeto atual" no servidor; `is_openable_project` só confere se
  `project_dir/project.db` existe, e toda função de `project_service`
  (`list_types`, `list_functions`, `list_pointers`, `transpile_project`,
  etc.) abre esse arquivo do zero a cada chamada. Isso significa que a
  resolução "de dentro/de fora de um diretório" é **inteiramente
  responsabilidade da CLI** — o servidor não precisa de nenhuma mudança
  para suportar isso.

Existe também um registro global de projetos recentes, independente de
diretório (`GET/POST/DELETE /projects`, `RecentProject` em
`project_service.rs`) — é o que a UI usa para listar projetos já
conhecidos. Isso importa porque um projeto criado pela CLI deve aparecer
nessa lista, e um projeto criado pela UI deve ser encontrável pela CLI.

## Os dois objetivos, e a tensão entre eles

**Objetivo 1 — acesso remoto/celular sem instalar nada.** Isolado, o mais
simples é a própria API HTTP existente, acessada via um cliente qualquer
(terminal SSH num app de celular, ou até um front-end web leve). A CLI por
si só não resolve "sem instalar" — ela ainda é um binário. O que resolve é
o servidor já estar rodando num host acessível (ex.: a máquina de
desenvolvimento, sempre ligada) e o acesso ao celular ser via algo que já
existe lá (Termius/Blink por SSH, por exemplo).

**Objetivo 2 — interação direta de agente de IA.** Aqui a CLI (ou a API
JSON por trás dela) é diretamente o caminho certo — é a superfície que um
agente consegue chamar de forma determinística, sem simular clique em
Flutter.

**A tensão real não é objetivo 1 vs. objetivo 2, é resolução por diretório
vs. acesso remoto.** O modelo "estilo git" (cwd → sobe diretórios → acha
`project.db`) só funciona quando CLI e servidor enxergam o mesmo
filesystem — que é exatamente o caso quando o acesso remoto é via SSH para
a própria máquina onde o servidor roda (o cwd da sessão SSH é real). Se no
futuro a visão for um cliente fino batendo numa API remota que hospeda
vários projetos sem filesystem compartilhado, cwd deixa de fazer sentido e
seria necessário algo como "contexto selecionado" (`kubectl config
use-context`), não coberto aqui — ver [Fora de
escopo](#fora-de-escopo-desta-proposta).

## Modelo de resolução de projeto

Resolução por diretório, decidida inteiramente do lado da CLI:

- A CLI sobe a árvore de diretórios a partir do cwd procurando
  `project.db` (mesmo mecanismo que `is_openable_project` já usa, só que
  caminhando por diretórios pais em vez de checar um único caminho — como
  o git faz com `.git`).
- Se achar, resolve `project_dir` e passa esse caminho absoluto em toda
  chamada à API — os comandos "de dentro" (`types`, `functions`,
  `pointers`, `transpile`, etc.) não pedem esse caminho como argumento.
- Se não achar, a CLI está "fora" de um projeto. **Isso não deveria
  disparar criação automática** — no espírito do git, a maioria dos
  comandos deve simplesmente errar ("nenhum projeto encontrado a partir
  daqui; use `sb init` ou `sb open <path>`"). Só um comando explícito de
  criação (`sb init`) age fora de um projeto por definição.
- Escape hatch explícito para automação: `--project <path>` (equivalente
  ao `git -C <path>`) e/ou uma env var (`SYNTAX_BRIDGE_PROJECT`), porque um
  agente disparado por automação nem sempre tem um cwd confiável dentro do
  projeto.

## Como a CLI chega ao servidor

Ponto que a investigação do código deixou em aberto: hoje não há, em
lugar nenhum verificado, um processo que suba o servidor automaticamente
para a UI Flutter — o cliente Flutter só sabe falar com
`127.0.0.1:37651`, mas quem inicia esse processo (empacotamento Flatpak,
script de desenvolvimento) não foi confirmado nesta investigação.

Duas formas de a CLI funcionar, não mutuamente exclusivas:

1. **Cliente HTTP puro**, batendo em `--server-url` (default
   `http://127.0.0.1:37651`), exigindo um servidor já rodando. Mais
   simples, e é literalmente a mesma coisa que um agente faria batendo na
   API direto, sem CLI nenhuma — o que reforça o objetivo 2.
2. **Auto-start de um servidor local efêmero** quando `--server-url` não
   for passado e nada responder em `127.0.0.1:37651` — no estilo de vários
   CLIs que sobem um daemon sob demanda. Cobre o caso "só quero rodar
   `sb types` numa sessão SSH sem me preocupar em subir servidor à parte".

Como `project_service` reabre `project.db` do zero a cada chamada (não há
cache em memória entre requisições), a CLI também poderia, em tese, linkar
`crates/server` como lib e pular o HTTP inteiramente no caso local — mas
isso duplicaria a superfície (uma forma de chamar local, outra remota) sem
necessidade clara ainda. Recomendação: **sempre HTTP**, com opção 2 (auto-start)
como conveniência, para manter um único caminho de código e uma CLI capaz
de apontar tanto para localhost quanto para um host remoto por SSH sem
distinção de comportamento.

## Superfície de comandos (v1)

Mapeada 1:1 contra as rotas que já existem em `server.rs` — nada aqui
pede rota nova, exceto onde marcado.

**Fora de um projeto (ou em qualquer lugar, via registro global):**

| Comando | Rota |
|---|---|
| `sb projects` | `GET /projects` — lista projetos recentes |
| `sb projects forget <path>` | `DELETE /projects` |
| `sb init <archive> [--name] [--workspace <dir>]` | `POST /projects`, depois poll em `GET /projects/jobs/{id}` até `succeeded`/`failed` |
| `sb open <path>` | `POST /projects/open` — registra/abre um projeto que já existe em outro lugar (ex.: criado pela UI) |
| `sb status <job-id>` | `GET /projects/jobs/{id}`, uma única consulta (sem polling) — para acompanhar de outro terminal/processo uma ingestão que `sb init` já iniciou em outro lugar |

**Dentro de um projeto (path resolvido por cwd, ver acima):**

| Comando | Rota |
|---|---|
| `sb cat <file>` | `GET /projects/source-file` |
| `sb types [--kind K] [--namespace N]` | `GET /projects/types` |
| `sb types usages <type>` | `GET /projects/types/usages` |
| `sb pointers` | `GET /projects/pointers` (já inclui `possible_types` da narrowing) |
| `sb functions [--filter nome]` | `GET /projects/functions` (já inclui `caller_counts`) |
| `sb functions callers <usr\|nome>` | `GET /projects/functions/callers` |
| `sb functions calls --file <path>` | `GET /projects/functions/calls-in-file` |
| `sb transpile` | `POST /projects/transpile` |

Um caso não coberto por rota existente: **listar os arquivos-fonte do
projeto** (US-2 fala em "lista de arquivos" + "leitura de conteúdo", mas só
achei a rota de leitura — `read_source_file_from_http`). A lista de
arquivos parece vir embutida em `CreatedProject.compilation_units` na
resposta de criação/abertura, não como consulta independente. Para `sb
files` funcionar sem precisar rechamar `open` a cada vez, isso é uma
decisão em aberto (ver abaixo).

## Formato de saída: texto por padrão, `--json` para agente

Isso é a resposta direta à preocupação de densidade de informação: o
problema não é texto vs. gráfico, é **dump bruto vs. consulta navegável**.
A API já devolve listas filtráveis (tipos por kind/namespace, funções com
contagem de chamadores) em vez de um blob único — a CLI só precisa não
jogar tudo isso na tela de uma vez:

- Saída padrão: tabular compacta (colunas alinhadas, como `git log
  --oneline` ou `kubectl get`), truncada com paginação (`--limit`,
  `--page`) quando a lista for grande.
- `--json`: devolve o corpo JSON da API sem transformação — é o modo que
  um agente usa, porque não precisa parsear texto tabular.
- Filtros por flag (`--kind`, `--namespace`, `--filter <substring>`) em vez
  de sempre listar tudo, mesmo quando a API já devolve a lista completa —
  a CLI filtra client-side se a rota não tiver query param equivalente.

## Grafo de chamadas em texto

O caso mais forte contra "texto não comporta grafo" é justamente o grafo
de chamadas — e ele tem uma representação textual direta: árvore indentada,
expandida recursivamente com um limite de profundidade.

```
$ sb functions callers Triangulo::area --depth 2
Triangulo::area
├─ Forma::calcularTotal        (main.cpp:42)
│  └─ main                     (main.cpp:88)
└─ Relatorio::resumir           (relatorio.cpp:17)
```

Isso é a mesma expansão recursiva que a view de grafo da UI Flutter faz
visualmente — só serializada como indentação em vez de posição de pixel.
Para os casos em que a forma do grafo importa mais que a leitura linear
(muitos ciclos, muitos nós), `--format dot` exportando Graphviz DOT é
suficiente sem a CLI precisar renderizar nada — um agente pode consumir o
DOT diretamente, e um humano pode `dot -Tsvg` ou colar num visualizador.

## Progresso de operações longas

`sb init` e `sb transpile` cobrem operações que já são assíncronas no
servidor por serem lentas (extração via `libclang`, minutos em projetos
grandes — ver `jobs.rs`/`JobRegistry`/`JobPhase`). A CLI precisa fazer
polling em `GET /projects/jobs/{job_id}` e renderizar uma barra de
progresso por fase, não só "processando..." genérico — a informação de
fase já existe no servidor (`JobPhase`), seria desperdício não expô-la.

`sb init` só cobre acompanhar o progresso de dentro do próprio processo que
disparou a criação — ele bloqueia o terminal até o job terminar. Para
acompanhar de outro lugar (outro terminal, um script separado), `sb init`
agora imprime o `job_id` como a primeira linha de saída (`commands::init::
announce_job_id`), e `sb status <job-id>` (`commands::status`) faz uma
única consulta a `GET /projects/jobs/{id}` — sem loop de polling, no
espírito "one-shot" de `git status` — mostrando a fase atual, os contadores
`completed`/`total` de cada uma das quatro passadas de extração, e uma
fração agregada aproximada (soma dos `completed`/`total` das passadas que
já reportaram um total conhecido — ver `overall_progress`). Estados
terminais (`succeeded`/`cancelled`/`failed`) reaproveitam
`commands::init::render_outcome`, já que o corpo JSON é o mesmo.

## Onde isso mora no código

Proposta: novo crate binário `crates/cli`, membro do workspace, dependendo
apenas de um cliente HTTP (`reqwest` ou similar) contra as mesmas rotas
que `http_server_client.dart` já consome — sem depender de `crates/server`
como lib, para não acoplar a CLI aos internals do servidor além do
contrato HTTP. Os tipos de request/response (`CreateProjectRequest`,
`TypeDeclaration`, `CallEdge`, etc.) já são `Serialize`/`Deserialize` — se
fizer sentido compartilhar essas structs em vez de duplicá-las, seria um
crate `syntax-bridge-wire` extraído de `crates/server`, mas isso só se
justifica se a duplicação começar a doer, não preventivamente.

## Fora de escopo desta proposta

- **Cliente remoto multi-projeto sem filesystem compartilhado** (celular
  batendo numa API que hospeda projetos de várias origens, sem SSH para a
  máquina do servidor). Exigiria um conceito de "contexto selecionado" que
  o servidor não tem hoje — toda rota é stateless por `project_dir`. Não
  construir sem um caso de uso concreto puxando por trás.
- **Expor a porta do servidor na rede.** Hoje é bind local
  (`127.0.0.1:37651`); o acesso remoto do objetivo 1 é satisfeito via
  túnel/SSH para o host, não abrindo a porta. Abrir de fato exigiria
  autenticação, que não está desenhada aqui.
- **Renderização de imagem real do grafo** (SVG/PNG) pela própria CLI —
  a saída `--format dot` é o limite desta proposta; renderizar fica a
  cargo de outra ferramenta.

## Decisões em aberto

Resolvidas ao implementar (2026-08-15):

1. **Como listar arquivos-fonte sem rota dedicada — resolvido: reabrir.**
   `commands::files::request_files` chama `POST /projects/open` (que já
   inclui `source_files`, ver `LoadedProject`) toda vez que `sb files`
   roda — sem re-extração, é uma leitura de `project.db`. Não foi criada
   rota nova. Exibição encurta o caminho para relativo a
   `<project_dir>/input-source` (`display_path`); `sb cat` aceita esse
   mesmo caminho relativo e resolve para o absoluto que a rota exige
   (`resolve_cat_path`), além de aceitar um caminho absoluto direto (o que
   `sb files --json` imprime).
2. **Auto-start do servidor local — resolvido: não, para v1.** A CLI é um
   cliente HTTP puro que exige um servidor já respondendo em
   `--server-url` (padrão `http://127.0.0.1:37651`); erro de conexão vira
   mensagem clara (`http::ClientError::Connect`), não uma tentativa de
   subir processo. Mantém um único caminho de código e não implica decisão
   de lifecycle — fica para quando houver um caso de uso puxando por trás.
3. **Nome do binário e do crate — resolvido: `syntax-bridge`.** Pacote
   `syntax-bridge-cli`, binário `syntax-bridge`, evita a colisão de `sb`
   levantada aqui.
4. **Autenticação — inalterado.** Nenhuma foi adicionada; a CLI herda a
   mesma superfície sem autenticação que a API já tinha (loopback).
   Continua fora de escopo até o cenário de túnel/rede real aparecer.
