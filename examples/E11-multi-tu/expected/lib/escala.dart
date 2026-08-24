import 'comum.dart';

Ponto3D escalar(Ponto3D p, double fator) {
  p = Ponto3D.syntaxBridgeCopyOf(p);
  Ponto3D resultado = Ponto3D(0, 0, 0);
  resultado.x = p.x * fator;
  resultado.y = p.y * fator;
  resultado.z = p.z * fator;
  return resultado;
}

double testarNormaEscalada() {
  Ponto3D p = Ponto3D(0, 0, 0);
  p.x = 2;
  p.y = 0;
  p.z = 0;
  Ponto3D escalado = escalar(p, 3);
  return normaAoQuadrado(escalado);
}
