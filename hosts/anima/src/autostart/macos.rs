//! A per-user LaunchAgent, `~/Library/LaunchAgents/dev.animaos.anima.plist`.

use std::io;
use std::path::{Path, PathBuf};

use super::Autostart;
use crate::paths::AnimaPaths;

/// The LaunchAgent label, and the name of its plist.
pub const LABEL: &str = "dev.animaos.anima";

/// What a `launchctl` run came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launched {
    pub success: bool,
    pub code: Option<i32>,
    /// Standard output and error, trimmed.
    pub output: String,
}

/// Runs `launchctl` with the arguments. A test swaps in a recorder.
pub type Launchctl = Box<dyn Fn(&[&str]) -> io::Result<Launched>>;

pub struct MacAutostart {
    agents_dir: PathBuf,
    logs: PathBuf,
    uid: Option<u32>,
    launchctl: Launchctl,
}

impl MacAutostart {
    pub fn new(agents_dir: PathBuf, logs: PathBuf, uid: Option<u32>, launchctl: Launchctl) -> Self {
        Self {
            agents_dir,
            logs,
            uid,
            launchctl,
        }
    }

    /// The real `~/Library/LaunchAgents` and `launchctl`; with `dry_run` the
    /// same folder, but `launchctl` is not run (it reports success).
    pub fn for_paths(paths: &AnimaPaths, dry_run: bool) -> Self {
        let home = paths.root.parent().unwrap_or(&paths.root);
        let launchctl: Launchctl = if dry_run {
            Box::new(|_| {
                Ok(Launched {
                    success: true,
                    code: Some(0),
                    output: String::new(),
                })
            })
        } else {
            Box::new(run_launchctl)
        };
        Self::new(
            home.join("Library").join("LaunchAgents"),
            paths.logs.clone(),
            user_id(home).ok(),
            launchctl,
        )
    }

    fn plist_path(&self) -> PathBuf {
        self.agents_dir.join(format!("{LABEL}.plist"))
    }

    fn uid(&self) -> io::Result<u32> {
        self.uid
            .ok_or_else(|| io::Error::other("could not determine the user id for launchd"))
    }

    fn launchctl(&self, args: &[&str]) -> io::Result<Launched> {
        (self.launchctl)(args)
    }
}

impl Autostart for MacAutostart {
    fn enable(&self, exe: &Path) -> io::Result<()> {
        let uid = self.uid()?;
        std::fs::create_dir_all(&self.agents_dir)?;
        let plist_path = self.plist_path();
        write_atomic(&plist_path, plist(exe, &self.logs).as_bytes())?;

        let domain = format!("gui/{uid}");
        let bootstrap = self.launchctl(&["bootstrap", &domain, &plist_path.to_string_lossy()])?;
        if bootstrap.success || already_loaded(&bootstrap) {
            return Ok(());
        }
        // Not every launchd words "already loaded" alike: ask it.
        if self
            .launchctl(&["print", &format!("{domain}/{LABEL}")])?
            .success
        {
            return Ok(());
        }
        Err(io::Error::other(format!(
            "launchctl bootstrap failed: {}",
            bootstrap.output
        )))
    }

    fn disable(&self) -> io::Result<()> {
        let uid = self.uid()?;
        // Fails when the agent is not loaded, which is the state we want.
        self.launchctl(&["bootout", &format!("gui/{uid}/{LABEL}")])?;
        match std::fs::remove_file(self.plist_path()) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }

    fn is_enabled(&self) -> io::Result<bool> {
        self.plist_path().try_exists()
    }
}

/// `launchctl` exits 37 (EALREADY) when the service is loaded already.
fn already_loaded(launched: &Launched) -> bool {
    launched.code == Some(37) || launched.output.to_lowercase().contains("already")
}

fn run_launchctl(args: &[&str]) -> io::Result<Launched> {
    let output = std::process::Command::new("launchctl")
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok(Launched {
        success: output.status.success(),
        code: output.status.code(),
        output: text.trim().to_string(),
    })
}

/// The owner of the home folder, which is the logged-in user (`gui/<uid>`).
pub fn user_id(home: &Path) -> io::Result<u32> {
    use std::os::unix::fs::MetadataExt;
    Ok(std::fs::metadata(home)?.uid())
}

/// Temp file in the same folder, then a rename; launchd needs it to be 0644.
fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(format!(".tmp{}", std::process::id()));
    let tmp = path.with_file_name(tmp_name);
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o644)
            .open(&tmp)?;
        file.write_all(contents)?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// The LaunchAgent plist: runs `exe supervise --background` at login, and
/// again whenever it exits with an error. `logs` is the logs folder.
pub fn plist(exe: &Path, logs: &Path) -> String {
    let log = escape(&logs.join("supervisor.log").to_string_lossy());
    let exe = escape(&exe.to_string_lossy());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{LABEL}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{exe}</string>
		<string>supervise</string>
		<string>--background</string>
	</array>
	<key>RunAtLoad</key>
	<true/>
	<key>KeepAlive</key>
	<dict>
		<key>SuccessfulExit</key>
		<false/>
	</dict>
	<key>StandardOutPath</key>
	<string>{log}</string>
	<key>StandardErrorPath</key>
	<string>{log}</string>
</dict>
</plist>
"#
    )
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::io::Cursor;
    use std::rc::Rc;

    use super::*;

    type Calls = Rc<RefCell<Vec<Vec<String>>>>;

    fn done(success: bool, code: i32, output: &str) -> Launched {
        Launched {
            success,
            code: Some(code),
            output: output.to_string(),
        }
    }

    /// A `launchctl` that records its arguments and answers from `answer`.
    fn fake(answer: impl Fn(&[&str]) -> Launched + 'static) -> (Launchctl, Calls) {
        let calls = Calls::default();
        let recorded = calls.clone();
        let runner: Launchctl = Box::new(move |args| {
            recorded
                .borrow_mut()
                .push(args.iter().map(|arg| arg.to_string()).collect());
            Ok(answer(args))
        });
        (runner, calls)
    }

    fn ok(_: &[&str]) -> Launched {
        done(true, 0, "")
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        agents: PathBuf,
        logs: PathBuf,
        agent: MacAutostart,
        calls: Calls,
    }

    fn fixture(answer: impl Fn(&[&str]) -> Launched + 'static) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        // The agents folder does not exist yet: `enable` creates it.
        let agents = dir.path().join("Library").join("LaunchAgents");
        let logs = dir.path().join(".animaos-swarm").join("logs");
        let (runner, calls) = fake(answer);
        let agent = MacAutostart::new(agents.clone(), logs.clone(), Some(501), runner);
        Fixture {
            _dir: dir,
            agents,
            logs,
            agent,
            calls,
        }
    }

    fn parse(text: &str) -> plist::Dictionary {
        plist::Value::from_reader(Cursor::new(text.as_bytes()))
            .unwrap()
            .into_dictionary()
            .unwrap()
    }

    fn strings(value: &plist::Value) -> Vec<String> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item.as_string().unwrap().to_string())
            .collect()
    }

    #[test]
    fn plist_contains_label_args_and_keepalive() {
        let exe = Path::new("/Applications/Anima.app/Contents/MacOS/anima");
        let logs = Path::new("/Users/u/.animaos-swarm/logs");
        let dict = parse(&plist(exe, logs));

        assert_eq!(dict["Label"].as_string(), Some("dev.animaos.anima"));
        assert_eq!(
            strings(&dict["ProgramArguments"]),
            [exe.to_str().unwrap(), "supervise", "--background"]
        );
        assert_eq!(dict["RunAtLoad"].as_boolean(), Some(true));
        let keep_alive = dict["KeepAlive"].as_dictionary().unwrap();
        assert_eq!(keep_alive["SuccessfulExit"].as_boolean(), Some(false));
        let log = "/Users/u/.animaos-swarm/logs/supervisor.log";
        assert_eq!(dict["StandardOutPath"].as_string(), Some(log));
        assert_eq!(dict["StandardErrorPath"].as_string(), Some(log));
    }

    #[test]
    fn plist_escapes_paths() {
        let exe = Path::new("/Apps/R&D <beta>/anima");
        let dict = parse(&plist(exe, Path::new("/logs")));
        assert_eq!(
            strings(&dict["ProgramArguments"])[0],
            "/Apps/R&D <beta>/anima"
        );
    }

    #[test]
    fn enable_writes_the_plist_and_bootstraps_it() {
        let fx = fixture(ok);
        let exe = Path::new("/opt/anima/anima");
        assert!(!fx.agent.is_enabled().unwrap());

        fx.agent.enable(exe).unwrap();

        let plist_path = fx.agents.join("dev.animaos.anima.plist");
        assert!(fx.agent.is_enabled().unwrap());
        assert_eq!(
            std::fs::read_to_string(&plist_path).unwrap(),
            plist(exe, &fx.logs)
        );
        assert_eq!(
            *fx.calls.borrow(),
            [vec![
                "bootstrap".to_string(),
                "gui/501".to_string(),
                plist_path.to_string_lossy().into_owned(),
            ]]
        );
    }

    #[test]
    fn enable_treats_an_already_loaded_agent_as_enabled() {
        // The exit code alone says it.
        let fx = fixture(|args| match args[0] {
            "bootstrap" => done(false, 37, ""),
            _ => done(false, 1, "no"),
        });
        fx.agent.enable(Path::new("/opt/anima/anima")).unwrap();
        assert_eq!(fx.calls.borrow().len(), 1);

        // So does the message.
        let fx = fixture(|args| match args[0] {
            "bootstrap" => done(false, 5, "Bootstrap failed: service already loaded"),
            _ => done(false, 1, "no"),
        });
        fx.agent.enable(Path::new("/opt/anima/anima")).unwrap();

        // And launchd still knowing the service, however it words the failure.
        let fx = fixture(|args| match args[0] {
            "bootstrap" => done(false, 5, "Bootstrap failed: 5: Input/output error"),
            _ => done(true, 0, "state = running"),
        });
        fx.agent.enable(Path::new("/opt/anima/anima")).unwrap();
        let calls = fx.calls.borrow();
        assert_eq!(calls[1], ["print", "gui/501/dev.animaos.anima"]);
    }

    #[test]
    fn enable_fails_with_what_launchctl_said() {
        let fx = fixture(|_| done(false, 5, "Bootstrap failed: 5: Input/output error"));
        let error = fx.agent.enable(Path::new("/opt/anima/anima")).unwrap_err();
        assert!(error.to_string().contains("Input/output error"), "{error}");
    }

    #[test]
    fn disable_boots_the_agent_out_and_removes_the_plist() {
        let fx = fixture(ok);
        fx.agent.enable(Path::new("/opt/anima/anima")).unwrap();
        fx.calls.borrow_mut().clear();

        fx.agent.disable().unwrap();

        assert!(!fx.agent.is_enabled().unwrap());
        assert!(!fx.agents.join("dev.animaos.anima.plist").exists());
        assert_eq!(
            *fx.calls.borrow(),
            [vec!["bootout", "gui/501/dev.animaos.anima"]]
        );
    }

    #[test]
    fn disable_succeeds_when_nothing_was_enabled() {
        // launchctl: "Could not find service"; and there is no file either.
        let fx = fixture(|_| done(false, 113, "Boot-out failed: 3: No such process"));
        fx.agent.disable().unwrap();
        assert!(!fx.agent.is_enabled().unwrap());
    }

    #[test]
    fn without_a_user_id_enable_and_disable_say_so() {
        let dir = tempfile::tempdir().unwrap();
        let (runner, calls) = fake(ok);
        let agent = MacAutostart::new(dir.path().join("a"), dir.path().join("l"), None, runner);
        assert!(agent.enable(Path::new("/x")).is_err());
        assert!(agent.disable().is_err());
        assert!(calls.borrow().is_empty());
        assert!(!dir.path().join("a").exists());
    }
}
