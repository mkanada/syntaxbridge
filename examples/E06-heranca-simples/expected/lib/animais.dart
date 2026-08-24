abstract class Animal {
  Animal();

  Animal.syntaxBridgeCopyOf(Animal other) {}

  String falar();

  String apresentar() {
    return 'Eu digo: ' + falar();
  }
}

class Cachorro extends Animal {
  Cachorro();

  Cachorro.syntaxBridgeCopyOf(Cachorro other)
    : super.syntaxBridgeCopyOf(other) {}

  @override
  String falar() {
    return 'Au au';
  }
}

class Gato extends Animal {
  Gato();

  Gato.syntaxBridgeCopyOf(Gato other) : super.syntaxBridgeCopyOf(other) {}

  @override
  String falar() {
    return 'Miau';
  }
}

String apresentarAnimal(Animal animal) {
  return animal.apresentar();
}

String testarCachorro() {
  Cachorro c = Cachorro();
  return apresentarAnimal(c);
}

String testarGato() {
  Gato g = Gato();
  return apresentarAnimal(g);
}
