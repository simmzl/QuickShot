use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

#[derive(Debug, Clone)]
pub enum UserEvent {
    HotkeyFired,
}

pub struct App {
    // Placeholder; fleshed out in Task 6/7.
}

impl App {
    pub fn new() -> Self {
        Self {}
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _id: WindowId,
        _event: WindowEvent,
    ) {
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::HotkeyFired => {
                println!("hotkey fired");
            }
        }
    }
}
