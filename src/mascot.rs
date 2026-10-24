//! Small puppy-robot head built from `@`, `$` and `.` rings with a blank (terminal
//! background) centre. Each ring gets a color, so the head fades from the edge inward.
//! Generated from an ellipse so spacing is always even. Terminal cells are about twice
//! as tall as they are wide, so the vertical radius is scaled to keep the head round.
//! The pointer shifts only the eyes and nose by one column.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mood {
    Idle,
    Thinking,
    Happy,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    Left,
    Center,
    Right,
}

/// Every frame is exactly `WIDTH` columns and `HEIGHT` rows.
pub const WIDTH: usize = 17;
pub const HEIGHT: usize = 8;

/// The mascot is not drawn when the terminal is smaller than this.
pub const MIN_COLUMNS: u16 = 50;
pub const MIN_ROWS: u16 = 32;

const CENTER_X: f32 = 8.0;
const CENTER_Y: f32 = 4.5;
const RADIUS_X: f32 = 7.5;
const RADIUS_Y: f32 = 3.4;

/// Antenna sits on the centre column above the head.
const ANTENNA_X: usize = 8;
const ANTENNA_ROW: usize = 1;
const TIP_ROW: usize = 0;

const EYE_ROW: usize = 4;
const EYE_OFFSET: i32 = 3;
const NOSE_ROW: usize = 6;

pub fn fits(columns: u16, rows: u16) -> bool {
    columns >= MIN_COLUMNS && rows >= MIN_ROWS
}

/// Columns the eyes and nose move toward the pointer.
fn gaze(look: Look) -> i32 {
    match look {
        Look::Left => -1,
        Look::Center => 0,
        Look::Right => 1,
    }
}

/// Ring character for a cell: `@` on the rim, `$` inside it, `.` further in,
/// and a blank centre that shows the terminal background.
fn ring(x: usize, y: usize) -> char {
    let dx = (x as f32 - CENTER_X) / RADIUS_X;
    let dy = (y as f32 - CENTER_Y) / RADIUS_Y;
    let distance = dx * dx + dy * dy;
    if distance > 1.0 {
        ' '
    } else if distance > 0.72 {
        '@'
    } else if distance > 0.42 {
        '$'
    } else if distance > 0.12 {
        '.'
    } else {
        ' '
    }
}

/// Rows for one look and mood, each exactly `WIDTH` characters.
pub fn frame(look: Look, mood: Mood) -> Vec<String> {
    let eye = match mood {
        Mood::Idle => 'o',
        Mood::Happy => '^',
        Mood::Thinking => '?',
        Mood::Error => 'x',
    };
    let gaze = gaze(look);
    let center = CENTER_X as i32;
    (0..HEIGHT)
        .map(|y| {
            (0..WIDTH)
                .map(|x| {
                    let column = x as i32;
                    if y == EYE_ROW
                        && (column == center - EYE_OFFSET + gaze
                            || column == center + EYE_OFFSET + gaze)
                    {
                        eye
                    } else if y == NOSE_ROW && column == center + gaze {
                        'v'
                    } else if x == ANTENNA_X && y == TIP_ROW {
                        '@'
                    } else if x == ANTENNA_X && y == ANTENNA_ROW {
                        '|'
                    } else {
                        ring(x, y)
                    }
                })
                .collect()
        })
        .collect()
}
