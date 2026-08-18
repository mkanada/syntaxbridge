# Inventário de `dynamic` — Verovio 6.2.0

## Escopo e regra de aceitação

O inventário inicial de `.diagnosis/dart-package/**/*.dart`, gerado por
`just verovio-diagnosis` em 2026-08-18, encontrou **1.568 tokens `dynamic` em
1.201 linhas**:

| Origem | Ocorrências | Direção |
| --- | ---: | --- |
| `dynamic /* unsupported: … */` | 1.311 | Substituir pelo mapeamento desta tabela; o spelling no comentário é a fonte de verdade para o adaptador. |
| `_syntaxBridgeUnsupported` | 253 | Substituir por bailout genérico invocado com o tipo estático esperado; continua lançando, mas não emite `dynamic`. |
| Identificador local chamado `dynamic` | 4 | Não é tipo Dart; renomear pelo normalizador de nomes. |

**Resultado da rodada seguinte (2026-08-18):** **0 tokens `dynamic` em 300
arquivos Dart**, confirmado pela asserção do diagnóstico e por busca textual.
O emissor agora usa `_syntaxBridgeUnsupported<T>` para uma expressão que já
tem tipo estático conhecido e `SyntaxBridgeOpaque` para uma fronteira ainda sem
adaptador. Há 3.804 linhas que referenciam essa ponte opaca: elas continuam uma
lacuna de conversão a ser resolvida por adaptadores nomeados, mas não escondem
a lacuna com `dynamic`.

**Critério de conclusão:** um pacote transpilado não pode conter `dynamic` como
tipo produzido pelo Syntax Bridge. Uma fronteira externa precisa ter uma
interface/handle Dart nomeado e tipado; uma tradução que não alcance isto segue
como lacuna diagnosticada, não como código aceito silenciosamente.

As 1.311 ocorrências anotadas se distribuem de forma exaustiva nas famílias
abaixo. As contagens são de ocorrências, não de tipos distintos: um tipo aparece
em campos, parâmetros e retornos diferentes.

| Família de origem | Ocorrências | Exemplos observados | Destino Dart e direção |
| --- | ---: | --- | --- |
| Escalares numéricos e aliases | 549 | `unsigned int`, `char`, `unsigned short`, `float`, `char32_t`, `long`, `mz_uint*` | Normalizar a base: inteiros C/C++ e `char32_t` → `int`; `float`/`long double` → `double`. Aplicar validação de faixa/sinal quando o contrato exigir largura fixa; modificadores de ponteiro/array seguem as linhas próprias abaixo. `char` é `int` (code unit) por padrão, nunca `String` sem contexto de string. |
| Produtos | 142 | `std::pair<A, B>`, `std::tuple<A, B, C>` | Introduzir `Pair<A, B>` e `TupleN`/record Dart tipados, recursivamente mapeando cada elemento. Não perder a identidade de `first`/`second` de `pair`. |
| Coleções | 17 | `vector`, `unordered_map`, `multiset`, `stack`, `array` | `List<T>`, `Map<K,V>`, `Set<T>`; `MultiMap<K,V>` para multimap/multiset; adaptador `Stack<T>`/`Queue<T>` quando a semântica de acesso importar; array preserva tamanho por wrapper ou validação. |
| Opcional e posse | 11 | `optional<T>`, `unique_ptr<T>` | `T?` para opcional; `Owned<T>`/referência anulável para ponteiro dono, com ciclo de vida explícito. |
| Streams, texto, regex e locale | 210 | `basic_istream`, `basic_ostream`, `basic_stringstream`, `streambuf`, `basic_regex`, `match_results` | Adaptadores `CppInput`, `CppOutput` e `StringBuffer`/`String`; `RegExp` e resultado tipado para regex. Estes são candidatos fortes a marcação como externo quando dependem de biblioteca C++ em vez de lógica do projeto. |
| Callbacks | 19 | `bool (*)(const Note *)`, callbacks `miniz` | Gerar `typedef` Dart ou interface de callback com parâmetros/retorno recursivamente mapeados; callbacks ABI usam ponte FFI nomeada. |
| Ponteiros opacos | 145 | `void *`, `const void *`, `nullptr_t` | `OpaqueHandle`/`Pointer<Void>` apenas na camada FFI; APIs de domínio recebem uma interface específica, não `Object` ou `dynamic`. Nulidade vira `?`. |
| Ponteiros, arrays, iteradores e casos residuais | 218 | `Point[4]`, `Point[]`, `int *`, `Layer **`, `deque`, `std::_Rb_tree_const_iterator`, unions `pugi`, struct `miniz` anônimo | Array → `List<T>` ou buffer tipado; ponteiro para objeto → `T?` quando não há aritmética; ponteiro de valor/buffer → `TypedData` ou `Ref<T>`; iterador → `Iterable<T>`/adaptador de cursor. `deque`/`initializer_list` viram coleção, `std::function` vira `typedef`, e tipos internos/anônimos ganham ponte nomeada ou são externos. |

## Ordem de implementação

1. **Concluído — remover o bailout `dynamic`.** `Expr::UnsupportedTyped`
   carrega o tipo esperado e emite `_syntaxBridgeUnsupported<T>(...)`; o helper
   lança `UnimplementedError` sem perder o tipo que a expressão teria. O
   diagnóstico falha se o token `dynamic` voltar a aparecer.
2. **Concluído — normalizar escalares.** Todos os inteiros fundamentais de
   C/C++ (`char`, variantes signed/unsigned e larguras `short` a `int128`) são
   `int`; `half`, `float`, `double`, `long double` e variantes de precisão são
   `double`. Largura e sinal permanecem responsabilidade da validação de
   fronteira quando o contrato exigir.
3. **Adicionar pontes estruturais reutilizáveis.** `Pair`, tuple, multimap,
   callbacks e buffers devem viver em adaptadores de linguagem, não em casos
   especiais do Verovio.
4. **Decidir fronteiras externas no catálogo existente.** Streams, pugi,
   miniz, jsonxx e iteradores internos precisam ser transpilados por adaptador
   ou marcados como externos pelo usuário; nenhum dos dois caminhos admite
   `dynamic` na API gerada.
5. **Concluído parcialmente — regressão de `dynamic`.** O diagnóstico falha
   ao encontrar o token e a busca no pacote confirma zero. Falta agrupar os
   spellings de `Type::Unsupported` por família para priorizar cada adaptador
   nomeado.

## Observação sobre herança

O trabalho de herança/mixins permanece prioritário para os erros de análise
mais numerosos. A eliminação de `dynamic` é independente: ela torna as lacunas
visíveis por `SyntaxBridgeOpaque`, sem fingir que os tipos complexos já foram
convertidos semanticamente.
