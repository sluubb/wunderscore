use std::error::Error;

use backend::{vulkan, Backend};
use winit::dpi::LogicalSize;
use winit::event::{Event, WindowEvent};
use winit::event_loop::EventLoop;
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::WindowBuilder;

use w_gfx::*;

fn main() -> Result<(), Box<dyn Error>> {
    pretty_env_logger::init();

    let event_loop = EventLoop::new()?;
    let window = WindowBuilder::new()
        .with_title("Vulkan Tutorial (Rust)")
        .with_inner_size(LogicalSize::new(1024, 768))
        .build(&event_loop)?;

    let mut vulkan = vulkan::Vulkan::new(window.display_handle()?)?;
    vulkan.create_surface(window.display_handle()?, window.window_handle()?)?;
    vulkan.create_device()?;

    event_loop.run(move |event, elwt| match event {
        Event::AboutToWait => window.request_redraw(),
        Event::WindowEvent { event, .. } => match event {
            WindowEvent::RedrawRequested if !elwt.exiting() => (),
            WindowEvent::CloseRequested => {
                elwt.exit();
                vulkan.destroy();
            }
            _ => {}
        },
        _ => {}
    })?;

    Ok(())
}
