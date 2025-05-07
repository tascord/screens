use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::process::Command;
use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::xproto::*;

fn main() {
    if env::args().nth(1) == Some("-info".to_string()) {
        let mut screens = get_screen_info();
        screens.sort_by_key(|screen| screen.pos.0);
        for screen in screens {
            println!(
                "Monitor {} at ({}, {})",
                screen.id, screen.pos.0, screen.pos.1
            );
        }
    } else {
        let config = match fs::read_to_string("config.json") {
            Ok(content) => content,
            Err(_) => {
                let screens = get_screen_info();
                let default_displays: HashMap<u32, Display> = screens
                    .iter()
                    .map(|screen| {
                        (
                            screen.id,
                            Display::Webpage {
                                url: "https://oopsallmarquees.com/".to_string(),
                            },
                        )
                    })
                    .collect();
                let default_config = serde_json::to_string_pretty(&default_displays).unwrap();
                fs::write("config.json", &default_config).expect("Failed to write default config");
                default_config
            }
        };
        let displays: HashMap<u32, Display> =
            serde_json::from_str(&config).expect("Failed to parse config");
        let screens = get_screen_info();
        create_displays(&screens, &displays);
    }
}

fn get_screen_info() -> Vec<Monitor> {
    let connect = x11rb::connect(None).unwrap();
    let (conn, _) = connect;
    let setup = conn.setup();
    let mut screens = Vec::new();

    let resources = conn
        .randr_get_screen_resources(setup.roots[0].root)
        .unwrap()
        .reply()
        .unwrap();

    for output in resources.outputs {
        if let Ok(info) = conn.randr_get_output_info(output, 0).unwrap().reply() {
            if info.crtc != 0 && info.connection == 0.into() {
                // 0 means Connected
                if let Ok(crtc_info) = conn.randr_get_crtc_info(info.crtc, 0).unwrap().reply() {
                    screens.push(Monitor {
                        pos: (crtc_info.x, crtc_info.y),
                        id: output,
                    });
                }
            }
        }
    }
    screens
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum Display {
    Webpage {
        url: String,
    },
    Split {
        vertical: bool,
        items: Vec<Box<Display>>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Monitor {
    pos: (i16, i16),
    id: u32,
}

fn create_displays(monitors: &[Monitor], displays: &HashMap<u32, Display>) {
    let (conn, _) = x11rb::connect(None).unwrap();

    for monitor in monitors {
        if let Some(display) = displays.get(&monitor.id) {
            spawn_display(&conn, monitor, display, monitor.pos, (800, 600));
        }
    }
}

fn spawn_display(
    conn: &impl Connection,
    monitor: &Monitor,
    display: &Display,
    pos: (i16, i16),
    size: (u16, u16),
) {
    match display {
        Display::Webpage { url } => {
            // Move the window to the correct position using i3-msg
            // Create a unique class name using monitor ID and position
            let class_name = format!("screen_firefox_{}_{}_{}", monitor.id, pos.0, pos.1);

            // Launch Firefox
            Command::new("firefox")
                .args(["--new-window", url])
                .args(["--class", &class_name])
                .spawn()
                .expect("Failed to spawn browser");

            // Give the window a moment to appear
            std::thread::sleep(std::time::Duration::from_secs(3));

            // Get window position using xwininfo
            let class_selector = format!("{class_name}");
            let output = Command::new("xdotool")
                .args(["search", "--class", &class_selector])
                .output()
                .expect("Failed to get window ID");

            let window_id = String::from_utf8_lossy(&output.stdout).trim().to_string();

            let info = Command::new("xwininfo")
                .args(["-id", &window_id])
                .output()
                .expect("Failed to get window info");

            let info_str = String::from_utf8_lossy(&info.stdout);
            let current_x = info_str
                .lines()
                .find(|l| l.contains("Absolute upper-left X:"))
                .and_then(|l| l.split(':').nth(1))
                .and_then(|x| x.trim().parse::<i16>().ok())
                .unwrap_or(0);
            let current_y = info_str
                .lines()
                .find(|l| l.contains("Absolute upper-left Y:"))
                .and_then(|l| l.split(':').nth(1))
                .and_then(|y| y.trim().parse::<i16>().ok())
                .unwrap_or(0);

            let dx = pos.0 - current_x;
            let dy = pos.1 - current_y;

            let mut c = Command::new("i3-msg");
            let c = c.args([
                format!("[class=\"{class_name}\"]").as_str(),
                "floating enable,",
                "move",
                "left",
                &format!("{}", dx.abs()),
                "px",
                if dx >= 0 { "right" } else { "left" },
                &format!("{}", dy.abs()),
                "px",
                if dy >= 0 { "down" } else { "up" },
            ]);

            println!(
                "{} {}",
                c.get_program().to_string_lossy(),
                c.get_args()
                    .map(|v| v.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            c.output().expect("Failed to move window");
        }
        Display::Split { vertical, items } => {
            let item_count = items.len() as u16;
            let (split_dim, fixed_dim) = if *vertical {
                (size.1 / item_count, size.0)
            } else {
                (size.0 / item_count, size.1)
            };

            for (i, item) in items.iter().enumerate() {
                let new_pos = if *vertical {
                    (pos.0, pos.1 + (i as i16 * split_dim as i16))
                } else {
                    (pos.0 + (i as i16 * split_dim as i16), pos.1)
                };

                let new_size = if *vertical {
                    (fixed_dim, split_dim)
                } else {
                    (split_dim, fixed_dim)
                };

                spawn_display(conn, monitor, item, new_pos, new_size);
            }
        }
    }

    conn.flush().unwrap();
}
