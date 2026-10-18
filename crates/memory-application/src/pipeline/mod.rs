//! The asynchronous distillation pipeline L0 → L1 → L2 → L3.

pub mod generation_logs;
pub mod l1;
pub mod l2;
pub mod l3;
pub mod llm_json;
pub mod memory_prompts;
pub mod metrics;
pub mod prompts;
pub mod service;
