#include "io/logger.hpp"

#include <iostream>

namespace io {

Logger::Logger(std::string prefix) : prefix_(std::move(prefix)) {}

void Logger::log(LogLevel level, const std::string& message) const {
    std::cout << "[" << prefix_ << "][" << level_to_string(level) << "] "
              << message << std::endl;
}

const char* Logger::level_to_string(LogLevel level) {
    switch (level) {
        case LogLevel::Info:
            return "INFO";
        case LogLevel::Warning:
            return "WARNING";
        case LogLevel::Error:
            return "ERROR";
    }
    return "UNKNOWN";
}

}  // namespace io
