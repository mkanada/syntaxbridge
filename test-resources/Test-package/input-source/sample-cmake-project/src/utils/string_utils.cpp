#include "utils/string_utils.hpp"

#include <algorithm>
#include <cctype>

namespace utils {

std::string to_upper(const std::string& input) {
    std::string result = input;
    std::transform(result.begin(), result.end(), result.begin(),
                    [](unsigned char c) { return std::toupper(c); });
    return result;
}

std::string trim(const std::string& input) {
    const auto begin = input.find_first_not_of(" \t\n\r");
    if (begin == std::string::npos) {
        return "";
    }
    const auto end = input.find_last_not_of(" \t\n\r");
    return input.substr(begin, end - begin + 1);
}

}  // namespace utils
