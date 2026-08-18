# Inventário e resolução de bailouts — Verovio 6.2.0

## Escopo e evidência

Este inventário vem de `just verovio-diagnosis` em 2026-08-18, sobre as 298
unidades de compilação do Verovio 6.2.0. O diagnóstico percorre o IR antes da
emissão e grava todas as causas individuais em
`.diagnosis/verovio-6.2.0.json.bailouts`; portanto, as tabelas abaixo não
dependem de procurar textos no Dart formatado.

| Origem do bailout | Ocorrências | Causas distintas |
| --- | ---: | ---: |
| Tipo C++ sem mapeamento (`Type::Unsupported`) | 2.034 | 228 spellings |
| Expressão sem lowering (`Expr::Unsupported*`) | 25.312 | 2.015 razões |
| Statement sem lowering (`Stmt::Unsupported`) | 1.065 | 30 razões |
| **Total** | **28.411** | — |

Uma ocorrência é um nó do IR, não uma linha do pacote Dart: um mesmo nó pode
aparecer em mais de um arquivo por inclusão de headers. `SyntaxBridgeOpaque` e
`_syntaxBridgeUnsupported<T>` são a forma explícita como parte desses nós chega
ao Dart; não são uma solução semântica para a categoria que os originou.

As famílias de cada tabela são mutuamente exclusivas no *snapshot-base* abaixo.
As listas exatas de spellings e razões continuam no JSON, para que uma nova
causa não seja escondida pela agregação deste plano.

### Atualização de 2026-08-18 — casos 1 e 2

Esta execução removeu completamente três causas que estavam no snapshot-base:

- `unsupported unary operator kind 10` (`!`): 2.153 → 0;
- `unsupported implicit conversion from Int to Bool`: 567 → 0;
- `standard-library method call's first child was not the expected
  member-reference cursor`: 1.785 → 0.

O terceiro item não declara os métodos resolvidos: ele apenas encontra o
receptor também na forma de chamada de operador (por exemplo, `destino =
origem`). Assim, o diagnóstico passa a apontar a operação real — por exemplo,
`std::basic_string::operator=` (1.142) e `std::vector::operator=` (49) — que
serão tratadas pela tabela de adaptadores. O total de expressões caiu de
27.905 para 26.917 (−988); a diferença não é a soma das três causas porque
um lowering que antes parava cedo passa a expor o próximo nó ainda não
suportado.

### Atualização de 2026-08-18 — atribuição, fluxo, STL e atualizações

Esta rodada acrescentou atribuição tipada de `basic_string`, escrita indexada,
controle de fluxo, adaptadores STL frequentes e `++`/`--`. O snapshot atual
reduziu expressões de 26.917 para 25.312 (−1.605) e statements de 2.214 para
1.065 (−1.149). Tipos subiram de 1.964 para 2.034: ao atravessar trechos que
antes paravam em um bailout de expressão/statement, o diagnóstico agora vê os
tipos ainda sem mapeamento que estavam depois deles. Isso é exposição de
cobertura, não justificativa para aceitar o tipo como opaco.

| Causa no Verovio | Antes | Agora | Decisão implementada |
| --- | ---: | ---: | --- |
| `basic_string::empty` | 379 | 0 | `String.isEmpty` tipado. |
| `basic_string::find` | 426 | 0 | busca por bytes com `utf8.encode(...).indexOf(...)`, preservando o contrato de posição da string C++. |
| `vector::push_back` | 335 | 0 | `List<T>.add`. |
| `vector::at` | 271 | 0 | índice `List<T>[int]` com elemento tipado. |
| pré/pós-incremento e pré/pós-decremento | 970 | 0 | operadores Dart prefixados/pós-fixados, preservando a posição do efeito. |
| `continue` | 469 | 0 | `Stmt::Continue` → `continue;`. |
| `break` | 297 | 0 | `Stmt::Break` → `break;`. |
| `CXXForRangeStmt` comum | 320 | 0 | `for (T item in itens)`, com binding `final` quando a referência C++ é `const`. |
| atribuição indexada na forma simples | 417 | 284 | `Expr::Index` passou a ser alvo atribuível; os 284 restantes chegam por outra forma de AST e continuam rastreados. |
| `basic_string::operator=` | 1.142 | 511 | apenas as formas cujo destino e valor podem ser reatribuídos com segurança como `String`. |
| `basic_string::operator+=` | 366 | 106 | reatribuição `texto = texto + sufixo`; as sobrecargas restantes não foram presumidas equivalentes. |

Dois limites permanecem intencionais. `operator=` genérico ainda tem 1.159
ocorrências (e `vector::operator=` tem 50): em C++ ele pode significar cópia
de valor, enquanto atribuir uma `List` Dart compartilharia a mesma coleção. Um
lowering direto seria semanticamente errado; o próximo adaptador precisa
modelar cópia de coleção/record. Há também 46 range-for com referência mutável:
o item do `for` Dart é um valor local e não escreve de volta na coleção, então
eles ficam como bailout até existir um adaptador de leitura-escrita.

O pacote emitido tem 1/301 arquivos sintaticamente inválido:
`iohumdrum.dart` contém um literal C++ com quebra de linha emitida sem escape.
É o próximo defeito de emissão isolado. O `dart analyze` reportou 11.418 erros
e 5.022 avisos; parte do aumento em relação ao snapshot anterior é uma
consequência observada de agora alcançar mais código. A próxima rodada deve
separar essas causas em adaptadores semânticos, começando pelo literal de
string e por cópia tipada, sem converter nada para `dynamic` ou
`SyntaxBridgeOpaque`.

## 1. Tipos sem mapeamento — snapshot-base de 4.384 ocorrências

### Progresso executado

| Mapeamento | Representação Dart | Efeito verificado |
| --- | --- | --- |
| `std::array`, `deque`, `initializer_list` | `List<T>` | preserva a forma de coleção; o bound de `array` continua disponível no catálogo. |
| `unordered_set` / `unordered_map` | `Set<T>` / `Map<K,V>` | remove o bailout de tipo; diferenças de ordem permanecem responsabilidade das operações. |
| `optional`, `unique_ptr`, `shared_ptr`, `weak_ptr` | `T?` | representa presença/ausência sem alegar preservar ownership. |
| `char_t*` e aliases canônicos de caractere | `String?` | cobre a superfície de texto do PugiXML sem classificar byte buffer como texto. |
| `uint8_t*`, `mz_uint8*` | `Uint8List?` | introduz o tipo IR `Bytes` e a importação `dart:typed_data`. |
| `void*` com parâmetro escalar irmão `*_size`/`*_length` | `Uint8List?` | só classifica buffer quando o nome prova a relação; handles `void*` isolados continuam bailout explícito. |
| `R (*)(Args...)` com assinatura representável | `R Function(Args...)` | closure Dart tipada para callback C/C++; callbacks de ABI continuam fronteira FFI. |
| arrays nativos `T[N]` | `List<T>` | preserva elemento e forma de valor em campos; tamanho continua uma regra de fronteira futura. |
| `std::pair<A, B>` | `SyntaxBridgePair<A, B>` | adaptador compartilhado preserva `first`/`second` e identidade nominal entre arquivos. |
| `std::tuple<T...>` | record Dart `(T, ...)` | preserva slots posicionais na fronteira de tipo; `std::get` ainda precisa de lowering de expressão próprio. |
| aliases dependentes de contêiner, como `size_type` | tipo canônico (`int`, `double`, ...) | descarta `TemplateRef` de navegação do libclang antes de escolher o inicializador da variável. |

Os testes de lowering cobrem cada linha da tabela, inclusive a armadilha de
libclang que expunha o `3` de `std::array<int, 3>` como se fosse argumento
padrão do parâmetro Dart.

| Família | Qtde. | Causa raiz | Direção de solução |
| --- | ---: | --- | --- |
| Ponteiros, `void*` e callbacks | 2.654 | `lower_type` só reduz `T*` a `T?` quando `T` já é record/`String`/coleção. Ponteiros escalares, buffers, callbacks e handles caem no ramo FFI genérico. | Classificar por uso: referência de objeto → `T?`; buffer numérico → `TypedData`/`Span<T>`; ABI/callback → `Pointer`/`NativeFunction` ou `typedef` Dart; `void*` → handle de domínio nomeado. A aritmética de ponteiro continua uma fronteira FFI explícita. |
| Streams, texto e regex C++ | 483 | `basic_istream`, `basic_ostream`, `stringstream`, `regex` e iteradores de regex não têm adaptador no lowering. | `CppInput`/`CppOutput` como fronteira externa, `StringBuffer`/`String` para casos locais, `RegExp` e resultado tipado quando a semântica for equivalente. Não fingir que um stream C++ é uma `String`. |
| Produtos (`pair`/`tuple`) | 313 | Especializações `std::pair` e `std::tuple` chegam como records de template não reconhecidos. | Adaptadores recursivos `Pair<A,B>` e records Dart. Preservar `first`/`second`; tuples de aridade variável precisam de record nomeado ou classe auxiliar. |
| Arrays e buffers fixos | 192 | Arrays C/C++ não têm variante IR; aparecem `Point[4]`, `mz_uint8[ ]`, arrays multidimensionais. | `FixedArray<T,N>`/`List<T>` para valor; `Uint8List`/`Uint16List` etc. para buffer; arrays multidimensionais como wrapper de *stride*. Nunca reduzir um buffer a ponteiro opaco sem preservar tamanho. |
| Coleções, iteradores e adaptadores STL | 147 | Há suporte parcial para `vector`/`list`/`set`/`map`; `deque`, `stack`, `multiset`, iteradores e `initializer_list` ainda são desconhecidos. | Mapear para `List`/`Set`/`Map`, `Queue`, `Iterable`/cursor tipado e `MultiMap`; reter adaptador quando ordem, multiplicidade ou invalidação de iterador for relevante. |
| Escalares e aliases que escapam | 188 | Tipos dependentes/aliases (`size_type`, `value_type`, `int`, `double`, `void`) chegam como `Unexposed`/typedef e não são sempre des-açucarados até o tipo canônico. | Aplicar `clang_getCanonicalType`/des-açucaramento antes do fallback, mantendo a informação de largura/sinal no catálogo. Inteiros → `int`; ponto flutuante → `double`; `void` só é válido como retorno, nunca valor. |
| Lambdas | 93 | O tipo da lambda não tem nome Dart e seu cursor ainda não é convertido em fechamento. | Closure Dart quando captura e assinatura puderem ser lowered; `typedef`/ponte FFI para callback ABI; caso contrário, fronteira externa nomeada com origem e contrato. |
| Unions e tipos anônimos | 57 | União e enum/struct anônimos não têm representação nominal segura; alguns spellings incluem a localização do header. | União de domínio → tipo discriminado/`sealed`; união de layout → `Struct` FFI; tipo anônimo só ganha nome sintetizado estável se todos os campos forem representáveis. |
| Opcional, posse e nulidade | 19 | `optional`, `unique_ptr` e `nullptr_t` ainda não têm lowering recursivo. | `T?` para `optional`; `Owned<T>`/handle com `dispose` para posse; `null` para `nullptr`. Decidir transferência de propriedade antes de emitir. |
| Spelling vazio | 171 | A declaração/type canônico não foi resolvido e o fallback perdeu o nome. | Nunca emitir ponte sem identidade: registrar cursor, USR, tipo canônico e localização. Resolver como alias/externo ou falhar o diagnóstico com causa rastreável. |
| Outros tipos nomeados | 67 | Bibliotecas e aliases específicos (`tm`, `Scale`, `KeyboardMapping`, `basic_fstream` etc.). | Classificar no catálogo como record do projeto, adaptador de biblioteca ou API externa. Cada spelling novo deve ter decisão registrada antes de sair do diagnóstico. |

## 2. Expressões sem lowering — snapshot-base de 27.905 ocorrências

| Família | Qtde. | Causa raiz | Direção de solução |
| --- | ---: | --- | --- |
| Conversões implícitas | 12.689 | O wrapper de conversão só aceita tipos idênticos e `int → double`; C++ usa verdade de inteiro/ponteiro, enums, upcasts e conversões numéricas. | Criar IR de conversão explícita: `int → bool` (`!= 0`), ponteiro → bool (`!= null`), `bool → int` (`? 1 : 0`), enum ↔ valor subjacente, upcast seguro pela hierarquia e conversão numérica validada. Downcast só com cast C++ explícito ou prova de subtipo; nunca inventar `as` para uma conversão insegura. |
| Chamadas STL não mapeadas | 3.582 | O dono da chamada é reconhecido, mas o método não possui regra (`string.find`, `vector.push_back`, `at`, `c_str`, `map`, `list`, streams etc.). | Tabela de adaptadores por coleção/string/stream. Prioridade: `basic_string` e `vector`; depois `list`/`map`/`stack`; operações de byte (`c_str`, `find`, `compare`) precisam preservar a semântica C++ ou ir para ponte externa. |
| Forma de chamada STL inesperada | 1.783 | O lowering exige `MemberRefExpr` como primeiro filho; wrappers, conversões e chamadas dependentes quebram essa suposição. | Normalizar receptor e argumentos antes de despachar o método STL; extrair a chamada de `UnexposedExpr`/casts e só então aplicar a tabela de adaptadores. |
| Operadores unários | 3.744 | Só o menos unário é lowered. O corpus contém `!` (2.153), `++`/`--`, endereço e dereferência. | `!` → `!` após política de verdade; `++`/`--` → atribuição com semântica pré/pós preservada; `&`/`*` somente através da ponte de ponteiros/buffers. |
| Operadores binários | 989 | Faltam `||`, atribuição como expressão, bitwise, deslocamentos e vírgula. | Para inteiros, mapear bitwise/deslocamentos para operadores Dart; `||` direto; reescrever atribuição encadeada em statements/temporários; preservar ordem de avaliação para vírgula. |
| Chamadas de operadores | 1.738 | Sobrecargas como `operator=`, `operator<<` e `operator[]` não passam pelo mapeamento de métodos Dart. | Despachar por semântica: cópia/atribuição, índice, comparação, fluxo de saída. Métodos sem equivalente direto recebem adaptador nomeado, não bailout opaco. |
| Cursor de expressão ainda não tratado | 2.640 | Há cursores C++ com significado conhecido, mas sem variante IR. | Resolver a tabela detalhada abaixo; qualquer cursor novo deve entrar no relatório com seu nome Clang, não apenas o número. |
| Valor padrão de campo | 589 | `default_record_construct` não conhece valor seguro para vários tipos. | Política de inicialização por tipo: construtor obrigatório, `late`, `null` apenas para anuláveis, factory/zero somente quando semanticamente definido. |
| `VarDecl` com forma composta | 410 | O lowering aceita no máximo um inicializador-filho. | Separar declaradores e inicializadores em múltiplos `Stmt::VarDecl`, preservando a ordem e efeitos colaterais. |
| Alvo de chamada não reduzido | 168 | `lower_call_expr` aceita apenas função livre, método e construtor; chamadas via campo/variável/callback/conversion operator ficam de fora. | Resolver um `Callable` unificado: `FunctionDecl`, `CXXMethod`, `Constructor`, `ConversionFunction` e valores chamáveis (campo/parâmetro com tipo de função). |
| Referência de membro com filhos extras | 95 | A forma do AST traz qualificadores/receptores implícitos além dos 0/1 filhos esperados. | Normalizar `MemberRefExpr`, descartando referências de tipo/template e preservando o único receptor de valor. |
| Wrapper/caso residual | 3 | Wrappers sem filho de valor ou alvo de chamada irresolúvel. | Capturar árvore e token fonte no diagnóstico; adicionar normalização específica ou rejeitar como fronteira externa. |

### Cursores de expressão ainda sem variante IR — 2.640 ocorrências

| Cursor Clang | Qtde. | Solução |
| --- | ---: | --- |
| `CXXNewExpr` (134) | 617 | Criar record Dart quando a alocação é de tipo representável; caso dono/ponteiro seja relevante, criar `Owned<T>` ou usar FFI. |
| `CharacterLiteral` (110) | 496 | Lower para `int` code unit; converter para `String` somente em contexto textual explícito. |
| `ConditionalOperator` (116) | 471 | Nova expressão ternária Dart `cond ? a : b`, com unificação de tipos. |
| `ArraySubscriptExpr` (113) | 215 | Reusar `Expr::Index`; a escrita correspondente vai para atribuição indexada. |
| `TypeRef` (43) / `TemplateRef` (45) | 363 | Não são valores: removê-los da normalização de filhos de expressões/chamadas. |
| `UnaryExpr` (136) | 178 | Distinguir `sizeof`/`alignof`/extensões; mapear apenas quando o tamanho Dart/FFI estiver definido. |
| `CXXDynamicCastExpr` (125) | 169 | Cast verificado: `is` + `as T`/`T?`, condicionado à hierarquia lowered. |
| `LambdaExpr` (144) | 51 | Closure Dart ou callback FFI, conforme a captura e ABI. |
| `CXXThrowExpr` (133) | 33 | `Stmt::Throw`/expressão `throw`, preservando o objeto de exceção. |
| `CXXReinterpretCastExpr` (126) / `CXXConstCastExpr` (127) | 23 | Apenas adaptador FFI seguro ou remoção de const quando não altera representação; nunca cast Dart inventado. |
| `CompoundAssignOperator` (115) | 10 | Reusar lowering de atribuição composta, inclusive alvo indexado/campo. |
| `InitListExpr` (119) | 7 | `List`/`Set`/record literal tipado conforme o destino. |
| `ConceptSpecializationExpr` (153) / `DeclStmt` vazando como expressão (231) | 7 | Tratar conceito como informação de tipo e elevar declaração para statement. |

## 3. Statements sem lowering — snapshot-base de 2.214 ocorrências

| Família | Qtde. | Causa raiz | Direção de solução |
| --- | ---: | --- | --- |
| `continue` | 469 | `CXCursor_ContinueStmt` (212) não tem variante IR. | Adicionar `Stmt::Continue` → `continue;`. |
| Atribuição indexada | 417 | Só variável simples e campo são alvos de atribuição. | Generalizar alvo para expressão atribuível (`Index`, `FieldAccess`, `Ref`) e emitir `target = value`. |
| `for` baseado em intervalo | 320 | `CXXForRangeStmt` (225) não é lowered. | `for (final item in iterable)`; quando houver mutação/referência, usar cursor/adaptador explícito. |
| `break` | 297 | `CXCursor_BreakStmt` (213) não tem variante IR. | Adicionar `Stmt::Break` → `break;`. |
| `switch` | 141 | `CXCursor_SwitchStmt` (206) sem representação. | IR de `switch`/`case`/`default`, com `break` e queda controlada explicitamente. |
| Declarações múltiplas/não-variável | 212 | `DeclStmt` admite vários declaradores e alguns não são `VarDecl`. | Desdobrar em statements ordenados; tratar declarações de tipo/using separadamente. |
| Statement vazio/bloco | 147 | `NullStmt` (230) e `CompoundStmt` (202) chegam sem regra. | Omitir vazio; achatar bloco mantendo escopo quando houver declaração/RAII. |
| `do…while` | 56 | `CXCursor_DoStmt` (208) sem IR. | Emitir `do { ... } while (cond);` em Dart. |
| Atribuição composta | 60 | Operadores `<<=`, `>>=`, `&=`, `|=` e alvos não simples não são tratados. | Reusar alvo atribuível e operadores Dart equivalentes; para índice/buffer, usar adaptador de escrita. |
| `for`/`if` com forma parcial | 44 | O lowering assume conjunto fixo de filhos; C++ permite cláusulas omitidas. | Representar `init`/`condition`/`increment` opcionalmente (o IR já comporta isso) e aceitar as formas AST válidas. |
| `delete`, `goto`, label | 47 | `CXXDeleteExpr` (135), `GotoStmt` (210) e `LabelStmt` (201) chegam como statement não suportado. | `delete` vira `dispose` somente para `Owned<T>`; `goto`/labels exigem reestruturação de fluxo ou fronteira externa — não há equivalente Dart direto. |
| Exceções C++ múltiplas/catch-all | 2 | `CXXTryStmt`/`CXXCatchStmt` aceitam mais formas que o IR atual. | Vários `catch` Dart e `catch (_, stack)` para catch-all, com tradução de tipos de exceção. |
| Declaração sem definição | 2 | Função externamente declarada sem corpo do projeto. | Marcar como externo e gerar mock/ponte pelo fluxo de externos; não emitir statement de bailout. |

## Ordem de execução

1. **Conversões + controle de fluxo barato.** Verdade C++, upcast seguro,
   enum/inteiro, `continue`, `break`, statement vazio, `do…while` e range-for
   removem uma fração grande sem dependência de bibliotecas externas.
2. **Normalização de AST.** Remover `TypeRef`/`TemplateRef` dos filhos de
   valor, aceitar cláusulas opcionais e normalizar receptores STL. Isso reduz
   bailouts sem decidir semântica de domínio.
3. **Adaptadores de `String` e coleções.** Implementar a tabela de métodos
   de maior volume com testes de equivalência, distinguindo APIs por byte de
   APIs por code unit.
4. **Ponteiros, buffers e callbacks.** Usar os fatos do catálogo para escolher
   referência, buffer ou FFI. Esta etapa não pode ser substituída por cast
   dinâmico/ponteiro opaco genérico.
5. **Produtos, streams, regex, ownership e tipos anônimos.** Cada um ganha
   adaptador reutilizável ou decisão explícita de fronteira externa.
6. **Casos sem equivalente (`goto`, `reinterpret_cast`, união de layout).**
   Exigir ponte externa/FFI ou uma transformação de controle comprovada; o
   bailout permanece até existir essa decisão, mas é rastreado por causa.

## Regra de regressão

`just verovio-diagnosis` agora serializa as três tabelas completas no campo
`bailouts` do snapshot. Antes de reduzir uma família, acrescente uma fixture
mínima e um teste que falhe; depois compare a contagem daquela razão no
Verovio. Uma nova razão, spelling vazio ou retorno de `dynamic` é regressão de
diagnóstico, não ruído aceitável.
