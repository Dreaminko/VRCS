#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameCorners {
    pub corners: [[f32; 3]; 2],
}

#[derive(Clone)]
pub struct HandSample {
    pub origin: u64,
    pub curls: [f32; 5],
    pub wrist: [f32; 3],
    pub thumb_base: [f32; 3],
    pub thumb_tip: [f32; 3],
    pub index_base: [f32; 3],
    pub index_tip: [f32; 3],
}

pub fn camera_frame(hands: &[HandSample; 2]) -> bool {
    let span_x = hands[1].wrist[0] - hands[0].wrist[0];
    let span_y = hands[1].wrist[1] - hands[0].wrist[1];
    if hands[0].origin == 0
        || hands[0].origin == hands[1].origin
        || !(0.12..=0.7).contains(&span_x)
        || !(0.08..=0.5).contains(&span_y.abs())
        || (hands[0].wrist[2] - hands[1].wrist[2]).abs() > 0.12
    {
        return false;
    }
    for (index, hand) in hands.iter().enumerate() {
        if hand
            .curls
            .iter()
            .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
            || hand.curls[0] > 0.35
            || hand.curls[1] > 0.35
            || hand.curls[2..].iter().any(|curl| *curl < 0.5)
            || !(-1.0..=-0.15).contains(&hand.wrist[2])
            || hand.wrist[0].abs() > 0.6
            || hand.wrist[1].abs() > 0.5
            || [
                hand.wrist,
                hand.thumb_base,
                hand.thumb_tip,
                hand.index_base,
                hand.index_tip,
            ]
            .iter()
            .flatten()
            .any(|value| !value.is_finite())
        {
            return false;
        }
        let Some(thumb) = direction(hand.thumb_base, hand.thumb_tip, 0.02, 0.2) else {
            return false;
        };
        let Some(finger) = direction(hand.index_base, hand.index_tip, 0.04, 0.25) else {
            return false;
        };
        let horizontal = if index == 0 { 1. } else { -1. };
        if thumb[0] * horizontal < 0.7
            || finger[1] * horizontal * span_y.signum() < 0.7
            || thumb[2].abs() > 0.4
            || finger[2].abs() > 0.4
            || (0..3)
                .map(|axis| thumb[axis] * finger[axis])
                .sum::<f32>()
                .abs()
                > 0.35
        {
            return false;
        }
    }
    true
}

pub fn drag_frame(hands: &[HandSample; 2]) -> Option<FrameCorners> {
    if hands[0].origin == 0 || hands[1].origin == 0 || hands[0].origin == hands[1].origin {
        return None;
    }
    for hand in hands {
        if hand
            .curls
            .iter()
            .any(|curl| !curl.is_finite() || !(0.0..=1.0).contains(curl))
            || hand.curls[0] > 0.45
            || hand.curls[1] > 0.45
            || !(-3.0..=-0.05).contains(&hand.wrist[2])
            || [
                hand.wrist,
                hand.thumb_base,
                hand.thumb_tip,
                hand.index_base,
                hand.index_tip,
            ]
            .iter()
            .flatten()
            .any(|value| !value.is_finite())
            || direction(hand.thumb_base, hand.thumb_tip, 0.02, 0.2).is_none()
            || direction(hand.index_base, hand.index_tip, 0.04, 0.25).is_none()
        {
            return None;
        }
    }
    Some(FrameCorners {
        corners: [hands[0].wrist, hands[1].wrist],
    })
}

pub fn gesture_frame(hands: &[HandSample; 2], confirm_pressed: bool) -> Option<FrameCorners> {
    if !confirm_pressed {
        return drag_frame(hands);
    }
    if hands[0].origin == 0
        || hands[1].origin == 0
        || hands[0].origin == hands[1].origin
        || hands.iter().any(|hand| {
            hand.wrist.iter().any(|value| !value.is_finite())
                || !(-3.0..=-0.05).contains(&hand.wrist[2])
        })
    {
        return None;
    }
    Some(FrameCorners {
        corners: [hands[0].wrist, hands[1].wrist],
    })
}

fn direction(base: [f32; 3], tip: [f32; 3], minimum: f32, maximum: f32) -> Option<[f32; 3]> {
    let delta: [f32; 3] = std::array::from_fn(|axis| tip[axis] - base[axis]);
    let length = delta.iter().map(|value| value * value).sum::<f32>().sqrt();
    (minimum..=maximum)
        .contains(&length)
        .then(|| delta.map(|value| value / length))
}

pub fn point_in_head(head: [[f32; 4]; 3], device: [[f32; 4]; 3], point: [f32; 3]) -> [f32; 3] {
    let delta: [f32; 3] = std::array::from_fn(|row| {
        (0..3)
            .map(|axis| device[row][axis] * point[axis])
            .sum::<f32>()
            + device[row][3]
            - head[row][3]
    });
    std::array::from_fn(|axis| (0..3).map(|row| head[row][axis] * delta[row]).sum())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> [HandSample; 2] {
        [
            HandSample {
                origin: 1,
                curls: [0.1, 0.1, 0.8, 0.8, 0.8],
                wrist: [-0.18, -0.12, -0.45],
                thumb_base: [-0.18, -0.12, -0.45],
                thumb_tip: [-0.11, -0.12, -0.45],
                index_base: [-0.18, -0.12, -0.45],
                index_tip: [-0.18, -0.02, -0.45],
            },
            HandSample {
                origin: 2,
                curls: [0.1, 0.1, 0.8, 0.8, 0.8],
                wrist: [0.18, 0.12, -0.45],
                thumb_base: [0.18, 0.12, -0.45],
                thumb_tip: [0.11, 0.12, -0.45],
                index_base: [0.18, 0.12, -0.45],
                index_tip: [0.18, 0.02, -0.45],
            },
        ]
    }

    #[test]
    fn diagonal_finger_frames_work_on_both_diagonals() {
        let mut hands = frame();
        assert!(camera_frame(&hands));
        for hand in &mut hands {
            hand.wrist[1] = -hand.wrist[1];
            hand.thumb_base[1] = -hand.thumb_base[1];
            hand.thumb_tip[1] = -hand.thumb_tip[1];
            hand.index_base[1] = -hand.index_base[1];
            hand.index_tip[1] = -hand.index_tip[1];
        }
        assert!(camera_frame(&hands));
    }

    #[test]
    fn dragging_keeps_extended_fingers_without_requiring_the_activation_orientation() {
        let mut hands = frame();
        hands[1].wrist = [-0.1, 0.03, -1.4];
        hands[0].thumb_tip = [-0.18, -0.12, -0.38];
        hands[0].curls[2] = 0.1;
        assert!(!camera_frame(&hands));
        assert_eq!(
            drag_frame(&hands).unwrap().corners,
            [[-0.18, -0.12, -0.45], [-0.1, 0.03, -1.4]]
        );
        hands[0].curls[1] = 0.8;
        assert!(drag_frame(&hands).is_none());
        hands[0].curls[1] = 0.1;
        hands[1].wrist[0] = f32::NAN;
        assert!(drag_frame(&hands).is_none());
    }

    #[test]
    fn trigger_confirmation_keeps_valid_wrists_when_the_index_finger_curls() {
        let mut hands = frame();
        hands[1].curls[1] = 0.9;
        assert!(gesture_frame(&hands, false).is_none());
        assert_eq!(
            gesture_frame(&hands, true).unwrap().corners,
            [[-0.18, -0.12, -0.45], [0.18, 0.12, -0.45]]
        );
        hands[1].wrist[2] = 0.1;
        assert!(gesture_frame(&hands, true).is_none());
        hands[1].wrist[2] = -0.45;
        hands[1].wrist[0] = f32::NAN;
        assert!(gesture_frame(&hands, true).is_none());
    }

    #[test]
    fn ordinary_hand_poses_and_invalid_tracking_do_not_form_a_frame() {
        for change in 0..7 {
            let mut hands = frame();
            match change {
                0 => hands[0].curls[1] = 0.9,
                1 => hands[0].curls[2] = 0.1,
                2 => hands[0].thumb_tip = hands[0].thumb_base,
                3 => hands[0].index_tip[1] = -0.22,
                4 => hands[0].wrist[2] = 0.45,
                5 => hands[1].wrist[2] = -0.8,
                _ => hands[0].index_tip[0] = f32::NAN,
            }
            assert!(
                !camera_frame(&hands),
                "unexpected gesture for case {change}"
            );
        }
    }

    #[test]
    fn hand_points_use_head_coordinates_after_rotation_and_translation() {
        let head = super::super::transform::matrix(0., 90., 0., [1., 2., 3.]);
        let device = super::super::transform::matrix(0., 90., 0., [0.55, 1.88, 3.18]);
        let point = point_in_head(head, device, [0.1, 0., 0.]);
        for (actual, expected) in point.into_iter().zip([-0.08, -0.12, -0.45]) {
            assert!((actual - expected).abs() < 0.0001);
        }
    }
}
