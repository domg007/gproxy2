//! Provider rule processing (§6.1): applies §8-B2 rule-set mutations to the
//! provider-native request, after transform and before the channel. Fixed kind
//! order: prelude → cache_breakpoint → rewrite → sanitize → beta_header.

mod compile;

pub use compile::{
    CacheBreakpointCfg, CompiledRule, RewriteAction, RuleConfig, compile_rules, order_for_apply,
};
