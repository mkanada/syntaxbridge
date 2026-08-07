#pragma once

#include <string>

namespace io {

enum class LogLevel { Info, Warning, Error };

class Logger {
public:
    explicit Logger(std::string prefix);

    void log(LogLevel level, const std::string& message) const;

private:
    static const char* level_to_string(LogLevel level);

    std::string prefix_;
};

}  // namespace io
