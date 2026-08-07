#pragma once

#include "core/shape.hpp"

namespace geometry {

class Rectangle : public Shape {
public:
    Rectangle(double width, double height);

    Area area() const override;
    std::string name() const override;

private:
    double width_;
    double height_;
};

}  // namespace geometry
