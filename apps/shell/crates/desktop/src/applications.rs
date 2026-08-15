use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplicationEntry {
    pub desktop_id: String,
    pub name: String,
    pub comment: String,
    pub command: Vec<String>,
}

impl ApplicationEntry {
    pub fn search_text(&self) -> String {
        format!("{} {} {}", self.name, self.comment, self.desktop_id).to_lowercase()
    }
}

pub fn index() -> Vec<ApplicationEntry> {
    let mut by_id = HashMap::new();
    let mut hidden = HashSet::new();

    for directory in application_directories() {
        visit_desktop_files(&directory, &mut |path| {
            let Some(desktop_id) = desktop_id(&directory, path) else {
                return;
            };
            if by_id.contains_key(&desktop_id) || hidden.contains(&desktop_id) {
                return;
            }
            match parse_desktop_entry(path, desktop_id.clone()) {
                DesktopFile::Visible(entry) => {
                    by_id.insert(desktop_id, entry);
                }
                DesktopFile::Hidden => {
                    hidden.insert(desktop_id);
                }
                DesktopFile::Invalid => {}
            }
        });
    }

    let mut entries = by_id.into_values().collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.name.to_lowercase());
    entries
}

pub fn matches(entries: &[ApplicationEntry], query: &str, limit: usize) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    let mut scored = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let score = if query.is_empty() {
                0
            } else {
                fuzzy_score(&entry.search_text(), &query)?
            };
            Some((index, score))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|(left_index, left_score), (right_index, right_score)| {
        right_score
            .cmp(left_score)
            .then_with(|| entries[*left_index].name.cmp(&entries[*right_index].name))
    });
    scored
        .into_iter()
        .take(limit)
        .map(|(index, _)| index)
        .collect()
}

pub fn find_by_desktop_id(
    entries: &[ApplicationEntry],
    desktop_id: &str,
) -> Option<ApplicationEntry> {
    let requested = desktop_id.strip_suffix(".desktop").unwrap_or(desktop_id);
    entries
        .iter()
        .find(|entry| {
            entry
                .desktop_id
                .strip_suffix(".desktop")
                .unwrap_or(&entry.desktop_id)
                == requested
        })
        .cloned()
}

pub fn launch(application: ApplicationEntry) {
    std::thread::spawn(move || {
        if let Err(error) = foyer_shell_niri::spawn(application.command) {
            tracing::error!(application = %application.name, %error, "failed to launch application");
        }
    });
}

fn application_directories() -> Vec<PathBuf> {
    let data_home = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    let data_dirs = env::var_os("XDG_DATA_DIRS")
        .map(|dirs| env::split_paths(&dirs).collect::<Vec<_>>())
        .unwrap_or_else(|| {
            vec![
                PathBuf::from("/usr/local/share"),
                PathBuf::from("/usr/share"),
            ]
        });

    data_home
        .into_iter()
        .chain(data_dirs)
        .map(|directory| directory.join("applications"))
        .collect()
}

fn visit_desktop_files(directory: &Path, visit: &mut impl FnMut(&Path)) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit_desktop_files(&path, visit);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "desktop")
        {
            visit(&path);
        }
    }
}

fn desktop_id(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    Some(
        relative
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "-"),
    )
}

enum DesktopFile {
    Visible(ApplicationEntry),
    Hidden,
    Invalid,
}

fn parse_desktop_entry(path: &Path, desktop_id: String) -> DesktopFile {
    let Ok(contents) = fs::read_to_string(path) else {
        return DesktopFile::Invalid;
    };
    let mut in_desktop_entry = false;
    let mut fields = HashMap::new();
    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            fields.entry(key.trim()).or_insert(value.trim());
        }
    }

    let hidden = fields.get("Hidden").is_some_and(|value| *value == "true");
    if hidden {
        return DesktopFile::Hidden;
    }
    if fields
        .get("NoDisplay")
        .is_some_and(|value| *value == "true")
        || fields
            .get("Type")
            .is_some_and(|value| *value != "Application")
    {
        return DesktopFile::Invalid;
    }
    let Some(name) = fields.get("Name").filter(|name| !name.is_empty()) else {
        return DesktopFile::Invalid;
    };
    let Some(exec) = fields.get("Exec") else {
        return DesktopFile::Invalid;
    };
    let Some(command) = parse_exec(exec) else {
        return DesktopFile::Invalid;
    };

    DesktopFile::Visible(ApplicationEntry {
        desktop_id,
        name: (*name).to_string(),
        comment: fields
            .get("Comment")
            .copied()
            .unwrap_or_default()
            .to_string(),
        command,
    })
}

fn parse_exec(exec: &str) -> Option<Vec<String>> {
    let command = shlex::split(exec)?
        .into_iter()
        .filter_map(|argument| {
            let mut output = String::new();
            let mut chars = argument.chars();
            while let Some(character) = chars.next() {
                if character != '%' {
                    output.push(character);
                    continue;
                }
                match chars.next() {
                    Some('%') => output.push('%'),
                    Some(_) => {}
                    None => output.push('%'),
                }
            }
            (!output.is_empty()).then_some(output)
        })
        .collect::<Vec<_>>();
    (!command.is_empty()).then_some(command)
}

fn fuzzy_score(haystack: &str, needle: &str) -> Option<i64> {
    let mut score = 0_i64;
    let mut search_from = 0;
    let mut previous = None;
    for character in needle.chars() {
        let relative = haystack[search_from..].find(character)?;
        let position = search_from + relative;
        score -= position as i64;
        if position == 0 || haystack.as_bytes().get(position.wrapping_sub(1)) == Some(&b' ') {
            score += 28;
        }
        if previous.is_some_and(|previous| previous + 1 == position) {
            score += 18;
        }
        previous = Some(position);
        search_from = position + character.len_utf8();
    }
    if haystack.starts_with(needle) {
        score += 120;
    } else if haystack.contains(needle) {
        score += 60;
    }
    Some(score)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_field_codes_are_removed_without_using_a_shell() {
        assert_eq!(
            parse_exec("code --reuse-window %F --title='A %% title'"),
            Some(vec![
                "code".into(),
                "--reuse-window".into(),
                "--title=A % title".into()
            ])
        );
    }

    #[test]
    fn fuzzy_match_prefers_prefixes_and_consecutive_text() {
        let entries = vec![
            ApplicationEntry {
                desktop_id: "org.gnome.Terminal.desktop".into(),
                name: "Terminal".into(),
                comment: String::new(),
                command: vec!["terminal".into()],
            },
            ApplicationEntry {
                desktop_id: "example.desktop".into(),
                name: "Text Editor Minimal".into(),
                comment: String::new(),
                command: vec!["editor".into()],
            },
        ];
        assert_eq!(matches(&entries, "term", 10), vec![0, 1]);
    }

    #[test]
    fn desktop_entry_hints_match_with_or_without_suffix() {
        let entries = vec![ApplicationEntry {
            desktop_id: "org.example.Player.desktop".into(),
            name: "Player".into(),
            comment: String::new(),
            command: vec!["player".into()],
        }];
        assert!(find_by_desktop_id(&entries, "org.example.Player").is_some());
        assert!(find_by_desktop_id(&entries, "org.example.Player.desktop").is_some());
    }
}
