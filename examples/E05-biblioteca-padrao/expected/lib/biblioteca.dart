import 'dart:convert';

String saudacao(String nome) {
  return 'Ola, ' + nome;
}

int tamanhoDaMensagem(String mensagem) {
  return utf8.encode(mensagem).length;
}

bool mensagensIguais(String a, String b) {
  return a == b;
}

int somaVetor(List<int> valores) {
  int soma = 0;
  for (int i = 0; i < valores.length; i = i + 1) {
    soma = soma + valores[i];
  }
  return soma;
}

int maiorValor(List<int> valores) {
  int maior = valores[0];
  for (int i = 1; i < valores.length; i = i + 1) {
    if (valores[i] > maior) {
      maior = valores[i];
    }
  }
  return maior;
}

int somaComLista(List<int> valores) {
  int soma = 0;
  for (final int it in valores) {
    soma = soma + it;
  }
  return soma;
}

int contaOcorrencias(List<bool> valores) {
  int total = 0;
  for (int i = 0; i < valores.length; i = i + 1) {
    if (valores[i]) {
      total = total + 1;
    }
  }
  return total;
}

int ultimoElementoReverso(List<int> valores) {
  int ultimo = 0;
  for (final int it in valores.reversed) {
    ultimo = it;
    break;
  }
  return ultimo;
}

int valorOuPadrao(Map<String, int> mapa, String chave, int padrao) {
  int? it = mapa[chave];
  if (it != null) {
    return it;
  }
  return padrao;
}
