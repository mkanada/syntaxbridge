abstract class Animal {
  Animal();

  String falar();

  String apresentar() {
    return 'Eu digo: ' + falar();
  }
}

class Cachorro extends Animal {
  Cachorro();

  @override
  String falar() {
    return 'Au au';
  }
}

class Gato extends Animal {
  Gato();

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
