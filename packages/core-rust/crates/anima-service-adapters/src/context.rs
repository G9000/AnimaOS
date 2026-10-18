//! Context costs for content projected by the service adapters.

use anima_core::{Content, ContextTokenEstimator, Message, TokenEstimator};

use crate::document_content::document_rendered_bytes;

/// Ordinary engine costs plus the exact derived document text sent by the
/// attachment adapter. Document bytes are never discounted by calibration.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ServiceTokenEstimator {
    ordinary: TokenEstimator,
}

impl ServiceTokenEstimator {
    pub fn new(calibration: f64) -> Self {
        Self {
            ordinary: TokenEstimator::new(calibration),
        }
    }

    pub fn calibration(&self) -> f64 {
        self.ordinary.calibration()
    }

    pub fn raw_message_tokens(message: &Message) -> u64 {
        TokenEstimator::raw_message_tokens(message)
            .saturating_add(document_rendered_bytes(&message.content) as u64)
    }

    pub fn content_tokens(&self, content: &Content) -> u64 {
        let derived =
            (document_rendered_bytes(content) as f64 * self.calibration().max(1.0)).ceil() as u64;
        self.ordinary
            .content_tokens(content)
            .saturating_add(derived)
    }

    pub fn message_tokens(&self, message: &Message) -> u64 {
        self.content_tokens(&message.content)
    }

    pub fn text_tokens(&self, text: &str) -> u64 {
        self.ordinary.text_tokens(text)
    }
}

impl ContextTokenEstimator for ServiceTokenEstimator {
    fn message_tokens(&self, message: &Message) -> u64 {
        ServiceTokenEstimator::message_tokens(self, message)
    }

    fn text_tokens(&self, text: &str) -> u64 {
        ServiceTokenEstimator::text_tokens(self, text)
    }
}
