#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    vetochki::run(winit::event_loop::EventLoop::new().expect("event loop"));
}
