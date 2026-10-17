//! Browser entry point; native workspace checks build an empty binary.

#[cfg(target_arch = "wasm32")]
fn main() {
    vetochki::run(winit::event_loop::EventLoop::new().expect("event loop"));
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {}
