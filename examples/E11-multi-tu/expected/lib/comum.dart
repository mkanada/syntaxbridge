class Ponto3D {
  double x;
  double y;
  double z;

  Ponto3D(this.x, this.y, this.z);
}

double normaAoQuadrado(Ponto3D p) {
  p = Ponto3D(p.x, p.y, p.z);
  return p.x * p.x + p.y * p.y + p.z * p.z;
}
