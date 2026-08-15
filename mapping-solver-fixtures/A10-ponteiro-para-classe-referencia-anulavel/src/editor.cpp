#include "editor.hpp"

void Editor::Definir(Nota *nota) { m_atual = nota; }

Nota *Editor::Atual() const { return m_atual; }
