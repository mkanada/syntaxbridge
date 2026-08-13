#pragma once

// `std::thread` compartilha memória de verdade, protegida por
// `std::mutex`. Isolates de Dart NÃO compartilham memória — cada isolate
// tem seu próprio heap, e comunicação é por mensagem. Não há opção de
// mapeamento de tipo que preserve "duas threads incrementam a mesma
// variável sob lock"; o único caminho possível é código ponte que
// reestrutura o algoritmo em torno de troca de mensagens entre isolates
// (ou, minimamente, documentar que o paralelismo real não é preservado).
class ContadorCompartilhado {
public:
    void incrementarEmParalelo(int vezesPorThread, int quantidadeDeThreads);
    int valor() const { return valor_; }

private:
    int valor_ = 0;
};
