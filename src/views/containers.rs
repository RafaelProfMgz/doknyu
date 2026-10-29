//! Container list. Only the shared state types exist here; the list itself arrives in task 2.3.

use std::collections::HashSet;

use crate::model::ContainerInfo;

#[derive(Debug, Clone, PartialEq)]
pub struct ContainersState {
    pub items: Vec<ContainerInfo>,
    pub query: String,
    pub show_stopped: bool,
    pub pending: HashSet<String>,
    pub confirm_remove: Option<String>,
}

impl Default for ContainersState {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            query: String::new(),
            show_stopped: true,
            pending: HashSet::new(),
            confirm_remove: None,
        }
    }
}

/// Messages of the container list (filled in by task 2.3).
#[derive(Debug, Clone)]
pub enum Msg {}
