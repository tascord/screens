use serde::{Deserialize, Serialize};
use smithay::output::Output;
use smithay::{
    delegate_compositor, delegate_data_device, delegate_seat, delegate_shm, delegate_xdg_shell,
    desktop::{Space, Window},
    input::{Seat, SeatState},
    reexports::wayland_server::{Display as WlDisplay, DisplayHandle},
    wayland::{
        compositor::{CompositorClientState, CompositorState},
        selection::data_device::DataDeviceState,
        shell::xdg::XdgShellState,
        shm::ShmState,
    },
};
use std::{collections::HashMap, fs};
use wayland_server::backend::{ClientData, ClientId, DisconnectReason};

mod basic;

#[derive(Serialize, Deserialize, Clone)]
enum Display {
    Webpage {
        url: String,
    },
    Split {
        vertical: bool,
        items: Vec<Box<Display>>,
    },
}

pub struct App {
    display_handle: DisplayHandle,
    space: Space<Window>,
    compositor_state: CompositorState,
    xdg_shell_state: XdgShellState,
    seat_state: SeatState<App>,
    displays: HashMap<u32, Display>,
    data_device_state: DataDeviceState,
    shm_state: ShmState,
    seat: Seat<Self>,
}

#[derive(Default)]
pub struct ClientState {
    compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {
        println!("initialized");
    }

    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {
        println!("disconnected");
    }
}

impl App {
    fn new() -> Self {
        let display: WlDisplay<ClientState> = WlDisplay::new().expect("Failed to create display");
        let display_handle = display.handle();

        let compositor_state = CompositorState::new::<Self>(&display_handle);
        let xdg_shell_state = XdgShellState::new::<Self>(&display_handle);
        let mut seat_state = SeatState::new();

        let displays = match fs::read_to_string("config.json") {
            Ok(content) => serde_json::from_str(&content).expect("Failed to parse config"),
            Err(err) => {
                eprintln!("Warning: Failed to read 'config.json': {}", err);
                HashMap::new()
            }
        };

        let data_device_state = DataDeviceState::new::<Self>(&display_handle);
        let shm_state = ShmState::new::<Self>(&display_handle, vec![]);
        let seat = seat_state.new_wl_seat(&display_handle, "pickle");

        Self {
            display_handle,
            space: Space::default(),
            compositor_state,
            xdg_shell_state,
            seat_state,
            displays,
            data_device_state,
            shm_state,
            seat,
        }
    }

    fn spawn_configured_windows(&mut self) {
        for (id, display) in self.displays.clone() {
            self.spawn_display(id, &display, None);
        }
    }

    fn spawn_display(
        &mut self,
        id: u32,
        display: &Display,
        window_info: Option<(i32, i32, i32, i32)>,
    ) {
        let window_info = window_info.unwrap_or_else(|| {
            // Get all outputs (monitors) and their positions
            let outputs: Vec<&Output> = self.space.outputs().collect();
            if outputs.is_empty() {
                // Fallback if no outputs
                return (0, 0, 800, 600);
            }

            // Get logical position and size of the first output
            let o = outputs[0];
            let info = o.current_mode().unwrap();
            let position = self.space.output_geometry(o).unwrap();

            (
                position.loc.x,
                position.loc.y,
                info.size.w as i32,
                info.size.h as i32,
            )
        });

        match display {
            Display::Webpage { url } => {
                println!("Spawning Firefox for URL: {}", url);
                let window_class = format!("firefox_window_{}", id);

                let (x, y, width, height) = window_info;
                let mut command = std::process::Command::new("firefox");
                command.args([
                    "--new-window",
                    url,
                    "--class",
                    &window_class,
                    "--width",
                    &width.to_string(),
                    "--height",
                    &height.to_string(),
                    "--geometry",
                    &format!("{}x{}+{}+{}", width, height, x, y),
                ]);

                let _ = command.spawn();
            }
            Display::Split { vertical, items } => {
                let (start_x, start_y, total_width, total_height) = window_info;
                let total_items = items.len();

                for (index, item) in items.iter().enumerate() {
                    // Calculate subdivision size and position
                    let sub_info = if *vertical {
                        let height = total_height / total_items as i32;
                        let y = start_y + (index as i32 * height);
                        Some((start_x, y, total_width, height))
                    } else {
                        let width = total_width / total_items as i32;
                        let x = start_x + (index as i32 * width);
                        Some((x, start_y, width, total_height))
                    };

                    // Generate unique ID for subdivision
                    let sub_id = id * 100 + index as u32;
                    self.spawn_display(sub_id, item, sub_info);
                }
            }
        }
    }
}

delegate_xdg_shell!(App);
delegate_compositor!(App);
delegate_shm!(App);
delegate_seat!(App);
delegate_data_device!(App);

fn main() {
    let mut wm = App::new();
    wm.spawn_configured_windows();

    // Main event loop with minimal window management
    loop {
        // Keep the display alive but don't allow window movements
        wm.display_handle.flush_clients().expect("Failed to flush");
        // Optional: Add a small sleep to prevent CPU spinning
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}
