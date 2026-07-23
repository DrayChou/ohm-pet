use crate::agent_ipc::{AgentEvent, AgentSignal};
use ohm_pet_core::AnimationState;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

const COMPLETED_RETENTION: Duration = Duration::from_secs(10);
const STALE_TASK_TIMEOUT: Duration = Duration::from_secs(12 * 60 * 60);

#[derive(Debug, Clone)]
pub struct TrackedTask {
    pub source: String,
    pub title: String,
    pub event: AgentEvent,
    pub started_at: Instant,
    pub updated_at: Instant,
    pub finished_at: Option<Instant>,
}

impl TrackedTask {
    pub fn elapsed(&self, now: Instant) -> Duration {
        self.finished_at
            .unwrap_or(now)
            .saturating_duration_since(self.started_at)
    }

    pub fn display_line(&self, now: Instant, show_source: bool) -> String {
        let icon = match self.event {
            AgentEvent::Working => "●",
            AgentEvent::Waiting => "?",
            AgentEvent::Completed => "✓",
            AgentEvent::Failed => "!",
            AgentEvent::Idle => "○",
        };
        let source = if show_source {
            format!(" [{}]", display_source(&self.source))
        } else {
            String::new()
        };
        format!(
            "{icon}{source} {}  {}",
            truncate_title(&self.title, 38),
            format_duration(self.elapsed(now))
        )
    }
}

#[derive(Debug, Default)]
pub struct TaskUpdate {
    pub changed: bool,
    pub finished: Option<TrackedTask>,
}

#[derive(Default)]
pub struct TaskTracker {
    tasks: HashMap<String, TrackedTask>,
}

impl TaskTracker {
    pub fn apply(&mut self, signal: &AgentSignal, now: Instant) -> TaskUpdate {
        let key = signal.task_key();
        if signal.event == AgentEvent::Idle {
            return TaskUpdate {
                changed: self.tasks.remove(&key).is_some(),
                finished: None,
            };
        }
        let fallback_title = format!("{} task", display_source(&signal.source));
        let title = signal
            .title
            .as_deref()
            .map(clean_title)
            .filter(|value| !value.is_empty())
            .unwrap_or(fallback_title);
        let is_new = !self.tasks.contains_key(&key);
        let task = self.tasks.entry(key).or_insert_with(|| TrackedTask {
            source: signal.source.clone(),
            title: title.clone(),
            event: signal.event,
            started_at: now,
            updated_at: now,
            finished_at: None,
        });
        let was_finished = task.finished_at.is_some();
        let title_changed = signal.title.is_some() && task.title != title;
        let event_changed = task.event != signal.event;
        if was_finished && signal.event == AgentEvent::Working {
            task.started_at = now;
        }
        if signal.title.is_some() {
            task.title = title;
        }
        task.event = signal.event;
        task.updated_at = now;
        task.finished_at =
            matches!(signal.event, AgentEvent::Completed | AgentEvent::Failed).then_some(now);
        TaskUpdate {
            changed: is_new || title_changed || event_changed,
            finished: (!was_finished
                && matches!(signal.event, AgentEvent::Completed | AgentEvent::Failed))
            .then(|| task.clone()),
        }
    }

    pub fn prune(&mut self, now: Instant) -> bool {
        let before = self.tasks.len();
        self.tasks.retain(|_, task| {
            if let Some(finished) = task.finished_at {
                now.saturating_duration_since(finished) < COMPLETED_RETENTION
            } else {
                now.saturating_duration_since(task.updated_at) < STALE_TASK_TIMEOUT
            }
        });
        before != self.tasks.len()
    }

    pub fn animation_state(&self) -> AnimationState {
        if self
            .tasks
            .values()
            .any(|task| task.event == AgentEvent::Waiting)
        {
            AnimationState::Waiting
        } else if self
            .tasks
            .values()
            .any(|task| task.event == AgentEvent::Working)
        {
            AnimationState::Running
        } else if self
            .tasks
            .values()
            .any(|task| task.event == AgentEvent::Failed)
        {
            AnimationState::Failed
        } else if self
            .tasks
            .values()
            .any(|task| task.event == AgentEvent::Completed)
        {
            AnimationState::Review
        } else {
            AnimationState::Idle
        }
    }

    pub fn display_lines(&self, now: Instant, limit: usize) -> Vec<String> {
        let mut tasks: Vec<_> = self.tasks.values().collect();
        tasks.sort_by_key(|task| {
            (
                event_priority(task.event),
                std::cmp::Reverse(task.updated_at),
            )
        });
        let source_count = tasks
            .iter()
            .map(|task| task.source.as_str())
            .collect::<std::collections::HashSet<_>>()
            .len();
        let mut lines: Vec<_> = tasks
            .iter()
            .take(limit)
            .map(|task| task.display_line(now, source_count > 1))
            .collect();
        if tasks.len() > limit {
            lines.push(format!("+{} more tasks", tasks.len() - limit));
        }
        lines
    }
}

fn event_priority(event: AgentEvent) -> u8 {
    match event {
        AgentEvent::Waiting => 0,
        AgentEvent::Working => 1,
        AgentEvent::Failed => 2,
        AgentEvent::Completed => 3,
        AgentEvent::Idle => 4,
    }
}

fn display_source(source: &str) -> &str {
    match source {
        "pi" => "Pi",
        "claude" => "Claude",
        "codex" => "Codex",
        value => value,
    }
}

fn clean_title(value: &str) -> String {
    value
        .lines()
        .next()
        .unwrap_or(value)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn truncate_title(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    let mut result: String = value.chars().take(max_chars.saturating_sub(1)).collect();
    result.push('…');
    result
}

pub fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds < 60 {
        format!("{seconds:02}s")
    } else if seconds < 60 * 60 {
        format!("{:02}:{:02}", seconds / 60, seconds % 60)
    } else {
        format!("{:02}:{:02}", seconds / 3600, (seconds % 3600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signal(session: &str, event: AgentEvent, title: &str) -> AgentSignal {
        AgentSignal {
            source: "pi".into(),
            event,
            title: Some(title.into()),
            session_id: Some(session.into()),
            task_id: None,
        }
    }

    #[test]
    fn keeps_concurrent_sessions_separate() {
        let now = Instant::now();
        let mut tracker = TaskTracker::default();
        tracker.apply(&signal("a", AgentEvent::Working, "First"), now);
        tracker.apply(&signal("b", AgentEvent::Working, "Second"), now);
        tracker.apply(&signal("a", AgentEvent::Completed, "First"), now);
        assert_eq!(tracker.animation_state(), AnimationState::Running);
        assert_eq!(tracker.display_lines(now, 5).len(), 2);
    }

    #[test]
    fn freezes_duration_when_finished() {
        let now = Instant::now();
        let mut tracker = TaskTracker::default();
        tracker.apply(&signal("a", AgentEvent::Working, "First"), now);
        let finished = now + Duration::from_secs(65);
        tracker.apply(&signal("a", AgentEvent::Completed, "First"), finished);
        let line = tracker
            .display_lines(finished + Duration::from_secs(30), 5)
            .remove(0);
        assert!(line.ends_with("01:05"));
    }
}
