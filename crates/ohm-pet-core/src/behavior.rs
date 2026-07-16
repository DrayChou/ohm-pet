use crate::AnimationState;
use rand::Rng;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct BehaviorContext {
    pub idle_for: Duration,
    pub pointer_nearby: bool,
    pub recent_interactions: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BehaviorDecision {
    pub state: AnimationState,
    pub duration: Duration,
    pub next_thought_in: Duration,
}

#[derive(Default)]
pub struct BehaviorBrain {
    last_action: Option<AnimationState>,
}

impl BehaviorBrain {
    pub fn decide<R: Rng>(&mut self, context: BehaviorContext, rng: &mut R) -> BehaviorDecision {
        let state = if context.pointer_nearby && context.recent_interactions > 0 {
            if rng.random_bool(0.65) {
                AnimationState::Waving
            } else {
                AnimationState::Jumping
            }
        } else if context.idle_for >= Duration::from_secs(5 * 60) {
            AnimationState::Waiting
        } else {
            let roll = rng.random_range(0..100);
            match roll {
                0..=39 => AnimationState::Waving,
                40..=69 => AnimationState::Waiting,
                _ => AnimationState::Jumping,
            }
        };

        let state = if self.last_action == Some(state) {
            match state {
                AnimationState::Waving => AnimationState::Waiting,
                AnimationState::Waiting => AnimationState::Jumping,
                _ => AnimationState::Waving,
            }
        } else {
            state
        };
        self.last_action = Some(state);

        let duration = match state {
            AnimationState::Waving => Duration::from_millis(1_500),
            AnimationState::Jumping => Duration::from_millis(1_200),
            AnimationState::Waiting => Duration::from_millis(2_400),
            _ => Duration::from_millis(1_600),
        };
        let next_thought_in = if context.pointer_nearby {
            Duration::from_secs(rng.random_range(12..=20))
        } else if context.idle_for >= Duration::from_secs(5 * 60) {
            Duration::from_secs(rng.random_range(35..=60))
        } else {
            Duration::from_secs(rng.random_range(18..=38))
        };

        BehaviorDecision {
            state,
            duration,
            next_thought_in,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::StdRng, SeedableRng};

    #[test]
    fn long_idle_prefers_a_quiet_waiting_state() {
        let mut brain = BehaviorBrain::default();
        let mut rng = StdRng::seed_from_u64(7);
        let decision = brain.decide(
            BehaviorContext {
                idle_for: Duration::from_secs(600),
                pointer_nearby: false,
                recent_interactions: 0,
            },
            &mut rng,
        );
        assert_eq!(decision.state, AnimationState::Waiting);
        assert!(decision.next_thought_in >= Duration::from_secs(35));
    }

    #[test]
    fn avoids_repeating_the_same_autonomous_action() {
        let mut brain = BehaviorBrain::default();
        let mut rng = StdRng::seed_from_u64(1);
        let context = BehaviorContext {
            idle_for: Duration::from_secs(30),
            pointer_nearby: true,
            recent_interactions: 1,
        };
        let first = brain.decide(context, &mut rng);
        let second = brain.decide(context, &mut rng);
        assert_ne!(first.state, second.state);
    }
}
