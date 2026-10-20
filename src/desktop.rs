//! The desktop app: the Work Desk page in its own Mac window instead of a browser tab.
//!
//! The window shows the same page and API that `desk open` serves, but requests go
//! straight from the window to the router over a private `workdesk://` scheme, so
//! no network port is opened and it can run alongside the browser version.

use crate::config::tilde;
use crate::server::{self, App, PickRequest};
use anyhow::{Context, Result};
use axum::{
    body::Body,
    http::{header, HeaderValue, Request, Response},
    Router,
};
use muda::{
    accelerator::Accelerator, AboutMetadata, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu,
};
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::{Theme, WindowBuilder},
};
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use tower::ServiceExt;
use wry::{DragDropEvent, NewWindowResponse, WebViewBuilder};

const SCHEME: &str = "workdesk";
const HOME: &str = "workdesk://localhost/";
const ZOOM_STEPS: &[f64] = &[0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0];

/// Files dragged in from Finder, passed to the page so it can link them to what's under the pointer.
enum Drag {
    Over(i32, i32),
    Drop(Vec<PathBuf>, i32, i32),
    Leave,
}

enum UserEvent {
    Menu(MenuEvent),
    Pick(PickRequest),
    Drag(Drag),
}

struct MenuIds {
    reload: MenuId,
    actual_size: MenuId,
    zoom_in: MenuId,
    zoom_out: MenuId,
}

fn item(text: &str, accel: &str) -> MenuItem {
    MenuItem::new(text, true, accel.parse::<Accelerator>().ok())
}

/// The standard Mac menu bar. The Edit menu is what makes ⌘C / ⌘V / ⌘Z work in the page.
fn build_menu() -> Result<(Menu, MenuIds)> {
    let about = AboutMetadata {
        name: Some("Work Desk".into()),
        version: Some(env!("CARGO_PKG_VERSION").into()),
        comments: Some("Projects, kanban, timeline and sticky notes.".into()),
        ..Default::default()
    };
    let sep = PredefinedMenuItem::separator;
    let app_menu = Submenu::with_items(
        "Work Desk",
        true,
        &[
            &PredefinedMenuItem::about(Some("About Work Desk"), Some(about)),
            &sep(),
            &PredefinedMenuItem::services(None),
            &sep(),
            &PredefinedMenuItem::hide(Some("Hide Work Desk")),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &sep(),
            &PredefinedMenuItem::quit(Some("Quit Work Desk")),
        ],
    )?;
    let edit_menu = Submenu::with_items(
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(None),
            &PredefinedMenuItem::redo(None),
            &sep(),
            &PredefinedMenuItem::cut(None),
            &PredefinedMenuItem::copy(None),
            &PredefinedMenuItem::paste(None),
            &PredefinedMenuItem::select_all(None),
        ],
    )?;
    let reload = item("Reload", "CmdOrCtrl+R");
    let actual_size = item("Actual Size", "CmdOrCtrl+0");
    let zoom_in = item("Zoom In", "CmdOrCtrl+=");
    let zoom_out = item("Zoom Out", "CmdOrCtrl+-");
    let view_menu = Submenu::with_items(
        "View",
        true,
        &[&reload, &sep(), &actual_size, &zoom_in, &zoom_out, &sep(), &PredefinedMenuItem::fullscreen(None)],
    )?;
    let window_menu = Submenu::with_items(
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(None),
            &PredefinedMenuItem::maximize(Some("Zoom")),
            &sep(),
            &PredefinedMenuItem::close_window(None),
            &sep(),
            &PredefinedMenuItem::bring_all_to_front(None),
        ],
    )?;
    let menu = Menu::with_items(&[&app_menu, &edit_menu, &view_menu, &window_menu])?;
    menu.init_for_nsapp();
    window_menu.set_as_windows_menu_for_nsapp();
    let ids = MenuIds {
        reload: reload.id().clone(),
        actual_size: actual_size.id().clone(),
        zoom_in: zoom_in.id().clone(),
        zoom_out: zoom_out.id().clone(),
    };
    Ok((menu, ids))
}

/// Answer one request from the window with the same router the browser version uses.
async fn answer(router: Router, req: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let (mut parts, body) = req.into_parts();
    // The router's guard only serves local hosts; the window is local by definition.
    parts.headers.insert(header::HOST, HeaderValue::from_static("localhost"));
    let Ok(res) = router.oneshot(Request::from_parts(parts, Body::from(body))).await;
    let (parts, body) = res.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap_or_default();
    Response::from_parts(parts, bytes.to_vec())
}

/// Links that point off the page open in the default browser, never in the app window.
fn open_outside(url: &str) {
    if url.starts_with("https://") || url.starts_with("http://") || url.starts_with("mailto:") {
        server::open_url(url);
    }
}

/// Open the window and run until it is closed. Never returns.
pub fn run(mut app: App, rt: Runtime) -> Result<()> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let menu_proxy = proxy.clone();
    MenuEvent::set_event_handler(Some(move |e| {
        let _ = menu_proxy.send_event(UserEvent::Menu(e));
    }));
    // The file chooser has to run on the main thread, so requests are posted to the event loop.
    let pick_proxy = Mutex::new(proxy.clone());
    app.picker = Some(Arc::new(move |req| {
        let _ = pick_proxy.lock().unwrap().send_event(UserEvent::Pick(req));
    }));
    let router = server::router(app);
    let handle = rt.handle().clone();
    let (_menu, ids) = build_menu()?;

    let window = WindowBuilder::new()
        .with_title("Work Desk")
        .with_inner_size(LogicalSize::new(1320.0, 900.0))
        .with_min_inner_size(LogicalSize::new(720.0, 520.0))
        .build(&event_loop)
        .context("opening the window")?;
    // Match the page background so the window doesn't flash white while it loads.
    let bg = match window.theme() {
        Theme::Dark => (0x13, 0x14, 0x1c, 0xff),
        _ => (0xec, 0xee, 0xf5, 0xff),
    };

    // Only drags that carry files are ours; dragging cards between lanes stays with the page.
    let carrying_files = Rc::new(Cell::new(false));
    let drag_proxy = proxy.clone();
    let on_drag = move |e: DragDropEvent| {
        let msg = match e {
            DragDropEvent::Enter { paths, position: (x, y) } => {
                carrying_files.set(!paths.is_empty());
                Drag::Over(x, y)
            }
            DragDropEvent::Over { position: (x, y) } => Drag::Over(x, y),
            DragDropEvent::Drop { paths, position: (x, y) } => {
                carrying_files.set(!paths.is_empty());
                Drag::Drop(paths, x, y)
            }
            DragDropEvent::Leave => Drag::Leave,
            _ => return false,
        };
        if !carrying_files.get() {
            return false;
        }
        if matches!(msg, Drag::Drop(..) | Drag::Leave) {
            carrying_files.set(false);
        }
        let _ = drag_proxy.send_event(UserEvent::Drag(msg));
        true
    };

    let webview = WebViewBuilder::new()
        .with_asynchronous_custom_protocol(SCHEME.into(), move |_id, req, responder| {
            let router = router.clone();
            handle.spawn(async move { responder.respond(answer(router, req).await) });
        })
        .with_url(HOME)
        .with_initialization_script("window.__deskApp = true;")
        .with_drag_drop_handler(on_drag)
        .with_background_color(bg)
        .with_navigation_handler(|url| {
            if url.starts_with("workdesk://") || url == "about:blank" {
                return true;
            }
            open_outside(&url);
            false
        })
        .with_new_window_req_handler(|url, _| {
            open_outside(&url);
            NewWindowResponse::Deny
        })
        .with_devtools(cfg!(debug_assertions))
        .build(&window)
        .context("starting the web view")?;

    let mut zoom = ZOOM_STEPS.iter().position(|&z| z == 1.0).unwrap();
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        // The runtime answers the window's requests, so it lives as long as the window.
        let _ = &rt;
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => *control_flow = ControlFlow::Exit,
            Event::UserEvent(UserEvent::Pick(req)) => {
                let mut dialog = rfd::FileDialog::new().set_title(if req.folder { "Link folders" } else { "Link files" });
                if let Some(dir) = &req.start {
                    dialog = dialog.set_directory(dir);
                }
                let picked = if req.folder { dialog.pick_folders() } else { dialog.pick_files() };
                let _ = req.reply.send(picked.unwrap_or_default());
            }
            Event::UserEvent(UserEvent::Drag(d)) => {
                // Drag positions are in window points; the page measures in CSS pixels.
                let z = ZOOM_STEPS[zoom];
                let at = |x: i32, y: i32| format!("{}, {}", (x as f64 / z).round(), (y as f64 / z).round());
                let js = match d {
                    Drag::Over(x, y) => format!("window.__deskDrop && window.__deskDrop.over({})", at(x, y)),
                    Drag::Leave => "window.__deskDrop && window.__deskDrop.leave()".to_string(),
                    Drag::Drop(paths, x, y) => {
                        let paths: Vec<String> = paths.iter().map(|p| tilde(p)).collect();
                        let paths = serde_json::to_string(&paths).unwrap_or_else(|_| "[]".into());
                        format!("window.__deskDrop && window.__deskDrop.drop({paths}, {})", at(x, y))
                    }
                };
                let _ = webview.evaluate_script(&js);
            }
            Event::UserEvent(UserEvent::Menu(e)) => {
                let id = e.id();
                if id == &ids.reload {
                    let _ = webview.reload();
                    return;
                }
                if id == &ids.actual_size {
                    zoom = ZOOM_STEPS.iter().position(|&z| z == 1.0).unwrap();
                } else if id == &ids.zoom_in {
                    zoom = (zoom + 1).min(ZOOM_STEPS.len() - 1);
                } else if id == &ids.zoom_out {
                    zoom = zoom.saturating_sub(1);
                } else {
                    return;
                }
                let _ = webview.zoom(ZOOM_STEPS[zoom]);
            }
            _ => {}
        }
    })
}
