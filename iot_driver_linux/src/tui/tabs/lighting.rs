// Lighting tab (Tab 6) — Userpic editor (Mode 13)

use crossterm::event::KeyCode;
use ratatui::{prelude::*, widgets::*};
use throbber_widgets_tui::Throbber;

use crate::tui::shared::{AsyncResult, LoadState};
use crate::tui::App;

/// Handle input for the lighting tab
pub(in crate::tui) fn handle_lighting_input(app: &mut App, key: KeyCode) {
    use KeyCode::*;

    match key {
        Up => {
            app.lighting_cursor.1 = app.lighting_cursor.1.saturating_sub(1);
        }
        Down => {
            if app.lighting_cursor.1 < 5 {
                app.lighting_cursor.1 += 1;
            }
        }
        Left => {
            app.lighting_cursor.0 = app.lighting_cursor.0.saturating_sub(1);
        }
        Right => {
            if app.lighting_cursor.0 < 15 {
                app.lighting_cursor.0 += 1;
            }
        }
        Char(' ') => {
            app.set_pixel_color();
        }
        Char('s') => {
            app.save_userpic();
        }
        Char('r') => {
            app.load_userpic();
        }
        Char('c') => {
            app.clear_userpic();
        }
        Char('+') | Char('=') => {
            app.lighting_slot = (app.lighting_slot + 1).min(4);
            app.load_userpic();
        }
        Char('-') | Char('_') => {
            app.lighting_slot = app.lighting_slot.saturating_sub(1);
            app.load_userpic();
        }
        _ => {}
    }
}

impl App {
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

    /// Set current pixel color to main LED color
    pub(in crate::tui) fn set_pixel_color(&mut self) {
        let (col, row) = self.lighting_cursor;
        let off = (col as usize * 18) + (row as usize * 3);
        if off + 2 < self.lighting_data.len() {
            self.lighting_data[off] = self.info.led_r;
            self.lighting_data[off + 1] = self.info.led_g;
            self.lighting_data[off + 2] = self.info.led_b;
        }
    }

    /// Clear all pixels in current data
    pub(in crate::tui) fn clear_userpic(&mut self) {
        self.lighting_data = vec![0; 288];
        self.status_msg = "Userpic cleared (unsaved)".to_string();
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
            Constraint::Length(15), // Grid area
            Constraint::Min(5),    // Controls area
        ])
        .split(area);

    // Render 16x6 grid
    render_userpic_grid(f, app, chunks[0]);

    // Render controls
    render_lighting_controls(f, app, chunks[1]);
}

fn render_userpic_grid(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("Userpic Slot {} Editor [16x6 Grid]", app.lighting_slot));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let key_width = 4u16;
    let key_height = 2u16;

    for col in 0..16 {
        for row in 0..6 {
            let off = (col as usize * 18) + (row as usize * 3);
            let r = app.lighting_data[off];
            let g = app.lighting_data[off + 1];
            let b = app.lighting_data[off + 2];

            let x = inner.x + (col as u16 * key_width);
            let y = inner.y + (row as u16 * key_height);

            if x + key_width > inner.x + inner.width || y + key_height > inner.y + inner.height {
                continue;
            }

            let is_selected = app.lighting_cursor == (col as u8, row as u8);
            let color = Color::Rgb(r, g, b);
            
            // Contrast color for border/text
            let brightness = (r as f32 * 0.299 + g as f32 * 0.587 + b as f32 * 0.114) / 255.0;
            let contrast = if brightness > 0.5 { Color::Black } else { Color::White };

            let cell_block = Block::default()
                .borders(Borders::ALL)
                .border_style(if is_selected {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                })
                .bg(color);

            let cell_rect = Rect::new(x, y, key_width, key_height);
            let inner_rect = cell_block.inner(cell_rect);
            
            // Draw a small dot or something to show color if it's too dark
            let label = if is_selected { "X" } else { "" };
            let p = Paragraph::new(label)
                .alignment(Alignment::Center)
                .style(Style::default().fg(contrast).bg(color));
            
            f.render_widget(cell_block, cell_rect);
            f.render_widget(p, inner_rect);
        }
    }
}

fn render_lighting_controls(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Controls");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let (col, row) = app.lighting_cursor;
    let off = (col as usize * 18) + (row as usize * 3);
    let r = app.lighting_data[off];
    let g = app.lighting_data[off + 1];
    let b = app.lighting_data[off + 2];

    let lines = vec![
        Line::from(vec![
            Span::raw("Slot: "),
            Span::styled(format!("< {} >", app.lighting_slot), Style::default().fg(Color::Cyan)),
            Span::raw("  (0-4) [+/- to change]"),
        ]),
        Line::from(vec![
            Span::raw("Cursor: "),
            Span::styled(format!("Col {}, Row {}", col, row), Style::default().fg(Color::Yellow)),
            Span::raw(format!("  Color: #{:02X}{:02X}{:02X}", r, g, b)),
            Span::styled("  \u{2588}".repeat(4), Style::default().fg(Color::Rgb(r, g, b))),
        ]),
        Line::from(vec![
            Span::raw("Main Color: "),
            Span::styled(
                format!("#{:02X}{:02X}{:02X}", app.info.led_r, app.info.led_g, app.info.led_b),
                Style::default().fg(Color::Rgb(app.info.led_r, app.info.led_g, app.info.led_b))
            ),
            Span::raw(" [Press Space to paint cursor with this color]"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Arrows: move  Space: paint  s: save to keyboard  r: reload from keyboard  c: clear slot  Tab: next tab",
            Style::default().fg(Color::DarkGray)
        )),
    ];

    let p = Paragraph::new(lines);
    f.render_widget(p, inner);
}
