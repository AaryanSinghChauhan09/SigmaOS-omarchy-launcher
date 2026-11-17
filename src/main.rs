//! SigmaOS-omarchy-launcher — rofi/walker-style launcher CLI.
//!
//! Usage:
//!   omarchy-launcher list [dir]         (scan .desktop files; default seed)
//!   omarchy-launcher search <query>
//!   omarchy-launcher launch <id>        (print exec line)
//!   omarchy-launcher favorite <id>
//!   omarchy-launcher recent
//!   omarchy-launcher category <cat>

use omarchy_launcher::{DesktopEntry, OmarchyLauncher};
use std::path::Path;
use std::process::exit;

fn seed() -> OmarchyLauncher {
    let mut m = OmarchyLauncher::new();
    let apps = [
        ("org.gnome.Nautilus.desktop", "Files", "nautilus %U", "System-Files", "file;folder;"),
        ("gimp.desktop", "GIMP", "gimp %U", "Graphics", "paint;photo;image;"),
        ("firefox.desktop", "Firefox", "firefox %u", "Network-WebBrowser", "web;internet;"),
        ("org.kde.konsole.desktop", "Konsole", "konsole", "System-TerminalEmulator", "terminal;shell;"),
        ("code.desktop", "VS Code", "code %F", "Development-TextEditor", "editor;code;ide;"),
        ("org.gnome.Calculator.desktop", "Calculator", "gnome-calculator", "Utility", "calc;math;"),
    ];
    for (id, name, exec, cat, kw) in apps {
        let e = DesktopEntry {
            id: id.into(), name: name.into(), generic_name: None, comment: None,
            exec: exec.into(), icon: None, categories: cat.split('-').map(|s| s.to_string()).collect(),
            keywords: kw.split(';').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect(),
            terminal: false, no_display: false, hidden: false, startup_notify: true,
            path: format!("/usr/share/applications/{id}"),
        };
        let _ = m.register(e, false);
    }
    m
}

fn load_dir(m: &mut OmarchyLauncher, dir: &Path) -> usize {
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for f in rd.flatten() {
            let p = f.path();
            if p.extension().map(|e| e == "desktop").unwrap_or(false) {
                if let Ok(text) = std::fs::read_to_string(&p) {
                    let id = p.file_name().unwrap().to_string_lossy().to_string();
                    if let Ok(e) = DesktopEntry::parse(id.clone(), p.display().to_string(), &text) {
                        if m.register(e, false).is_ok() { n += 1; }
                    }
                }
            }
        }
    }
    n
}

fn usage() -> ! {
    eprintln!("usage: omarchy-launcher <list|search|launch|favorite|recent|category> [args]");
    exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() { usage(); }
    let cmd = args[0].as_str();
    let rest = &args[1..];
    let mut m = seed();
    // optionally load a real .desktop dir
    if let Some(dir) = std::env::var_os("XDG_DATA_DIRS").map(|d| Path::new(&d).join("applications"))
        .or_else(|| Some(Path::new("/usr/share/applications").to_path_buf())) {
        let _ = load_dir(&mut m, &dir);
    }

    let result: Result<(), String> = (|| -> Result<(), String> { match cmd {
        "list" => {
            for e in m.apps() {
                let fav = if m.is_favorite(&e.id) { "*" } else { " " };
                println!("{} {:<28} {:<24} {}", fav, e.name, e.id, e.exec);
            }
            eprintln!("{}", m.status_line());
            Ok(())
        }
        "search" => {
            let q = rest.first().map(|s| s.as_str()).unwrap_or_else(|| usage());
            let hits = m.search(q, 15).map_err(|e| e.to_string())?;
            for (score, e) in hits {
                println!("{:>4}  {:<28} {}", score, e.name, e.exec);
            }
            Ok(())
        }
        "launch" => {
            let id = rest.first().map(|s| s.as_str()).unwrap_or_else(|| usage());
            match m.launch(id) {
                Ok(exec) => { println!("{exec}"); Ok(()) }
                Err(e) => Err(e.to_string()),
            }
        }
        "favorite" => {
            let id = rest.first().map(|s| s.as_str()).unwrap_or_else(|| usage());
            match m.toggle_favorite(id) {
                Ok(true) => println!("favorited {id}"),
                Ok(false) => println!("unfavorited {id}"),
                Err(e) => return Err(e.to_string()),
            }
            Ok(())
        }
        "recent" => {
            for id in m.recent() { println!("{id}"); }
            Ok(())
        }
        "category" => {
            let cat = rest.first().map(|s| s.as_str()).unwrap_or_else(|| usage());
            for e in m.in_category(cat) { println!("{:<28} {}", e.name, e.exec); }
            Ok(())
        }
        _ => usage(),
    } })();

    if let Err(e) = result {
        eprintln!("omarchy-launcher: {e}");
        exit(1);
    }
}
