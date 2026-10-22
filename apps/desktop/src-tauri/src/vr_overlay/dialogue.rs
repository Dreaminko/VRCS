use super::placement::Pose;

pub fn in_front(head: Pose) -> Pose {
    super::transform::compose(head, super::transform::matrix(0., 0., 0., [0., -0.12, -1.]))
}
