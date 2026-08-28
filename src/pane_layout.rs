pub const DATASET_DIVIDER_HEIGHT: f32 = 12.0;
pub const MIN_RIGHT_PANE_HEIGHT: f32 = 120.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RightPaneSplit {
    pub inspector_height: f32,
    pub dataset_height: f32,
}

impl RightPaneSplit {
    pub fn resolve(available_height: f32, requested_dataset_height: f32) -> Self {
        let available_height = if available_height.is_finite() {
            available_height.max(0.0)
        } else {
            0.0
        };
        let usable_height = (available_height - DATASET_DIVIDER_HEIGHT).max(0.0);

        if usable_height < MIN_RIGHT_PANE_HEIGHT * 2.0 {
            let dataset_height = usable_height / 2.0;
            return Self {
                inspector_height: usable_height - dataset_height,
                dataset_height,
            };
        }

        let dataset_height = requested_dataset_height
            .clamp(MIN_RIGHT_PANE_HEIGHT, usable_height - MIN_RIGHT_PANE_HEIGHT);
        Self {
            inspector_height: usable_height - dataset_height,
            dataset_height,
        }
    }

    pub fn after_drag(
        available_height: f32,
        current_dataset_height: f32,
        pointer_delta_y: f32,
    ) -> Self {
        Self::resolve(available_height, current_dataset_height - pointer_delta_y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divider_drag_resizes_in_the_expected_direction_and_clamps() {
        let taller = RightPaneSplit::after_drag(700.0, 280.0, -50.0);
        assert_eq!(taller.dataset_height, 330.0);
        assert_eq!(taller.inspector_height, 358.0);

        let top_limit = RightPaneSplit::after_drag(700.0, 280.0, -10_000.0);
        assert_eq!(top_limit.dataset_height, 568.0);
        assert_eq!(top_limit.inspector_height, MIN_RIGHT_PANE_HEIGHT);

        let bottom_limit = RightPaneSplit::after_drag(700.0, 280.0, 10_000.0);
        assert_eq!(bottom_limit.dataset_height, MIN_RIGHT_PANE_HEIGHT);
        assert_eq!(bottom_limit.inspector_height, 568.0);
    }

    #[test]
    fn narrow_sidebar_never_allocates_beyond_available_height() {
        let split = RightPaneSplit::resolve(200.0, 280.0);
        assert_eq!(split.inspector_height, 94.0);
        assert_eq!(split.dataset_height, 94.0);
        assert_eq!(
            split.inspector_height + DATASET_DIVIDER_HEIGHT + split.dataset_height,
            200.0
        );

        let invalid = RightPaneSplit::resolve(f32::NAN, f32::INFINITY);
        assert_eq!(invalid.inspector_height, 0.0);
        assert_eq!(invalid.dataset_height, 0.0);
    }
}
