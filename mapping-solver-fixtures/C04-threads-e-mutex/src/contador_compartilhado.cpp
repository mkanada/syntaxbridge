#include "contador_compartilhado.hpp"

#include <mutex>
#include <thread>
#include <vector>

void ContadorCompartilhado::incrementarEmParalelo(int vezesPorThread, int quantidadeDeThreads) {
    std::mutex mutex;
    std::vector<std::thread> trabalhadores;

    for (int t = 0; t < quantidadeDeThreads; ++t) {
        trabalhadores.emplace_back([this, &mutex, vezesPorThread]() {
            for (int i = 0; i < vezesPorThread; ++i) {
                std::lock_guard<std::mutex> guarda(mutex);
                valor_ = valor_ + 1;
            }
        });
    }

    for (auto& trabalhador : trabalhadores) {
        trabalhador.join();
    }
}
