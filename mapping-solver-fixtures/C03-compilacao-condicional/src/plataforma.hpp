#pragma once

// Dois layouts de `Config` incompatíveis entre si, escolhidos em tempo de
// compilação. `libclang` só enxerga o ramo ativo na unidade de compilação
// que foi de fato compilada (aqui, SYNTAX_BRIDGE_PLATAFORMA_A) — o outro
// ramo é texto morto do ponto de vista da análise, mas não do ponto de
// vista do produto real, que pode precisar compilar ambos os alvos. Dart
// não tem pré-processador: não há opção de mapeamento de tipo que resolva
// isso, só uma decisão de produto (gerar as duas variantes atrás de uma
// flag, ou perguntar ao usuário qual configuração converter).
#ifdef SYNTAX_BRIDGE_PLATAFORMA_A
struct Config {
    int modoA;
};
#else
struct Config {
    double modoB;
};
#endif

int valorPadrao(const Config& config);
