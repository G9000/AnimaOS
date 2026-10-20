//! The tray (Windows) and menu-bar (macOS) icon of `anima supervise`.
//!
//! The tao event loop owns the main thread, as both systems require; the
//! supervisor runs on a tokio runtime in a background thread. Menu clicks
//! become the same action requests the control API makes, and the menu is
//! rebuilt whenever the status changes. When the supervisor ends (Quit from
//! the tray or the CLI, or Ctrl-C), so does the event loop.

#[cfg(any(target_os = "macos", windows))]
mod icon;
mod menu;
#[cfg(test)]
mod menu_tests;

use std::io;

use crate::supervisor::Supervisor;

/// Test-only switch, like `--no-tray`: `ANIMA_NO_TRAY=1` keeps a supervisor a
/// test starts indirectly (through `anima start`) from showing a tray icon.
pub const NO_TRAY_ENV: &str = "ANIMA_NO_TRAY";

/// Whether `anima supervise` shows the tray: not with `--no-tray`, and not
/// with `--background` when there is no desktop session to show it in.
pub fn wanted(no_tray: bool, background: bool) -> bool {
    if no_tray || std::env::var_os(NO_TRAY_ENV).is_some_and(|value| value == "1") {
        return false;
    }
    !background || desktop_session()
}

/// macOS and Windows supervisors run in the user's desktop session; elsewhere
/// there is one when a display server is named.
fn desktop_session() -> bool {
    cfg!(any(target_os = "macos", windows))
        || ["DISPLAY", "WAYLAND_DISPLAY"]
            .iter()
            .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
}

/// Runs a closure when dropped, on unwinding as on a normal return.
#[cfg(any(target_os = "macos", windows, test))]
struct OnDrop<F: FnOnce()>(Option<F>);

#[cfg(any(target_os = "macos", windows, test))]
impl<F: FnOnce()> OnDrop<F> {
    fn new(run: F) -> Self {
        Self(Some(run))
    }
}

#[cfg(any(target_os = "macos", windows, test))]
impl<F: FnOnce()> Drop for OnDrop<F> {
    fn drop(&mut self) {
        if let Some(run) = self.0.take() {
            run();
        }
    }
}

#[cfg(test)]
mod on_drop_tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::OnDrop;

    #[test]
    fn fires_on_return() {
        let fired = AtomicBool::new(false);
        {
            let _guard = OnDrop::new(|| fired.store(true, Ordering::SeqCst));
        }
        assert!(fired.load(Ordering::SeqCst));
    }

    #[test]
    fn fires_when_a_panic_unwinds() {
        let fired = AtomicBool::new(false);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = OnDrop::new(|| fired.store(true, Ordering::SeqCst));
            panic!("the supervisor panicked");
        }));
        assert!(outcome.is_err());
        assert!(fired.load(Ordering::SeqCst));
    }
}

/// This build has no tray on this system: runs headless.
#[cfg(not(any(target_os = "macos", windows)))]
pub fn run(runtime: tokio::runtime::Runtime, supervisor: Supervisor) -> io::Result<()> {
    supervisor
        .link(runtime.handle().clone())
        .note("The tray is not available on this system; running without it");
    runtime.block_on(supervisor.run())
}

/// Runs the event loop with the tray on this thread and `supervisor` on
/// `runtime` in a background thread, until the supervisor ends.
#[cfg(any(target_os = "macos", windows))]
pub fn run(runtime: tokio::runtime::Runtime, supervisor: Supervisor) -> io::Result<()> {
    native::run(runtime, supervisor)
}

#[cfg(any(target_os = "macos", windows))]
mod native {
    use std::io;
    use std::time::Duration;

    use tao::event::{Event, StartCause};
    use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
    use tao::platform::run_return::EventLoopExtRunReturn;
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

    use super::icon;
    use super::menu::{icon_for, menu_model, MenuItemModel, TrayIconKind, SEPARATOR};
    use super::OnDrop;
    use crate::supervisor::{Action, Link, Supervisor, SupervisorStatus};

    /// How often the status is looked at for a change to show.
    const STATUS_POLL: Duration = Duration::from_millis(250);
    /// How long the supervisor's leftover tasks get once it has ended.
    const RUNTIME_SHUTDOWN: Duration = Duration::from_secs(2);
    /// macOS draws the icon as a template image.
    const TEMPLATE: bool = cfg!(target_os = "macos");

    enum UserEvent {
        Status(Box<SupervisorStatus>),
        Menu(String),
        /// The supervisor has ended.
        Exited,
    }

    /// The tray and what it shows now.
    struct Shown {
        tray: TrayIcon,
        model: Vec<MenuItemModel>,
        kind: TrayIconKind,
    }

    pub fn run(runtime: tokio::runtime::Runtime, supervisor: Supervisor) -> io::Result<()> {
        let link = supervisor.link(runtime.handle().clone());
        let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
        #[cfg(target_os = "macos")]
        {
            use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
            // A menu-bar item only: no Dock icon, no app menu.
            event_loop.set_activation_policy(ActivationPolicy::Accessory);
        }

        let supervising = {
            let proxy = event_loop.create_proxy();
            std::thread::Builder::new()
                .name("supervisor".to_string())
                .spawn(move || {
                    // Ends the event loop however this thread ends, a panic
                    // included, so no tray outlives the supervisor.
                    let _end_loop = OnDrop::new(move || {
                        let _ = proxy.send_event(UserEvent::Exited);
                    });
                    let result = runtime.block_on(supervisor.run());
                    runtime.shutdown_timeout(RUNTIME_SHUTDOWN);
                    result
                })?
        };
        link.runtime()
            .spawn(watch_status(link.clone(), event_loop.create_proxy()));
        let menu_proxy = event_loop.create_proxy();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let _ = menu_proxy.send_event(UserEvent::Menu(event.id.0));
        }));

        let mut shown: Option<Shown> = None;
        event_loop.run_return(move |event, _, control_flow| {
            *control_flow = ControlFlow::Wait;
            match event {
                // The earliest a tray icon may be made (macOS).
                Event::NewEvents(StartCause::Init) => match create(&link.status()) {
                    Ok(created) => shown = Some(created),
                    Err(error) => link.note(&format!(
                        "The tray icon could not be created ({error}); running without it"
                    )),
                },
                Event::UserEvent(UserEvent::Status(status)) => {
                    if let Some(shown) = shown.as_mut() {
                        shown.update(&status);
                    }
                }
                Event::UserEvent(UserEvent::Menu(id)) => {
                    on_click(&id, &link);
                    // A check item ticks itself: show what the status says
                    // until the change lands (or fails).
                    if let (Some(shown), "autostart") = (shown.as_mut(), id.as_str()) {
                        shown.model.clear();
                        shown.update(&link.status());
                    }
                }
                Event::UserEvent(UserEvent::Exited) => *control_flow = ControlFlow::Exit,
                _ => {}
            }
        });
        MenuEvent::set_event_handler(None::<fn(MenuEvent)>);
        supervising
            .join()
            .unwrap_or_else(|_| Err(io::Error::other("the supervisor thread panicked")))
    }

    /// Sends the status to the event loop each time it changes.
    async fn watch_status(link: Link, proxy: EventLoopProxy<UserEvent>) {
        let mut last: Option<SupervisorStatus> = None;
        loop {
            let status = link.status();
            if last.as_ref() != Some(&status) {
                if proxy
                    .send_event(UserEvent::Status(Box::new(status.clone())))
                    .is_err()
                {
                    return;
                }
                last = Some(status);
            }
            tokio::time::sleep(STATUS_POLL).await;
        }
    }

    fn create(status: &SupervisorStatus) -> Result<Shown, String> {
        let model = menu_model(status);
        let kind = icon_for(&status.state, status.update.available.is_some());
        let builder = TrayIconBuilder::new()
            .with_menu(Box::new(build_menu(&model)))
            .with_tooltip(tooltip(&model));
        #[cfg(target_os = "macos")]
        let builder = builder.with_icon_templated(tray_icon(kind)?);
        #[cfg(not(target_os = "macos"))]
        let builder = builder.with_icon(tray_icon(kind)?);
        let tray = builder.build().map_err(|error| error.to_string())?;
        Ok(Shown { tray, model, kind })
    }

    impl Shown {
        fn update(&mut self, status: &SupervisorStatus) {
            let model = menu_model(status);
            if model != self.model {
                self.tray.set_menu(Some(Box::new(build_menu(&model))));
                let _ = self.tray.set_tooltip(Some(tooltip(&model)));
                self.model = model;
            }
            let kind = icon_for(&status.state, status.update.available.is_some());
            if kind != self.kind {
                if let Ok(icon) = tray_icon(kind) {
                    #[cfg(target_os = "macos")]
                    let _ = self.tray.set_icon_templated(Some(icon));
                    #[cfg(not(target_os = "macos"))]
                    let _ = self.tray.set_icon(Some(icon));
                }
                self.kind = kind;
            }
        }
    }

    fn tray_icon(kind: TrayIconKind) -> Result<Icon, String> {
        Icon::from_rgba(icon::rgba(kind, TEMPLATE), icon::SIZE, icon::SIZE)
            .map_err(|error| error.to_string())
    }

    /// "Anima · <status line>".
    fn tooltip(model: &[MenuItemModel]) -> String {
        match model.iter().find(|item| item.id == "status") {
            Some(line) => format!("Anima · {}", line.label),
            None => "Anima".to_string(),
        }
    }

    fn build_menu(model: &[MenuItemModel]) -> Menu {
        let menu = Menu::new();
        for item in model {
            // Appending fails only for an item already in the menu.
            let _ = if item.id == SEPARATOR {
                menu.append(&PredefinedMenuItem::separator())
            } else if let Some(checked) = item.checked {
                menu.append(&CheckMenuItem::with_id(
                    item.id,
                    &item.label,
                    item.enabled,
                    checked,
                    None,
                ))
            } else {
                menu.append(&MenuItem::with_id(item.id, &item.label, item.enabled, None))
            };
        }
        menu
    }

    fn on_click(id: &str, link: &Link) {
        let action = match id {
            "open" => return open_console(link),
            "autostart" => return toggle_autostart(link),
            "start" => Action::Start,
            "stop" => Action::Stop,
            "restart" => Action::Restart,
            "update" => Action::Update,
            "check" => Action::CheckUpdate,
            "cancel" => Action::Cancel,
            "quit" => Action::Quit,
            _ => return,
        };
        link.request(action);
    }

    fn open_console(link: &Link) {
        let url = link.console_url();
        let task_link = link.clone();
        link.runtime().spawn_blocking(move || {
            let link = task_link;
            if let Err(error) = open::that(&url) {
                // Not the URL: its launch code is a secret.
                link.note(&format!("Could not open the console: {error}"));
            }
        });
    }

    /// The new setting reaches the menu with the status.
    fn toggle_autostart(link: &Link) {
        let on = !link.status().autostart;
        let task_link = link.clone();
        link.runtime().spawn_blocking(move || {
            let link = task_link;
            if let Err(error) = link.set_autostart(on) {
                link.note(&format!("Could not change start at login: {error}"));
            }
        });
    }
}
