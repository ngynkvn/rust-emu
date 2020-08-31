//SDL
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::PixelFormatEnum;
use sdl2::rect::Rect;
use sdl2::render::{Canvas, Texture};
use sdl2::video::Window;
use std::time::Duration;
use std::time::Instant;

//File IO
use log::info;
use std::env;
use std::fs::File;
use std::io::Read;

use rust_emu::emu::Emu;
use rust_emu::texture::{Map, Tile};
use rust_emu::*;

const FRAME_TIME: Duration = Duration::from_nanos(16670000);

fn setup_logger() -> Result<(), fern::InitError> {
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
        .apply()?;
    Ok(())
}

// #[cfg(not(sdl))]
fn main() {
    // just_cpu();
    info!("Setup logging");
    setup_logger().unwrap();
    info!("Running SDL Main");
    sdl_main().unwrap();
    // decompiler();
}

fn init() -> Result<Emu, std::io::Error> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: ./gboy [rom]");
        panic!();
    }
    info!("{:?}", args);
    info!("Attempting to load {:?}", args[1]);
    let mut file = File::open(args[1].to_string())?;
    let mut rom = Vec::new();
    file.read_to_end(&mut rom)?;
    let emu = Emu::new(rom);
    Ok(emu)
}

fn sdl_main() -> std::io::Result<()> {
    let mut emu = init().unwrap();
    let context = sdl2::init().unwrap();
    let window = create_window(&context);
    let mut canvas = window.into_canvas().build().unwrap();
    let tex_creator = canvas.texture_creator();
    let mut texture = tex_creator
        .create_texture_streaming(PixelFormatEnum::RGB565, WINDOW_WIDTH, WINDOW_HEIGHT)
        .unwrap();

    // let mut event_pump = context.event_pump().unwrap();

    let boot_timer = Instant::now();
    let mut timer = Instant::now();
    let mut event_pump = context.event_pump().unwrap();

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

        let f = frame(&mut emu, &mut texture, &mut canvas);
        if f.is_err() {
            break;
        }
        delay_min(FRAME_TIME, &timer);
        timer = Instant::now();
    }
    std::mem::drop(event_pump);
    println!(
        "It took {:?} seconds.",
        Instant::now().duration_since(boot_timer)
    );
    // vram_viewer(&context, emu.bus.gpu.vram).unwrap();
    map_viewer(&context, emu).unwrap();
    Ok(())
}

const WINDOW_HEIGHT: u32 = 144;
const WINDOW_WIDTH: u32 = 160;

fn frame(emu: &mut Emu, texture: &mut Texture, canvas: &mut Canvas<Window>) -> Result<(), ()> {
    let mut i = 0;
    while i < 17476 {
        match emu.cycle() {
            Ok(c) => i += c,
            Err(s) => panic!(s),
        }
    }
    emu.bus.gpu.render(&mut emu.framebuffer);
    let (h, v) = emu.bus.gpu.scroll();
    texture
        .with_lock(None, |buffer, _| {
            let mut i = 0;
            for y in v..v + WINDOW_HEIGHT {
                let y = (y % 256) as usize;
                for x in h..h + WINDOW_WIDTH {
                    let x = (x % 256) as usize;
                    let [lo, hi] = emu.framebuffer[y][x].to_le_bytes();
                    buffer[i] = lo;
                    buffer[i + 1] = hi;
                    i += 2;
                }
            }
        })
        .unwrap();
    canvas.copy(&texture, None, None).unwrap();
    canvas.present();
    Ok(())
}

fn delay_min(min_dur: Duration, timer: &Instant) {
    let time = timer.elapsed();
    if time < min_dur {
        ::std::thread::sleep(min_dur - time);
    }
    // println!("Frame time: {}", timer.elapsed().as_secs_f64());
}

fn create_window(context: &sdl2::Sdl) -> Window {
    let video = context.video().unwrap();
    video
        .window("Window", WINDOW_WIDTH * 3, WINDOW_HEIGHT * 3)
        .position_centered()
        .build()
        .unwrap()
}

fn map_viewer(sdl_context: &sdl2::Sdl, emu: emu::Emu) -> Result<(), String> {
    let gpu = emu.bus.gpu;
    let background = gpu.background();
    let video_subsystem = sdl_context.video()?;
    let (w, h) = background.pixel_dims();
    let window = video_subsystem
        .window("Map Viewer", w as u32, h as u32)
        .position_centered()
        .build()
        .map_err(|e| e.to_string())?;
    let mut canvas = window.into_canvas().build().map_err(|e| e.to_string())?;

    let texture_creator = canvas.texture_creator();
    //Texture width = map_w * tile_w
    let (map_w, map_h) = background.dimensions();
    let tile_w = 8;
    let mut texture = texture_creator
        .create_texture_static(
            PixelFormatEnum::RGB565,
            (map_w * tile_w) as u32,
            (map_h * tile_w) as u32,
        )
        .map_err(|e| e.to_string())?;

    // Pitch = n_bytes(3) * map_w * tile_w
    texture
        .update(None, &(background.texture()), 256 * 2)
        .map_err(|e| e.to_string())?;
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

#[allow(dead_code)]
fn vram_viewer(sdl_context: &sdl2::Sdl, vram: [u8; 0x2000]) -> Result<(), String> {
    // 0x8000-0x87ff
    let mut tiles: Vec<Tile> = vec![];
    for i in (0..0x7ff).step_by(16) {
        let tile_data = &vram[Tile::range(i)];
        tiles.push(Tile::construct(228, tile_data));
    }
    let video_subsystem = sdl_context.video()?;

    let scale = 8;
    let mut map = vec![0; tiles.len()];
    for i in 0..tiles.len() {
        let (x, y) = (i % 16, i / 16);
        map[x + y * 16] = i as u8;
    }
    let map = Map {
        width: 16,
        height: 10,
        tile_set: tiles,
        map: map.as_slice(),
    };
    let (w, h) = map.pixel_dims();
    let window = video_subsystem
        .window("VRAM Viewer", (scale * w) as u32, (scale * h) as u32)
        .position_centered()
        .build()
        .map_err(|e| e.to_string())?;
    let mut canvas = window.into_canvas().build().map_err(|e| e.to_string())?;

    let texture_creator = canvas.texture_creator();
    //Texture width = map_w * tile_w
    let (map_w, map_h) = map.dimensions();
    let tile_w = 8;
    let mut texture = texture_creator
        .create_texture_static(
            PixelFormatEnum::RGB565,
            (map_w * tile_w) as u32,
            (map_h * tile_w) as u32,
        )
        .map_err(|e| e.to_string())?;

    // Pitch = n_bytes(3) * map_w * tile_w
    texture
        .update(None, &(map.texture()), map.pitch())
        .map_err(|e| e.to_string())?;
    canvas
        .copy(&texture, None, None)
        .map_err(|e| e.to_string())?;
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
