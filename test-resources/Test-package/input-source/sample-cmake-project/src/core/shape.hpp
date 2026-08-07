#pragma once

#include <string>

namespace geometry {

#define GEOMETRY_EPSILON 1e-6

struct Point {
    double x;
    double y;
};

using Area = double;

class Shape {
public:
    virtual ~Shape() = default;
    virtual Area area() const = 0;
    virtual std::string name() const = 0;
};

bool are_equal(Area a, Area b);

}  // namespace geometry
