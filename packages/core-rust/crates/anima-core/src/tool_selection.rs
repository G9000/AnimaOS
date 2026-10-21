//! Host-owned selection of descriptors advertised to the next model request.
//! Selection never authorizes execution or modifies canonical tool descriptors.
use std::collections::BTreeSet;

pub trait ModelToolSelector: Send + Sync {
    fn visible_tool_names(&self) -> BTreeSet<String>;
}
