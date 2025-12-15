use std::io::{self, Write, Read};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::Color as RColor,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use std::time::Duration;
use std::fs::File;

type Color = [u8; 3];

#[derive(Clone)]
struct Canvas {
    width: usize,
    height: usize,
    pixels: Vec<Color>,
}

impl Canvas {
    fn new(width: usize, height: usize) -> Self {
        Canvas {
            width,
            height,
            pixels: vec![[255, 255, 255]; width * height],
        }
    }

    fn clone_for_preview(&self) -> Self {
        Canvas {
            width: self.width,
            height: self.height,
            pixels: self.pixels.clone(),
        }
    }

    fn set_pixel(&mut self, x: usize, y: usize, color: Color) {
        if x < self.width && y < self.height {
            self.pixels[y * self.width + x] = color;
        }
    }

    fn get_pixel(&self, x: usize, y: usize) -> Color {
        if x < self.width && y < self.height {
            self.pixels[y * self.width + x]
        } else {
            [255, 255, 255]
        }
    }

    #[allow(dead_code)]
    fn render_to_string(&self) -> String {
        let mut output = String::new();
        let rows = (self.height + 1) / 2;

        for row in 0..rows {
            for col in 0..self.width {
                let top_y = row * 2;
                let bottom_y = top_y + 1;

                let top_color = self.get_pixel(col, top_y);
                let bottom_color = if bottom_y < self.height {
                    self.get_pixel(col, bottom_y)
                } else {
                    [255, 255, 255]
                };

                output.push_str(&format!(
                    "\x1b[38;2;{};{};{}m\x1b[48;2;{};{};{}m▀",
                    top_color[0], top_color[1], top_color[2],
                    bottom_color[0], bottom_color[1], bottom_color[2]
                ));
            }
            output.push_str("\x1b[0m\n");
        }

        output
    }

    fn render_to_spans(&self) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        for row in 0..self.height {
            let mut spans = Vec::new();
            for col in 0..self.width {
                let color = self.get_pixel(col, row);
                let fg = RColor::Rgb(color[0], color[1], color[2]);
                let span = Span::styled("██", ratatui::style::Style::default().fg(fg));
                spans.push(span);
            }
            
            let reset_span = Span::styled("", ratatui::style::Style::default().fg(RColor::Reset));
            spans.push(reset_span);
            
            lines.push(Line::from(spans));
        }

        lines
    }
}

fn clamp(val: usize, min: usize, max: usize) -> usize {
    if val < min { min } else if val > max { max } else { val }
}

fn draw_line_with_brush(canvas: &mut Canvas, x0: i32, y0: i32, x1: i32, y1: i32, thickness: usize, color: Color) {
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };

    let mut err = dx - dy;
    let mut x = x0;
    let mut y = y0;

    loop {
        if x >= 0 && x < canvas.width as i32 && y >= 0 && y < canvas.height as i32 {
            draw_brush_stroke(canvas, x as usize, y as usize, thickness, color);
        }

        if x == x1 && y == y1 { break; }

        let e2 = 2 * err;
        if e2 > -dy {
            err -= dy;
            x += sx;
        }
        if e2 < dx {
            err += dx;
            y += sy;
        }
    }
}

fn draw_line(canvas: &mut Canvas, x0: i32, y0: i32, x1: i32, y1: i32, color: Color) {
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };

    let mut err = dx - dy;
    let mut x = x0;
    let mut y = y0;

    loop {
        if x >= 0 && x < canvas.width as i32 && y >= 0 && y < canvas.height as i32 {
            canvas.set_pixel(x as usize, y as usize, color);
        }

        if x == x1 && y == y1 { break; }

        let e2 = 2 * err;
        if e2 > -dy {
            err -= dy;
            x += sx;
        }
        if e2 < dx {
            err += dx;
            y += sy;
        }
    }
}

fn draw_circle(canvas: &mut Canvas, cx: i32, cy: i32, radius: i32, color: Color) {
    let r2 = radius * radius;

    for y in -radius..=radius {
        for x in -radius..=radius {
            if x * x + y * y <= r2 {
                let px = cx + x;
                let py = cy + y;
                if px >= 0 && px < canvas.width as i32 && py >= 0 && py < canvas.height as i32 {
                    canvas.set_pixel(px as usize, py as usize, color);
                }
            }
        }
    }
}

fn draw_rectangle(canvas: &mut Canvas, cx: i32, cy: i32, half_size: i32, color: Color) {
    let x_min = (cx - half_size).max(0) as usize;
    let x_max = ((cx + half_size).min(canvas.width as i32 - 1) + 1) as usize;
    let y_min = (cy - half_size).max(0) as usize;
    let y_max = ((cy + half_size).min(canvas.height as i32 - 1) + 1) as usize;

    for y in y_min..y_max {
        for x in x_min..x_max {
            canvas.set_pixel(x, y, color);
        }
    }
}

fn draw_rect_preview(canvas: &mut Canvas, cx: i32, cy: i32, hx: i32, hy: i32, color: Color) {
    let x_min = (cx - hx).max(0) as usize;
    let x_max = ((cx + hx).min(canvas.width as i32 - 1) + 1) as usize;
    let y_min = (cy - hy).max(0) as usize;
    let y_max = ((cy + hy).min(canvas.height as i32 - 1) + 1) as usize;

    for y in y_min..y_max {
        for x in x_min..x_max {
            canvas.set_pixel(x, y, color);
        }
    }
}

fn draw_brush_stroke(canvas: &mut Canvas, x: usize, y: usize, thickness: usize, color: Color) {
    let t = thickness as i32;
    let x = x as i32;
    let y = y as i32;
    
    for dy in 0..t {
        for dx in 0..t {
            let px = x + dx - t / 2;
            let py = y + dy - t / 2;
            if px >= 0 && px < canvas.width as i32 && py >= 0 && py < canvas.height as i32 {
                canvas.set_pixel(px as usize, py as usize, color);
            }
        }
    }
}

fn prompt(msg: &str) -> String {
    disable_raw_mode().ok();
    print!("{}", msg);
    let _ = io::stdout().flush();

    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);

    enable_raw_mode().ok();
    input.trim().to_string()
}

fn clear_input_buffer() {
    while event::poll(Duration::from_millis(0)).ok().unwrap_or(false) {
        let _ = event::read();
    }
}

fn expand_path(path: &str) -> String {
    if path.starts_with('~') {
        if let Ok(home) = std::env::var("HOME") {
            return path.replacen('~', &home, 1);
        }
    }
    path.to_string()
}

fn save_canvas(canvas: &Canvas, filename: &str) -> Result<(), Box<dyn std::error::Error>> {
    let expanded_path = expand_path(filename);
    
    if let Some(parent) = std::path::Path::new(&expanded_path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    
    let mut file = File::create(&expanded_path)?;
    
    file.write_all(&(canvas.width as u32).to_le_bytes())?;
    file.write_all(&(canvas.height as u32).to_le_bytes())?;
    
    for pixel in &canvas.pixels {
        file.write_all(&[pixel[0], pixel[1], pixel[2]])?;
    }
    
    Ok(())
}

fn load_canvas(filename: &str) -> Result<Canvas, Box<dyn std::error::Error>> {
    let expanded_path = expand_path(filename);
    let mut file = File::open(&expanded_path)?;
    
    let mut width_bytes = [0u8; 4];
    let mut height_bytes = [0u8; 4];
    
    file.read_exact(&mut width_bytes)?;
    file.read_exact(&mut height_bytes)?;
    
    let width = u32::from_le_bytes(width_bytes) as usize;
    let height = u32::from_le_bytes(height_bytes) as usize;
    
    let mut pixels = vec![[255u8, 255u8, 255u8]; width * height];
    
    for pixel in &mut pixels {
        let mut rgb = [0u8; 3];
        file.read_exact(&mut rgb)?;
        *pixel = rgb;
    }
    
    Ok(Canvas { width, height, pixels })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::process::Command::new("clear").status()?;
    
    println!("\n╔════════════════════════════════════════╗");
    println!("║      Raint - v.1.0.0                  ║");
    println!("╚════════════════════════════════════════╝\n");
    println!("Enter canvas size (width height) or single number for square.");
    println!("Range: 2-80 pixels\nExamples: '40' or '80 40'\n");

    let mut width: usize = 40;
    let mut height: usize = 40;

    loop {
        print!("Size> ");
        let _ = io::stdout().flush();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            return Ok(());
        }

        let parts: Vec<&str> = input.trim().split_whitespace().collect();

        if parts.is_empty() {
            continue;
        }

        if parts.len() == 1 {
            if let Ok(n) = parts[0].parse::<usize>() {
                let n = clamp(n, 2, 80);
                width = n;
                height = n;
                break;
            }
        } else if parts.len() >= 2 {
            if let (Ok(w), Ok(h)) = (parts[0].parse::<usize>(), parts[1].parse::<usize>()) {
                width = clamp(w, 2, 80);
                height = clamp(h, 2, 80);
                break;
            }
        }

        println!("Invalid input. Try again.");
    }

    let mut canvas = Canvas::new(width, height);
    let mut canvas_history: Vec<Canvas> = vec![canvas.clone_for_preview()];
    let mut history_index = 0;
    let mut current_color: Color = [0, 0, 0];
    let mut brush_thickness: usize = 1;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    'main_loop: loop {
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(0)
                .constraints([Constraint::Min(1), Constraint::Length(2)])
                .split(f.size());

            let canvas_spans = canvas.render_to_spans();
            let canvas_widget = Paragraph::new(canvas_spans).block(Block::default().borders(Borders::NONE));
            f.render_widget(canvas_widget, chunks[0]);

            let info_text = format!(
                "H - Help | Color: RGB({}, {}, {}) | Thickness: {}",
                current_color[0], current_color[1], current_color[2], brush_thickness
            );
            let info_widget = Paragraph::new(info_text).block(Block::default().borders(Borders::TOP));
            f.render_widget(info_widget, chunks[1]);
        })?;

        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(KeyEvent {
                    code: KeyCode::Char('q'),
                    ..
                })
                | Event::Key(KeyEvent {
                    code: KeyCode::Char('Q'),
                    ..
                }) => break 'main_loop,

                Event::Key(KeyEvent {
                    code: KeyCode::Char('h'),
                    ..
                })
                | Event::Key(KeyEvent {
                    code: KeyCode::Char('H'),
                    ..
                }) => {
                    'help_loop: loop {
                        terminal.draw(|f| {
                            let chunks = Layout::default()
                                .direction(Direction::Vertical)
                                .margin(1)
                                .constraints([Constraint::Min(1)])
                                .split(f.size());

                            let help_text = vec![
                                Line::from(""),
                                Line::from("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"),
                                Line::from("                    HELP MENU"),
                                Line::from("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"),
                                Line::from(""),
                                Line::from("H - Show this help menu"),
                                Line::from("C - Change brush color (RGB values)"),
                                Line::from("S - Draw a shape (circle or square)"),
                                Line::from("L - Draw a line"),
                                Line::from("P - Paint mode (draw with mouse drag)"),
                                Line::from("E - Eraser mode (erase with mouse drag)"),
                                Line::from("T - Set brush thickness (1-10)"),
                                Line::from("Z - Undo last action"),
                                Line::from("Y - Redo last action"),
                                Line::from("[ - Export image as .rai file (supports paths and ~)"),
                                Line::from("] - Open and load a .rai file (supports paths and ~)"),
                                Line::from("* - Save to existing .rai file (supports paths and ~)"),
                                Line::from("Q - Quit the application"),
                                Line::from(""),
                                Line::from("Press any key to exit help menu..."),
                                Line::from(""),
                            ];

                            let help_widget = Paragraph::new(help_text)
                                .block(Block::default().borders(Borders::ALL).title(" Help "));
                            f.render_widget(help_widget, chunks[0]);
                        })?;

                        if event::poll(Duration::from_millis(50))? {
                            match event::read()? {
                                Event::Key(_) => {
                                    break 'help_loop;
                                }
                                _ => {}
                            }
                        }
                    }
                    terminal.clear()?;
                }

                Event::Key(KeyEvent {
                    code: KeyCode::Char('z'),
                    ..
                })
                | Event::Key(KeyEvent {
                    code: KeyCode::Char('Z'),
                    ..
                }) => {
                    if history_index > 0 {
                        history_index -= 1;
                        canvas = canvas_history[history_index].clone_for_preview();
                    }
                }

                Event::Key(KeyEvent {
                    code: KeyCode::Char('y'),
                    ..
                })
                | Event::Key(KeyEvent {
                    code: KeyCode::Char('Y'),
                    ..
                }) => {
                    if history_index < canvas_history.len() - 1 {
                        history_index += 1;
                        canvas = canvas_history[history_index].clone_for_preview();
                    }
                }

                Event::Key(KeyEvent {
                    code: KeyCode::Char('t'),
                    ..
                })
                | Event::Key(KeyEvent {
                    code: KeyCode::Char('T'),
                    ..
                }) => {
                    let input = prompt("Brush thickness (1-10): ");
                    if let Ok(t) = input.parse::<usize>() {
                        brush_thickness = clamp(t, 1, 10);
                    }
                    terminal.clear()?;
                }

                Event::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    ..
                })
                | Event::Key(KeyEvent {
                    code: KeyCode::Char('C'),
                    ..
                }) => {
                    let input = prompt("RGB values (R G B): ");
                    let parts: Vec<&str> = input.split_whitespace().collect();

                    if parts.len() >= 3 {
                        if let (Ok(r), Ok(g), Ok(b)) = (
                            parts[0].parse::<u8>(),
                            parts[1].parse::<u8>(),
                            parts[2].parse::<u8>(),
                        ) {
                            current_color = [r, g, b];
                        }
                    }
                    terminal.clear()?;
                }

                Event::Key(KeyEvent {
                    code: KeyCode::Char('s'),
                    ..
                })
                | Event::Key(KeyEvent {
                    code: KeyCode::Char('S'),
                    ..
                }) => {
                    let shape_type = prompt("Shape (c=circle/s=square): ").to_lowercase();
                    let is_circle = shape_type.starts_with('c');

                    execute!(io::stdout(), EnableMouseCapture)?;
                    let mut start_pos: Option<(usize, usize)> = None;
                    let mut end_pos: Option<(usize, usize)> = None;
                    let mut canvas_height = 0;

                    'shape_loop: loop {
                        let mut preview_canvas = canvas.clone_for_preview();

                        if let (Some((sx, sy)), Some((ex, ey))) = (start_pos, end_pos) {
                            let sx_px = (sx / 2) as i32;
                            let sy_px = sy as i32;
                            let ex_px = (ex / 2) as i32;
                            let ey_px = ey as i32;
                            
                            let dx = (ex_px - sx_px).abs();
                            let dy = (ey_px - sy_px).abs();
                            let cx = ((sx_px + ex_px) / 2) as i32;
                            let cy = ((sy_px + ey_px) / 2) as i32;

                            if is_circle {
                                let r = (dx.max(dy) / 2).max(1);
                                draw_circle(&mut preview_canvas, cx, cy, r, current_color);
                            } else {
                                let hx = (dx / 2) as i32;
                                let hy = (dy / 2) as i32;
                                draw_rect_preview(&mut preview_canvas, cx, cy, hx.max(1), hy.max(1), current_color);
                            }
                        }

                        terminal.draw(|f| {
                            let chunks = Layout::default()
                                .direction(Direction::Vertical)
                                .margin(0)
                                .constraints([Constraint::Min(1), Constraint::Length(3)])
                                .split(f.size());

                            canvas_height = chunks[0].height as usize;

                            let canvas_spans = preview_canvas.render_to_spans();
                            let canvas_widget = Paragraph::new(canvas_spans).block(Block::default().borders(Borders::NONE));
                            f.render_widget(canvas_widget, chunks[0]);

                            let shape_name = if is_circle { "CIRCLE" } else { "SQUARE" };
                            let info = if start_pos.is_some() && end_pos.is_none() {
                                Paragraph::new(format!("[{}] Move to resize. Click to finalize. Press ESC to cancel.", shape_name)).block(Block::default().borders(Borders::TOP))
                            } else {
                                Paragraph::new(format!("[{}] Click and drag. Press ESC to cancel.", shape_name)).block(Block::default().borders(Borders::TOP))
                            };
                            f.render_widget(info, chunks[1]);
                        })?;

                        if event::poll(Duration::from_millis(50))? {
                            match event::read()? {
                                Event::Mouse(mouse_event) => {
                                    use crossterm::event::MouseEventKind;

                                    match mouse_event.kind {
                                        MouseEventKind::Down(_) => {
                                            if start_pos.is_none() {
                                                start_pos = Some((mouse_event.column as usize, mouse_event.row as usize));
                                            } else {
                                                end_pos = Some((mouse_event.column as usize, mouse_event.row as usize));
                                            }
                                        }
                                        MouseEventKind::Drag(_) => {
                                            if start_pos.is_some() && end_pos.is_none() {
                                                end_pos = Some((mouse_event.column as usize, mouse_event.row as usize));
                                            }
                                        }
                                        MouseEventKind::Moved => {
                                            if start_pos.is_some() && end_pos.is_some() {
                                                end_pos = Some((mouse_event.column as usize, mouse_event.row as usize));
                                            }
                                        }
                                        MouseEventKind::Up(_) => {
                                            if let (Some((sx, sy)), Some((ex, ey))) = (start_pos, end_pos) {
                                                let sx_px = (sx / 2) as i32;
                                                let sy_px = sy as i32;
                                                let ex_px = (ex / 2) as i32;
                                                let ey_px = ey as i32;
                                                
                                                let dx = (ex_px - sx_px).abs();
                                                let dy = (ey_px - sy_px).abs();
                                                let cx = ((sx_px + ex_px) / 2) as i32;
                                                let cy = ((sy_px + ey_px) / 2) as i32;

                                                if is_circle {
                                                    let r = (dx.max(dy) / 2).max(1);
                                                    draw_circle(&mut canvas, cx, cy, r, current_color);
                                                } else {
                                                    let hx = (dx / 2) as i32;
                                                    let hy = (dy / 2) as i32;
                                                    draw_rect_preview(&mut canvas, cx, cy, hx.max(1), hy.max(1), current_color);
                                                }
                                                canvas_history.truncate(history_index + 1);
                                                canvas_history.push(canvas.clone_for_preview());
                                                history_index = canvas_history.len() - 1;
                                                break 'shape_loop;
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                                Event::Key(KeyEvent {
                                    code: KeyCode::Esc,
                                    ..
                                }) => {
                                    break 'shape_loop;
                                }
                                _ => {}
                            }
                        }
                    }
                    execute!(io::stdout(), DisableMouseCapture)?;
                    clear_input_buffer();
                    terminal.clear()?;
                }

                Event::Key(KeyEvent {
                    code: KeyCode::Char('l'),
                    ..
                })
                | Event::Key(KeyEvent {
                    code: KeyCode::Char('L'),
                    ..
                }) => {
                    execute!(io::stdout(), EnableMouseCapture)?;
                    let mut start_pos: Option<(i32, i32)> = None;

                    'line_loop: loop {
                        terminal.draw(|f| {
                            let chunks = Layout::default()
                                .direction(Direction::Vertical)
                                .margin(0)
                                .constraints([Constraint::Min(1), Constraint::Length(3)])
                                .split(f.size());

                            let canvas_spans = canvas.render_to_spans();
                            let canvas_widget = Paragraph::new(canvas_spans).block(Block::default().borders(Borders::NONE));
                            f.render_widget(canvas_widget, chunks[0]);

                            let info = if start_pos.is_some() {
                                Paragraph::new("[LINE] Click endpoint or press ESC to cancel.").block(Block::default().borders(Borders::TOP))
                            } else {
                                Paragraph::new("[LINE] Click startpoint. Press ESC to cancel.").block(Block::default().borders(Borders::TOP))
                            };
                            f.render_widget(info, chunks[1]);
                        })?;

                        if event::poll(Duration::from_millis(50))? {
                            match event::read()? {
                                Event::Mouse(mouse_event) => {
                                    use crossterm::event::MouseEventKind;

                                    if matches!(mouse_event.kind, MouseEventKind::Down(_)) {
                                        let col = (mouse_event.column / 2) as i32;
                                        let row = mouse_event.row as i32;

                                        if let Some((sx, sy)) = start_pos {
                                            draw_line(&mut canvas, sx, sy, col, row, current_color);
                                            canvas_history.truncate(history_index + 1);
                                            canvas_history.push(canvas.clone_for_preview());
                                            history_index = canvas_history.len() - 1;
                                            start_pos = None;
                                            break 'line_loop;
                                        } else {
                                            start_pos = Some((col, row));
                                        }
                                    }
                                }
                                Event::Key(KeyEvent {
                                    code: KeyCode::Esc,
                                    ..
                                }) => {
                                    break 'line_loop;
                                }
                                _ => {}
                            }
                        }
                    }
                    execute!(io::stdout(), DisableMouseCapture)?;
                    clear_input_buffer();
                    terminal.clear()?;
                }

                Event::Key(KeyEvent {
                    code: KeyCode::Char('p'),
                    ..
                })
                | Event::Key(KeyEvent {
                    code: KeyCode::Char('P'),
                    ..
                }) => {
                    execute!(io::stdout(), EnableMouseCapture)?;
                    let mut last_pos: Option<(i32, i32)> = None;
                    'paint_loop: loop {
                        terminal.draw(|f| {
                            let chunks = Layout::default()
                                .direction(Direction::Vertical)
                                .margin(0)
                                .constraints([Constraint::Min(1), Constraint::Length(2)])
                                .split(f.size());

                            let canvas_spans = canvas.render_to_spans();
                            let canvas_widget =
                                Paragraph::new(canvas_spans).block(Block::default().borders(Borders::NONE));
                            f.render_widget(canvas_widget, chunks[0]);

                            let info = Paragraph::new("[PAINT MODE] Click/drag to draw. Press ESC or P to exit.")
                                .block(Block::default().borders(Borders::TOP));
                            f.render_widget(info, chunks[1]);
                        })?;

                        if event::poll(Duration::from_millis(50))? {
                            match event::read()? {
                                Event::Mouse(mouse_event) => {
                                    use crossterm::event::MouseEventKind;
                                    
                                    match mouse_event.kind {
                                        MouseEventKind::Drag(_) => {
                                            let col = (mouse_event.column / 2) as i32;
                                            let row = mouse_event.row as i32;

                                            if let Some((last_x, last_y)) = last_pos {
                                                draw_line_with_brush(&mut canvas, last_x, last_y, col, row, brush_thickness, current_color);
                                            } else {
                                                draw_brush_stroke(&mut canvas, col as usize, row as usize, brush_thickness, current_color);
                                            }
                                            last_pos = Some((col, row));
                                        }
                                        MouseEventKind::Up(_) => {
                                            last_pos = None;
                                        }
                                        _ => {}
                                    }
                                }
                                Event::Key(KeyEvent {
                                    code: KeyCode::Esc,
                                    ..
                                })
                                | Event::Key(KeyEvent {
                                    code: KeyCode::Char('p'),
                                    ..
                                })
                                | Event::Key(KeyEvent {
                                    code: KeyCode::Char('P'),
                                    ..
                                }) => {
                                    execute!(io::stdout(), DisableMouseCapture)?;
                                    clear_input_buffer();
                                    canvas_history.truncate(history_index + 1);
                                    canvas_history.push(canvas.clone_for_preview());
                                    history_index = canvas_history.len() - 1;
                                    terminal.clear()?;
                                    break 'paint_loop;
                                }
                                _ => {}
                            }
                        }
                    }
                }

                Event::Key(KeyEvent {
                    code: KeyCode::Char('e'),
                    ..
                })
                | Event::Key(KeyEvent {
                    code: KeyCode::Char('E'),
                    ..
                }) => {
                    execute!(io::stdout(), EnableMouseCapture)?;
                    let mut last_pos: Option<(i32, i32)> = None;
                    'erase_loop: loop {
                        terminal.draw(|f| {
                            let chunks = Layout::default()
                                .direction(Direction::Vertical)
                                .margin(0)
                                .constraints([Constraint::Min(1), Constraint::Length(2)])
                                .split(f.size());

                            let canvas_spans = canvas.render_to_spans();
                            let canvas_widget =
                                Paragraph::new(canvas_spans).block(Block::default().borders(Borders::NONE));
                            f.render_widget(canvas_widget, chunks[0]);

                            let info = Paragraph::new("[ERASER MODE] Click/drag to erase. Press ESC or E to exit.")
                                .block(Block::default().borders(Borders::TOP));
                            f.render_widget(info, chunks[1]);
                        })?;

                        if event::poll(Duration::from_millis(50))? {
                            match event::read()? {
                                Event::Mouse(mouse_event) => {
                                    use crossterm::event::MouseEventKind;
                                    
                                    match mouse_event.kind {
                                        MouseEventKind::Drag(_) => {
                                            let col = (mouse_event.column / 2) as i32;
                                            let row = mouse_event.row as i32;

                                            if let Some((last_x, last_y)) = last_pos {
                                                draw_line_with_brush(&mut canvas, last_x, last_y, col, row, brush_thickness, [255, 255, 255]);
                                            } else {
                                                draw_brush_stroke(&mut canvas, col as usize, row as usize, brush_thickness, [255, 255, 255]);
                                            }
                                            last_pos = Some((col, row));
                                        }
                                        MouseEventKind::Up(_) => {
                                            last_pos = None;
                                        }
                                        _ => {}
                                    }
                                }
                                Event::Key(KeyEvent {
                                    code: KeyCode::Esc,
                                    ..
                                })
                                | Event::Key(KeyEvent {
                                    code: KeyCode::Char('e'),
                                    ..
                                })
                                | Event::Key(KeyEvent {
                                    code: KeyCode::Char('E'),
                                    ..
                                }) => {
                                    execute!(io::stdout(), DisableMouseCapture)?;
                                    clear_input_buffer();
                                    canvas_history.truncate(history_index + 1);
                                    canvas_history.push(canvas.clone_for_preview());
                                    history_index = canvas_history.len() - 1;
                                    terminal.clear()?;
                                    break 'erase_loop;
                                }
                                _ => {}
                            }
                        }
                    }
                }

                Event::Key(KeyEvent {
                    code: KeyCode::Char('['),
                    ..
                }) => {
                    let filename = prompt("Export filename (without .rai): ");
                    if !filename.trim().is_empty() {
                        let filename = filename.trim();
                        let filepath = if filename.ends_with(".rai") {
                            filename.to_string()
                        } else {
                            format!("{}.rai", filename)
                        };
                        match save_canvas(&canvas, &filepath) {
                            Ok(_) => {
                                disable_raw_mode()?;
                                let expanded = expand_path(&filepath);
                                println!("Image exported to: {}", expanded);
                                let _ = io::stdout().flush();
                                let _ = io::stdin().read_line(&mut String::new());
                                enable_raw_mode()?;
                            }
                            Err(e) => {
                                disable_raw_mode()?;
                                println!("Error saving file: {}", e);
                                let _ = io::stdout().flush();
                                let _ = io::stdin().read_line(&mut String::new());
                                enable_raw_mode()?;
                            }
                        }
                    }
                    terminal.clear()?;
                }

                Event::Key(KeyEvent {
                    code: KeyCode::Char(']'),
                    ..
                }) => {
                    let filename = prompt("Open .rai file (with .rai extension): ");
                    if !filename.trim().is_empty() {
                        match load_canvas(filename.trim()) {
                            Ok(loaded_canvas) => {
                                canvas = loaded_canvas;
                                canvas_history.truncate(history_index + 1);
                                canvas_history.push(canvas.clone_for_preview());
                                history_index = canvas_history.len() - 1;
                                disable_raw_mode()?;
                                println!("Image loaded successfully!");
                                let _ = io::stdout().flush();
                                let _ = io::stdin().read_line(&mut String::new());
                                enable_raw_mode()?;
                            }
                            Err(e) => {
                                disable_raw_mode()?;
                                println!("Error loading file: {}", e);
                                let _ = io::stdout().flush();
                                let _ = io::stdin().read_line(&mut String::new());
                                enable_raw_mode()?;
                            }
                        }
                    }
                    terminal.clear()?;
                }

                Event::Key(KeyEvent {
                    code: KeyCode::Char('*'),
                    ..
                }) => {
                    let filename = prompt("Save to existing .rai file (path): ");
                    if !filename.trim().is_empty() {
                        let filename = filename.trim();
                        let filepath = if filename.ends_with(".rai") {
                            filename.to_string()
                        } else {
                            format!("{}.rai", filename)
                        };
                        match save_canvas(&canvas, &filepath) {
                            Ok(_) => {
                                disable_raw_mode()?;
                                let expanded = expand_path(&filepath);
                                println!("File saved: {}", expanded);
                                let _ = io::stdout().flush();
                                let _ = io::stdin().read_line(&mut String::new());
                                enable_raw_mode()?;
                            }
                            Err(e) => {
                                disable_raw_mode()?;
                                println!("Error saving file: {}", e);
                                let _ = io::stdout().flush();
                                let _ = io::stdin().read_line(&mut String::new());
                                enable_raw_mode()?;
                            }
                        }
                    }
                    terminal.clear()?;
                }

                _ => {}
            }
        }
    }

    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;
    println!("Thanks for using the ASCII Image Editor!");
    Ok(())
}