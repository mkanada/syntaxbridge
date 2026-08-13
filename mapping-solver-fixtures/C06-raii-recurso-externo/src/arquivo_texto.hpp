#pragma once

#include <cstdio>
#include <string>

// O destrutor fecha o arquivo (`fclose`) de forma determinística, no exato
// ponto em que `ArquivoTexto` sai de escopo. Dart não tem destrutor
// determinístico — `Finalizer` roda em tempo não previsível, depois do GC,
// o que é tarde demais para um recurso do sistema operacional (descritor
// de arquivo) que outro processo pode precisar. Não há mapeamento de tipo
// que resolva isso: o código ponte precisa expor `dispose()`/`close()`
// explícito e reescrever cada scope que dependia do RAII para chamá-lo (ou
// usar um padrão `try`/`finally`) — mudando a FORMA do código do usuário,
// não só o tipo.
class ArquivoTexto {
public:
    explicit ArquivoTexto(const std::string& caminho);
    ~ArquivoTexto();

    void escrever(const std::string& conteudo);

private:
    std::FILE* alca_;
};
