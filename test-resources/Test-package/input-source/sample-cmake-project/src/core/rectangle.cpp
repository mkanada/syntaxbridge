#include "core/rectangle.hpp"

namespace geometry {

Rectangle::Rectangle(double width, double height)
    : width_(width), height_(height) {}

Area Rectangle::area() const {
    return width_ * height_;
}

std::string Rectangle::name() const {
    return "Rectangle";
}

}  // namespace geometry
