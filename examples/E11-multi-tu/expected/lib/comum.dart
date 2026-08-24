class Ponto3D {
  double x;
  double y;
  double z;

  Ponto3D(this.x, this.y, this.z);

  Ponto3D.syntaxBridgeCopyOf(Ponto3D other)
    : x = other.x,
      y = other.y,
      z = other.z {}
}

double normaAoQuadrado(Ponto3D p) {
  p = Ponto3D.syntaxBridgeCopyOf(p);
  return p.x * p.x + p.y * p.y + p.z * p.z;
}
