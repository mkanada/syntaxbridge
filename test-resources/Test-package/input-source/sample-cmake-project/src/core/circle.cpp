#include "core/circle.hpp"

#include "utils/math_utils.hpp"

namespace geometry {

Circle::Circle(double radius) : radius_(radius) {}

Area Circle::area() const {
    return utils::PI * radius_ * radius_;
}

std::string Circle::name() const {
    return "Circle";
}

}  // namespace geometry
