extern crate gl;
extern crate imgui_opengl_renderer;
//SDL
use std::{collections::VecDeque, error::Error, path::PathBuf};

use cpu::GB_CYCLE_SPEED;
use imgui::{im_str, Context, Slider, Ui};

use imgui_opengl_renderer::Renderer;
use sdl2::keyboard::Keycode;
use sdl2::pixels::PixelFormatEnum;
use sdl2::rect::Rect;
use sdl2::render::Texture;
use sdl2::video::Window;
use sdl2::{event::Event, video::GLContext};
use std::time::Duration;
use std::time::Instant;

//File IO
use log::info;

use gpu::{PixelData};
use rust_emu::{cpu::JOYPAD, emu::gen_il, emu::Emu, emu::IL};
use structopt::StructOpt;

use rust_emu::*;

pub const CYCLES_PER_FRAME: usize = GB_CYCLE_SPEED / 60;
const FRAME_TIME: Duration = Duration::from_nanos(16670000);

#[derive(StructOpt)]
#[structopt(name = ".rsboy", about = "Rust emulator")]
struct Settings {
    #[structopt(parse(from_os_str))]
    input: PathBuf,
    #[structopt(parse(from_os_str))]
    logfile: Option<PathBuf>,
    #[structopt(short = "-b")]
    bootrom: Option<PathBuf>,
    #[structopt(short = "-r")]
    repl: bool,
}

type MaybeErr<T> = Result<T, Box<dyn Error>>;

fn setup_logger() -> MaybeErr<()> {
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{}][{}:{}] {}",
                record.level(),
                record.file().unwrap(),
                record.line().unwrap(),
                message
            ))
        })
        // Output to stdout, files, and other Dispatch configurations
        .chain(std::io::stdout())
        .chain(fern::log_file("output.log")?)
        // Apply globally
        .apply()
        .map_err(|x| x.into())
}


fn main() -> MaybeErr<()> {
    let settings = Settings::from_args();
    if let Some(_output) = settings.logfile {
        info!("Setup logging");
        setup_logger()?;
    }
    info!("Running SDL Main");
    sdl_main(settings.input, settings.bootrom, settings.repl)
}

// fn calc_relative_error(x: f32, y: f32) -> f32 {
//     (x - y) * 100.0 / x
// }
#[derive(Default)]
struct Info {
    frame_times: VecDeque<f32>,
    il: Vec<IL>,
}

struct Imgui<'a> {
    imgui: Context,
    renderer: Renderer,
    window: &'a Window,
    _gl_context: GLContext,
    info: Info,
}

impl<'a> Imgui<'a> {
    fn new(window: &'a Window) -> MaybeErr<Self> {
        let mut imgui = imgui::Context::create();
        imgui.fonts().build_rgba32_texture();
        let _gl_context = window.gl_create_context()?;
        gl::load_with(|s| window.subsystem().gl_get_proc_address(s) as _);

        let renderer = imgui_opengl_renderer::Renderer::new(&mut imgui, |s| {
            window.subsystem().gl_get_proc_address(s) as _
        });

        Ok(Self {
            imgui,
            renderer,
            window,
            _gl_context,
            info: Default::default(),
        })
    }
    fn capture_io(&mut self, event_pump: &mut sdl2::EventPump) {
        let io = self.imgui.io_mut();
        let state = event_pump.mouse_state();
        let (width, height) = self.window.drawable_size();
        io.display_size = [width as f32, height as f32];
        io.mouse_down = [
            state.left(),
            state.right(),
            state.middle(),
            state.x1(),
            state.x2(),
        ];
        io.mouse_pos = [state.x() as f32, state.y() as f32];
    }
    fn frame<F: FnOnce(&mut Info, &Ui)>(&mut self, event_pump: &mut sdl2::EventPump, f: F) {
        self.capture_io(event_pump);
        let ui = self.imgui.frame();
        unsafe {
            gl::ClearColor(0.2, 0.2, 0.2, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT);
        }
        f(&mut self.info, &ui);
        self.renderer.render(ui);
        self.window.gl_swap_window();
    }
    fn add_frame_time(&mut self, time: f32) {
        self.info.frame_times.push_back(time * 1000.0);
        if self.info.frame_times.len() > 200 {
            self.info.frame_times.pop_front();
        }
    }
}

use std::thread;
use rustyline::error::ReadlineError;

enum REPL {
    Kill
}

fn sdl_main(input: PathBuf, bootrom: Option<PathBuf>, repl: bool) -> MaybeErr<()> {
    let mut emu = Emu::from_path(input, bootrom)?;
    let context = sdl2::init()?;
    let video = context.video()?;
    {
        let gl_attr = video.gl_attr();
        gl_attr.set_context_profile(sdl2::video::GLProfile::Core);
        gl_attr.set_context_version(3, 0);
    }

    let mut rsboy = video
        .window(".rsboy", WINDOW_WIDTH * 3, WINDOW_HEIGHT * 3)
        .position_centered()
        .opengl()
        .build()?
        .into_canvas()
        .build()?;
    let tc = rsboy.texture_creator();
    let mut texture =
        tc.create_texture_streaming(PixelFormatEnum::RGBA32, WINDOW_WIDTH, WINDOW_HEIGHT)?;

    let debugger = video
        .window("debugger", 512, 512)
        .position(0, 20)
        .opengl()
        .resizable()
        .build()?;

    let mut debugger = Imgui::new(&debugger)?;
    let mut cycle_jump = 0;
    let mut pause = true;

    let mut event_pump = context.event_pump()?;

    let il = gen_il(&emu.bus.memory);
    debugger.info.il = il;

    use std::sync::mpsc::channel;

    let (sender, receiver) = channel();

    if repl {
        thread::spawn(move ||{
            let mut rl = rustyline::Editor::<()>::new();
            loop {
                let readline = rl.readline(">> ");
                match readline {
                    Ok(line) => {
                        println!("Line: {:?}", line)
                    },
                    Err(ReadlineError::Interrupted) => {
                        println!("CTRL-C");
                        break
                    },
                    Err(ReadlineError::Eof) => {
                        println!("CTRL-D");
                        break
                    },
                    Err(_) => println!("No input"),
                }
            }
            // Send a kill.
            sender.send(REPL::Kill);
        });
    }

    'running: loop {
        let now = Instant::now();
        if let Ok(REPL::Kill) = receiver.try_recv() {
            break;
        }
        for event in event_pump.poll_iter() {
            emu.bus.directions |= 0x0F;
            emu.bus.keypresses |= 0x0F;
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => break 'running,
                Event::KeyDown {
                    keycode: Some(keycode),
                    ..
                } => match keycode {
                    Keycode::Down => {
                        emu.bus.directions &= !0b1000;
                        emu.bus.int_flags |= JOYPAD;
                    }
                    Keycode::Up => {
                        emu.bus.directions &= !0b0100;
                        emu.bus.int_flags |= JOYPAD;
                    }
                    Keycode::Left => {
                        emu.bus.directions &= !0b0010;
                        emu.bus.int_flags |= JOYPAD;
                    }
                    Keycode::Right => {
                        emu.bus.directions &= !0b0001;
                        emu.bus.int_flags |= JOYPAD;
                    }
                    Keycode::Return => {
                        emu.bus.keypresses &= !0b1000;
                        emu.bus.int_flags |= JOYPAD;
                    }
                    Keycode::Z => {
                        //A?
                    }
                    Keycode::B => {
                        //B?
                    }
                    key => {
                        println!("{:?}", key);
                    }
                },
                Event::MouseWheel { y, .. } => {
                    debugger.imgui.io_mut().mouse_wheel = y as f32;
                }
                _ => {}
            }
        }

        let mut delta_clock = 0;
        if !pause {
            let before = emu.bus.clock;
            while emu.bus.clock < before + CYCLES_PER_FRAME {
                emu.emulate_step();
            }
            delta_clock = emu.bus.clock - before;
        }
        // Render to framebuffer and copy.
        {
            let time = now.elapsed();
            emu.bus.gpu.render(&mut emu.framebuffer);
            let (h, v) = emu.bus.gpu.scroll();
            texture.copy_window(h, v, &emu.framebuffer);
            rsboy.copy(&texture, None, None).unwrap();
            rsboy.present();
            delay_min(time);
        }
        let after_delay = now.elapsed();
        debugger.add_frame_time(after_delay.as_secs_f32());

        //ImGui display frame.
        debugger.frame(&mut event_pump, |info, ui| {
            ui.text(format!("Frame time: {:?}", after_delay));
            let i = info.frame_times.make_contiguous();
            ui.plot_lines(im_str!("Frame times"), i)
                .graph_size([300.0, 100.0])
                .build();
            let cpu_hz = delta_clock as f64 / after_delay.as_secs_f64();
            ui.text(format!("CPU HZ: {}", cpu_hz));
            ui.text(format!("Register State:\n{}", emu.cpu.registers));
            if ui.button(im_str!("Pause"), [200.0, 50.0]) {
                println!("Pause");
                pause = !pause;
            }
            ui.input_int(im_str!("Run for n cycles"), &mut cycle_jump)
                .build();
            Slider::new(im_str!(""))
                .range(0..=(69905))
                .build(ui, &mut cycle_jump);
            if ui.button(im_str!("Go"), [200.0, 50.0]) {
                let before = emu.bus.clock as i32;
                while emu.bus.clock < (before + cycle_jump) as usize {
                    emu.emulate_step();
                }
            }
            ui.text(format!("Bus Info:\n{}", emu.bus));
            ui.text(format!("GPU Info:\n{}", emu.bus.gpu));
            if ui.button(im_str!("Hex Dump"), [200.0, 50.0]) {
                emu.bus.gpu.hex_dump()
            }
            if ui.button(im_str!("Frame"), [200.0, 50.0]) {
                println!("Frame");
                let before = emu.bus.clock;
                while emu.bus.clock < before + CYCLES_PER_FRAME {
                    emu.emulate_step();
                }
            }
        });
    }
    std::mem::drop(event_pump);

    map_viewer(&context, &emu)?;
    vram_viewer(&context, &emu)?;
    Ok(())
}

const WINDOW_HEIGHT: u32 = 144;
const WINDOW_WIDTH: u32 = 160;
const MAP_WIDTH: u32 = 256;

trait GBWindow {
    fn copy_window(&mut self, h: u32, v: u32, buffer: &PixelData);
    fn copy_map(&mut self, buffer: &PixelData);
}
impl GBWindow for Texture<'_> {
    fn copy_window(&mut self, horz: u32, vert: u32, framebuffer: &PixelData) {
        self.with_lock(None, |buffer, _| {
            let mut i = 0;
            for y in vert..vert + WINDOW_HEIGHT {
                let y = (y % MAP_WIDTH) as usize;
                for x in horz..horz + WINDOW_WIDTH {
                    let x = (x % MAP_WIDTH) as usize;
                    let bytes = framebuffer[y][x].to_be_bytes();
                    buffer[i..(i + 4)].copy_from_slice(&bytes);
                    i += 4;
                }
            }
        })
        .unwrap();
    }
    fn copy_map(&mut self, buffer: &PixelData) {
        let mut i = 0;
        self.with_lock(None, |tbuffer, _| {
            for y in buffer.iter() {
                for x in y.iter() {
                    let bytes = x.to_be_bytes();
                    tbuffer[i..(i + 4)].copy_from_slice(&bytes);
                    i += 4;
                }
            }
        })
        .unwrap();
    }
}

fn delay_min(elapsed: Duration) {
    if let Some(time) = FRAME_TIME.checked_sub(elapsed) {
        spin_sleep::sleep(time);
    }
}

fn map_viewer(sdl_context: &sdl2::Sdl, emu: &emu::Emu) -> Result<(), String> {
    let gpu = &emu.bus.gpu;
    let video_subsystem = sdl_context.video()?;
    let window = video_subsystem
        .window("Map Viewer", 256, 256)
        .position_centered()
        .build()
        .map_err(|e| e.to_string())?;
    let mut canvas = window.into_canvas().build().map_err(|e| e.to_string())?;

    let texture_creator = canvas.texture_creator();
    let mut texture = texture_creator
        .create_texture_streaming(PixelFormatEnum::RGBA32, 256, 256)
        .map_err(|e| e.to_string())?;

    // Pitch = n_bytes(3) * map_w * tile_w
    texture.copy_map(&emu.framebuffer);
    canvas.copy(&texture, None, None)?;
    let (h, v) = gpu.scroll();
    println!("{} {}", h, v);
    canvas
        .draw_rect(Rect::from((
            h as i32,
            v as i32,
            WINDOW_WIDTH,
            WINDOW_HEIGHT,
        )))
        .unwrap();
    canvas.present();
    let mut event_pump = sdl_context.event_pump()?;

    'running: loop {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => break 'running,
                _ => {}
            }
        }

        ::std::thread::sleep(Duration::new(0, 1_000_000_000u32 / 30));
        // The rest of the game loop goes here...
    }

    Ok(())
}

fn vram_viewer(sdl_context: &sdl2::Sdl, emu: &emu::Emu) -> MaybeErr<()> {
    let gpu = &emu.bus.gpu;
    let video_subsystem = sdl_context.video()?;
    let window = video_subsystem
        .window("VRAM Viewer", 1024, 512)
        .position_centered()
        .build()?;
    let mut canvas = window.into_canvas().build()?;

    let texture_creator = canvas.texture_creator();

    let mut update = |palette: u8| -> MaybeErr<()> {
        let tiles = gpu.tiles(palette);
        for (i, t) in tiles.iter().enumerate() {
            let i = i as i32;
            let mut tex =
                texture_creator.create_texture_streaming(PixelFormatEnum::RGBA32, 8, 8)?;
            tex.with_lock(None, |data, _| {
                let mut c = 0;
                for i in t.texture.iter() {
                    for j in i.iter() {
                        let d = j.to_be_bytes();
                        data[c..(c + 4)].copy_from_slice(&d);
                        c += 4;
                    }
                }
            })?;
            let rect = ((i % 32) * 32, (i / 32) * 32, 32, 32);
            let rect = Rect::from(rect);
            canvas.copy(&tex, None, rect)?
        }
        canvas.present();
        Ok(())
    };
    let ps = [gpu.bgrdpal, gpu.obj0pal, gpu.obj1pal];
    let mut i = 0;
    update(ps[i])?;
    let mut event_pump = sdl_context.event_pump()?;

    'running: loop {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => break 'running,
                Event::KeyDown {
                    keycode: Some(key), ..
                } => match key {
                    Keycode::Return => {
                        i += 1;
                        i %= ps.len();
                        println!("{}", i);
                        update(ps[i])?;
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        ::std::thread::sleep(Duration::new(0, 1_000_000_000u32 / 30));
        // The rest of the game loop goes here...
    }

    Ok(())
}
