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
