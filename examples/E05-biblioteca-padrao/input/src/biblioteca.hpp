#pragma once

#include <list>
#include <map>
#include <string>
#include <vector>

std::string saudacao(const std::string& nome);
int tamanhoDaMensagem(const std::string& mensagem);
bool mensagensIguais(const std::string& a, const std::string& b);
int somaVetor(const std::vector<int>& valores);
int maiorValor(const std::vector<int>& valores);
int somaComLista(const std::list<int>& valores);
int contaOcorrencias(const std::vector<bool>& valores);
int ultimoElementoReverso(const std::vector<int>& valores);
int valorOuPadrao(const std::map<std::string, int>& mapa, const std::string& chave, int padrao);
