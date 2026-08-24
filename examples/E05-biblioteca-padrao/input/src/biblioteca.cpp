#include "biblioteca.hpp"
#include <regex>

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

int somaComLista(const std::list<int>& valores) {
    int soma = 0;
    for (auto it = valores.begin(); it != valores.end(); ++it) {
        soma = soma + *it;
    }
    return soma;
}

int contaOcorrencias(const std::vector<bool>& valores) {
    int total = 0;
    for (int i = 0; i < valores.size(); i = i + 1) {
        if (valores[i]) {
            total = total + 1;
        }
    }
    return total;
}

int ultimoElementoReverso(const std::vector<int>& valores) {
    int ultimo = 0;
    for (auto it = valores.rbegin(); it != valores.rend(); ++it) {
        ultimo = *it;
        break;
    }
    return ultimo;
}

int valorOuPadrao(const std::map<std::string, int>& mapa, const std::string& chave, int padrao) {
    auto it = mapa.find(chave);
    if (it != mapa.end()) {
        return it->second;
    }
    return padrao;
}

bool temNota(const std::string& s) {
    std::regex re("([A-G])([#b]?)([0-9])");
    std::smatch m;
    if (std::regex_search(s, m, re)) {
        return m[1] == "C";
    }
    return false;
}

std::string limpa(const std::string& s) {
    std::regex espacos("\\s+");
    return std::regex_replace(s, espacos, " ");
}

