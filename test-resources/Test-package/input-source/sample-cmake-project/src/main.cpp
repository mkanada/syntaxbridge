#include <iostream>
#include <memory>
#include <vector>

#include "core/circle.hpp"
#include "core/rectangle.hpp"
#include "core/shape.hpp"
#include "io/logger.hpp"
#include "utils/math_utils.hpp"
#include "utils/string_utils.hpp"

int main() {
    io::Logger logger("sample_app");

    std::vector<std::unique_ptr<geometry::Shape>> shapes;
    shapes.push_back(std::make_unique<geometry::Circle>(2.0));
    shapes.push_back(std::make_unique<geometry::Rectangle>(3.0, 4.0));

    for (const auto& shape : shapes) {
        const std::string message = utils::to_upper(shape->name()) +
                                     " area=" + std::to_string(shape->area());
        logger.log(io::LogLevel::Info, message);
    }

    const double clamped = utils::clamp(150.0, 0.0, 100.0);
    logger.log(io::LogLevel::Info,
               "clamped value=" + std::to_string(clamped));

    const int common_divisor = utils::gcd(48, 18);
    logger.log(io::LogLevel::Info,
               "gcd(48, 18)=" + std::to_string(common_divisor));

    return 0;
}
