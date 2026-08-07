#include "core/shape.hpp"

#include <cmath>

namespace geometry {

bool are_equal(Area a, Area b) {
    return std::fabs(a - b) < GEOMETRY_EPSILON;
}

}  // namespace geometry
