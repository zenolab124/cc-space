use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
}

impl TokenUsage {
    pub fn total(&self) -> u64 {
        self.input_tokens
            + self.output_tokens
            + self.cache_creation_input_tokens
            + self.cache_read_input_tokens
    }

    pub fn is_reported(&self) -> bool {
        self.total() > 0
    }

    pub fn accumulate(&mut self, other: &TokenUsage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_creation_input_tokens += other.cache_creation_input_tokens;
        self.cache_read_input_tokens += other.cache_read_input_tokens;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UsageSnapshot {
    pub usage: TokenUsage,
    pub terminal: bool,
    pub timestamp: Option<String>,
    pub sequence: u64,
}

impl UsageSnapshot {
    pub fn new(
        usage: TokenUsage,
        stop_reason: Option<&str>,
        timestamp: Option<String>,
        sequence: u64,
    ) -> Self {
        Self {
            usage,
            terminal: stop_reason.is_some_and(|reason| !reason.is_empty()),
            timestamp,
            sequence,
        }
    }

    fn quality(&self) -> (bool, bool) {
        (self.usage.is_reported(), self.terminal)
    }

    pub fn is_better_than(&self, current: &Self) -> bool {
        match self.quality().cmp(&current.quality()) {
            Ordering::Greater => true,
            Ordering::Less => false,
            Ordering::Equal => match self.timestamp.cmp(&current.timestamp) {
                Ordering::Greater => true,
                Ordering::Less => false,
                Ordering::Equal => match (
                    self.usage.total(),
                    self.usage.input_tokens,
                    self.usage.output_tokens,
                    self.usage.cache_creation_input_tokens,
                    self.usage.cache_read_input_tokens,
                )
                    .cmp(&(
                        current.usage.total(),
                        current.usage.input_tokens,
                        current.usage.output_tokens,
                        current.usage.cache_creation_input_tokens,
                        current.usage.cache_read_input_tokens,
                    )) {
                    Ordering::Greater => true,
                    Ordering::Less => false,
                    Ordering::Equal => self.sequence > current.sequence,
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(total: u64) -> TokenUsage {
        TokenUsage {
            input_tokens: total,
            ..Default::default()
        }
    }

    #[test]
    fn snapshot_prefers_reported_terminal_usage() {
        let zero = UsageSnapshot::new(usage(0), None, Some("2026-08-01T10:00:00Z".into()), 1);
        let partial = UsageSnapshot::new(usage(10), None, Some("2026-08-01T10:00:01Z".into()), 2);
        let terminal = UsageSnapshot::new(
            usage(20),
            Some("tool_use"),
            Some("2026-08-01T10:00:02Z".into()),
            3,
        );

        assert!(partial.is_better_than(&zero));
        assert!(terminal.is_better_than(&partial));
        assert!(!zero.is_better_than(&terminal));
    }

    #[test]
    fn snapshot_never_replaces_reported_usage_with_zero() {
        let reported = UsageSnapshot::new(usage(10), None, None, 1);
        let later_zero = UsageSnapshot::new(usage(0), Some("end_turn"), None, 2);

        assert!(!later_zero.is_better_than(&reported));
    }

    #[test]
    fn empty_stop_reason_is_not_terminal() {
        let snapshot = UsageSnapshot::new(usage(10), Some(""), None, 1);

        assert!(!snapshot.terminal);
    }
}
