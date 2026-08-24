mixin Voador {
  int altitude = 0;

  void subir() {
    altitude = altitude + 10;
  }

  String mover() {
    return 'voa';
  }
}

mixin Nadador {
  int profundidade = 0;

  void mergulhar() {
    profundidade = profundidade + 5;
  }

  String mover() {
    return 'nada';
  }
}

class PatoDaguaVoador with Voador, Nadador {
  PatoDaguaVoador();

  PatoDaguaVoador.syntaxBridgeCopyOf(PatoDaguaVoador other) {
    altitude = other.altitude;
    profundidade = other.profundidade;
  }

  String mover() {
    return 'voa e nada';
  }
}

int testarAltitude() {
  PatoDaguaVoador pato = PatoDaguaVoador();
  pato.subir();
  pato.subir();
  return pato.altitude;
}

int testarProfundidade() {
  PatoDaguaVoador pato = PatoDaguaVoador();
  pato.mergulhar();
  return pato.profundidade;
}

String testarMovimento() {
  PatoDaguaVoador pato = PatoDaguaVoador();
  return pato.mover();
}
