#pragma once

// `setjmp`/`longjmp` é desvio de controle não local sem pilha de exceção —
// Dart não tem equivalente (nem `goto` cruza função). Não existe mapeamento
// de tipo aqui: a decisão de US-7 é sobre o próprio *formato* do código —
// código ponte teria que reescrever isto como uma máquina de estados ou
// recusar a conversão explicitamente (a "armadilha" do E10 registrada em
// conversao-guiada-por-exemplos.md: "talvez a resposta certa seja
// recusar").
int protegido();
