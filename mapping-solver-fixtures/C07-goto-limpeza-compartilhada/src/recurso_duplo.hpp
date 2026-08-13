#pragma once

// Padrão comum em C (e ainda em C++ mais antigo): `goto` pulando para um
// rótulo de limpeza compartilhado por múltiplos pontos de saída, um
// pobre-homem's RAII escrito à mão. Dart não tem `goto` entre blocos assim.
// A tradução mecânica ("Dart tem `label: while` para `continue`/`break`")
// NÃO serve — não é um laço, é controle de fluxo através de saídas
// antecipadas de função. O único caminho é código ponte que reestrutura
// isto como `try`/`finally` (ou uma sequência de `if` aninhados) — mudando
// a forma do código, exatamente como a armadilha de C06.
int processarComDoisRecursos(bool falharAoAbrirSegundoRecurso);
