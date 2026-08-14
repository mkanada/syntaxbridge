import 'comum.dart';

double testarNormaOrigem() {
  Ponto3D p = Ponto3D(0, 0, 0);
  p.x = 3;
  p.y = 4;
  p.z = 0;
  return normaAoQuadrado(p);
}
