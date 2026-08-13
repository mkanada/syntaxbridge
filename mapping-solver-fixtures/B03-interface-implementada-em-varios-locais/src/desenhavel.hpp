#pragma once

#include <string>

// `desenhar()` é puro — até aqui, candidato natural a `abstract interface
// class` em Dart. Mas `descricaoPadrao()` NÃO é pura: tem corpo próprio.
// Circulo e Quadrado (arquivos separados) nunca a chamam, então olhar só um
// dos dois sugere que o corpo default é morto e pode ser ignorado. Só
// triangulo.hpp usa `descricaoPadrao()` de verdade — e isso é o que torna
// "interface pura" inviável para `Desenhavel` como um todo: uma interface
// Dart não carrega implementação herdável, então o corpo de
// `descricaoPadrao()` obriga `Desenhavel` a virar mixin (ou a implementação
// default a ser duplicada em cada classe), não interface. A decisão certa
// só aparece depois de ler os três implementadores.
class Desenhavel {
public:
    virtual ~Desenhavel() = default;
    virtual void desenhar() const = 0;
    virtual std::string descricaoPadrao() const { return "forma sem nome"; }
};
