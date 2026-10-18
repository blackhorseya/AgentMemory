//! Scheduling policies of the distillation pipeline. These are pure
//! decisions; timers and queues live in the application/infrastructure.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PipelineSettings {
    /// Run L1 after this many user rounds (after warm-up).
    pub every_n_conversations: u32,
    pub enable_warmup: bool,
    /// Debounce: run L1 when a session has been idle this long.
    pub l1_idle_timeout_secs: u64,
    pub l2_delay_after_l1_secs: u64,
    pub l2_min_interval_secs: u64,
    pub l2_max_interval_secs: u64,
    /// Regenerate the persona after this many new memories.
    pub persona_trigger_every_n: u32,
    pub max_scenes: usize,
    pub max_memories_per_session: usize,
}

impl Default for PipelineSettings {
    fn default() -> Self {
        Self {
            every_n_conversations: 5,
            enable_warmup: true,
            l1_idle_timeout_secs: 600,
            l2_delay_after_l1_secs: 90,
            l2_min_interval_secs: 900,
            l2_max_interval_secs: 3600,
            persona_trigger_every_n: 50,
            max_scenes: 15,
            max_memories_per_session: 20,
        }
    }
}

/// Warm-up makes a brand-new session distil quickly (after 1, 2, 4 … rounds)
/// before settling on `every_n`. A `warmup` of 0 means "graduated".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Warmup(pub u32);

impl Warmup {
    pub fn initial(settings: &PipelineSettings) -> Self {
        if settings.enable_warmup && settings.every_n_conversations > 1 {
            Self(1)
        } else {
            Self(0)
        }
    }

    pub fn threshold(self, every_n: u32) -> u32 {
        if self.0 == 0 {
            every_n.max(1)
        } else {
            self.0.min(every_n).max(1)
        }
    }

    pub fn advance(self, every_n: u32) -> Self {
        if self.0 == 0 {
            return self;
        }
        let next = self.0 * 2;
        if next >= every_n {
            Self(0)
        } else {
            Self(next)
        }
    }
}

/// When the next L2 run may happen after an L1 run produced memories:
/// soon (`delay`) but never earlier than `min_interval` after the last L2.
pub fn next_l2_due(now_secs: i64, last_l2_secs: Option<i64>, s: &PipelineSettings) -> i64 {
    let soon = now_secs + s.l2_delay_after_l1_secs as i64;
    match last_l2_secs {
        Some(last) => soon.max(last + s.l2_min_interval_secs as i64),
        None => soon,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonaState {
    pub has_persona: bool,
    pub scene_count: usize,
    pub scenes_processed: u32,
    pub memories_since_last_persona: u32,
    pub update_requested: Option<String>,
}

/// Reasons, in priority order, that the L3 persona should be regenerated.
pub fn persona_trigger(state: &PersonaState, every_n: u32) -> Option<String> {
    if state.scene_count == 0 {
        return None;
    }
    if let Some(reason) = &state.update_requested {
        return Some(format!("requested: {reason}"));
    }
    if !state.has_persona {
        return Some("cold_start".into());
    }
    if state.scenes_processed == 1 && state.memories_since_last_persona > 0 {
        return Some("first_scene_batch".into());
    }
    if state.memories_since_last_persona >= every_n {
        return Some(format!(
            "{} new memories",
            state.memories_since_last_persona
        ));
    }
    None
}

/// Tiered guidance for the scene extractor as the scene cap approaches.
pub fn scene_capacity_guidance(current: usize, max: usize) -> &'static str {
    if current >= max {
        "Scene limit reached: you MUST merge 2-4 related scenes before anything else; do not create new scenes."
    } else if current + 1 == max {
        "One slot left: only UPDATE existing scenes."
    } else if current + 3 >= max {
        "Approaching the scene limit: prefer UPDATE or MERGE over CREATE."
    } else {
        "Capacity available: prefer UPDATE; CREATE at most one new scene."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warmup_sequence_is_1_2_4_then_every_n() {
        let s = PipelineSettings::default();
        let mut w = Warmup::initial(&s);
        let mut thresholds = vec![];
        for _ in 0..5 {
            thresholds.push(w.threshold(s.every_n_conversations));
            w = w.advance(s.every_n_conversations);
        }
        assert_eq!(thresholds, [1, 2, 4, 5, 5]);
    }

    #[test]
    fn l2_respects_min_interval() {
        let s = PipelineSettings::default();
        assert_eq!(next_l2_due(1000, None, &s), 1090);
        assert_eq!(next_l2_due(1000, Some(500), &s), 1400);
    }

    #[test]
    fn persona_triggers_in_priority_order() {
        let mut st = PersonaState {
            scene_count: 2,
            scenes_processed: 3,
            ..Default::default()
        };
        assert_eq!(persona_trigger(&st, 50).as_deref(), Some("cold_start"));
        st.has_persona = true;
        assert_eq!(persona_trigger(&st, 50), None);
        st.memories_since_last_persona = 50;
        assert!(persona_trigger(&st, 50).is_some());
        st.scene_count = 0;
        assert_eq!(persona_trigger(&st, 50), None);
    }
}
