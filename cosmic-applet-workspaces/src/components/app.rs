// Copyright 2023 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

use cctk::{
    sctk::reexports::{calloop::channel::SyncSender, protocols::ext::workspace::v1::client::ext_workspace_handle_v1},
    workspace::Workspace,
};
use cosmic::{
    Element, Task, app,
    applet::PanelType,
    iced::{Subscription, event, mouse::{self, ScrollDelta}, Event::Mouse},
    scroll::DiscreteScrollState,
    widget::icon,
};

use crate::{
    config,
    wayland::WorkspaceEvent,
    wayland_subscription::{WorkspacesUpdate, workspaces},
};

use std::time::Duration;

const SCROLL_RATE_LIMIT: Duration = Duration::from_millis(200);

// Same mark in two dresses, because the two bars have different conventions:
// the status panel is monochrome symbolic, the dock is full colour. Embedded
// so the applet does not depend on an icon theme shipping them.
const ICON_SYMBOLIC: &[u8] = include_bytes!("../../data/icons/workspace-switcher-symbolic.svg");
const ICON_COLOUR: &[u8] = include_bytes!("../../data/icons/workspace-switcher.svg");

pub fn run() -> cosmic::iced::Result {
    cosmic::applet::run::<IcedWorkspacesApplet>(())
}

// Client proxy for the `cosmic-workspaces` daemon's own D-Bus interface,
// which already owns a real floating popup (frosted, live-thumbnail grid,
// separate from its full-screen overview). This applet is just a button
// that toggles it — no capture/rendering logic lives here.
#[zbus::proxy(
    interface = "com.system76.CosmicWorkspaces",
    default_service = "com.system76.CosmicWorkspaces",
    default_path = "/com/system76/CosmicWorkspaces"
)]
trait CosmicWorkspacesDbus {
    fn toggle_popup(&self) -> zbus::Result<()>;
}

async fn toggle_workspaces_popup() {
    let Ok(conn) = zbus::Connection::session().await else {
        return;
    };
    let Ok(proxy) = CosmicWorkspacesDbusProxy::new(&conn).await else {
        return;
    };
    let _ = proxy.toggle_popup().await;
}

struct IcedWorkspacesApplet {
    core: cosmic::app::Core,
    workspaces: Vec<Workspace>,
    workspace_tx: Option<SyncSender<WorkspaceEvent>>,
    scroll: DiscreteScrollState,
}

#[derive(Debug, Clone)]
enum Message {
    WorkspaceUpdate(WorkspacesUpdate),
    WheelScrolled(ScrollDelta),
    TogglePopup,
}

impl cosmic::Application for IcedWorkspacesApplet {
    type Message = Message;
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    const APP_ID: &'static str = config::APP_ID;

    fn init(core: cosmic::app::Core, _flags: Self::Flags) -> (Self, app::Task<Self::Message>) {
        (
            Self {
                core,
                workspaces: Vec::new(),
                workspace_tx: Option::default(),
                scroll: DiscreteScrollState::default().rate_limit(Some(SCROLL_RATE_LIMIT)),
            },
            Task::none(),
        )
    }

    fn core(&self) -> &cosmic::app::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::app::Core {
        &mut self.core
    }

    fn update(&mut self, message: Self::Message) -> app::Task<Self::Message> {
        match message {
            Message::WorkspaceUpdate(msg) => match msg {
                WorkspacesUpdate::Workspaces(mut list) => {
                    list.retain(|w| !w.state.contains(ext_workspace_handle_v1::State::Hidden));
                    list.sort_by(|w1, w2| w1.coordinates.cmp(&w2.coordinates));
                    self.workspaces = list;
                }
                WorkspacesUpdate::Started(tx) => {
                    self.workspace_tx.replace(tx);
                }
                WorkspacesUpdate::Errored => {
                    // TODO
                }
            },
            Message::WheelScrolled(delta) => {
                let discrete_delta = self.scroll.update(delta);
                if discrete_delta.y != 0 {
                    if let Some(w_i) = self
                        .workspaces
                        .iter()
                        .position(|w| w.state.contains(ext_workspace_handle_v1::State::Active))
                    {
                        let d_i = (w_i as isize - discrete_delta.y)
                            .rem_euclid(self.workspaces.len() as isize)
                            as usize;

                        if let Some(tx) = self.workspace_tx.as_mut() {
                            let _ = tx.try_send(WorkspaceEvent::Activate(
                                self.workspaces[d_i].handle.clone(),
                            ));
                        }
                    }
                }
            }
            Message::TogglePopup => {
                return Task::perform(toggle_workspaces_popup(), |_| cosmic::Action::None);
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        if self.workspaces.is_empty() {
            return cosmic::iced::widget::row![].padding(8).into();
        }

        // In the dock this sits beside real app icons: full colour, app-icon
        // metrics. In the status panel it must be a monochrome symbolic glyph
        // at the smaller symbolic metrics. `suggested_size`/`suggested_padding`
        // take `is_symbolic`, so the same flag drives both.
        let in_dock = matches!(self.core.applet.panel_type, PanelType::Dock);
        let symbolic = !in_dock;

        let suggested_size = self.core.applet.suggested_size(symbolic);
        let applet_padding = self.core.applet.suggested_padding(symbolic);

        // `.symbolic(true)` recolours the SVG to a single theme colour, which
        // is wanted in the panel and would destroy the artwork in the dock.
        let handle = if symbolic {
            icon::from_svg_bytes(ICON_SYMBOLIC).symbolic(true)
        } else {
            icon::from_svg_bytes(ICON_COLOUR)
        };

        let btn = cosmic::widget::button::custom(icon::icon(handle).size(suggested_size.0))
        .on_press_down(Message::TogglePopup)
        .class(cosmic::theme::Button::AppletIcon)
        .padding([applet_padding.1, applet_padding.0]);

        self.core.applet.autosize_window(btn).into()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            workspaces().map(Message::WorkspaceUpdate),
            event::listen_with(|e, _, _| match e {
                Mouse(mouse::Event::WheelScrolled { delta }) => Some(Message::WheelScrolled(delta)),
                _ => None,
            }),
        ])
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}
