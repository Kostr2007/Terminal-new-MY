//! Analytic damped springs. Positions and velocities use seconds.
#[derive(Clone, Copy)]
pub struct Spring {
    pub damping: f32,
    pub stiffness: f32,
}

// Spatial springs allow overshoot. Terminal cells need slightly more travel
// time than the Android presets for it to be visible at terminal frame rates.
pub const SECTION: Spring = Spring {
    damping: 0.62,
    stiffness: 175.,
};
pub const FOCUS: Spring = Spring {
    damping: 0.60,
    stiffness: 420.,
};
pub const SECTION_STEPS: u8 = 24;
pub const SECTION_FRAME_MS: u8 = 28;

impl Spring {
    pub fn sample(self, position: f32, velocity: f32, target: f32, seconds: f32) -> (f32, f32) {
        let displacement = position - target;
        let rate = self.stiffness.sqrt();
        let decay = self.damping * rate;
        let angular = rate * (1. - self.damping * self.damping).sqrt();
        let sine_weight = (velocity + decay * displacement) / angular;
        let (sin, cos) = (angular * seconds).sin_cos();
        let envelope = (-decay * seconds).exp();
        let offset = displacement * cos + sine_weight * sin;
        let slope = angular * (sine_weight * cos - displacement * sin) - decay * offset;
        (target + envelope * offset, envelope * slope)
    }
}

/// The critically damped step response for opacity and other bounded effects.
pub fn effect(seconds: f32) -> f32 {
    let t = 24. * seconds;
    1. - (1. + t) * (-t).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spatial_overshoots_while_opacity_is_monotonic() {
        let mut last = 0.;
        let mut peak: f32 = 0.;
        for index in 0..=100 {
            let seconds = index as f32 * 0.007;
            let (position, _) = SECTION.sample(0., 0., 1., seconds);
            peak = peak.max(position);
            let opacity = effect(seconds);
            assert!((last..=1.).contains(&opacity));
            last = opacity;
        }
        assert!((1.05..1.10).contains(&peak));
    }

    #[test]
    fn changing_targets_preserves_velocity() {
        let (position, velocity) = FOCUS.sample(0., 0., 3., 0.06);
        let (continued, slope) = FOCUS.sample(position, velocity, -1., 0.);
        assert!((continued - position).abs() < 1e-6);
        assert!((slope - velocity).abs() < 1e-5);
        assert!(FOCUS.sample(position, velocity, -1., 0.01).0 > position);
        assert!((FOCUS.sample(position, velocity, -1., 0.6).0 + 1.).abs() < 0.01);
    }
}
