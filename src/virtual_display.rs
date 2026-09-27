//! Optional compositor-managed virtual displays.
//!
//! The functions here never install a driver. `remove` accepts only an output
//! recorded by a successful `create` call in this process and compositor session.

use anyhow::{Context, Result, anyhow, bail};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Compositor {
    Hyprland,
    Sway,
}

#[derive(Clone, Debug)]
pub struct Capability {
    pub compositor: Option<Compositor>,
    pub available: bool,
    pub reason: String,
}

#[derive(Clone, Debug)]
pub struct VirtualDisplay {
    pub name: String,
    pub compositor: Compositor,
    /// Whether this process created the output and can remove it.
    pub owned: bool,
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use serde_json::Value;
    use std::{
        collections::{HashMap, HashSet},
        env,
        path::Path,
        process::Command,
        sync::{Mutex, OnceLock},
        time::Duration,
    };

    #[derive(Clone)]
    struct Owner {
        compositor: Compositor,
        session: String,
    }

    static OWNED: OnceLock<Mutex<HashMap<String, Owner>>> = OnceLock::new();

    fn owned() -> &'static Mutex<HashMap<String, Owner>> {
        OWNED.get_or_init(|| Mutex::new(HashMap::new()))
    }

    fn session(compositor: Compositor) -> Option<String> {
        match compositor {
            Compositor::Hyprland => env::var("HYPRLAND_INSTANCE_SIGNATURE").ok(),
            Compositor::Sway => env::var("SWAYSOCK").ok(),
        }
        .filter(|value| !value.is_empty())
    }

    fn detected() -> Option<Compositor> {
        if session(Compositor::Hyprland).is_some() {
            return Some(Compositor::Hyprland);
        }
        if session(Compositor::Sway).is_some() {
            return Some(Compositor::Sway);
        }
        // A desktop label alone cannot identify the compositor instance to
        // which we would send a destructive remove command.
        None
    }

    fn executable(name: &str) -> bool {
        env::split_paths(&env::var_os("PATH").unwrap_or_default()).any(|directory| {
            let path = directory.join(name);
            path.is_file() && is_executable(&path)
        })
    }

    fn is_executable(path: &Path) -> bool {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|meta| meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    pub fn capabilities() -> Capability {
        let desktop = env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        let Some(compositor) = detected() else {
            return Capability {
                compositor: None,
                available: false,
                reason: format!(
                    "No supported compositor session socket found (XDG_CURRENT_DESKTOP={desktop})"
                ),
            };
        };
        let program = match compositor {
            Compositor::Hyprland => "hyprctl",
            Compositor::Sway => "swaymsg",
        };
        if !executable(program) {
            return Capability {
                compositor: Some(compositor),
                available: false,
                reason: format!("{program} is unavailable in PATH"),
            };
        }
        Capability {
            compositor: Some(compositor),
            available: true,
            reason: format!("{compositor:?} compositor IPC is available"),
        }
    }

    fn ready() -> Result<(Compositor, String)> {
        let capability = capabilities();
        if !capability.available {
            bail!("{}", capability.reason);
        }
        let compositor = capability.compositor.context("no compositor")?;
        let session = session(compositor).context("compositor session vanished")?;
        Ok((compositor, session))
    }

    fn run(compositor: Compositor, args: &[&str]) -> Result<String> {
        let program = match compositor {
            Compositor::Hyprland => "hyprctl",
            Compositor::Sway => "swaymsg",
        };
        let output = Command::new(program)
            .args(args)
            .output()
            .with_context(|| format!("running {program}"))?;
        if !output.status.success() {
            bail!(
                "{program} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        String::from_utf8(output.stdout).context("compositor returned non-UTF-8 output")
    }

    fn output_names(compositor: Compositor) -> Result<HashSet<String>> {
        let data = match compositor {
            Compositor::Hyprland => run(compositor, &["-j", "monitors", "all"])?,
            Compositor::Sway => run(compositor, &["-r", "-t", "get_outputs"])?,
        };
        parse_output_names(&data)
    }

    fn parse_output_names(data: &str) -> Result<HashSet<String>> {
        let value: Value = serde_json::from_str(data).context("invalid output list JSON")?;
        let items = value.as_array().context("output list must be an array")?;
        items
            .iter()
            .map(|item| {
                item.get("name")
                    .and_then(Value::as_str)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
                    .context("output is missing a name")
            })
            .collect()
    }

    fn valid_name(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= 80
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    }

    fn is_sway_headless(name: &str) -> bool {
        name.strip_prefix("HEADLESS-")
            .map(|suffix| !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit()))
            .unwrap_or(false)
    }

    pub fn list() -> Result<Vec<VirtualDisplay>> {
        let (compositor, session) = ready()?;
        let owner_map = owned()
            .lock()
            .map_err(|_| anyhow!("virtual display lock poisoned"))?;
        let mut displays: Vec<_> = output_names(compositor)?
            .into_iter()
            .filter(|name| match compositor {
                Compositor::Hyprland => name.starts_with("QUESTDISPLAY-"),
                Compositor::Sway => is_sway_headless(name),
            })
            .map(|name| {
                let is_owned = owner_map.get(&name).is_some_and(|owner| {
                    owner.compositor == compositor && owner.session == session
                });
                VirtualDisplay {
                    name,
                    compositor,
                    owned: is_owned,
                }
            })
            .collect();
        displays.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(displays)
    }

    /// Creates one headless output. Sway assigns the name; Hyprland uses a
    /// process-specific name. An ambiguous Sway output delta is left untouched.
    pub fn create() -> Result<VirtualDisplay> {
        let (compositor, session) = ready()?;
        let mut owner_map = owned()
            .lock()
            .map_err(|_| anyhow!("virtual display lock poisoned"))?;
        let before = output_names(compositor)?;
        let name = match compositor {
            Compositor::Hyprland => {
                let counter = owner_map.len();
                let name = format!("QUESTDISPLAY-{}-{counter}", std::process::id());
                if before.contains(&name) {
                    bail!("virtual output name {name} already exists");
                }
                run(compositor, &["output", "create", "headless", &name])?;
                name
            }
            Compositor::Sway => {
                run(compositor, &["create_output"])?;
                String::new()
            }
        };
        for _ in 0..10 {
            let after = output_names(compositor)?;
            let new: Vec<_> = after.difference(&before).cloned().collect();
            let observed = match compositor {
                Compositor::Hyprland if after.contains(&name) => Some(name.clone()),
                Compositor::Sway if new.len() == 1 && is_sway_headless(&new[0]) => {
                    Some(new[0].clone())
                }
                _ => None,
            };
            if let Some(name) = observed {
                owner_map.insert(
                    name.clone(),
                    Owner {
                        compositor,
                        session,
                    },
                );
                return Ok(VirtualDisplay {
                    name,
                    compositor,
                    owned: true,
                });
            }
            if new.len() > 1 {
                bail!("multiple outputs appeared; cannot safely identify created output");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        bail!("compositor accepted create command but new output was not observed")
    }

    /// Removes only a display created by this process in the current session.
    pub fn remove(name: &str) -> Result<()> {
        if !valid_name(name) {
            bail!("invalid virtual display name");
        }
        let (compositor, session) = ready()?;
        let mut owner_map = owned()
            .lock()
            .map_err(|_| anyhow!("virtual display lock poisoned"))?;
        let owner = owner_map
            .get(name)
            .context("output was not created by this process")?;
        if owner.compositor != compositor || owner.session != session {
            bail!("output belongs to another compositor session");
        }
        if !output_names(compositor)?.contains(name) {
            owner_map.remove(name);
            bail!("output no longer exists");
        }
        match compositor {
            Compositor::Hyprland => {
                run(compositor, &["output", "remove", name])?;
            }
            Compositor::Sway => {
                if !is_sway_headless(name) {
                    bail!("refusing to unplug a non-headless Sway output");
                }
                run(compositor, &["output", name, "unplug"])?;
            }
        }
        owner_map.remove(name);
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn names_are_constrained_to_single_compositor_tokens() {
            assert!(valid_name("QUESTDISPLAY-42-0"));
            assert!(!valid_name("HEADLESS-1; exit"));
            assert!(!valid_name("--".repeat(50).as_str()));
            assert!(!valid_name(""));
            assert!(is_sway_headless("HEADLESS-12"));
            assert!(!is_sway_headless("HEADLESS-1; output eDP-1 unplug"));
        }

        #[test]
        fn output_json_requires_named_entries() {
            let names = parse_output_names(r#"[{"name":"HEADLESS-1"},{"name":"eDP-1"}]"#).unwrap();
            assert!(names.contains("HEADLESS-1"));
            assert!(parse_output_names(r#"[{"active":true}]"#).is_err());
            assert!(parse_output_names("{}").is_err());
        }
    }
}

#[cfg(target_os = "linux")]
pub use linux::{capabilities, create, list, remove};

#[cfg(not(target_os = "linux"))]
pub fn capabilities() -> Capability {
    Capability {
        compositor: None,
        available: false,
        reason: "No supported native virtual display API is available on this OS".into(),
    }
}

#[cfg(not(target_os = "linux"))]
pub fn create() -> Result<VirtualDisplay> {
    anyhow::bail!("virtual display creation is unavailable on this OS")
}

#[cfg(not(target_os = "linux"))]
pub fn remove(_name: &str) -> Result<()> {
    anyhow::bail!("virtual display removal is unavailable on this OS")
}

#[cfg(not(target_os = "linux"))]
pub fn list() -> Result<Vec<VirtualDisplay>> {
    Ok(Vec::new())
}
