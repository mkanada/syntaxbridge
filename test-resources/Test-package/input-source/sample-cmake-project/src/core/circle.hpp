#pragma once

#include "core/shape.hpp"

namespace geometry {

class Circle : public Shape {
public:
    explicit Circle(double radius);

    Area area() const override;
    std::string name() const override;

private:
    double radius_;
};

}  // namespace geometry
