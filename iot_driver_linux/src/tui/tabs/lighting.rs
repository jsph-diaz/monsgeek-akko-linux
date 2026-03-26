// Lighting tab (Tab 6) — Userpic editor (Mode 13)

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::{prelude::*, widgets::*};
use throbber_widgets_tui::Throbber;

use crate::tui::shared::{AsyncResult, LoadState};
use crate::tui::tabs::depth::get_key_label;
use crate::tui::App;

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
];

/// Handle input for the lighting tab
pub(in crate::tui) fn handle_lighting_input(app: &mut App, key: KeyEvent) {
    use KeyCode::*;

    // Handle space key press/release for continuous painting
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
        return;
    }

    // Only handle press events for other keys
    if key.kind != KeyEventKind::Press {
        return;
    }

    match key.code {
        Up => {
            app.lighting_move_up();
        }
        Down => {
            app.lighting_move_down();
        }
        Left => {
            app.lighting_move_left();
        }
        Right => {
            app.lighting_move_right();
        }
        Char('s') => {
            app.save_userpic();
        }
        Char('r') => {
            app.load_userpic();
        }
        Char('c') | Backspace | Delete => {
            app.clear_userpic();
        }
        Char('f') => {
            app.fill_userpic();
        }
        Char('p') => {
            app.toggle_preview();
        }
        Char('+') | Char('=') => {
            app.lighting_slot = (app.lighting_slot + 1).min(4);
            app.load_userpic();
        }
        Char('-') | Char('_') => {
            app.lighting_slot = app.lighting_slot.saturating_sub(1);
            app.load_userpic();
        }
        // Palette cycling with < and >
        Char(',') | Char('<') => {
            app.lighting_palette_idx = if app.lighting_palette_idx == 0 {
                LIGHTING_PALETTE.len() - 1
            } else {
                app.lighting_palette_idx - 1
            };
        }
        Char('.') | Char('>') => {
            app.lighting_palette_idx = (app.lighting_palette_idx + 1) % LIGHTING_PALETTE.len();
        }
        _ => {}
    }
}

impl App {
    /// Navigate up in the 16x6 grid
    pub(in crate::tui) fn lighting_move_up(&mut self) {
        let row = self.lighting_cursor_pos % 6;
        if row > 0 {
            self.lighting_cursor_pos -= 1;
            if self.lighting_is_painting {
                self.set_pixel_color();
            }
        }
    }

    /// Navigate down in the 16x6 grid
    pub(in crate::tui) fn lighting_move_down(&mut self) {
        let row = self.lighting_cursor_pos % 6;
        if row < 5 {
            self.lighting_cursor_pos += 1;
            if self.lighting_is_painting {
                self.set_pixel_color();
            }
        }
    }

    /// Navigate left in the 16x6 grid
    pub(in crate::tui) fn lighting_move_left(&mut self) {
        let col = self.lighting_cursor_pos / 6;
        if col > 0 {
            self.lighting_cursor_pos -= 6;
            if self.lighting_is_painting {
                self.set_pixel_color();
            }
        }
    }

    /// Navigate right in the 16x6 grid
    pub(in crate::tui) fn lighting_move_right(&mut self) {
        let col = self.lighting_cursor_pos / 6;
        if col < 15 {
            self.lighting_cursor_pos += 6;
            if self.lighting_is_painting {
                self.set_pixel_color();
            }
        }
    }

    /// Load userpic for current slot
    pub(in crate::tui) fn load_userpic(&mut self) {
        let Some(keyboard) = self.keyboard.clone() else {
            return;
        };

        self.loading.userpic = LoadState::Loading;
        let slot = self.lighting_slot;
        let tx = self.gen_sender();

        tokio::spawn(async move {
            let result = keyboard.download_userpic(slot).map_err(|e| e.to_string());
            tx.send(AsyncResult::Userpic(slot, result));
        });
    }

    /// Save current userpic data to current slot
    pub(in crate::tui) fn save_userpic(&mut self) {
        let Some(keyboard) = self.keyboard.clone() else {
            return;
        };

        let slot = self.lighting_slot;
        let data = self.lighting_data.clone();
        self.status_msg = format!("Saving slot {slot}...");

        if keyboard.upload_userpic(slot, &data).is_ok() {
            // Also set mode to 13 and apply this slot
            if keyboard.set_led_with_option(13, 4, 0, 0, 200, 200, false, slot).is_ok() {
                self.info.led_mode = 13;
                self.status_msg = format!("Slot {slot} saved and applied.");
            } else {
                self.status_msg = format!("Slot {slot} saved, but failed to apply mode 13.");
            }
        } else {
            self.status_msg = format!("Failed to save slot {slot}.");
        }
    }

    /// Set current pixel color to selected palette color
    pub(in crate::tui) fn set_pixel_color(&mut self) {
        let pos = self.lighting_cursor_pos;
        let (pr, pg, pb, _) = LIGHTING_PALETTE[self.lighting_palette_idx];
        let off = pos * 3;
        if off + 2 < self.lighting_data.len() {
            self.lighting_data[off] = pr;
            self.lighting_data[off + 1] = pg;
            self.lighting_data[off + 2] = pb;
        }
        if self.lighting_preview {
            self.send_lighting_preview();
        }
    }

    /// Clear all pixels in current data
    pub(in crate::tui) fn clear_userpic(&mut self) {
        self.lighting_data = vec![0; 288];
        self.status_msg = "Userpic cleared (unsaved)".to_string();
        if self.lighting_preview {
            self.send_lighting_preview();
        }
    }

    /// Fill all pixels with current palette color
    pub(in crate::tui) fn fill_userpic(&mut self) {
        let (pr, pg, pb, _) = LIGHTING_PALETTE[self.lighting_palette_idx];
        for i in 0..96 {
            self.lighting_data[i * 3] = pr;
            self.lighting_data[i * 3 + 1] = pg;
            self.lighting_data[i * 3 + 2] = pb;
        }
        self.status_msg = "Userpic filled (unsaved)".to_string();
        if self.lighting_preview {
            self.send_lighting_preview();
        }
    }

    /// Toggle live hardware preview
    pub(in crate::tui) fn toggle_preview(&mut self) {
        self.lighting_preview = !self.lighting_preview;
        if self.lighting_preview {
            self.status_msg = "Live preview ENABLED (Streaming mode)".to_string();
            self.send_lighting_preview();
        } else {
            self.status_msg = "Live preview DISABLED".to_string();
            // Re-apply current mode/slot to stop streaming
            let slot = self.lighting_slot;
            let mode = self.info.led_mode;
            if let Some(ref kb) = self.keyboard {
                let _ = kb.set_led_with_option(mode, self.info.led_brightness, self.info.led_speed, self.info.led_r, self.info.led_g, self.info.led_b, self.info.led_dazzle, slot);
            }
        }
    }

    /// Send current data to keyboard via streaming mode (25)
    pub(in crate::tui) fn send_lighting_preview(&mut self) {
        let Some(ref kb) = self.keyboard else { return };
        
        // Mode 25 expects 126 pixels (378 bytes)
        let mut stream_data = vec![0u8; 378];
        let len = self.lighting_data.len().min(378);
        stream_data[..len].copy_from_slice(&self.lighting_data[..len]);

        // Stream in 7 pages of 18 pixels (54 bytes) each
        for page in 0..7 {
            let start = page as usize * 54;
            let end = start + 54;
            let _ = kb.stream_led_page(page as u8, &stream_data[start..end]);
        }
        
        // Finalize the frame
        let _ = kb.stream_led_commit();
    }
}

pub(in crate::tui) fn render_lighting(f: &mut Frame, app: &mut App, area: Rect) {
    if app.loading.userpic == LoadState::Loading {
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Lighting [Userpic Mode 13]");
        let inner = block.inner(area);
        f.render_widget(block, area);

        let throbber = Throbber::default()
            .label(format!("Loading Userpic slot {}...", app.lighting_slot))
            .throbber_style(Style::default().fg(Color::Yellow));
        f.render_stateful_widget(throbber, inner, &mut app.throbber_state.clone());
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(16), // Grid area (increased for better spacing)
            Constraint::Min(5),    // Controls area
        ])
        .split(area);

    // Render 16x6 grid
    render_userpic_layout(f, app, chunks[0]);

    // Render controls
    render_lighting_controls(f, app, chunks[1]);
}

fn render_userpic_layout(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("Keyboard Layout [Userpic Slot {}]", app.lighting_slot));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Layout dimensions (matching triggers layout for consistency)
    let key_width = 5u16; 
    let key_height = 2u16;

    // Userpic covers the main 16 columns of the matrix (16x6)
    for pos in 0..96 {
        let col = pos / 6;
        let row = pos % 6;

        // Skip positions outside visible area or empty keys
        let key_name = get_key_label(app, pos);
        if key_name.is_empty() || key_name == "?" {
            continue;
        }

        // Calculate screen position
        let x = inner.x + (col as u16 * key_width);
        let y = inner.y + (row as u16 * key_height);

        // Skip if outside area
        if x + key_width > inner.x + inner.width || y + key_height > inner.y + inner.height {
            continue;
        }

        let is_selected = pos == app.lighting_cursor_pos;
        let off = pos * 3;
        let r = app.lighting_data[off];
        let g = app.lighting_data[off + 1];
        let b = app.lighting_data[off + 2];
        let color = Color::Rgb(r, g, b);

        // Dynamic contrast for label
        let brightness = (r as f32 * 0.299 + g as f32 * 0.587 + b as f32 * 0.114) / 255.0;
        let contrast = if brightness > 0.5 { Color::Black } else { Color::White };

        // Inverted color for cursor
        let inv_color = Color::Rgb(255 - r, 255 - g, 255 - b);

        let cell_block = Block::default()
            .borders(Borders::ALL)
            .border_style(if is_selected {
                Style::default().fg(inv_color).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            })
            .bg(color);

        let cell_rect = Rect::new(x, y, key_width, key_height);
        let inner_rect = cell_block.inner(cell_rect);

        // For cursor: use inverted block character
        let label = if is_selected { 
            "[\u{2588}]".to_string() 
        } else {
            key_name.chars().take(3).collect()
        };

        let p = Paragraph::new(label)
            .alignment(Alignment::Center)
            .style(if is_selected {
                Style::default().fg(inv_color).bg(color)
            } else {
                Style::default().fg(contrast).bg(color)
            });
        
        f.render_widget(cell_block, cell_rect);
        f.render_widget(p, inner_rect);
    }
}

fn render_lighting_controls(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Controls & Palette");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(inner);

    // Left: Status & Selection info
    let pos = app.lighting_cursor_pos;
    let key_name = get_key_label(app, pos);
    let off = pos * 3;
    let r = app.lighting_data[off];
    let g = app.lighting_data[off + 1];
    let b = app.lighting_data[off + 2];

    let status_lines = vec![
        Line::from(vec![
            Span::raw("Slot: "),
            Span::styled(format!("< {} >", app.lighting_slot), Style::default().fg(Color::Cyan)),
            Span::raw(" (0-4) [+/-]"),
        ]),
        Line::from(vec![
            Span::raw("Key:  "),
            Span::styled(format!("{:<8}", key_name), Style::default().fg(Color::Yellow)),
            Span::raw(format!("  RGB: #{:02X}{:02X}{:02X}", r, g, b)),
            Span::styled("  \u{2588}".repeat(4), Style::default().fg(Color::Rgb(r, g, b))),
        ]),
        Line::from(vec![
            Span::raw("Preview: "),
            Span::styled(
                if app.lighting_preview { "ENABLED (Live)" } else { "DISABLED" },
                if app.lighting_preview { Style::default().fg(Color::Green).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::DarkGray) }
            ),
            Span::raw(" [p]"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Arrows: move  Space: paint (hold to drag)  f: fill  p: preview  Backspace: clear key",
            Style::default().fg(Color::DarkGray)
        )),
        Line::from(Span::styled(
            "s: save to flash  r: reload from flash  c: clear all slots  Tab: next tab",
            Style::default().fg(Color::DarkGray)
        )),
    ];
    f.render_widget(Paragraph::new(status_lines), chunks[0]);

    // Right: Color Palette
    let mut palette_lines = vec![Line::from(Span::styled("Color Palette [ < / > ]:", Style::default().add_modifier(Modifier::BOLD)))];
    
    let (pr, pg, pb, pname) = LIGHTING_PALETTE[app.lighting_palette_idx];
    palette_lines.push(Line::from(vec![
        Span::raw("Active: "),
        Span::styled(format!("< {} >", pname), Style::default().fg(Color::Rgb(pr, pg, pb)).add_modifier(Modifier::BOLD)),
    ]));

    // Swatches
    let mut swatches = vec![Span::raw("  ")];
    for (i, (sr, sg, sb, _)) in LIGHTING_PALETTE.iter().enumerate() {
        let is_sel = i == app.lighting_palette_idx;
        let symbol = if is_sel { "\u{2588}\u{2588}" } else { "\u{2584}\u{2584}" };
        swatches.push(Span::styled(symbol, Style::default().fg(Color::Rgb(*sr, *sg, *sb))));
        swatches.push(Span::raw(" "));
    }
    palette_lines.push(Line::from(swatches));
    
    f.render_widget(Paragraph::new(palette_lines), chunks[1]);
}
