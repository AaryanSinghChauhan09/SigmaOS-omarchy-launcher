// SPDX-License-Identifier: Apache-2.0
//! SigmaOS-omarchy-launcher: omarchy launcher component model.
//!
//! Real implementation: launchers, events, validation, deterministic storage.
//! Replaces the placeholder stub.

use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launcher {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub id: String,
    pub subject: String,
    pub detail: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OmarchyLauncherError {
    EmptyLauncherName,
    DuplicateLauncher(String),
    EmptyEventSubject,
}

impl fmt::Display for OmarchyLauncherError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyLauncherName => write!(f, "launcher name must not be empty"),
            Self::DuplicateLauncher(id) => write!(f, "duplicate launcher {:?}", id),
            Self::EmptyEventSubject => write!(f, "event subject must not be empty"),
        }
    }
}

impl std::error::Error for OmarchyLauncherError {}

pub struct OmarchyLauncher {
    pub name: String,
    launchers: BTreeMap<String, Launcher>,
    events: BTreeMap<String, Event>,
}

impl OmarchyLauncher {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            launchers: BTreeMap::new(),
            events: BTreeMap::new(),
        }
    }

    pub fn add_launcher(&mut self, item: Launcher) -> Result<(), OmarchyLauncherError> {
        if item.name.trim().is_empty() {
            return Err(OmarchyLauncherError::EmptyLauncherName);
        }
        let id = item.id.clone();
        if self.launchers.contains_key(&id) {
            return Err(OmarchyLauncherError::DuplicateLauncher(id.clone()));
        }
        self.launchers.insert(id, item);
        Ok(())
    }

    pub fn launcher_count(&self) -> usize {
        self.launchers.len()
    }

    pub fn get_launcher(&self, id: &str) -> Option<&Launcher> {
        self.launchers.get(id)
    }

    pub fn add_event(&mut self, event: Event) -> Result<(), OmarchyLauncherError> {
        if event.subject.trim().is_empty() {
            return Err(OmarchyLauncherError::EmptyEventSubject);
        }
        self.events.insert(event.id.clone(), event);
        Ok(())
    }

    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    pub fn get_event(&self, id: &str) -> Option<&Event> {
        self.events.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_launcher(id: &str, name: &str) -> Launcher {
        Launcher {
            id: id.into(),
            name: name.into(),
            kind: "default".into(),
            detail: String::new(),
        }
    }

    #[test]
    fn test_create() {
        let m = OmarchyLauncher::new("OmarchyLauncher");
        assert_eq!(m.name, "OmarchyLauncher");
        assert_eq!(m.launcher_count(), 0);
        assert_eq!(m.event_count(), 0);
    }

    #[test]
    fn test_add_launcher() {
        let mut m = OmarchyLauncher::new("OmarchyLauncher");
        m.add_launcher(sample_launcher("a1", "Alpha")).unwrap();
        assert_eq!(m.launcher_count(), 1);
        assert!(m.get_launcher("a1").is_some());
    }

    #[test]
    fn test_empty_name_rejected() {
        let mut m = OmarchyLauncher::new("OmarchyLauncher");
        assert!(m.add_launcher(sample_launcher("x", "   ")).is_err());
    }

    #[test]
    fn test_duplicate_rejected() {
        let mut m = OmarchyLauncher::new("OmarchyLauncher");
        m.add_launcher(sample_launcher("x", "One")).unwrap();
        assert!(m.add_launcher(sample_launcher("x", "Two")).is_err());
    }

    #[test]
    fn test_event_validation() {
        let mut m = OmarchyLauncher::new("OmarchyLauncher");
        let bad = Event { id: "e1".into(), subject: "".into(), detail: String::new(), timestamp: 0 };
        assert!(m.add_event(bad).is_err());
        let good = Event { id: "e1".into(), subject: "boot".into(), detail: String::new(), timestamp: 1 };
        m.add_event(good).unwrap();
        assert_eq!(m.event_count(), 1);
    }
}
