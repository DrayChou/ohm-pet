use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnimationState {
    Idle,
    RunningRight,
    RunningLeft,
    Waving,
    Jumping,
    Failed,
    Waiting,
    Running,
    Review,
}

impl AnimationState {
    pub fn from_cli(value: &str) -> Option<Self> {
        match value {
            "idle" => Some(Self::Idle),
            "running-right" | "runningRight" => Some(Self::RunningRight),
            "running-left" | "runningLeft" => Some(Self::RunningLeft),
            "waving" | "wave" => Some(Self::Waving),
            "jumping" | "jump" => Some(Self::Jumping),
            "failed" => Some(Self::Failed),
            "waiting" => Some(Self::Waiting),
            "running" => Some(Self::Running),
            "review" | "ready" => Some(Self::Review),
            _ => None,
        }
    }

    fn row(self) -> u32 {
        match self {
            Self::Idle => 0,
            Self::RunningRight => 1,
            Self::RunningLeft => 2,
            Self::Waving => 3,
            Self::Jumping => 4,
            Self::Failed => 5,
            Self::Waiting => 6,
            Self::Running => 7,
            Self::Review => 8,
        }
    }

    fn frame_count(self) -> u32 {
        match self {
            Self::Idle => 6,
            Self::RunningRight | Self::RunningLeft | Self::Failed => 8,
            Self::Waving => 4,
            Self::Jumping => 5,
            Self::Waiting | Self::Running | Self::Review => 6,
        }
    }

    pub fn frame_interval(self) -> Duration {
        match self {
            Self::Idle => Duration::from_millis(480),
            Self::Waiting => Duration::from_millis(260),
            _ => Duration::from_millis(145),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameCoordinates {
    pub row: u32,
    pub column: u32,
}

pub fn direction_from_vector(x: f64, y: f64) -> u8 {
    let clockwise_from_up = x.atan2(-y).to_degrees().rem_euclid(360.0);
    ((clockwise_from_up / 22.5).round() as i32).rem_euclid(16) as u8
}

pub fn frame_coordinates(
    state: AnimationState,
    frame: u32,
    direction: Option<u8>,
) -> FrameCoordinates {
    if state == AnimationState::Idle {
        if let Some(direction) = direction {
            let normalized = direction % 16;
            return FrameCoordinates {
                row: if normalized < 8 { 9 } else { 10 },
                column: u32::from(normalized % 8),
            };
        }
    }
    FrameCoordinates {
        row: state.row(),
        column: frame % state.frame_count(),
    }
}

pub struct StateMachine {
    state: AnimationState,
    frame: u32,
    direction: Option<u8>,
    next_frame_at: Instant,
    temporary_until: Option<Instant>,
}

impl StateMachine {
    pub fn new(now: Instant) -> Self {
        Self {
            state: AnimationState::Idle,
            frame: 0,
            direction: None,
            next_frame_at: now + AnimationState::Idle.frame_interval(),
            temporary_until: None,
        }
    }

    pub fn state(&self) -> AnimationState {
        self.state
    }

    pub fn coordinates(&self) -> FrameCoordinates {
        frame_coordinates(self.state, self.frame, self.direction)
    }

    pub fn set_state(&mut self, state: AnimationState, now: Instant, duration: Option<Duration>) {
        self.state = state;
        self.frame = 0;
        self.direction = None;
        self.next_frame_at = now + state.frame_interval();
        self.temporary_until = duration.map(|value| now + value);
    }

    pub fn set_direction(&mut self, direction: Option<u8>) -> bool {
        if self.state != AnimationState::Idle {
            return false;
        }
        let direction = direction.map(|value| value % 16);
        if self.direction == direction {
            return false;
        }
        self.direction = direction;
        true
    }

    pub fn tick(&mut self, now: Instant) -> bool {
        if self.temporary_until.is_some_and(|deadline| now >= deadline) {
            self.set_state(AnimationState::Idle, now, None);
            return true;
        }
        if self.direction.is_none() && now >= self.next_frame_at {
            self.frame = (self.frame + 1) % self.state.frame_count();
            self.next_frame_at = now + self.state.frame_interval();
            return true;
        }
        false
    }

    pub fn next_deadline(&self) -> Instant {
        if self.direction.is_some() {
            return self
                .temporary_until
                .unwrap_or_else(|| Instant::now() + Duration::from_secs(24 * 60 * 60));
        }
        self.temporary_until
            .map_or(self.next_frame_at, |temporary| {
                temporary.min(self.next_frame_at)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_screen_vectors_clockwise_from_up() {
        assert_eq!(direction_from_vector(0.0, -10.0), 0);
        assert_eq!(direction_from_vector(10.0, -10.0), 2);
        assert_eq!(direction_from_vector(10.0, 0.0), 4);
        assert_eq!(direction_from_vector(10.0, 10.0), 6);
        assert_eq!(direction_from_vector(0.0, 10.0), 8);
        assert_eq!(direction_from_vector(-10.0, 10.0), 10);
        assert_eq!(direction_from_vector(-10.0, 0.0), 12);
        assert_eq!(direction_from_vector(-10.0, -10.0), 14);
    }

    #[test]
    fn maps_direction_rows() {
        assert_eq!(
            frame_coordinates(AnimationState::Idle, 0, Some(7)),
            FrameCoordinates { row: 9, column: 7 }
        );
        assert_eq!(
            frame_coordinates(AnimationState::Idle, 0, Some(8)),
            FrameCoordinates { row: 10, column: 0 }
        );
        assert_eq!(
            frame_coordinates(AnimationState::Idle, 0, Some(15)),
            FrameCoordinates { row: 10, column: 7 }
        );
    }

    #[test]
    fn never_selects_transparent_padding_frames() {
        assert_eq!(frame_coordinates(AnimationState::Waving, 7, None).column, 3);
        assert_eq!(
            frame_coordinates(AnimationState::Jumping, 7, None).column,
            2
        );
        assert_eq!(
            frame_coordinates(AnimationState::Waiting, 7, None).column,
            1
        );
        assert_eq!(
            frame_coordinates(AnimationState::Running, 7, None).column,
            1
        );
        assert_eq!(frame_coordinates(AnimationState::Review, 7, None).column, 1);
        assert_eq!(frame_coordinates(AnimationState::Failed, 7, None).column, 7);
    }

    #[test]
    fn returns_to_idle_after_temporary_state() {
        let now = Instant::now();
        let mut machine = StateMachine::new(now);
        machine.set_state(
            AnimationState::Jumping,
            now,
            Some(Duration::from_millis(10)),
        );
        assert!(machine.tick(now + Duration::from_millis(11)));
        assert_eq!(machine.state(), AnimationState::Idle);
    }
}
