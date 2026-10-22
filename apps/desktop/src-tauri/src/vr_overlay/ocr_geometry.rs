#[derive(Clone, Copy)]
pub struct Homography(pub [[f32; 3]; 3]);

impl Homography {
    /// Maps a unit square to an ordered convex image quadrilateral.
    pub fn from_quad(quad: [[f32; 2]; 4]) -> Option<Self> {
        if quad.iter().flatten().any(|value| !value.is_finite()) {
            return None;
        }
        let crosses: [f32; 4] = std::array::from_fn(|i| {
            let a = quad[i];
            let b = quad[(i + 1) % 4];
            let c = quad[(i + 2) % 4];
            (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0])
        });
        if crosses
            .iter()
            .any(|cross| cross.abs() < 0.001 || cross.signum() != crosses[0].signum())
        {
            return None;
        }
        let [[x0, y0], [x1, y1], [x2, y2], [x3, y3]] = quad;
        let (sx, sy) = (x0 - x1 + x2 - x3, y0 - y1 + y2 - y3);
        let (dx1, dx2, dy1, dy2) = (x1 - x2, x3 - x2, y1 - y2, y3 - y2);
        let denominator = dx1 * dy2 - dx2 * dy1;
        if denominator.abs() < 0.001 {
            return None;
        }
        let (g, h) = (
            (sx * dy2 - dx2 * sy) / denominator,
            (dx1 * sy - sx * dy1) / denominator,
        );
        let matrix = [
            [x1 - x0 + g * x1, x3 - x0 + h * x3, x0],
            [y1 - y0 + g * y1, y3 - y0 + h * y3, y0],
            [g, h, 1.],
        ];
        matrix
            .iter()
            .flatten()
            .all(|value| value.is_finite())
            .then_some(Self(matrix))
    }

    pub fn inverse(self) -> Option<Self> {
        let [[a, b, c], [d, e, f], [g, h, i]] = self.0;
        let adjugate = [
            [e * i - f * h, c * h - b * i, b * f - c * e],
            [f * g - d * i, a * i - c * g, c * d - a * f],
            [d * h - e * g, b * g - a * h, a * e - b * d],
        ];
        let determinant = a * adjugate[0][0] + b * adjugate[1][0] + c * adjugate[2][0];
        if !determinant.is_finite() || determinant.abs() < 0.000001 {
            return None;
        }
        Some(Self(
            adjugate.map(|row| row.map(|value| value / determinant)),
        ))
    }

    pub fn map(self, [x, y]: [f32; 2]) -> Option<[f32; 2]> {
        let m = self.0;
        let w = m[2][0] * x + m[2][1] * y + m[2][2];
        if !w.is_finite() || w.abs() < 0.000001 {
            return None;
        }
        let point = [
            (m[0][0] * x + m[0][1] * y + m[0][2]) / w,
            (m[1][0] * x + m[1][1] * y + m[1][2]) / w,
        ];
        point.iter().all(|value| value.is_finite()).then_some(point)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trapezoid_mapping_preserves_corners_and_projective_center() {
        let quad = [[20., 10.], [80., 10.], [100., 70.], [0., 70.]];
        let mapping = Homography::from_quad(quad).unwrap();
        for (square, corner) in [[0., 0.], [1., 0.], [1., 1.], [0., 1.]]
            .into_iter()
            .zip(quad)
        {
            for (actual, expected) in mapping.map(square).unwrap().into_iter().zip(corner) {
                assert!((actual - expected).abs() < 0.001);
            }
        }
        for (actual, expected) in mapping
            .map([0.5, 0.5])
            .unwrap()
            .into_iter()
            .zip([50., 32.5])
        {
            assert!((actual - expected).abs() < 0.001);
        }
        for coordinate in mapping.inverse().unwrap().map([50., 32.5]).unwrap() {
            assert!((coordinate - 0.5).abs() < 0.001);
        }
    }

    #[test]
    fn degenerate_crossed_and_concave_text_regions_are_rejected() {
        for quad in [
            [[0., 0.], [10., 0.], [0., 10.], [10., 10.]],
            [[0., 0.], [10., 0.], [2., 2.], [0., 10.]],
            [[1., 1.]; 4],
        ] {
            assert!(Homography::from_quad(quad).is_none());
        }
    }
}
