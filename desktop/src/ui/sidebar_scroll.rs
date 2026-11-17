use super::*;
use gpui::{DispatchPhase, HitboxBehavior, ScrollWheelEvent};

// Frequencies are radians/second. Critical damping gives a smooth glide
// without bounce; preserving velocity keeps trackpad input continuous.
const WHEEL_FREQUENCY: f32 = 11.;
const TRACKPAD_FREQUENCY: f32 = 11.;
const POSITION_EPSILON: f32 = 0.8;
const VELOCITY_EPSILON: f32 = 8.;

#[derive(Default)]
pub(crate) struct SidebarScroll {
    animation: Option<ScrollAnimation>,
}

struct ScrollAnimation {
    position: f32,
    velocity: f32,
    target: f32,
    updated: Instant,
    frequency: f32,
    direction: f32,
}

impl SidebarScroll {
    pub(super) fn cancel(&mut self) {
        self.animation = None;
    }

    fn scroll(&mut self, current: f32, delta: f32, maximum: f32, frequency: f32, now: Instant) {
        // Sample the old target first, including when several input events arrive
        // between display frames. Retarget without restarting the spring.
        let position = self
            .advance(maximum, now)
            .unwrap_or(current)
            .clamp(-maximum.max(0.), 0.);
        let motion = self.animation.get_or_insert(ScrollAnimation {
            position,
            velocity: 0.,
            target: position,
            updated: now,
            frequency,
            direction: delta.signum(),
        });
        if motion.direction != delta.signum() {
            // Cancel old momentum and queued distance on deliberate reversal.
            motion.velocity = 0.;
            motion.target = position;
        }
        motion.frequency = frequency;
        motion.direction = delta.signum();
        motion.target = (motion.target + delta).clamp(-maximum.max(0.), 0.);
        if motion.target == position && motion.velocity == 0. {
            self.cancel();
        }
    }

    fn advance(&mut self, maximum: f32, now: Instant) -> Option<f32> {
        let motion = self.animation.as_mut()?;
        let elapsed = now.saturating_duration_since(motion.updated).as_secs_f32();
        motion.updated = now;
        motion.target = motion.target.clamp(-maximum.max(0.), 0.);

        // Exact critically damped solution: continuous velocity and a gradual
        // stop, independent of display refresh rate, without oscillation.
        let omega = motion.frequency;
        let displacement = motion.position - motion.target;
        let coefficient = motion.velocity + omega * displacement;
        let decay = (-omega * elapsed).exp();
        let position = motion.target + (displacement + coefficient * elapsed) * decay;
        let velocity = (motion.velocity - omega * coefficient * elapsed) * decay;
        // Resizing or retargeting can leave excess velocity. Keep every step
        // between the previous position and the target so it cannot drift back.
        motion.position = position
            .clamp(
                motion.position.min(motion.target),
                motion.position.max(motion.target),
            )
            .clamp(-maximum.max(0.), 0.);
        motion.velocity = if position == motion.position {
            velocity
        } else {
            0.
        };

        let settled = (motion.position - motion.target).abs() < POSITION_EPSILON
            && motion.velocity.abs() < VELOCITY_EPSILON;
        let offset = if settled {
            motion.target
        } else {
            motion.position
        };
        if settled || maximum <= 0. {
            self.cancel();
        }
        Some(offset)
    }
}

impl PassApp {
    pub(super) fn advance_sidebar_scroll(&mut self, window: &mut Window) {
        if self.state.navigation.generator_open() || self.state.navigation.page() != Page::Entries {
            self.state.sidebar_scroll.cancel();
            return;
        }
        let handle = self.state.list_scroll.0.borrow().base_handle.clone();
        if let Some(y) = self
            .state
            .sidebar_scroll
            .advance(f32::from(handle.max_offset().height), Instant::now())
        {
            let mut offset = handle.offset();
            offset.y = px(y);
            handle.set_offset(offset);
            if self.state.sidebar_scroll.animation.is_some() {
                window.request_animation_frame();
            }
        }
    }

    pub(super) fn sidebar_scroll_input(&self, cx: &Context<Self>) -> impl IntoElement {
        let entity = cx.entity().downgrade();
        canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |_, hitbox, window, _| {
                window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture || !hitbox.should_handle_scroll(window) {
                        return;
                    }
                    let frequency = if event.delta.precise() {
                        TRACKPAD_FREQUENCY
                    } else {
                        WHEEL_FREQUENCY
                    };
                    let delta = event.delta.pixel_delta(window.line_height()).y;
                    if delta == px(0.) {
                        return;
                    }
                    let _ = entity.update(cx, |app, cx| {
                        let handle = app.state.list_scroll.0.borrow().base_handle.clone();
                        app.state.sidebar_scroll.scroll(
                            f32::from(handle.offset().y),
                            f32::from(delta),
                            f32::from(handle.max_offset().height),
                            frequency,
                            Instant::now(),
                        );
                        if app.state.sidebar_scroll.animation.is_some() {
                            cx.notify();
                        }
                    });
                    // Capture before UniformList applies the same wheel delta.
                    cx.stop_propagation();
                });
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settling_never_overshoots_or_reverses_in_either_direction() {
        for frequency in [WHEEL_FREQUENCY, TRACKPAD_FREQUENCY] {
            for delta in [-180., 180.] {
                let now = Instant::now();
                let mut scroll = SidebarScroll::default();
                let mut previous: f32 = -250.;
                let target = previous + delta;
                scroll.scroll(previous, delta, 1000., frequency, now);
                for frame in 1..=250 {
                    if let Some(offset) =
                        scroll.advance(1000., now + Duration::from_millis(frame * 8))
                    {
                        assert!(offset >= previous.min(target) && offset <= previous.max(target));
                        previous = offset;
                    }
                }
                assert_eq!(previous, target);
                assert!(scroll.animation.is_none());
            }
        }
    }

    #[test]
    fn spring_glides_and_stops_requesting_frames_when_settled() {
        let now = Instant::now();
        let mut scroll = SidebarScroll::default();
        scroll.scroll(0., -100., 500., WHEEL_FREQUENCY, now);
        let first = scroll
            .advance(500., now + Duration::from_millis(16))
            .unwrap();
        let next = scroll
            .advance(500., now + Duration::from_millis(32))
            .unwrap();
        assert!(first < 0. && next < first && next > -100.);
        assert_eq!(
            scroll.advance(500., now + Duration::from_secs(2)),
            Some(-100.)
        );
        assert!(scroll.animation.is_none());
        assert_eq!(scroll.advance(500., now + Duration::from_secs(3)), None);
    }

    #[test]
    fn repeated_input_preserves_position_and_velocity() {
        let now = Instant::now();
        let mut scroll = SidebarScroll::default();
        scroll.scroll(0., -100., 500., TRACKPAD_FREQUENCY, now);
        let later = now + Duration::from_millis(16);
        let current = scroll.advance(500., later).unwrap();
        let velocity = scroll.animation.as_ref().unwrap().velocity;
        scroll.scroll(current, -100., 500., TRACKPAD_FREQUENCY, later);
        let motion = scroll.animation.as_ref().unwrap();
        assert!((motion.position - current).abs() < 0.0001);
        assert!((motion.velocity - velocity).abs() < 0.0001);
        assert_eq!(motion.target, -200.);
    }

    #[test]
    fn motion_is_independent_of_frame_rate() {
        let now = Instant::now();
        let mut fast = SidebarScroll::default();
        let mut slow = SidebarScroll::default();
        for scroll in [&mut fast, &mut slow] {
            scroll.scroll(0., -100., 500., TRACKPAD_FREQUENCY, now);
        }
        for frame in 1..=8 {
            fast.advance(500., now + Duration::from_millis(frame * 8));
        }
        let slow_offset = slow.advance(500., now + Duration::from_millis(64)).unwrap();
        assert!((fast.animation.as_ref().unwrap().position - slow_offset).abs() < 0.001);
    }

    #[test]
    fn reversal_cancels_old_momentum_and_queued_distance() {
        let now = Instant::now();
        let mut scroll = SidebarScroll::default();
        scroll.scroll(-100., -100., 500., TRACKPAD_FREQUENCY, now);
        let later = now + Duration::from_millis(32);
        let current = scroll.advance(500., later).unwrap();
        scroll.scroll(current, 40., 500., TRACKPAD_FREQUENCY, later);
        let motion = scroll.animation.as_ref().unwrap();
        assert_eq!(motion.velocity, 0.);
        assert_eq!(motion.target, current + 40.);
        assert!(
            scroll
                .advance(500., later + Duration::from_millis(16))
                .unwrap()
                > current
        );
    }

    #[test]
    fn spring_and_resizing_never_scroll_past_the_list_edges() {
        let now = Instant::now();
        let mut scroll = SidebarScroll::default();
        scroll.scroll(-100., -1000., 300., WHEEL_FREQUENCY, now);
        for frame in 1..=100 {
            let offset = scroll.advance(300., now + Duration::from_millis(frame * 8));
            if let Some(offset) = offset {
                assert!((-300. ..=0.).contains(&offset));
            }
        }
        scroll.scroll(
            -300.,
            1000.,
            300.,
            WHEEL_FREQUENCY,
            now + Duration::from_secs(1),
        );
        let offset = scroll.advance(100., now + Duration::from_secs(1)).unwrap();
        assert!((-100. ..=0.).contains(&offset));
        assert_eq!(scroll.advance(100., now + Duration::from_secs(3)), Some(0.));
        scroll.scroll(0., -100., 0., WHEEL_FREQUENCY, now + Duration::from_secs(4));
        assert!(scroll.animation.is_none());
    }

    #[test]
    fn cancelling_prevents_motion_from_overriding_keyboard_or_search_scrolling() {
        let now = Instant::now();
        let mut scroll = SidebarScroll::default();
        scroll.scroll(0., -100., 500., WHEEL_FREQUENCY, now);
        scroll.cancel();
        assert_eq!(scroll.advance(500., now + Duration::from_millis(16)), None);
    }

    #[test]
    fn fractional_trackpad_and_momentum_events_preserve_total_distance() {
        let mut now = Instant::now();
        let mut scroll = SidebarScroll::default();
        let mut current = 0.;
        for delta in [-3.25, -2., -1.5, -0.75] {
            scroll.scroll(current, delta, 500., TRACKPAD_FREQUENCY, now);
            now += Duration::from_millis(16);
            let next = scroll.advance(500., now).unwrap();
            assert!(next < current);
            current = next;
        }
        assert_eq!(
            scroll.advance(500., now + Duration::from_secs(2)),
            Some(-7.5)
        );
        assert!(scroll.animation.is_none());
    }
}
