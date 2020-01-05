#![allow(dead_code)]
#![allow(unused_variables)]

//SDL
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::PixelFormatEnum;
use sdl2::rect::Rect;
use sdl2::render::{Canvas, Texture};
use sdl2::video::Window;
use std::time::Duration;
use std::time::Instant;
use std::sync::mpsc;

//File IO
use env_logger::Env;
use log::info;
use std::env;
use std::fs::File;
use std::io::Read;

use std::thread;

mod cpu;
mod gpu;
mod instructions;
mod memory;
mod registers;
mod texture;
use crate::cpu::CPU;
use crate::texture::{Map, Tile};

const FRAME_TIME: Duration = Duration::from_nanos(16670000);
const ZERO: Duration = Duration::from_secs(0);

fn main() -> std::io::Result<()> {
    env_logger::from_env(Env::default().default_filter_or("info")).init();
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
    let mut cpu = CPU::new(rom);
    let context = sdl2::init().unwrap();
    let window = create_window(&context);
    let mut canvas = window.into_canvas().build().unwrap();
    let tex_creator = canvas.texture_creator();
    let mut texture = tex_creator
        .create_texture_streaming(PixelFormatEnum::RGB24, 256, 256)
        .unwrap();

    // let mut event_pump = context.event_pump().unwrap();

    let boot_timer = Instant::now();
    let mut timer = Instant::now();
    let mut count_loop = 0;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let start = Instant::now();
        let mut frame_count = 0;
        loop {
            match rx.recv() {
                Ok(i) => {
                    frame_count += 1;
                    print!("\rFPS: {}", frame_count as f64 / start.elapsed().as_secs_f64());
                },
                Err(_) => panic!("Die")
            }
        }
    });
    loop {
        let f = frame(&mut cpu, &mut texture, &mut canvas);
        if f.is_err() {
            break;
        }
        tx.send(1).unwrap();
        delay_min(FRAME_TIME, &timer);
        timer = Instant::now();
        count_loop += 1;
    }
    println!(
        "It took {:?} seconds.",
        Instant::now().duration_since(boot_timer)
    );
    vram_viewer(&context, cpu.memory.gpu.vram).unwrap();
    map_viewer(&context, cpu.memory.gpu).unwrap();
    Ok(())
}

fn frame(cpu: &mut CPU, texture: &mut Texture, canvas: &mut Canvas<Window>) -> Result<(), ()> {
    let mut i = 0;
    while i < 17556 {
        let cpu_cycles = cpu.cycle();
        i += cpu_cycles;

        if cpu_cycles == 0 {
            ()
        }
        cpu.memory.gpu.cycle(cpu_cycles);
    }
    let bg = cpu.memory.gpu.background();
    texture
        .with_lock(None, |buffer: &mut [u8], pitch: usize| {
            if cpu.memory.gpu.is_on() {
                buffer[..].copy_from_slice(&bg.texture());
            }
        })
        .unwrap();
    let (h, v) = cpu.memory.gpu.scroll();
    canvas
        .copy(
            &texture,
            Rect::from((h as i32, v as i32, (h + 160) as u32, (v + 144) as u32)),
            None,
        )
        .unwrap();
    canvas.present();
    Ok(())
}

fn delay_min(min_dur: Duration, timer: &Instant) {
    if timer.elapsed() < min_dur {
        ::std::thread::sleep(min_dur - timer.elapsed());
    }
}

fn create_window(context: &sdl2::Sdl) -> Window {
    let video = context.video().unwrap();
    video
        .window("Window", 500, 500)
        .position_centered()
        .opengl()
        .build()
        .unwrap()
}

fn map_viewer(sdl_context: &sdl2::Sdl, gpu: gpu::GPU) -> Result<(), String> {
    let background = gpu.background();
    let video_subsystem = sdl_context.video()?;
    let (w, h) = background.pixel_dims();
    let scale = 2;
    let window = video_subsystem
        .window("Map Viewer", (scale * w) as u32, (scale * h) as u32)
        .position_centered()
        .opengl()
        .build()
        .map_err(|e| e.to_string())?;
    let mut canvas = window.into_canvas().build().map_err(|e| e.to_string())?;

    let texture_creator = canvas.texture_creator();
    //Texture width = map_w * tile_w
    let (map_w, map_h) = background.dimensions();
    let tile_w = 8;
    let mut texture = texture_creator
        .create_texture_static(
            PixelFormatEnum::RGB24,
            (map_w * tile_w) as u32,
            (map_h * tile_w) as u32,
        )
        .map_err(|e| e.to_string())?;

    println!("{}", background.texture().len());
    // Pitch = n_bytes(3) * map_w * tile_w
    texture
        .update(None, &(background.texture()), background.pitch())
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

fn vram_viewer(sdl_context: &sdl2::Sdl, vram: [u8; 0x2000]) -> Result<(), String> {
    // 0x8000-0x87ff
    let mut tiles: Vec<Tile> = vec![];
    for i in (0..0x7ff).step_by(16) {
        let tile_data = &vram[i..(i + 16)];
        tiles.push(Tile::construct(tile_data));
    }
    let video_subsystem = sdl_context.video()?;

    let scale = 8;
    let mut map = Map::new(16, 10, tiles);
    for i in 0..map.tile_set.len() {
        let (x, y) = (i % 16, i / 16);
        map.set(x, y, i);
    }
    let (w, h) = map.pixel_dims();
    let window = video_subsystem
        .window("VRAM Viewer", (scale * w) as u32, (scale * h) as u32)
        .position_centered()
        .opengl()
        .build()
        .map_err(|e| e.to_string())?;
    let mut canvas = window.into_canvas().build().map_err(|e| e.to_string())?;

    let texture_creator = canvas.texture_creator();
    //Texture width = map_w * tile_w
    let (map_w, map_h) = map.dimensions();
    let tile_w = 8;
    let mut texture = texture_creator
        .create_texture_static(
            PixelFormatEnum::RGB24,
            (map_w * tile_w) as u32,
            (map_h * tile_w) as u32,
        )
        .map_err(|e| e.to_string())?;

    println!("{}", map.texture().len());
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

fn run() -> Result<(), String> {
    let sdl_context = sdl2::init()?;
    let video_subsystem = sdl_context.video()?;

    let window = video_subsystem
        .window("rust-sdl2 demo: Video", 800, 600)
        .position_centered()
        .opengl()
        .build()
        .map_err(|e| e.to_string())?;

    let mut canvas = window.into_canvas().build().map_err(|e| e.to_string())?;

    canvas.clear();
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

        canvas.clear();
        canvas.present();
        ::std::thread::sleep(Duration::new(0, 1_000_000_000u32 / 30));
        // The rest of the game loop goes here...
    }

    Ok(())
}
