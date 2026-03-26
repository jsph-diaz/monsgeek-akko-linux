// Lighting tab (Tab 4) — Userpic editor (Mode 13)

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{prelude::*, widgets::*};
use throbber_widgets_tui::Throbber;

use crate::tui::shared::{AsyncResult, LoadState, RGB_SPINNER};
use crate::tui::tabs::depth::get_key_label;
use crate::tui::{App, LightingFocus};

/// Color palette for painting
pub(crate) const LIGHTING_PALETTE: &[(u8, u8, u8, &str)] = &[
    (255, 0, 0, "Red"),
    (0, 255, 0, "Green"),
    (0, 0, 255, "Blue"),
    (255, 255, 0, "Yellow"),
    (0, 255, 255, "Cyan"),
    (255, 0, 255, "Magenta"),
    (255, 255, 255, "White"),
    (0, 0, 0, "Black (Off)"),
    (255, 128, 0, "Orange"),
    (0, 0, 0, "Custom"), // Custom color, placeholder RGB
];
const CUSTOM_COLOR_IDX: usize = 9;

// ============================================================================
// Input Handling
// ============================================================================

/// Handle input for the lighting tab
pub(in crate::tui) fn handle_lighting_input(app: &mut App, key: KeyEvent) -> bool {
    // Handle focus switching first
    if key.kind == KeyEventKind::Press {
        if matches!(key.code, KeyCode::Enter | KeyCode::Esc) {
            if app.lighting_focus == LightingFocus::Picker {
                app.lighting_focus = LightingFocus::Layout;
                return true;
            }
        }
        if key.code == KeyCode::Char('e') && app.lighting_focus == LightingFocus::Layout {
            app.lighting_focus = LightingFocus::Picker;
            app.lighting_picker_field = 0;
            return true;
        }
    }

    match app.lighting_focus {
        LightingFocus::Layout => handle_layout_input(app, key),
        LightingFocus::Picker => handle_picker_input(app, key),
    }
}

/// Handle input when the color picker is focused
fn handle_picker_input(app: &mut App, key: KeyEvent) -> bool {
    if key.kind != KeyEventKind::Press { return false; }
    let (mut r, mut g, mut b) = app.lighting_custom_color;
    let coarse = key.modifiers.contains(KeyModifiers::SHIFT);

    let handled = match key.code {
        KeyCode::Left => {
            app.lighting_picker_field = app.lighting_picker_field.saturating_sub(1);
            true
        }
        KeyCode::Right | KeyCode::Tab => {
            app.lighting_picker_field = (app.lighting_picker_field + 1) % 3;
            true
        }
        KeyCode::Up => {
            match app.lighting_picker_field {
                0 => r = RGB_SPINNER.increment_u8(r, coarse),
                1 => g = RGB_SPINNER.increment_u8(g, coarse),
                _ => b = RGB_SPINNER.increment_u8(b, coarse),
            }
            true
        }
        KeyCode::Down => {
            match app.lighting_picker_field {
                0 => r = RGB_SPINNER.decrement_u8(r, coarse),
                1 => g = RGB_SPINNER.decrement_u8(g, coarse),
                _ => b = RGB_SPINNER.decrement_u8(b, coarse),
            }
            true
        }
        _ => false,
    };
    
    if handled {
        app.lighting_custom_color = (r, g, b);
    }
    handled
}

/// Handle input when the main layout is focused
fn handle_layout_input(app: &mut App, key: KeyEvent) -> bool {
    use KeyCode::*;

    if key.code == Char(' ') {
        match key.kind {
            KeyEventKind::Press => {
                app.lighting_is_painting = true;
                app.set_pixel_color();
            }
            KeyEventKind::Release => {
                app.lighting_is_painting = false;
            }
            _ => {}
        }
        return true;
    }

    if key.kind != KeyEventKind::Press { return false; }

    match key.code {
        Up => { app.lighting_move_up(); true }
        Down => { app.lighting_move_down(); true }
        Left => { app.lighting_move_left(); true }
        Right => { app.lighting_move_right(); true }
        Char('s') => { app.save_userpic(); true }
        Char('r') => { app.load_userpic(); true }
        Char('c') => { app.clear_userpic(); true }
        Backspace | Delete => {
            // Special case for clearing single key
            let pos = app.lighting_cursor_pos;
            let off = pos * 3;
            if off + 2 < app.lighting_data.len() {
                app.lighting_data[off] = 0;
                app.lighting_data[off + 1] = 0;
                app.lighting_data[off + 2] = 0;
            }
            if app.lighting_preview { app.send_lighting_preview(); }
            true
        }
        Char('f') => { app.fill_userpic(); true }
        Char('p') => { app.toggle_preview(); true }
        Char('[') => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                app.lighting_slot = app.lighting_slot.saturating_sub(1);
                app.load_userpic();
            } else {
                app.lighting_palette_idx = if app.lighting_palette_idx == 0 {
                    LIGHTING_PALETTE.len() - 1
                } else {
                    app.lighting_palette_idx - 1
                };
            }
            true
        }
        Char(']') => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                app.lighting_slot = (app.lighting_slot + 1).min(4);
                app.load_userpic();
            } else {
                app.lighting_palette_idx = (app.lighting_palette_idx + 1) % LIGHTING_PALETTE.len();
            }
            true
        }
        _ => false,
    }
}

// ============================================================================
// App Methods
// ============================================================================

impl App {
    pub(in crate::tui) fn lighting_move_up(&mut self) {
        let row = self.lighting_cursor_pos % 6;
        if row > 0 {
            self.lighting_cursor_pos -= 1;
            if self.lighting_is_painting { self.set_pixel_color(); }
        }
    }

    pub(in crate::tui) fn lighting_move_down(&mut self) {
        let row = self.lighting_cursor_pos % 6;
        if row < 5 {
            self.lighting_cursor_pos += 1;
            if self.lighting_is_painting { self.set_pixel_color(); }
        }
    }

    pub(in crate::tui) fn lighting_move_left(&mut self) {
        let col = self.lighting_cursor_pos / 6;
        if col > 0 {
            self.lighting_cursor_pos -= 6;
            if self.lighting_is_painting { self.set_pixel_color(); }
        }
    }

    pub(in crate::tui) fn lighting_move_right(&mut self) {
        let col = self.lighting_cursor_pos / 6;
        if col < 15 {
            self.lighting_cursor_pos += 6;
            if self.lighting_is_painting { self.set_pixel_color(); }
        }
    }

    pub(in crate::tui) fn load_userpic(&mut self) {
        if let Some(keyboard) = self.keyboard.clone() {
            self.loading.userpic = LoadState::Loading;
            let slot = self.lighting_slot;
            let tx = self.gen_sender();
            tokio::spawn(async move {
                let result = keyboard.download_userpic(slot).map_err(|e| e.to_string());
                tx.send(AsyncResult::Userpic(slot, result));
            });
        }
    }

    pub(in crate::tui) fn save_userpic(&mut self) {
        if let Some(keyboard) = self.keyboard.clone() {
            let slot = self.lighting_slot;
            let data = self.lighting_data.clone();
            self.status_msg = format!("Saving slot {slot}...");
            if keyboard.upload_userpic(slot, &data).is_ok() {
                if keyboard.set_led_with_option(13, self.info.led_brightness, self.info.led_speed, self.info.led_r, self.info.led_g, self.info.led_b, self.info.led_dazzle, slot).is_ok() {
                    self.info.led_mode = 13;
                    self.status_msg = format!("Slot {slot} saved and applied.");
                } else {
                    self.status_msg = format!("Slot {slot} saved, but failed to apply mode 13.");
                }
            } else {
                self.status_msg = format!("Failed to save slot {slot}.");
            }
        }
    }
    
    pub(in crate::tui) fn set_pixel_color(&mut self) {
        let (pr, pg, pb) = if self.lighting_palette_idx == CUSTOM_COLOR_IDX {
            self.lighting_custom_color
        } else {
            let (r, g, b, _) = LIGHTING_PALETTE[self.lighting_palette_idx];
            (r, g, b)
        };
        let pos = self.lighting_cursor_pos;
        let off = pos * 3;
        if off + 2 < self.lighting_data.len() {
            self.lighting_data[off] = pr;
            self.lighting_data[off + 1] = pg;
            self.lighting_data[off + 2] = pb;
        }
        if self.lighting_preview { self.send_lighting_preview(); }
    }
    
    pub(in crate::tui) fn fill_userpic(&mut self) {
        let (pr, pg, pb) = if self.lighting_palette_idx == CUSTOM_COLOR_IDX {
            self.lighting_custom_color
        } else {
            let (r, g, b, _) = LIGHTING_PALETTE[self.lighting_palette_idx];
            (r, g, b)
        };
        for i in 0..96 {
            self.lighting_data[i * 3] = pr;
            self.lighting_data[i * 3 + 1] = pg;
            self.lighting_data[i * 3 + 2] = pb;
        }
        self.status_msg = "Userpic filled (unsaved)".to_string();
        if self.lighting_preview { self.send_lighting_preview(); }
    }

    pub(in crate::tui) fn clear_userpic(&mut self) {
        self.lighting_data = vec![0; 288];
        self.status_msg = "Userpic cleared (unsaved)".to_string();
        if self.lighting_preview { self.send_lighting_preview(); }
    }
    
    pub(in crate::tui) fn toggle_preview(&mut self) {
        self.lighting_preview = !self.lighting_preview;
        if self.lighting_preview {
            self.status_msg = "Live preview ENABLED".to_string();
            self.send_lighting_preview();
        } else {
            self.status_msg = "Live preview DISABLED".to_string();
            if let Some(ref kb) = self.keyboard {
                let _ = kb.set_led_with_option(self.info.led_mode, self.info.led_brightness, self.info.led_speed, self.info.led_r, self.info.led_g, self.info.led_b, self.info.led_dazzle, self.lighting_slot);
            }
        }
    }
    
    pub(in crate::tui) fn send_lighting_preview(&mut self) {
        if let Some(ref kb) = self.keyboard {
            let mut stream_data = vec![0u8; 378];
            let len = self.lighting_data.len().min(378);
            stream_data[..len].copy_from_slice(&self.lighting_data[..len]);
            for page in 0..7 {
                let start = page * 54;
                let _ = kb.stream_led_page(page as u8, &stream_data[start..start + 54]);
            }
            let _ = kb.stream_led_commit();
        }
    }
}

// ============================================================================
// Rendering
// ============================================================================

pub(in crate::tui) fn render_lighting(f: &mut Frame, app: &mut App, area: Rect) {
    if app.loading.userpic == LoadState::Loading {
        let throbber = Throbber::default()
            .label(format!("Loading Userpic slot {}...", app.lighting_slot))
            .style(Style::default().fg(Color::Yellow));
        f.render_stateful_widget(throbber, area, &mut app.throbber_state.clone());
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(16), Constraint::Min(5)])
        .split(area);

    render_userpic_layout(f, app, chunks[0]);
    render_lighting_controls(f, app, chunks[1]);
}

fn render_userpic_layout(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("Keyboard Layout [Userpic Slot {}]", app.lighting_slot));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let key_width = 5u16; 
    let key_height = 2u16;

    for pos in 0..96 {
        let col = pos / 6;
        let row = pos % 6;
        let key_name = get_key_label(app, pos);
        if key_name.is_empty() || key_name == "?" { continue; }

        let x = inner.x + (col as u16 * key_width);
        let y = inner.y + (row as u16 * key_height);
        if x + key_width > inner.x + inner.width || y + key_height > inner.y + inner.height { continue; }

        let is_selected = pos == app.lighting_cursor_pos;
        let off = pos * 3;
        let r = app.lighting_data[off];
        let g = app.lighting_data[off + 1];
        let b = app.lighting_data[off + 2];
        let color = Color::Rgb(r, g, b);
        let brightness = (r as f32 * 0.299 + g as f32 * 0.587 + b as f32 * 0.114) / 255.0;
        let contrast = if brightness > 0.5 { Color::Black } else { Color::White };
        let inv_color = Color::Rgb(255 - r, 255 - g, 255 - b);

        let cell_block = Block::default()
            .borders(Borders::ALL)
            .border_style(if is_selected { Style::default().fg(inv_color).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::DarkGray) })
            .bg(color);
        
        let cell_rect = Rect::new(x, y, key_width, key_height);
        let inner_rect = cell_block.inner(cell_rect);

        let label = if is_selected { "[\u{2588}]".to_string() } else { key_name.chars().take(3).collect() };
        let p_style = if is_selected { Style::default().fg(inv_color) } else { Style::default().fg(contrast) };
        let p = Paragraph::new(label).alignment(Alignment::Center).style(p_style.bg(color));
        
        f.render_widget(cell_block, cell_rect);
        f.render_widget(p, inner_rect);
    }
}

fn render_lighting_controls(f: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title("Controls & Palette");
    let inner_area = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(3)])
        .split(inner_area);

    let pos = app.lighting_cursor_pos;
    let key_name = get_key_label(app, pos);
    let off = pos * 3;
    let r = app.lighting_data[off];
    let g = app.lighting_data[off + 1];
    let b = app.lighting_data[off + 2];

    let mut status_lines = vec![
        Line::from(vec![
            Span::raw("Slot: "),
            Span::styled(format!("[ {} ]", app.lighting_slot), Style::default().fg(Color::Cyan)),
            Span::raw(" (Shift+[]/])"),
        ]),
        Line::from(vec![
            Span::raw("Key:  "),
            Span::styled(format!("{:<8}", key_name), Style::default().fg(Color::Yellow)),
            Span::raw(format!(" RGB: #{:02X}{:02X}{:02X}", r, g, b)),
            Span::styled(" \u{2588}".repeat(4), Style::default().fg(Color::Rgb(r, g, b))),
        ]),
    ];
     if app.lighting_focus == LightingFocus::Picker {
        status_lines.push(Line::from(Span::styled("e: exit picker", Style::default().fg(Color::DarkGray))));
    } else {
        status_lines.push(Line::from(Span::styled("Arrows: move  Space: paint  f: fill  p: preview  e: edit custom", Style::default().fg(Color::DarkGray))));
    }
    f.render_widget(Paragraph::new(status_lines).block(Block::default().padding(Padding::horizontal(1))), chunks[0]);

    let bottom_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    render_palette(f, app, bottom_chunks[0]);
    render_rgb_picker(f, app, bottom_chunks[1]);
}

fn render_palette(f: &mut Frame, app: &mut App, area: Rect) {
    let mut palette_lines = vec![Line::from(Span::styled("Palette [ [] / ]] ]:", Style::default().add_modifier(Modifier::BOLD)))];
    
    let (pr, pg, pb) = if app.lighting_palette_idx == CUSTOM_COLOR_IDX {
        app.lighting_custom_color
    } else {
        let (r, g, b, _) = LIGHTING_PALETTE[app.lighting_palette_idx];
        (r, g, b)
    };
    let pname = LIGHTING_PALETTE[app.lighting_palette_idx].3;

    palette_lines.push(Line::from(vec![
        Span::raw("Active: "),
        Span::styled(format!("[ {} ]", pname), Style::default().fg(Color::Rgb(pr, pg, pb)).add_modifier(Modifier::BOLD)),
    ]));

    let mut swatches = vec![Span::raw("  ")];
    for (i, (sr, sg, sb, _)) in LIGHTING_PALETTE.iter().enumerate() {
        let color = if i == CUSTOM_COLOR_IDX { app.lighting_custom_color } else { (*sr, *sg, *sb) };
        swatches.push(Span::styled(if i == app.lighting_palette_idx { "\u{2588}\u{2588}" } else { "\u{2584}\u{2584}" }, Style::default().fg(Color::Rgb(color.0, color.1, color.2))));
        swatches.push(Span::raw(" "));
    }
    palette_lines.push(Line::from(swatches));
    
    f.render_widget(Paragraph::new(palette_lines), area);
}

fn render_rgb_picker(f: &mut Frame, app: &mut App, area: Rect) {
    let (r, g, b) = app.lighting_custom_color;
    let focus_style = Style::default().bg(Color::Blue);
    let is_focused = app.lighting_focus == LightingFocus::Picker;

    let picker_lines = vec![
        Line::from(Span::styled("Custom Color ('e' to edit):", if is_focused { Style::default().fg(Color::Yellow) } else { Style::default().add_modifier(Modifier::BOLD) })),
        Line::from(vec![
            Span::styled(" R:", Style::default().fg(Color::Red)),
            Span::styled(format!("{:>4}", r), if is_focused && app.lighting_picker_field == 0 { focus_style } else { Style::default() }),
            Span::styled(" G:", Style::default().fg(Color::Green)),
            Span::styled(format!("{:>4}", g), if is_focused && app.lighting_picker_field == 1 { focus_style } else { Style::default() }),
            Span::styled(" B:", Style::default().fg(Color::Blue)),
            Span::styled(format!("{:>4}", b), if is_focused && app.lighting_picker_field == 2 { focus_style } else { Style::default() }),
            Span::raw(" "),
            Span::styled("\u{2588}".repeat(4), Style::default().fg(Color::Rgb(r,g,b))),
        ]),
        Line::from(Span::styled(if is_focused { "↑↓: change  ←→: select  Shift: coarse" } else { "" }, Style::default().fg(Color::DarkGray))),
    ];

    f.render_widget(Paragraph::new(picker_lines), area);
}
