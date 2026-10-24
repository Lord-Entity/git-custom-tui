use crate::mascot::{self, Look, Mood};
use anyhow::{Context, Result};
use crossterm::{
    cursor,
    event::{DisableMouseCapture, EnableMouseCapture},
    execute, queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::{
    io::{stdout, Write},
    sync::Mutex,
};

const PANEL_MAX_WIDTH: u16 = 96;
const PANEL_MIN_WIDTH: u16 = 48;
const PANEL_MIN_HEIGHT: u16 = 12;
/// Rows that are not header or body: borders, header divider, footer divider and footer.
const CHROME_ROWS: u16 = 5;

/// Columns around the panel center where the mascot keeps the center frame.
const DEAD_ZONE: i32 = 6;

static MOOD: Mutex<Mood> = Mutex::new(Mood::Idle);
static LOOK: Mutex<Look> = Mutex::new(Look::Center);

/// Sets the mascot face used by the following frames.
pub fn set_mood(mood: Mood) {
    if let Ok(mut current) = MOOD.lock() {
        *current = mood;
    }
}

fn mood() -> Mood {
    MOOD.lock().map_or(Mood::Idle, |current| *current)
}

fn look() -> Look {
    LOOK.lock().map_or(Look::Center, |current| *current)
}

/// Turns the mascot toward the pointer. Returns true only when the frame changes,
/// so small moves inside the dead zone do not force a redraw.
pub fn track_pointer(column: u16) -> bool {
    let (columns, rows) = terminal::size().unwrap_or((100, 30));
    if !mascot::fits(columns, rows) {
        return false;
    }
    let (left, _, width, _) = panel(columns, rows);
    let offset = column as i32 - (left + width / 2) as i32;
    let target = if offset < -DEAD_ZONE {
        Look::Left
    } else if offset > DEAD_ZONE {
        Look::Right
    } else {
        Look::Center
    };
    match LOOK.lock() {
        Ok(mut current) if *current != target => {
            *current = target;
            true
        }
        _ => false,
    }
}

fn panel(columns: u16, rows: u16) -> (u16, u16, u16, u16) {
    let width = PANEL_MAX_WIDTH
        .min(columns.saturating_sub(4))
        .max(PANEL_MIN_WIDTH);
    let height = rows.saturating_sub(2).max(PANEL_MIN_HEIGHT);
    let left = columns.saturating_sub(width) / 2;
    (left, 1, width, height)
}

fn border(width: u16, left: &str, fill: &str, right: &str) -> String {
    format!(
        "{left}{fill}{right}",
        fill = fill.repeat(width.saturating_sub(2) as usize)
    )
}

fn visible_width(value: &str) -> usize {
    let mut visible = 0;
    let mut escape = false;
    for character in value.chars() {
        if escape {
            escape = character != 'm';
        } else if character == '\x1b' {
            escape = true;
        } else {
            visible += 1;
        }
    }
    visible
}

/// Cuts `value` to `columns` visible characters and keeps its color codes intact.
fn fit(value: &str, columns: usize) -> String {
    let mut out = String::new();
    let mut visible = 0;
    let mut escape = false;
    for character in value.chars() {
        if escape {
            out.push(character);
            escape = character != 'm';
        } else if character == '\x1b' {
            out.push(character);
            escape = true;
        } else {
            if visible == columns {
                out.push_str("\x1b[0m");
                break;
            }
            out.push(character);
            visible += 1;
        }
    }
    out
}

/// Fits one line inside the frame: two spaces of margin on each side, padded to `inner`.
fn pad(value: &str, inner: u16) -> String {
    let text_width = inner.saturating_sub(4) as usize;
    let shown = fit(value, text_width);
    let spare = text_width.saturating_sub(visible_width(&shown));
    format!("  {shown}{}  ", " ".repeat(spare))
}

/// Puts `text` at `offset` columns inside a `text_area`-wide row.
fn place(text: &str, offset: usize, text_area: usize) -> String {
    let spare = text_area.saturating_sub(offset + visible_width(text));
    format!("{}{text}{}", " ".repeat(offset), " ".repeat(spare))
}

/// Centered title, then the mascot centered below it when the terminal is big enough.
fn header(columns: u16, rows: u16, text_area: usize) -> Vec<String> {
    let mood = mood();

    let info = [
        "\x1b[1m◆  GCT  ·  GIT COMMIT CONTROL  ◆\x1b[0m".to_string(),
        muted("Fast, dated commits from your terminal"),
    ];
    let mut lines: Vec<String> = info
        .iter()
        .map(|text| {
            let offset = text_area.saturating_sub(visible_width(text)) / 2;
            place(text, offset, text_area)
        })
        .collect();
    if mascot::fits(columns, rows) {
        let offset = text_area.saturating_sub(mascot::WIDTH) / 2;
        lines.extend(
            mascot::frame(look(), mood)
                .iter()
                .map(|row| place(&shade(row), offset, text_area)),
        );
        let status = format!("{}{}\x1b[0m", rgb(MID_HEX), status_text(mood));
        let offset = text_area.saturating_sub(visible_width(&status)) / 2;
        lines.push(place(&status, offset, text_area));
    }
    lines
}

/// Hex colors for the mascot gradient, from the rim inward, plus the eyes and nose.
const RIM_HEX: u32 = 0x6ee7b7;
const MID_HEX: u32 = 0x10b981;
const INNER_HEX: u32 = 0x059669;
const FACE_HEX: u32 = 0xa7f3d0;

/// 24-bit foreground escape for a `0xRRGGBB` color.
fn rgb(hex: u32) -> String {
    format!(
        "\x1b[38;2;{};{};{}m",
        hex >> 16 & 0xff,
        hex >> 8 & 0xff,
        hex & 0xff
    )
}

/// Colors each mascot character: `@` rim, `$` middle, `.` inner, eyes and nose bright.
fn shade(row: &str) -> String {
    row.chars()
        .map(|ch| match ch {
            ' ' => " ".to_string(),
            '@' => format!("{}{ch}\x1b[0m", rgb(RIM_HEX)),
            '$' => format!("{}{ch}\x1b[0m", rgb(MID_HEX)),
            '.' => format!("{}{ch}\x1b[0m", rgb(INNER_HEX)),
            '^' | '?' | 'x' | 'o' | 'v' => format!("\x1b[1m{}{ch}\x1b[0m", rgb(FACE_HEX)),
            _ => format!("{}{ch}\x1b[0m", rgb(MID_HEX)),
        })
        .collect()
}

fn status_text(mood: Mood) -> &'static str {
    match mood {
        Mood::Idle => "ready when you are",
        Mood::Thinking => "working on it...",
        Mood::Happy => "all done!",
        Mood::Error => "something went wrong",
    }
}

fn frame_row(out: &mut impl Write, left: u16, row: u16, inner: u16, text: &str) -> Result<()> {
    queue!(
        out,
        cursor::MoveTo(left, row),
        SetForegroundColor(Color::Cyan),
        Print("│"),
        ResetColor,
        Print(pad(text, inner)),
        SetForegroundColor(Color::Cyan),
        Print("│"),
    )?;
    Ok(())
}

fn divider(out: &mut impl Write, left: u16, row: u16, width: u16) -> Result<()> {
    queue!(
        out,
        cursor::MoveTo(left, row),
        SetForegroundColor(Color::Cyan),
        Print(border(width, "├", "─", "┤")),
    )?;
    Ok(())
}

/// Panel geometry for the current terminal size, shared by `draw` and `body_rows`.
struct Layout {
    left: u16,
    top: u16,
    width: u16,
    height: u16,
    inner: u16,
    header: Vec<String>,
    body_top: u16,
    body_rows: u16,
}

fn layout() -> Layout {
    let (columns, rows) = terminal::size().unwrap_or((100, 30));
    let (left, top, width, height) = panel(columns, rows);
    let inner = width - 2;
    let header = header(columns, rows, inner.saturating_sub(4) as usize);
    let header_rows = header.len() as u16;
    let body_top = top + header_rows + 2;
    let body_rows = height.saturating_sub(header_rows + CHROME_ROWS);
    Layout {
        left,
        top,
        width,
        height,
        inner,
        header,
        body_top,
        body_rows,
    }
}

/// Number of body rows `draw` shows at the current terminal size.
pub fn body_rows() -> usize {
    layout().body_rows as usize
}

/// Draws the panel: mascot header, body lines, footer hints, and an optional cursor.
/// `cursor` is (row, column) inside the body.
pub fn draw(body: &[String], footer: &str, cursor: Option<(u16, u16)>) -> Result<()> {
    let Layout {
        left,
        top,
        width,
        height,
        inner,
        header,
        body_top,
        body_rows,
    } = layout();
    let header_rows = header.len() as u16;
    let footer_divider = body_top + body_rows;

    let mut out = stdout();
    queue!(out, terminal::BeginSynchronizedUpdate, cursor::Hide)?;
    for row in 0..top + height {
        queue!(out, cursor::MoveTo(0, row), Clear(ClearType::CurrentLine))?;
    }
    queue!(
        out,
        cursor::MoveTo(left, top),
        SetForegroundColor(Color::Cyan),
        Print(border(width, "╭", "─", "╮")),
    )?;
    for (index, line) in header.iter().enumerate() {
        frame_row(&mut out, left, top + 1 + index as u16, inner, line)?;
    }
    divider(&mut out, left, top + header_rows + 1, width)?;
    for row in 0..body_rows {
        let line = body.get(row as usize).map_or("", String::as_str);
        frame_row(&mut out, left, body_top + row, inner, line)?;
    }
    divider(&mut out, left, footer_divider, width)?;
    frame_row(&mut out, left, footer_divider + 1, inner, &muted(footer))?;
    queue!(
        out,
        cursor::MoveTo(left, top + height - 1),
        SetForegroundColor(Color::Cyan),
        Print(border(width, "╰", "─", "╯")),
        ResetColor,
    )?;
    match cursor {
        Some((row, column)) => queue!(
            out,
            cursor::MoveTo(left + 3 + column, body_top + row),
            cursor::Show,
            terminal::EndSynchronizedUpdate,
        )?,
        None => queue!(out, cursor::Hide, terminal::EndSynchronizedUpdate)?,
    }
    out.flush()?;
    Ok(())
}

pub struct Guard;

impl Guard {
    pub fn enter() -> Result<Self> {
        resume()?;
        Ok(Self)
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        restore();
    }
}

/// Leaves raw mode and the alternate screen. Safe to call more than once.
pub fn restore() {
    let _ = terminal::disable_raw_mode();
    let _ = execute!(
        stdout(),
        DisableMouseCapture,
        cursor::Show,
        LeaveAlternateScreen,
        ResetColor
    );
}

/// Hands the terminal back to the shell, for example while an external editor runs.
pub fn suspend() {
    restore();
}

pub fn resume() -> Result<()> {
    terminal::enable_raw_mode().context("could not enable terminal raw mode")?;
    execute!(
        stdout(),
        EnterAlternateScreen,
        EnableMouseCapture,
        Clear(ClearType::All),
        cursor::Hide
    )
    .context("could not enter terminal UI")
}

pub fn muted(message: impl AsRef<str>) -> String {
    format!("\x1b[90m{}\x1b[0m", message.as_ref())
}

pub fn accent(message: impl AsRef<str>) -> String {
    format!("\x1b[36m{}\x1b[0m", message.as_ref())
}

pub fn success(message: impl AsRef<str>) -> String {
    format!("\x1b[32m{}\x1b[0m", message.as_ref())
}

pub fn warning(message: impl AsRef<str>) -> String {
    format!("\x1b[33m{}\x1b[0m", message.as_ref())
}

pub fn danger(message: impl AsRef<str>) -> String {
    format!("\x1b[31m{}\x1b[0m", message.as_ref())
}

pub fn selected(message: impl AsRef<str>) -> String {
    format!("\x1b[1;30;46m{}\x1b[0m", message.as_ref())
}
