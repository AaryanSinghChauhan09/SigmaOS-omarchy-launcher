// SPDX-License-Identifier: Apache-2.0
//! SigmaOS-omarchy-launcher — rofi/walker-style application launcher.
//!
//! Parses `.desktop` entries, fuzzy-searches them, tracks recency/
//! favorites, and produces the exec line to launch. Deterministic core;
//! a Wayland layer-shell frontend can bind to the same API.

use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone)]
pub struct DesktopEntry {
    pub id: String,               // desktop-file id, e.g. "org.gnome.Nautilus.desktop"
    pub name: String,
    pub generic_name: Option<String>,
    pub comment: Option<String>,
    pub exec: String,             // command line (may contain %U/%f)
    pub icon: Option<String>,
    pub categories: Vec<String>,
    pub keywords: Vec<String>,
    pub terminal: bool,
    pub no_display: bool,
    pub hidden: bool,
    pub startup_notify: bool,
    pub path: String,             // source .desktop file path
}

impl DesktopEntry {
    /// Parse the subset of the Desktop Entry spec we need.
    pub fn parse(id: impl Into<String>, path: impl Into<String>, text: &str) -> Result<Self, LaunchError> {
        let id = id.into();
        let mut e = DesktopEntry {
            id: id.clone(), name: String::new(), generic_name: None, comment: None,
            exec: String::new(), icon: None, categories: Vec::new(), keywords: Vec::new(),
            terminal: false, no_display: false, hidden: false, startup_notify: false,
            path: path.into(),
        };
        let mut in_entry = false;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') { continue; }
            if line.starts_with('[') {
                in_entry = line == "[Desktop Entry]";
                continue;
            }
            if !in_entry { continue; }
            let Some((k, v)) = line.split_once('=') else { continue; };
            let k = k.trim(); let v = v.trim();
            match k {
                "Name" => e.name = v.to_string(),
                "GenericName" => e.generic_name = Some(v.to_string()),
                "Comment" => e.comment = Some(v.to_string()),
                "Exec" => e.exec = v.to_string(),
                "Icon" => e.icon = Some(v.to_string()),
                "Path" => {} // already have path field for source; ignore Exec working dir
                "Categories" => e.categories = v.split(';').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect(),
                "Keywords" => e.keywords = v.split(';').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect(),
                "Terminal" => e.terminal = v.eq_ignore_ascii_case("true"),
                "NoDisplay" => e.no_display = v.eq_ignore_ascii_case("true"),
                "Hidden" => e.hidden = v.eq_ignore_ascii_case("true"),
                "StartupNotify" => e.startup_notify = v.eq_ignore_ascii_case("true"),
                _ => {}
            }
        }
        if e.name.is_empty() { return Err(LaunchError::BadDesktopFile(id)); }
        if e.exec.is_empty() { return Err(LaunchError::NoExec(id)); }
        Ok(e)
    }

    /// Fields searchable by the fuzzy matcher.
    fn haystack(&self) -> String {
        let mut s = format!("{} {} {}", self.name, self.generic_name.clone().unwrap_or_default(), self.exec);
        s.push(' ');
        s.push_str(&self.keywords.join(" "));
        s.push(' ');
        s.push_str(&self.categories.join(" "));
        s
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchError {
    BadDesktopFile(String),
    NoExec(String),
    Duplicate(String),
    EmptyQuery,
    NoSuchApp(String),
}
impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadDesktopFile(id) => write!(f, "bad desktop file {id}"),
            Self::NoExec(id) => write!(f, "no Exec in {id}"),
            Self::Duplicate(id) => write!(f, "duplicate app {id}"),
            Self::EmptyQuery => write!(f, "query must not be empty"),
            Self::NoSuchApp(id) => write!(f, "no such app {id}"),
        }
    }
}
impl std::error::Error for LaunchError {}

#[derive(Debug, Clone)]
struct LaunchRecord { id: String, at: u64 }

pub struct OmarchyLauncher {
    apps: BTreeMap<String, DesktopEntry>,
    favorites: Vec<String>,
    recent: Vec<LaunchRecord>,
    clock: u64,
}

/// Subsequence fuzzy score: higher = better. Returns None if not a match.
fn fuzzy_score(query: &str, target: &str) -> Option<i32> {
    let q: Vec<char> = query.chars().flat_map(|c| c.to_lowercase()).collect();
    let t: Vec<char> = target.chars().flat_map(|c| c.to_lowercase()).collect();
    if q.is_empty() { return None; }
    let mut qi = 0usize;
    let mut score = 0i32;
    let mut last = usize::MAX;
    let mut streak = 0i32;
    for (ti, &tc) in t.iter().enumerate() {
        if qi < q.len() && tc == q[qi] {
            // consecutive bonus, word-start bonus, earlier-match bonus
            if last != usize::MAX && ti == last + 1 { streak += 1; score += 5 + streak * 3; }
            else { streak = 0; score += 10; }
            if ti == 0 || t[ti - 1] == ' ' || t[ti - 1] == '-' || t[ti - 1] == '_' { score += 8; }
            score += (t.len().saturating_sub(ti)) as i32 / 50;
            last = ti;
            qi += 1;
        }
    }
    if qi == q.len() { Some(score) } else { None }
}

impl OmarchyLauncher {
    pub fn new() -> Self {
        Self { apps: BTreeMap::new(), favorites: Vec::new(), recent: Vec::new(), clock: 0 }
    }

    /// Register a parsed desktop entry. Skips Hidden/NoDisplay unless forced.
    pub fn register(&mut self, e: DesktopEntry, include_hidden: bool) -> Result<(), LaunchError> {
        if e.id.trim().is_empty() { return Err(LaunchError::Duplicate(String::new())); }
        if self.apps.contains_key(&e.id) { return Err(LaunchError::Duplicate(e.id)); }
        if (e.hidden || e.no_display) && !include_hidden { return Ok(()); }
        self.apps.insert(e.id.clone(), e);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&DesktopEntry> { self.apps.get(id) }
    pub fn apps(&self) -> impl Iterator<Item = &DesktopEntry> { self.apps.values() }
    pub fn count(&self) -> usize { self.apps.len() }

    /// Fuzzy search -> ranked (score, entry). Favorites/recent get a boost.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<(i32, &DesktopEntry)>, LaunchError> {
        if query.trim().is_empty() { return Err(LaunchError::EmptyQuery); }
        let mut hits: Vec<(i32, &DesktopEntry)> = Vec::new();
        for e in self.apps.values() {
            let target = e.haystack();
            if let Some(mut score) = fuzzy_score(query, &target) {
                if self.favorites.contains(&e.id) { score += 1000; }
                // recency boost: most-recent gets up to +200 decaying
                if let Some(pos) = self.recent.iter().position(|r| r.id == e.id) {
                    score += 200 - (pos as i32 * 20).min(200);
                }
                hits.push((score, e));
            }
        }
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.cmp(&b.1.name)));
        hits.truncate(limit);
        Ok(hits)
    }

    /// Launch an app: returns its exec line and records recency.
    pub fn launch(&mut self, id: &str) -> Result<String, LaunchError> {
        let e = self.apps.get(id).ok_or_else(|| LaunchError::NoSuchApp(id.into()))?;
        self.clock += 1;
        self.recent.retain(|r| r.id != id);
        self.recent.insert(0, LaunchRecord { id: id.into(), at: self.clock });
        if self.recent.len() > 50 { self.recent.truncate(50); }
        Ok(e.exec.clone())
    }

    pub fn toggle_favorite(&mut self, id: &str) -> Result<bool, LaunchError> {
        if !self.apps.contains_key(id) { return Err(LaunchError::NoSuchApp(id.into())); }
        if let Some(pos) = self.favorites.iter().position(|f| f == id) {
            self.favorites.remove(pos);
            Ok(false)
        } else {
            self.favorites.push(id.into());
            Ok(true)
        }
    }
    pub fn is_favorite(&self, id: &str) -> bool { self.favorites.contains(&id.to_string()) }
    pub fn favorites(&self) -> &[String] { &self.favorites }
    pub fn recent(&self) -> impl Iterator<Item = &str> { self.recent.iter().map(|r| r.id.as_str()) }

    /// All apps in a category.
    pub fn in_category<'a>(&'a self, cat: &'a str) -> impl Iterator<Item = &'a DesktopEntry> + 'a {
        self.apps.values().filter(move |e| e.categories.iter().any(|c| c == cat))
    }

    /// Persist favorites + recents + clock (apps are re-scanned from disk).
    pub fn save_state(&self, path: &std::path::Path) -> Result<(), String> {
        let recent: Vec<(String, u64)> = self.recent.iter().map(|r| (r.id.clone(), r.at)).collect();
        #[derive(serde::Serialize)]
        struct Disk<'a> { favorites: &'a [String], recent: Vec<(String,u64)>, clock: u64 }
        let d = Disk { favorites: &self.favorites, recent, clock: self.clock };
        let j = serde_json::to_string_pretty(&d).map_err(|e| e.to_string())?;
        if let Some(p) = path.parent() { std::fs::create_dir_all(p).map_err(|e| e.to_string())?; }
        std::fs::write(path, j).map_err(|e| e.to_string())
    }

    pub fn load_state(&mut self, path: &std::path::Path) {
        #[derive(serde::Deserialize)]
        struct Disk { favorites: Vec<String>, recent: Vec<(String,u64)>, clock: u64 }
        if let Ok(txt) = std::fs::read_to_string(path) {
            if let Ok(d) = serde_json::from_str::<Disk>(&txt) {
                self.favorites = d.favorites;
                self.recent = d.recent.into_iter().map(|(id, at)| LaunchRecord { id, at }).collect();
                self.clock = d.clock;
            }
        }
    }

    pub fn state_path() -> std::path::PathBuf {
        if let Ok(p) = std::env::var("SIGMAOS_LAUNCHER_FILE") { return std::path::PathBuf::from(p); }
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        std::path::PathBuf::from(home).join(".local/share/sigmaos/launcher.json")
    }

    pub fn status_line(&self) -> String {
        format!("launcher: {} apps, {} favorites, {} recent", self.apps.len(), self.favorites.len(), self.recent.len())
    }
}

impl Default for OmarchyLauncher {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, name: &str, exec: &str) -> DesktopEntry {
        DesktopEntry {
            id: id.into(), name: name.into(), generic_name: None, comment: None,
            exec: exec.into(), icon: None, categories: vec!["Utility".into()],
            keywords: vec![], terminal: false, no_display: false, hidden: false,
            startup_notify: true, path: format!("/usr/share/applications/{id}"),
        }
    }

    fn sample_desktop() -> &'static str {
        "[Desktop Entry]\nType=Application\nName=GNU Image Manipulation Program\nGenericName=Image Editor\nExec=gimp %U\nIcon=gimp\nCategories=Graphics;2DGraphics;\nKeywords=paint;photo;\nStartupNotify=true\n"
    }

    #[test]
    fn parse_desktop_entry() {
        let e = DesktopEntry::parse("gimp.desktop", "/x/gimp.desktop", sample_desktop()).unwrap();
        assert_eq!(e.name, "GNU Image Manipulation Program");
        assert_eq!(e.exec, "gimp %U");
        assert_eq!(e.categories, vec!["Graphics", "2DGraphics"]);
        assert!(e.startup_notify);
        assert!(!e.terminal);
    }

    #[test]
    fn parse_rejects_missing_exec() {
        let bad = "[Desktop Entry]\nName=NoExec\n";
        assert!(DesktopEntry::parse("x.desktop", "/x", bad).is_err());
        let bad2 = "[Desktop Entry]\nExec=foo\n";
        assert!(DesktopEntry::parse("x.desktop", "/x", bad2).is_err()); // no Name
    }

    #[test]
    fn register_and_search() {
        let mut m = OmarchyLauncher::new();
        m.register(entry("gimp.desktop", "GIMP", "gimp %U"), false).unwrap();
        m.register(entry("nautilus.desktop", "Files", "nautilus %U"), false).unwrap();
        m.register(entry("hidden.desktop", "Secret", "secret"), false).unwrap();
        // hidden filtered? entry() has hidden=false so all register
        let hits = m.search("gimp", 10).unwrap();
        assert_eq!(hits[0].1.name, "GIMP");
        let hits = m.search("file", 10).unwrap();
        assert!(hits.iter().any(|(_, e)| e.name == "Files"));
        assert!(m.search("", 10).is_err());
    }

    #[test]
    fn fuzzy_subsequence() {
        assert!(fuzzy_score("gmp", "gimp").is_some());
        assert!(fuzzy_score("fls", "files").is_some());
        assert!(fuzzy_score("zzz", "gimp").is_none());
        // word-start bonus: "nautilus" query "nau" scores higher than mid-word
        assert!(fuzzy_score("image", "image editor").unwrap() > fuzzy_score("ime", "image").unwrap_or(0));
    }

    #[test]
    fn launch_records_recent() {
        let mut m = OmarchyLauncher::new();
        m.register(entry("a.desktop", "App A", "app-a"), false).unwrap();
        m.register(entry("b.desktop", "App B", "app-b"), false).unwrap();
        assert_eq!(m.launch("a.desktop").unwrap(), "app-a");
        assert_eq!(m.launch("b.desktop").unwrap(), "app-b");
        let recent: Vec<&str> = m.recent().collect();
        assert_eq!(recent, vec!["b.desktop", "a.desktop"]);
        // recent app ranks higher in search
        let hits = m.search("app", 10).unwrap();
        assert_eq!(hits[0].1.id, "b.desktop");
        assert!(m.launch("nope.desktop").is_err());
    }

    #[test]
    fn favorites_boost_and_toggle() {
        let mut m = OmarchyLauncher::new();
        m.register(entry("z.desktop", "Zeta", "z"), false).unwrap();
        m.register(entry("a.desktop", "Alpha", "a"), false).unwrap();
        assert!(m.toggle_favorite("z.desktop").unwrap());
        assert!(m.is_favorite("z.desktop"));
        assert!(!m.toggle_favorite("z.desktop").unwrap());
        assert_eq!(m.favorites().len(), 0);
        assert!(m.toggle_favorite("missing").is_err());
    }

    #[test]
    fn categories_and_status() {
        let mut m = OmarchyLauncher::new();
        let mut e = entry("g.desktop", "GIMP", "gimp");
        e.categories = vec!["Graphics".into(), "2DGraphics".into()];
        m.register(e, false).unwrap();
        assert_eq!(m.in_category("Graphics").count(), 1);
        assert!(m.status_line().contains("1 apps"));
    }
}
