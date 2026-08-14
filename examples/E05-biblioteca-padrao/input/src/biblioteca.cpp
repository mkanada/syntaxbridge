#include "biblioteca.hpp"

std::string saudacao(const std::string& nome) {
    return "Ola, " + nome;
}

int tamanhoDaMensagem(const std::string& mensagem) {
    return mensagem.size();
}

bool mensagensIguais(const std::string& a, const std::string& b) {
    return a == b;
}

int somaVetor(const std::vector<int>& valores) {
    int soma = 0;
    for (int i = 0; i < valores.size(); i = i + 1) {
        soma = soma + valores[i];
    }
    return soma;
}

int maiorValor(const std::vector<int>& valores) {
    int maior = valores[0];
    for (int i = 1; i < valores.size(); i = i + 1) {
        if (valores[i] > maior) {
            maior = valores[i];
        }
    }
    return maior;
}
